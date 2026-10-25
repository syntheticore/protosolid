//! Euler operators: local topology edits that keep a cell complex consistent.
//!
//! Primitive construction and local seam edits are expressed with these
//! operators. Global operations instead sew exact face patches (see `sew`).
//! Operators record full provenance but perform no geometric validation;
//! the enclosing transaction validates the finished result.

use std::collections::BTreeSet;

use cgmath::prelude::*;

use crate::Point3;

use super::geometry::finite_point;
use super::history::ChangeKind;
use super::topology::{Body, Loop, Shell, Vertex};
use super::transaction::Transaction;
use super::{
    geometry_error, invalid, topology_error, BodyId, CoedgeId, Curve2, Curve3, DerivationRole,
    EdgeId, FaceId, FaceOrientation, KernelResult, LoopId, LoopKind, ParameterRange, ShellId,
    ShellKind, Surface3, VertexId,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct MvfsResult {
    pub body: BodyId,
    pub shell: ShellId,
    pub face: FaceId,
    pub loop_id: LoopId,
    pub vertex: VertexId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MevResult {
    pub edge: EdgeId,
    pub vertex: VertexId,
    pub forward: CoedgeId,
    pub reverse: CoedgeId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MefResult {
    pub edge: EdgeId,
    pub face: FaceId,
    pub loop_id: LoopId,
    pub forward: CoedgeId,
    pub reverse: CoedgeId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct KefResult {
    pub surviving_face: FaceId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct KemrResult {
    pub outer_loop: LoopId,
    pub inner_loop: LoopId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MekrResult {
    pub edge: EdgeId,
    pub loop_id: LoopId,
    pub forward: CoedgeId,
    pub reverse: CoedgeId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct KfmrhResult {
    pub inner_loop: LoopId,
}

impl Transaction<'_> {
    /// Make vertex, face, shell: the seed of a new body.
    pub(crate) fn mvfs(
        &mut self,
        point: Point3,
        surface: Surface3,
        orientation: FaceOrientation,
    ) -> KernelResult<MvfsResult> {
        if !finite_point(point) {
            return Err(invalid("initial vertex must be finite"));
        }
        let projection = surface.project(point, self.kernel.tolerance)?;
        if projection.distance
            > self
                .kernel
                .tolerance
                .linear_at_scale(point.to_vec().magnitude())
        {
            return Err(geometry_error(
                "MVFS seed vertex must lie on its face surface",
            ));
        }
        let body_name = self.names.next_for("body")?;
        let shell_name = self.names.next_for("shell")?;
        let face_name = self.names.next_for("face")?;
        let loop_name = self.names.next_for("loop.outer")?;
        let vertex_name = self.names.next_for("vertex")?;

        let body = self.working.bodies.insert(Body::new(body_name));
        let shell = self
            .working
            .shells
            .insert(Shell::new(shell_name, body, ShellKind::Open));
        self.working.bodies.get_mut(body)?.shells.push(shell);
        let (face, loop_id) =
            self.make_face_loop(shell, face_name, loop_name, surface, orientation)?;
        let vertex = self.working.vertices.insert(Vertex::new(
            vertex_name,
            point,
            self.kernel.tolerance.linear,
        ));

        for name in [body_name, shell_name, face_name, loop_name, vertex_name] {
            self.history.created(name);
        }
        self.touch(body);
        Ok(MvfsResult {
            body,
            shell,
            face,
            loop_id,
            vertex,
        })
    }

    /// Make edge and vertex by inserting a spike into a face loop.
    ///
    /// `after` is absent for the first edge in the empty MVFS loop. Otherwise it
    /// identifies the coedge whose end vertex is `from`. Two exact p-curves are
    /// required because both uses can have different face parameterizations.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mev(
        &mut self,
        loop_id: LoopId,
        after: Option<CoedgeId>,
        from: VertexId,
        point: Point3,
        curve: Curve3,
        curve_range: ParameterRange,
        forward_pcurve: Curve2,
        reverse_pcurve: Curve2,
        pcurve_range: ParameterRange,
    ) -> KernelResult<MevResult> {
        let loop_first = self.working.loop_(loop_id)?.first;
        match (loop_first, after) {
            (None, None) => {}
            (Some(_), Some(after_id)) => {
                if self.working.coedge(after_id)?.loop_id != loop_id {
                    return Err(invalid("insertion coedge belongs to another loop"));
                }
                if self.working.coedge_vertices(after_id)?[1] != from {
                    return Err(invalid("insertion coedge does not end at the source vertex"));
                }
            }
            (None, Some(_)) => return Err(invalid("an empty loop has no insertion coedge")),
            (Some(_), None) => return Err(invalid("a nonempty loop requires an insertion coedge")),
        }

        let source_name = self.working.vertex(from)?.name;
        let vertex_name = self.names.next_for("vertex")?;
        let edge_name = self.names.next_for("edge")?;
        let forward_name = self.names.next_for("coedge.forward")?;
        let reverse_name = self.names.next_for("coedge.reverse")?;
        let vertex = self.make_vertex(vertex_name, point);
        let edge = self.make_edge(edge_name, [from, vertex], curve, curve_range)?;
        let forward =
            self.make_coedge(forward_name, edge, loop_id, true, forward_pcurve, pcurve_range)?;
        let reverse = self.make_coedge(
            reverse_name,
            edge,
            loop_id,
            false,
            reverse_pcurve,
            pcurve_range.reversed(),
        )?;

        if let Some(after) = after {
            let next = self.working.coedge(after)?.next;
            self.link(after, forward)?;
            self.link(forward, reverse)?;
            self.link(reverse, next)?;
        } else {
            self.link(forward, reverse)?;
            self.link(reverse, forward)?;
            self.working.loops.get_mut(loop_id)?.first = Some(forward);
        }

        self.history
            .derived(vertex_name, [source_name], DerivationRole::MevVertex);
        self.history
            .derived(edge_name, [source_name, vertex_name], DerivationRole::MevEdge);
        self.history
            .derived(forward_name, [edge_name], DerivationRole::MevForwardUse);
        self.history
            .derived(reverse_name, [edge_name], DerivationRole::MevReverseUse);
        self.history
            .changed(source_name, ChangeKind::Modified, [source_name]);

        self.record_loop_edit(loop_id)?;
        Ok(MevResult {
            edge,
            vertex,
            forward,
            reverse,
        })
    }

    /// Make edge and face by connecting the start vertices of two coedges in
    /// one loop. The original loop retains `first`; the other cycle becomes the
    /// outer loop of the newly created face.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mef(
        &mut self,
        first: CoedgeId,
        second: CoedgeId,
        curve: Curve3,
        curve_range: ParameterRange,
        new_face_surface: Surface3,
        new_face_orientation: FaceOrientation,
        new_face_pcurve: Curve2,
        old_face_pcurve: Curve2,
        pcurve_range: ParameterRange,
    ) -> KernelResult<MefResult> {
        if first == second {
            return Err(invalid("MEF requires two distinct coedges"));
        }
        let old_loop = self.working.coedge(first)?.loop_id;
        if self.working.coedge(second)?.loop_id != old_loop {
            return Err(invalid("MEF coedges must belong to one loop"));
        }
        let old_face = self.working.loop_(old_loop)?.face;
        let shell = self.working.face(old_face)?.shell;
        let [start, _] = self.working.coedge_vertices(first)?;
        let [end, _] = self.working.coedge_vertices(second)?;
        if start == end {
            return Err(invalid("MEF may not create a zero-length edge"));
        }

        let edge_name = self.names.next_for("edge")?;
        let forward_name = self.names.next_for("coedge.forward")?;
        let reverse_name = self.names.next_for("coedge.reverse")?;
        let face_name = self.names.next_for("face")?;
        let loop_name = self.names.next_for("loop.outer")?;
        let (new_face, new_loop) = self.make_face_loop(
            shell,
            face_name,
            loop_name,
            new_face_surface,
            new_face_orientation,
        )?;
        let edge = self.make_edge(edge_name, [start, end], curve, curve_range)?;
        let forward =
            self.make_coedge(forward_name, edge, new_loop, true, new_face_pcurve, pcurve_range)?;
        let reverse = self.make_coedge(
            reverse_name,
            edge,
            old_loop,
            false,
            old_face_pcurve,
            pcurve_range.reversed(),
        )?;

        let previous_first = self.working.coedge(first)?.previous;
        let previous_second = self.working.coedge(second)?.previous;
        // Existing cycle: first ... previous_second -> reverse -> first.
        self.link(previous_second, reverse)?;
        self.link(reverse, first)?;
        // New cycle: second ... previous_first -> forward -> second.
        self.link(previous_first, forward)?;
        self.link(forward, second)?;
        self.working.loops.get_mut(old_loop)?.first = Some(first);
        self.working.loops.get_mut(new_loop)?.first = Some(second);
        self.assign_cycle(new_loop, second)?;

        let old_face_name = self.working.face(old_face)?.name;
        let start_name = self.working.vertex(start)?.name;
        let end_name = self.working.vertex(end)?.name;
        self.history
            .derived(edge_name, [start_name, end_name], DerivationRole::MefEdge);
        self.history
            .derived(forward_name, [edge_name], DerivationRole::MefNewFaceUse);
        self.history
            .derived(reverse_name, [edge_name], DerivationRole::MefOldFaceUse);
        self.history
            .derived(face_name, [old_face_name], DerivationRole::MefFace);
        self.history
            .derived(loop_name, [face_name], DerivationRole::MefOuterLoop);
        self.history
            .changed(old_face_name, ChangeKind::Split, [old_face_name, face_name]);

        self.record_loop_edit(old_loop)?;
        Ok(MefResult {
            edge,
            face: new_face,
            loop_id: new_loop,
            forward,
            reverse,
        })
    }

    /// Kill edge and face, the inverse of MEF for a face with one outer loop.
    pub(crate) fn kef(&mut self, edge: EdgeId, face_to_remove: FaceId) -> KernelResult<KefResult> {
        let (edge_coedges, edge_name) = {
            let edge_value = self.working.edge(edge)?;
            (edge_value.coedges.clone(), edge_value.name)
        };
        if edge_coedges.len() != 2 {
            return Err(invalid("KEF requires a two-sided edge"));
        }
        let (remove_face_loops, shell, remove_face_name) = {
            let remove_face = self.working.face(face_to_remove)?;
            (
                remove_face.loops.clone(),
                remove_face.shell,
                remove_face.name,
            )
        };
        if remove_face_loops.len() != 1 {
            return Err(invalid(
                "basic KEF only removes a face with one outer loop; transfer holes first",
            ));
        }
        let remove_loop = remove_face_loops[0];
        let remove_use = *edge_coedges
            .iter()
            .find(|id| {
                self.working
                    .coedge(**id)
                    .is_ok_and(|coedge| coedge.loop_id == remove_loop)
            })
            .ok_or_else(|| invalid("edge is not incident to the face being removed"))?;
        let keep_use = *edge_coedges
            .iter()
            .find(|id| **id != remove_use)
            .expect("a two-sided edge has a mate");
        let keep_loop = self.working.coedge(keep_use)?.loop_id;
        let keep_face = self.working.loop_(keep_loop)?.face;
        if keep_face == face_to_remove {
            return Err(invalid("KEF cannot remove a seam edge from one face"));
        }
        if self.working.face(keep_face)?.shell != shell {
            return Err(invalid("KEF faces must belong to the same shell"));
        }

        let keep_previous = self.working.coedge(keep_use)?.previous;
        let keep_next = self.working.coedge(keep_use)?.next;
        let remove_previous = self.working.coedge(remove_use)?.previous;
        let remove_next = self.working.coedge(remove_use)?.next;
        if keep_previous == keep_use || remove_previous == remove_use {
            return Err(invalid("KEF would create an empty merged loop"));
        }
        self.link(keep_previous, remove_next)?;
        self.link(remove_previous, keep_next)?;
        self.working.loops.get_mut(keep_loop)?.first = Some(keep_next);
        self.assign_cycle(keep_loop, keep_next)?;

        let remove_loop_name = self.working.loop_(remove_loop)?.name;
        let keep_face_name = self.working.face(keep_face)?.name;
        let use_names = [
            self.working.coedge(keep_use)?.name,
            self.working.coedge(remove_use)?.name,
        ];
        self.working.coedges.remove(keep_use)?;
        self.working.coedges.remove(remove_use)?;
        self.working.edges.remove(edge)?;
        self.working.loops.remove(remove_loop)?;
        self.working.faces.remove(face_to_remove)?;
        self.working
            .shells
            .get_mut(shell)?
            .faces
            .retain(|id| *id != face_to_remove);

        for name in [
            edge_name,
            remove_face_name,
            remove_loop_name,
            use_names[0],
            use_names[1],
        ] {
            self.history.changed(name, ChangeKind::Deleted, []);
        }
        self.history
            .changed(keep_face_name, ChangeKind::Merged, [keep_face_name]);
        self.history
            .merge_construction(remove_face_name, keep_face_name);
        // A merge within a primitive's construction keeps its authored role,
        // such as the extrusion side formed by joining two half cylinders.
        if !matches!(
            self.history.origin(keep_face_name),
            Some(super::Origin::Created { role: Some(_) })
        ) {
            let sources = [keep_face_name, remove_face_name]
                .into_iter()
                .flat_map(|name| self.history.source_roots(name))
                .collect::<BTreeSet<_>>();
            self.history
                .derived(keep_face_name, sources, DerivationRole::KefFace);
        }
        self.record_loop_edit(keep_loop)?;
        Ok(KefResult {
            surviving_face: keep_face,
        })
    }

    /// Kill edge, make ring: remove a bridge edge and split one loop into an
    /// outer loop and a new inner loop on the same face. `inner_use` selects
    /// which removed coedge's successor begins the hole cycle.
    pub(crate) fn kemr(&mut self, edge: EdgeId, inner_use: CoedgeId) -> KernelResult<KemrResult> {
        let (uses, edge_name) = {
            let edge = self.working.edge(edge)?;
            (edge.coedges.clone(), edge.name)
        };
        if uses.len() != 2 || !uses.contains(&inner_use) {
            return Err(invalid(
                "KEMR requires one of the two coedges of a two-sided edge",
            ));
        }
        let other_use = *uses
            .iter()
            .find(|id| **id != inner_use)
            .expect("a two-sided edge has a mate");
        let inner = self.working.coedge(inner_use)?;
        let other = self.working.coedge(other_use)?;
        if inner.loop_id != other.loop_id {
            return Err(invalid("KEMR edge uses must belong to one loop"));
        }
        if inner.next == other_use || other.next == inner_use {
            return Err(invalid("KEMR cannot remove a spike edge"));
        }
        let outer_loop = inner.loop_id;
        let face = self.working.loop_(outer_loop)?.face;
        let inner_next = inner.next;
        let inner_previous = inner.previous;
        let other_next = other.next;
        let other_previous = other.previous;
        let removed_use_names = [inner.name, other.name];
        let loop_name = self.names.next_for("loop.inner")?;
        let inner_loop = self
            .working
            .loops
            .insert(Loop::new(loop_name, face, LoopKind::Inner));
        self.working.faces.get_mut(face)?.loops.push(inner_loop);

        // Cycle selected as inner: other_previous -> inner_next.
        self.link(other_previous, inner_next)?;
        // Remaining outer cycle: inner_previous -> other_next.
        self.link(inner_previous, other_next)?;
        self.working.loops.get_mut(outer_loop)?.first = Some(other_next);
        self.working.loops.get_mut(inner_loop)?.first = Some(inner_next);
        self.assign_cycle(inner_loop, inner_next)?;
        self.assign_cycle(outer_loop, other_next)?;

        self.working.coedges.remove(inner_use)?;
        self.working.coedges.remove(other_use)?;
        self.working.edges.remove(edge)?;

        let face_name = self.working.face(face)?.name;
        self.history
            .derived(loop_name, [face_name], DerivationRole::KemrInnerLoop);
        for name in [edge_name, removed_use_names[0], removed_use_names[1]] {
            self.history.changed(name, ChangeKind::Deleted, []);
        }
        self.history
            .changed(face_name, ChangeKind::Modified, [face_name]);
        self.record_loop_edit(outer_loop)?;
        Ok(KemrResult {
            outer_loop,
            inner_loop,
        })
    }

    /// Make edge, kill ring: bridge an inner loop to another loop on the same face.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mekr(
        &mut self,
        outer: CoedgeId,
        inner: CoedgeId,
        curve: Curve3,
        curve_range: ParameterRange,
        forward_pcurve: Curve2,
        reverse_pcurve: Curve2,
        pcurve_range: ParameterRange,
    ) -> KernelResult<MekrResult> {
        let outer_loop = self.working.coedge(outer)?.loop_id;
        let inner_loop = self.working.coedge(inner)?.loop_id;
        if outer_loop == inner_loop {
            return Err(invalid("MEKR requires two distinct loops"));
        }
        let face = self.working.loop_(outer_loop)?.face;
        if self.working.loop_(inner_loop)?.face != face
            || self.working.loop_(inner_loop)?.kind != LoopKind::Inner
        {
            return Err(invalid(
                "MEKR loops must belong to the same face and the removed loop must be inner",
            ));
        }
        let [start, _] = self.working.coedge_vertices(outer)?;
        let [end, _] = self.working.coedge_vertices(inner)?;
        if start == end {
            return Err(invalid("MEKR may not create a zero-length bridge"));
        }
        let outer_previous = self.working.coedge(outer)?.previous;
        let inner_previous = self.working.coedge(inner)?.previous;
        let edge_name = self.names.next_for("edge")?;
        let forward_name = self.names.next_for("coedge.forward")?;
        let reverse_name = self.names.next_for("coedge.reverse")?;
        let edge = self.make_edge(edge_name, [start, end], curve, curve_range)?;
        let forward =
            self.make_coedge(forward_name, edge, outer_loop, true, forward_pcurve, pcurve_range)?;
        let reverse = self.make_coedge(
            reverse_name,
            edge,
            outer_loop,
            false,
            reverse_pcurve,
            pcurve_range.reversed(),
        )?;

        self.link(outer_previous, forward)?;
        self.link(forward, inner)?;
        self.link(inner_previous, reverse)?;
        self.link(reverse, outer)?;
        self.assign_cycle(outer_loop, outer)?;
        let inner_loop_name = self.working.loop_(inner_loop)?.name;
        self.working
            .faces
            .get_mut(face)?
            .loops
            .retain(|id| *id != inner_loop);
        self.working.loops.remove(inner_loop)?;
        self.working.loops.get_mut(outer_loop)?.first = Some(outer);

        let face_name = self.working.face(face)?.name;
        self.history
            .derived(edge_name, [face_name], DerivationRole::MekrBridgeEdge);
        self.history
            .derived(forward_name, [edge_name], DerivationRole::MekrForwardUse);
        self.history
            .derived(reverse_name, [edge_name], DerivationRole::MekrReverseUse);
        self.history
            .changed(inner_loop_name, ChangeKind::Deleted, []);
        self.history
            .changed(face_name, ChangeKind::Modified, [face_name]);
        self.record_loop_edit(outer_loop)?;
        Ok(MekrResult {
            edge,
            loop_id: outer_loop,
            forward,
            reverse,
        })
    }

    /// Kill face, make ring hole: transfer a simple face's outer loop into
    /// another face as an inner loop.
    pub(crate) fn kfmrh(
        &mut self,
        face_to_keep: FaceId,
        face_to_remove: FaceId,
    ) -> KernelResult<KfmrhResult> {
        if face_to_keep == face_to_remove {
            return Err(invalid("KFMRH requires two faces"));
        }
        let (keep_shell, keep_name) = {
            let face = self.working.face(face_to_keep)?;
            (face.shell, face.name)
        };
        let (remove_shell, remove_name, loops) = {
            let face = self.working.face(face_to_remove)?;
            (face.shell, face.name, face.loops.clone())
        };
        if keep_shell != remove_shell || loops.len() != 1 {
            return Err(invalid(
                "KFMRH requires two faces in one shell and a removed face with one loop",
            ));
        }
        let inner_loop = loops[0];
        {
            let loop_value = self.working.loops.get_mut(inner_loop)?;
            loop_value.face = face_to_keep;
            loop_value.kind = LoopKind::Inner;
        }
        self.working
            .faces
            .get_mut(face_to_keep)?
            .loops
            .push(inner_loop);
        self.working
            .shells
            .get_mut(keep_shell)?
            .faces
            .retain(|id| *id != face_to_remove);
        self.working.faces.remove(face_to_remove)?;

        self.history
            .changed(remove_name, ChangeKind::Merged, [keep_name]);
        self.history
            .changed(keep_name, ChangeKind::Merged, [keep_name]);
        let loop_name = self.working.loop_(inner_loop)?.name;
        self.history
            .changed(loop_name, ChangeKind::Modified, [loop_name]);
        self.record_loop_edit(inner_loop)?;
        Ok(KfmrhResult { inner_loop })
    }

    /// Declare an open construction shell topologically closed. Publication
    /// still proves p-curve/edge consistency and physical validity.
    pub(crate) fn seal_shell(&mut self, shell: ShellId) -> KernelResult<()> {
        let (shell_name, face_ids, body) = {
            let shell_value = self.working.shell(shell)?;
            (
                shell_value.name,
                shell_value.faces.clone(),
                shell_value.body,
            )
        };
        if face_ids.is_empty() {
            return Err(topology_error("a closed shell needs at least one face"));
        }
        let face_set: BTreeSet<_> = face_ids.iter().copied().collect();
        let mut edge_ids = BTreeSet::new();
        for face_id in &face_ids {
            let face = self.working.face(*face_id)?;
            if face.loops.is_empty()
                || face.loops.iter().any(|loop_id| {
                    self.working
                        .loop_(*loop_id)
                        .map_or(true, |loop_| loop_.first.is_none())
                })
            {
                return Err(topology_error(
                    "every face of a closed shell needs nonempty loops",
                ));
            }
            edge_ids.extend(self.working.face_edges(*face_id)?);
        }
        for edge_id in edge_ids {
            let edge = self.working.edge(edge_id)?;
            if edge.coedges.len() != 2 {
                return Err(topology_error(
                    "every edge of a closed shell must have exactly two coedges",
                ));
            }
            for coedge_id in &edge.coedges {
                let loop_id = self.working.coedge(*coedge_id)?.loop_id;
                if !face_set.contains(&self.working.loop_(loop_id)?.face) {
                    return Err(topology_error(
                        "a closed shell edge may not be shared with a face outside the shell",
                    ));
                }
            }
        }
        self.working.shells.get_mut(shell)?.kind = ShellKind::Closed;
        self.touch(body);
        self.history
            .changed(shell_name, ChangeKind::Modified, [shell_name]);
        Ok(())
    }

    /// Move the sole shell of `source` into `target` and remove the empty
    /// body, as when a Boolean cavity joins its containing solid.
    pub(crate) fn merge_shell_into_body(
        &mut self,
        target: BodyId,
        source: BodyId,
    ) -> KernelResult<ShellId> {
        if target == source {
            return Err(invalid("body/shell merge requires distinct bodies"));
        }
        let target_name = self.working.body(target)?.name;
        let source_value = self.working.body(source)?;
        if source_value.shells.len() != 1 {
            return Err(topology_error(
                "the source of a body/shell merge must contain exactly one shell",
            ));
        }
        let source_name = source_value.name;
        let shell = source_value.shells[0];
        let shell_name = self.working.shell(shell)?.name;
        self.working.shells.get_mut(shell)?.body = target;
        self.working.bodies.get_mut(target)?.shells.push(shell);
        self.working.bodies.remove(source)?;
        self.touched_bodies.retain(|body| *body != source);
        self.touch(target);
        self.history
            .changed(target_name, ChangeKind::Modified, [target_name]);
        self.history
            .changed(shell_name, ChangeKind::Modified, [shell_name]);
        self.history.changed(source_name, ChangeKind::Deleted, []);
        Ok(shell)
    }

    /// Assign stable semantic names to the entities created by an MEV.
    pub(super) fn name_mev(
        &mut self,
        result: MevResult,
        vertex_key: &str,
        edge_key: &str,
    ) -> KernelResult<()> {
        let vertex_name = self.names.named("vertex", vertex_key)?;
        let edge_name = self.names.named("edge", edge_key)?;
        let forward_name = self.names.named("coedge.forward", edge_key)?;
        let reverse_name = self.names.named("coedge.reverse", edge_key)?;
        self.rename(result.vertex, vertex_name)?;
        self.rename(result.edge, edge_name)?;
        self.rename(result.forward, forward_name)?;
        self.rename(result.reverse, reverse_name)
    }

    /// Assign stable semantic names to the entities created by an MEF.
    pub(super) fn name_mef(
        &mut self,
        result: MefResult,
        edge_key: &str,
        face_key: Option<&str>,
    ) -> KernelResult<()> {
        let edge_name = self.names.named("edge", edge_key)?;
        let forward_name = self.names.named("coedge.forward", edge_key)?;
        let reverse_name = self.names.named("coedge.reverse", edge_key)?;
        self.rename(result.edge, edge_name)?;
        self.rename(result.forward, forward_name)?;
        self.rename(result.reverse, reverse_name)?;
        if let Some(face_key) = face_key {
            let face_name = self.names.named("face", face_key)?;
            let loop_name = self.names.named("loop.outer", face_key)?;
            self.rename(result.face, face_name)?;
            self.rename(result.loop_id, loop_name)?;
        }
        Ok(())
    }

    /// Make `next` follow `previous` in their loop cycle.
    fn link(&mut self, previous: CoedgeId, next: CoedgeId) -> KernelResult<()> {
        self.working.coedges.get_mut(previous)?.next = next;
        self.working.coedges.get_mut(next)?.previous = previous;
        Ok(())
    }

    /// Assign every coedge of the cycle starting at `start` to `loop_id`.
    fn assign_cycle(&mut self, loop_id: LoopId, start: CoedgeId) -> KernelResult<()> {
        let mut current = start;
        let mut count = 0usize;
        loop {
            self.working.coedges.get_mut(current)?.loop_id = loop_id;
            current = self.working.coedge(current)?.next;
            count += 1;
            if current == start {
                return Ok(());
            }
            if count > self.working.coedges.len() {
                return Err(topology_error("coedge cycle does not close"));
            }
        }
    }
}
