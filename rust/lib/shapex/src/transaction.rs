//! The isolated workspace through which every topology change is published.

use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;
use uuid::Uuid;

use crate::Point3;

use super::history::ChangeKind;
use super::naming::NameSource;
use super::physical::PhysicalCertificate;
use super::topology::{Coedge, Edge, Face, Loop, Vertex};
use super::{
    geometry_error, topology_error, BodyId, CoedgeId, Curve2, Curve3, EdgeId, FaceId,
    FaceOrientation, Kernel, KernelError, KernelResult, LoopId, LoopKind, OperationHistory,
    ParameterRange, PersistentId, ShellId, Surface3, Topology, VertexId,
};

#[derive(Debug)]
pub(crate) struct CommittedOperation {
    pub operation: Uuid,
    pub bodies: Vec<BodyId>,
    pub history: OperationHistory,
    pub certificates: BTreeMap<BodyId, PhysicalCertificate>,
}

/// An isolated copy of the kernel topology edited by one operation.
///
/// Operations assemble their result without intermediate validation. `finish`
/// proves the complete published-body invariants and only then replaces the
/// kernel state; dropping the transaction discards every edit.
pub(crate) struct Transaction<'kernel> {
    pub(super) kernel: &'kernel mut Kernel,
    pub(super) working: Topology,
    pub(super) names: NameSource,
    pub(super) history: OperationHistory,
    pub(super) touched_bodies: Vec<BodyId>,
    namespace: Uuid,
    pub(super) composes_imported_topology: bool,
}

/// Everything an edit can change, retained to retry an alternative edit.
#[derive(Clone)]
pub(super) struct Checkpoint {
    working: Topology,
    names: NameSource,
    history: OperationHistory,
    touched_bodies: Vec<BodyId>,
    composes_imported_topology: bool,
}

impl<'kernel> Transaction<'kernel> {
    pub(crate) fn new(kernel: &'kernel mut Kernel, namespace: Uuid) -> Self {
        Self {
            working: kernel.topology.clone(),
            kernel,
            names: NameSource::new(namespace),
            history: OperationHistory::default(),
            touched_bodies: Vec::new(),
            namespace,
            composes_imported_topology: false,
        }
    }

    /// Run `edit`, restoring this transaction to its prior state if it fails.
    pub(crate) fn atomic<R>(
        &mut self,
        edit: impl FnOnce(&mut Self) -> KernelResult<R>,
    ) -> KernelResult<R> {
        let checkpoint = self.checkpoint();
        let result = edit(self);
        if result.is_err() {
            self.restore(checkpoint);
        }
        result
    }

    /// Run `edit` only for its result, then restore this transaction.
    pub(crate) fn speculate<R>(&mut self, edit: impl FnOnce(&mut Self) -> R) -> R {
        let checkpoint = self.checkpoint();
        let result = edit(self);
        self.restore(checkpoint);
        result
    }

    pub(super) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            working: self.working.clone(),
            names: self.names.clone(),
            history: self.history.clone(),
            touched_bodies: self.touched_bodies.clone(),
            composes_imported_topology: self.composes_imported_topology,
        }
    }

    pub(super) fn restore(&mut self, checkpoint: Checkpoint) {
        self.working = checkpoint.working;
        self.names = checkpoint.names;
        self.history = checkpoint.history;
        self.touched_bodies = checkpoint.touched_bodies;
        self.composes_imported_topology = checkpoint.composes_imported_topology;
    }

    /// Include `body` in the operation's published output.
    pub(super) fn touch(&mut self, body: BodyId) {
        if !self.touched_bodies.contains(&body) {
            self.touched_bodies.push(body);
        }
    }

    pub(super) fn record_loop_edit(&mut self, id: LoopId) -> KernelResult<()> {
        let loop_ = self.working.loop_(id)?;
        let face = self.working.face(loop_.face)?;
        let shell = self.working.shell(face.shell)?;
        let body = self.working.body(shell.body)?;
        for name in [loop_.name, face.name, shell.name, body.name] {
            self.history.modified(name);
        }
        Ok(())
    }

    pub(super) fn rename_vertex(&mut self, id: VertexId, name: PersistentId) -> KernelResult<()> {
        let old = std::mem::replace(&mut self.working.vertices.get_mut(id)?.name, name);
        self.history.rename_entity(old, name);
        Ok(())
    }

    pub(super) fn rename_edge(&mut self, id: EdgeId, name: PersistentId) -> KernelResult<()> {
        let old = std::mem::replace(&mut self.working.edges.get_mut(id)?.name, name);
        self.history.rename_entity(old, name);
        Ok(())
    }

    pub(super) fn rename_coedge(&mut self, id: CoedgeId, name: PersistentId) -> KernelResult<()> {
        let old = std::mem::replace(&mut self.working.coedges.get_mut(id)?.name, name);
        self.history.rename_entity(old, name);
        Ok(())
    }

    pub(super) fn rename_face(&mut self, id: FaceId, name: PersistentId) -> KernelResult<()> {
        let old = std::mem::replace(&mut self.working.faces.get_mut(id)?.name, name);
        self.history.rename_entity(old, name);
        Ok(())
    }

    pub(super) fn rename_loop(&mut self, id: LoopId, name: PersistentId) -> KernelResult<()> {
        let old = std::mem::replace(&mut self.working.loops.get_mut(id)?.name, name);
        self.history.rename_entity(old, name);
        Ok(())
    }

    pub(super) fn make_vertex(&mut self, name: PersistentId, point: Point3) -> VertexId {
        self.working
            .vertices
            .insert(Vertex::new(name, point, self.kernel.tolerance.linear))
    }

    /// Create a face and its initial, empty outer loop.
    pub(super) fn make_face_loop(
        &mut self,
        shell: ShellId,
        face_name: PersistentId,
        loop_name: PersistentId,
        surface: Surface3,
        orientation: FaceOrientation,
    ) -> KernelResult<(FaceId, LoopId)> {
        self.working.shell(shell)?;
        let face = self.working.faces.insert(Face::new(
            face_name,
            shell,
            surface,
            orientation,
            self.kernel.tolerance.linear,
        ));
        let loop_id = self
            .working
            .loops
            .insert(Loop::new(loop_name, face, LoopKind::Outer));
        self.working.faces.get_mut(face)?.loops.push(loop_id);
        self.working.shells.get_mut(shell)?.faces.push(face);
        Ok((face, loop_id))
    }

    /// Create an exact edge without uses. Uses are attached explicitly while a
    /// surrounding rewrite is assembled.
    pub(super) fn make_edge(
        &mut self,
        name: PersistentId,
        vertices: [VertexId; 2],
        curve: Curve3,
        range: ParameterRange,
    ) -> KernelResult<EdgeId> {
        self.working.vertex(vertices[0])?;
        self.working.vertex(vertices[1])?;
        Ok(self.working.edges.insert(Edge::new(
            name,
            vertices,
            curve,
            range,
            self.kernel.tolerance.linear,
        )))
    }

    /// Create and attach one oriented use of an existing edge.
    pub(super) fn make_coedge(
        &mut self,
        name: PersistentId,
        edge: EdgeId,
        loop_id: LoopId,
        forward: bool,
        pcurve: Curve2,
        range: ParameterRange,
    ) -> KernelResult<CoedgeId> {
        self.working.edge(edge)?;
        self.working.loop_(loop_id)?;
        let coedge = self.working.coedges.insert_with(|placeholder| {
            Coedge::new(name, edge, loop_id, forward, pcurve, range, placeholder)
        });
        self.working.edges.get_mut(edge)?.coedges.push(coedge);
        Ok(coedge)
    }

    pub(super) fn orient_coedge(&mut self, coedge: CoedgeId, forward: bool) -> KernelResult<()> {
        let edge = self.working.coedge(coedge)?.edge;
        let range = self.working.edge(edge)?.range;
        let value = self.working.coedges.get_mut(coedge)?;
        if value.forward != forward {
            self.history.modified(value.name);
        }
        value.forward = forward;
        value.range = if forward { range } else { range.reversed() };
        Ok(())
    }

    pub(super) fn orient_face(
        &mut self,
        face: FaceId,
        orientation: FaceOrientation,
    ) -> KernelResult<()> {
        let face = self.working.faces.get_mut(face)?;
        if face.orientation != orientation {
            self.history.modified(face.name);
        }
        face.orientation = orientation;
        Ok(())
    }

    pub(super) fn delete_edge_and_uses(
        &mut self,
        edge: EdgeId,
        uses: &[CoedgeId],
    ) -> KernelResult<()> {
        let edge_value = self.working.edge(edge)?.clone();
        if edge_value.coedges != uses {
            return Err(topology_error(
                "edge deletion did not receive the edge's complete use set",
            ));
        }
        for use_id in uses {
            let name = self.working.coedge(*use_id)?.name;
            self.working.coedges.remove(*use_id)?;
            self.history.changed(name, ChangeKind::Deleted, []);
        }
        self.working.edges.remove(edge)?;
        self.history.changed(edge_value.name, ChangeKind::Deleted, []);
        Ok(())
    }

    pub(super) fn delete_split_vertex(
        &mut self,
        vertex: VertexId,
        outputs: impl IntoIterator<Item = PersistentId>,
    ) -> KernelResult<()> {
        let name = self.working.vertex(vertex)?.name;
        self.working.vertices.remove(vertex)?;
        self.history.changed(name, ChangeKind::Split, outputs);
        Ok(())
    }

    /// Point `vertex` at any remaining coedge leaving it, or none.
    pub(super) fn repair_outgoing(&mut self, vertex: VertexId) -> KernelResult<()> {
        let replacement = self.working.coedges().find_map(|(id, _)| {
            self.working
                .coedge_vertices(id)
                .ok()
                .and_then(|vertices| (vertices[0] == vertex).then_some(id))
        });
        self.working.vertices.get_mut(vertex)?.outgoing = replacement;
        Ok(())
    }

    /// Retarget one endpoint of an exact edge and trim all of its coedge ranges
    /// to the projected replacement point.
    pub(super) fn retarget_edge_endpoint(
        &mut self,
        edge_id: EdgeId,
        old_vertex: VertexId,
        new_vertex: VertexId,
        point: Point3,
    ) -> KernelResult<()> {
        self.move_edge_endpoint(edge_id, old_vertex, new_vertex, point, false)
    }

    /// Retarget one endpoint while permitting its exact curve and p-curves to
    /// extend beyond their previous trims, as when a fillet terminates on a
    /// face that grows into the removed material.
    pub(super) fn retarget_edge_endpoint_allow_extension(
        &mut self,
        edge_id: EdgeId,
        old_vertex: VertexId,
        new_vertex: VertexId,
        point: Point3,
    ) -> KernelResult<()> {
        self.move_edge_endpoint(edge_id, old_vertex, new_vertex, point, true)
    }

    fn move_edge_endpoint(
        &mut self,
        edge_id: EdgeId,
        old_vertex: VertexId,
        new_vertex: VertexId,
        point: Point3,
        allow_extension: bool,
    ) -> KernelResult<()> {
        let edge = self.working.edge(edge_id)?.clone();
        let endpoint = edge
            .vertices
            .iter()
            .position(|vertex| *vertex == old_vertex)
            .ok_or_else(|| topology_error("edge does not contain the endpoint being retargeted"))?;
        let mut projection = edge.curve.project(point, self.kernel.tolerance)?;
        let reference = if endpoint == 0 {
            edge.range.start
        } else {
            edge.range.end
        };
        projection.parameter = edge.curve.domain().nearest(projection.parameter, reference);
        let allowed = self
            .kernel
            .tolerance
            .linear_at_scale(point.to_vec().magnitude().max(1.0));
        if projection.distance > allowed {
            return Err(geometry_error(format!(
                "replacement endpoint misses its exact edge by {}",
                projection.distance
            )));
        }
        let remaining_span = if endpoint == 0 {
            edge.range.end - projection.parameter
        } else {
            projection.parameter - edge.range.start
        };
        let preserves_direction = remaining_span.signum() == edge.range.span().signum()
            && remaining_span.abs() > self.kernel.tolerance.parameter;
        if !preserves_direction {
            return Err(geometry_error(
                "replacement endpoint would collapse or reverse its exact edge",
            ));
        }
        if !allow_extension
            && !edge
                .range
                .contains(projection.parameter, self.kernel.tolerance)
        {
            return Err(geometry_error(format!(
                "replacement parameter {} lies outside edge range {:?}",
                projection.parameter, edge.range,
            )));
        }
        for coedge_id in edge.coedges.iter().copied() {
            let coedge = self.working.coedge(coedge_id)?.clone();
            let traversal_vertices = self.working.coedge_vertices(coedge_id)?;
            let parameter = coedge.use_parameter(&edge, projection.parameter);
            let extended_pcurve = if allow_extension {
                let range = if traversal_vertices[0] == old_vertex {
                    ParameterRange::new(parameter, coedge.range.end)?
                } else {
                    ParameterRange::new(coedge.range.start, parameter)?
                };
                Some(coedge.pcurve.extended(range)?)
            } else {
                None
            };
            let value = self.working.coedges.get_mut(coedge_id)?;
            if let Some(pcurve) = extended_pcurve {
                value.pcurve = pcurve;
            }
            if traversal_vertices[0] == old_vertex {
                value.range.start = parameter;
            } else if traversal_vertices[1] == old_vertex {
                value.range.end = parameter;
            } else {
                return Err(topology_error("edge use lost the endpoint being retargeted"));
            }
            self.history.modified(value.name);
            self.record_loop_edit(coedge.loop_id)?;
        }
        let value = self.working.edges.get_mut(edge_id)?;
        value.vertices[endpoint] = new_vertex;
        if endpoint == 0 {
            value.range.start = projection.parameter;
        } else {
            value.range.end = projection.parameter;
        }
        let name = value.name;
        self.history.changed(name, ChangeKind::Modified, [name]);
        Ok(())
    }

    /// Replace one coedge in its loop without changing the surrounding cycle.
    pub(super) fn replace_loop_coedge(
        &mut self,
        old: CoedgeId,
        replacement: CoedgeId,
    ) -> KernelResult<()> {
        let old_value = self.working.coedge(old)?.clone();
        self.record_loop_edit(old_value.loop_id)?;
        {
            let value = self.working.coedges.get_mut(replacement)?;
            value.loop_id = old_value.loop_id;
            value.previous = old_value.previous;
            value.next = old_value.next;
        }
        self.working.coedges.get_mut(old_value.previous)?.next = replacement;
        self.working.coedges.get_mut(old_value.next)?.previous = replacement;
        if self.working.loop_(old_value.loop_id)?.first == Some(old) {
            self.working.loops.get_mut(old_value.loop_id)?.first = Some(replacement);
        }
        Ok(())
    }

    /// Insert a coedge between two adjacent uses in one loop.
    pub(super) fn insert_loop_coedge(
        &mut self,
        previous: CoedgeId,
        next: CoedgeId,
        inserted: CoedgeId,
    ) -> KernelResult<()> {
        if self.working.coedge(previous)?.next != next
            || self.working.coedge(next)?.previous != previous
        {
            return Err(topology_error("coedge insertion endpoints are not adjacent"));
        }
        let loop_id = self.working.coedge(previous)?.loop_id;
        self.record_loop_edit(loop_id)?;
        self.working.coedges.get_mut(previous)?.next = inserted;
        self.working.coedges.get_mut(next)?.previous = inserted;
        let value = self.working.coedges.get_mut(inserted)?;
        value.loop_id = loop_id;
        value.previous = previous;
        value.next = next;
        Ok(())
    }

    /// Order an unordered set of oriented coedges into one closed loop.
    pub(super) fn close_loop_from_coedges(
        &mut self,
        loop_id: LoopId,
        uses: &[CoedgeId],
    ) -> KernelResult<()> {
        self.record_loop_edit(loop_id)?;
        if uses.len() < 3 {
            return Err(topology_error("a face loop needs at least three boundary uses"));
        }
        let mut remaining = uses.iter().copied().collect::<BTreeSet<_>>();
        let first = uses[0];
        remaining.remove(&first);
        let mut ordered = vec![first];
        while !remaining.is_empty() {
            let end = self.working.coedge_vertices(*ordered.last().unwrap())?[1];
            let next = remaining
                .iter()
                .copied()
                .find(|candidate| {
                    self.working
                        .coedge_vertices(*candidate)
                        .is_ok_and(|vertices| vertices[0] == end)
                })
                .ok_or_else(|| topology_error("boundary uses do not form an oriented cycle"))?;
            remaining.remove(&next);
            ordered.push(next);
        }
        if self.working.coedge_vertices(*ordered.last().unwrap())?[1]
            != self.working.coedge_vertices(first)?[0]
        {
            return Err(topology_error("boundary cycle does not close"));
        }
        for index in 0..ordered.len() {
            let previous = ordered[(index + ordered.len() - 1) % ordered.len()];
            let next = ordered[(index + 1) % ordered.len()];
            let value = self.working.coedges.get_mut(ordered[index])?;
            value.loop_id = loop_id;
            value.previous = previous;
            value.next = next;
        }
        self.working.loops.get_mut(loop_id)?.first = Some(first);
        Ok(())
    }

    /// Orient the missing coedge of an otherwise oriented open cycle.
    pub(super) fn orient_closing_coedge(
        &mut self,
        cycle: &[CoedgeId],
        closing: CoedgeId,
    ) -> KernelResult<()> {
        let mut balance = BTreeMap::<VertexId, i32>::new();
        for use_id in cycle.iter().copied().filter(|use_id| *use_id != closing) {
            let vertices = self.working.coedge_vertices(use_id)?;
            *balance.entry(vertices[0]).or_default() += 1;
            *balance.entry(vertices[1]).or_default() -= 1;
        }
        let start = balance
            .iter()
            .find_map(|(vertex, value)| (*value == -1).then_some(*vertex))
            .ok_or_else(|| topology_error("open boundary has no end vertex"))?;
        let end = balance
            .iter()
            .find_map(|(vertex, value)| (*value == 1).then_some(*vertex))
            .ok_or_else(|| topology_error("open boundary has no start vertex"))?;
        let edge = self.working.coedge(closing)?.edge;
        let edge_vertices = self.working.edge(edge)?.vertices;
        let forward = if edge_vertices == [start, end] {
            true
        } else if edge_vertices == [end, start] {
            false
        } else {
            return Err(topology_error("closing boundary has the wrong vertices"));
        };
        self.orient_coedge(closing, forward)
    }

    /// Validate and publish the result. This is the only path by which a
    /// transaction can replace kernel state.
    pub(crate) fn finish(mut self) -> KernelResult<CommittedOperation> {
        self.normalize_periodic_edge_subdivisions()?;
        super::validation::validate_structure(&self.working, self.kernel.tolerance)
            .into_result()?;
        let certificates = super::physical::certify_bodies(
            &self.working,
            self.kernel.tolerance,
            &self.touched_bodies,
        )
        .map_err(KernelError::from_diagnostics)?;
        if !self.composes_imported_topology {
            for body in &self.touched_bodies {
                if self
                    .working
                    .body_entity_names(*body)?
                    .iter()
                    .any(|name| name.namespace != self.namespace)
                {
                    return Err(topology_error(
                        "a transaction may publish only topology owned by its operation namespace",
                    ));
                }
            }
        }
        self.history
            .complete_retained(&self.kernel.topology, &self.working);
        self.kernel.topology = self.working;
        *self.kernel.tessellation_cache.get_mut().unwrap() = Default::default();
        Ok(CommittedOperation {
            operation: self.namespace,
            bodies: self.touched_bodies,
            history: self.history,
            certificates,
        })
    }

    #[cfg(test)]
    pub(crate) fn commit(self) -> KernelResult<OperationHistory> {
        self.finish().map(|committed| committed.history)
    }
}
