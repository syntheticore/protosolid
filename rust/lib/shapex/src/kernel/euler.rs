use uuid::Uuid;

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3};

use super::history::ChangeKind;
use super::naming::NameSource;
use super::polyhedron::{PolyhedralShellOrientation, PolyhedralTriangle};
use super::topology::{Body, Coedge, Edge, Face, Loop, Shell, Vertex};
use super::validation::{validate, ValidationMode};
use super::{
    BodyId, Circle2, Circle3, CoedgeId, Cone3, Curve2, Curve3, Cylinder3, DiagnosticKind, EdgeId,
    FaceId, FaceOrientation, Kernel, KernelError, KernelResult, Line2, Line3, LoopId, LoopKind,
    NurbsCurve2, NurbsCurve3, NurbsSurface3, OperationHistory, ParameterRange, Plane3,
    PolyhedralMesh, Profile, ShellId, ShellKind, Sphere3, Surface3, Topology, Torus3,
    TrimmedCurve2, VertexId, Wire2,
};

#[derive(Debug, Clone, Copy)]
pub struct MvfsResult {
    pub body: BodyId,
    pub shell: ShellId,
    pub face: FaceId,
    pub loop_id: LoopId,
    pub vertex: VertexId,
}

#[derive(Debug, Clone, Copy)]
pub struct MevResult {
    pub edge: EdgeId,
    pub vertex: VertexId,
    pub forward: CoedgeId,
    pub reverse: CoedgeId,
}

#[derive(Debug, Clone, Copy)]
pub struct MefResult {
    pub edge: EdgeId,
    pub face: FaceId,
    pub loop_id: LoopId,
    pub forward: CoedgeId,
    pub reverse: CoedgeId,
}

/// Complete geometric and topological input to the MEV Euler operator.
///
/// A named request type keeps call sites readable and makes it difficult to swap
/// the two face uses accidentally.
#[derive(Debug, Clone)]
pub struct MevInput {
    pub loop_id: LoopId,
    pub after: Option<CoedgeId>,
    pub from: VertexId,
    pub point: Point3,
    pub curve: Curve3,
    pub curve_range: ParameterRange,
    pub forward_pcurve: Curve2,
    pub reverse_pcurve: Curve2,
    pub pcurve_range: ParameterRange,
}

/// Complete geometric and topological input to the MEF Euler operator.
#[derive(Debug, Clone)]
pub struct MefInput {
    pub first: CoedgeId,
    pub second: CoedgeId,
    pub curve: Curve3,
    pub curve_range: ParameterRange,
    pub new_face_surface: Surface3,
    pub new_face_orientation: FaceOrientation,
    pub new_face_pcurve: Curve2,
    pub old_face_pcurve: Curve2,
    pub pcurve_range: ParameterRange,
}

#[derive(Debug, Clone, Copy)]
pub struct KevResult {
    pub surviving_vertex: VertexId,
    pub loop_id: LoopId,
}

#[derive(Debug, Clone, Copy)]
pub struct KefResult {
    pub surviving_face: FaceId,
    pub loop_id: LoopId,
}

#[derive(Debug, Clone, Copy)]
pub struct KemrResult {
    pub face: FaceId,
    pub outer_loop: LoopId,
    pub inner_loop: LoopId,
}

#[derive(Debug, Clone, Copy)]
pub struct MekrResult {
    pub edge: EdgeId,
    pub loop_id: LoopId,
    pub forward: CoedgeId,
    pub reverse: CoedgeId,
}

#[derive(Debug, Clone, Copy)]
pub struct KfmrhResult {
    pub face: FaceId,
    pub inner_loop: LoopId,
}

#[derive(Debug, Clone, Copy)]
pub struct MfkrhResult {
    pub face: FaceId,
    pub outer_loop: LoopId,
}

#[derive(Debug)]
pub(crate) struct CommittedOperation {
    pub operation: Uuid,
    pub bodies: Vec<BodyId>,
    pub history: OperationHistory,
    pub certificates: std::collections::BTreeMap<BodyId, super::physical::PhysicalCertificate>,
}

/// Transactional Euler editing context.
///
/// Each operator validates the construction topology before returning. The model
/// remains untouched unless final published-volume validation succeeds at commit.
pub struct EulerTransaction<'kernel> {
    kernel: &'kernel mut Kernel,
    working: Topology,
    names: NameSource,
    history: OperationHistory,
    touched_bodies: Vec<BodyId>,
    namespace: Uuid,
    composes_imported_topology: bool,
}

impl<'kernel> EulerTransaction<'kernel> {
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

    pub fn topology(&self) -> &Topology {
        &self.working
    }
    pub fn history(&self) -> &OperationHistory {
        &self.history
    }

    /// Make vertex, face, shell: the classical seed Euler operator.
    pub fn mvfs(&mut self, point: Point3, surface: Surface3) -> KernelResult<MvfsResult> {
        self.mvfs_oriented(point, surface, FaceOrientation::Forward)
    }

    pub fn mvfs_oriented(
        &mut self,
        point: Point3,
        surface: Surface3,
        orientation: FaceOrientation,
    ) -> KernelResult<MvfsResult> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self
            .mvfs_unchecked(point, surface, orientation)
            .and_then(|created| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| created)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn mvfs_unchecked(
        &mut self,
        point: Point3,
        surface: Surface3,
        orientation: FaceOrientation,
    ) -> KernelResult<MvfsResult> {
        if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "initial vertex must be finite",
            ));
        }
        let projection = surface.project(point, self.kernel.tolerance)?;
        if projection.distance
            > self
                .kernel
                .tolerance
                .linear_at_scale(point.to_vec().magnitude())
        {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
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
        let face = self.working.faces.insert(Face::new(
            face_name,
            shell,
            surface,
            orientation,
            self.kernel.tolerance.linear,
        ));
        self.working.shells.get_mut(shell)?.faces.push(face);
        let loop_id = self
            .working
            .loops
            .insert(Loop::new(loop_name, face, LoopKind::Outer));
        self.working.faces.get_mut(face)?.loops.push(loop_id);
        let vertex = self.working.vertices.insert(Vertex::new(
            vertex_name,
            point,
            self.kernel.tolerance.linear,
        ));

        for name in [body_name, shell_name, face_name, loop_name, vertex_name] {
            self.history.created(name);
        }
        self.touched_bodies.push(body);
        Ok(MvfsResult {
            body,
            shell,
            face,
            loop_id,
            vertex,
        })
    }

    /// Make edge and vertex by inserting an Euler spike into a face loop.
    ///
    /// `after` is absent for the first edge in the empty MVFS loop. Otherwise it
    /// identifies the coedge whose end vertex is `from`. Two exact p-curves are
    /// required because both uses can have different face parameterizations.
    pub fn mev(&mut self, input: MevInput) -> KernelResult<MevResult> {
        let MevInput {
            loop_id,
            after,
            from,
            point,
            curve,
            curve_range,
            forward_pcurve,
            reverse_pcurve,
            pcurve_range,
        } = input;
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let result = self
            .mev_unchecked(
                loop_id,
                after,
                from,
                point,
                curve,
                curve_range,
                forward_pcurve,
                reverse_pcurve,
                pcurve_range,
            )
            .and_then(|created| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| created)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn mev_unchecked(
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
        self.working.vertex(from)?;
        match (loop_first, after) {
            (None, None) => {}
            (Some(_), Some(after_id)) => {
                let after_coedge = self.working.coedge(after_id)?;
                if after_coedge.loop_id != loop_id {
                    return Err(KernelError::new(
                        DiagnosticKind::InvalidInput,
                        "insertion coedge belongs to another loop",
                    ));
                }
                let edge = self.working.edge(after_coedge.edge)?;
                let end = if after_coedge.forward {
                    edge.vertices[1]
                } else {
                    edge.vertices[0]
                };
                if end != from {
                    return Err(KernelError::new(
                        DiagnosticKind::InvalidInput,
                        "insertion coedge does not end at the source vertex",
                    ));
                }
            }
            (None, Some(_)) => {
                return Err(KernelError::new(
                    DiagnosticKind::InvalidInput,
                    "an empty loop has no insertion coedge",
                ))
            }
            (Some(_), None) => {
                return Err(KernelError::new(
                    DiagnosticKind::InvalidInput,
                    "a nonempty loop requires an insertion coedge",
                ))
            }
        }

        let source_name = self.working.vertex(from)?.name;
        let vertex_name = self.names.next_for("vertex")?;
        let edge_name = self.names.next_for("edge")?;
        let forward_name = self.names.next_for("coedge.forward")?;
        let reverse_name = self.names.next_for("coedge.reverse")?;
        let vertex = self.working.vertices.insert(Vertex::new(
            vertex_name,
            point,
            self.kernel.tolerance.linear,
        ));
        let edge = self.working.edges.insert(Edge::new(
            edge_name,
            [from, vertex],
            curve,
            curve_range,
            self.kernel.tolerance.linear,
        ));
        let forward = self.working.coedges.insert_with(|self_id| {
            Coedge::new(
                forward_name,
                edge,
                loop_id,
                true,
                forward_pcurve,
                pcurve_range,
                self_id,
            )
        });
        let reverse = self.working.coedges.insert_with(|self_id| {
            Coedge::new(
                reverse_name,
                edge,
                loop_id,
                false,
                reverse_pcurve,
                pcurve_range.reversed(),
                self_id,
            )
        });

        if let Some(after) = after {
            let next = self.working.coedge(after)?.next;
            self.working.coedges.get_mut(after)?.next = forward;
            {
                let value = self.working.coedges.get_mut(forward)?;
                value.previous = after;
                value.next = reverse;
            }
            {
                let value = self.working.coedges.get_mut(reverse)?;
                value.previous = forward;
                value.next = next;
            }
            self.working.coedges.get_mut(next)?.previous = reverse;
        } else {
            {
                let value = self.working.coedges.get_mut(forward)?;
                value.next = reverse;
                value.previous = reverse;
            }
            {
                let value = self.working.coedges.get_mut(reverse)?;
                value.next = forward;
                value.previous = forward;
            }
            self.working.loops.get_mut(loop_id)?.first = Some(forward);
        }
        self.working
            .edges
            .get_mut(edge)?
            .coedges
            .extend([forward, reverse]);
        if self.working.vertices.get(from)?.outgoing.is_none() {
            self.working.vertices.get_mut(from)?.outgoing = Some(forward);
        }
        self.working.vertices.get_mut(vertex)?.outgoing = Some(reverse);

        self.history
            .derived(vertex_name, [source_name], "mev.vertex");
        self.history
            .derived(edge_name, [source_name, vertex_name], "mev.edge");
        self.history
            .derived(forward_name, [edge_name], "mev.forward-use");
        self.history
            .derived(reverse_name, [edge_name], "mev.reverse-use");
        self.history
            .changed(source_name, ChangeKind::Modified, [source_name]);

        Ok(MevResult {
            edge,
            vertex,
            forward,
            reverse,
        })
    }

    /// Make edge and face by connecting the start vertices of two coedges in one loop.
    ///
    /// This is the classical MEF Euler operator. The original loop retains `first`;
    /// the other cycle becomes the outer loop of the newly created face.
    pub fn mef(&mut self, input: MefInput) -> KernelResult<MefResult> {
        let MefInput {
            first,
            second,
            curve,
            curve_range,
            new_face_surface,
            new_face_orientation,
            new_face_pcurve,
            old_face_pcurve,
            pcurve_range,
        } = input;
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let result = self
            .mef_unchecked(
                first,
                second,
                curve,
                curve_range,
                new_face_surface,
                new_face_orientation,
                new_face_pcurve,
                old_face_pcurve,
                pcurve_range,
            )
            .and_then(|created| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| created)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn mef_unchecked(
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
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "MEF requires two distinct coedges",
            ));
        }
        let first_use = self.working.coedge(first)?;
        let second_use = self.working.coedge(second)?;
        if first_use.loop_id != second_use.loop_id {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "MEF coedges must belong to one loop",
            ));
        }
        let old_loop = first_use.loop_id;
        let old_face = self.working.loop_(old_loop)?.face;
        let shell = self.working.face(old_face)?.shell;
        let [start, _] = self.working.coedge_vertices(first)?;
        let [end, _] = self.working.coedge_vertices(second)?;
        if start == end {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "MEF may not create a zero-length edge",
            ));
        }

        let edge_name = self.names.next_for("edge")?;
        let forward_name = self.names.next_for("coedge.forward")?;
        let reverse_name = self.names.next_for("coedge.reverse")?;
        let face_name = self.names.next_for("face")?;
        let loop_name = self.names.next_for("loop.outer")?;
        let new_face = self.working.faces.insert(Face::new(
            face_name,
            shell,
            new_face_surface,
            new_face_orientation,
            self.kernel.tolerance.linear,
        ));
        let new_loop = self
            .working
            .loops
            .insert(Loop::new(loop_name, new_face, LoopKind::Outer));
        self.working.faces.get_mut(new_face)?.loops.push(new_loop);
        self.working.shells.get_mut(shell)?.faces.push(new_face);

        let edge = self.working.edges.insert(Edge::new(
            edge_name,
            [start, end],
            curve,
            curve_range,
            self.kernel.tolerance.linear,
        ));
        let forward = self.working.coedges.insert_with(|self_id| {
            Coedge::new(
                forward_name,
                edge,
                new_loop,
                true,
                new_face_pcurve,
                pcurve_range,
                self_id,
            )
        });
        let reverse = self.working.coedges.insert_with(|self_id| {
            Coedge::new(
                reverse_name,
                edge,
                old_loop,
                false,
                old_face_pcurve,
                pcurve_range.reversed(),
                self_id,
            )
        });

        let previous_first = self.working.coedge(first)?.previous;
        let previous_second = self.working.coedge(second)?.previous;
        // Existing cycle: first ... previous_second -> reverse -> first.
        self.working.coedges.get_mut(previous_second)?.next = reverse;
        {
            let value = self.working.coedges.get_mut(reverse)?;
            value.previous = previous_second;
            value.next = first;
        }
        self.working.coedges.get_mut(first)?.previous = reverse;
        // New cycle: second ... previous_first -> forward -> second.
        self.working.coedges.get_mut(previous_first)?.next = forward;
        {
            let value = self.working.coedges.get_mut(forward)?;
            value.previous = previous_first;
            value.next = second;
        }
        self.working.coedges.get_mut(second)?.previous = forward;
        self.working.loops.get_mut(old_loop)?.first = Some(first);
        self.working.loops.get_mut(new_loop)?.first = Some(second);

        let mut current = second;
        loop {
            self.working.coedges.get_mut(current)?.loop_id = new_loop;
            if current == forward {
                break;
            }
            current = self.working.coedge(current)?.next;
            if current == second {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "MEF failed to form the new loop",
                ));
            }
        }
        self.working
            .edges
            .get_mut(edge)?
            .coedges
            .extend([forward, reverse]);

        let old_face_name = self.working.face(old_face)?.name;
        let start_name = self.working.vertex(start)?.name;
        let end_name = self.working.vertex(end)?.name;
        self.history
            .derived(edge_name, [start_name, end_name], "mef.edge");
        self.history
            .derived(forward_name, [edge_name], "mef.new-face-use");
        self.history
            .derived(reverse_name, [edge_name], "mef.old-face-use");
        self.history.derived(face_name, [old_face_name], "mef.face");
        self.history
            .derived(loop_name, [face_name], "mef.outer-loop");
        self.history
            .changed(old_face_name, ChangeKind::Split, [old_face_name, face_name]);

        Ok(MefResult {
            edge,
            face: new_face,
            loop_id: new_loop,
            forward,
            reverse,
        })
    }

    /// Kill edge and vertex, the exact topological inverse of MEV for an Euler spike.
    pub fn kev(&mut self, edge: EdgeId, vertex: VertexId) -> KernelResult<KevResult> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let result = self.kev_unchecked(edge, vertex).and_then(|changed| {
            validate(
                &self.working,
                self.kernel.tolerance,
                ValidationMode::Construction,
            )
            .into_result()
            .map(|_| changed)
        });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
        }
        result
    }

    fn kev_unchecked(&mut self, edge: EdgeId, vertex: VertexId) -> KernelResult<KevResult> {
        let edge_value = self.working.edge(edge)?;
        if !edge_value.vertices.contains(&vertex) || edge_value.coedges.len() != 2 {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KEV requires an endpoint and two coedges forming an Euler spike",
            ));
        }
        let incident = self
            .working
            .edges()
            .filter(|(_, candidate)| candidate.vertices.contains(&vertex))
            .count();
        if incident != 1 {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KEV can only remove a vertex incident to its one spike edge",
            ));
        }
        let forward = *edge_value
            .coedges
            .iter()
            .find(|id| {
                self.working
                    .coedge(**id)
                    .map(|coedge| coedge.forward)
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "edge has no forward coedge",
                )
            })?;
        let reverse = *edge_value
            .coedges
            .iter()
            .find(|id| {
                self.working
                    .coedge(**id)
                    .map(|coedge| !coedge.forward)
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "edge has no reverse coedge",
                )
            })?;
        let (first_use, second_use, surviving_vertex) = if edge_value.vertices[1] == vertex {
            (forward, reverse, edge_value.vertices[0])
        } else {
            (reverse, forward, edge_value.vertices[1])
        };
        let first_value = self.working.coedge(first_use)?;
        let second_value = self.working.coedge(second_use)?;
        if first_value.loop_id != second_value.loop_id || first_value.next != second_use {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KEV edge uses do not form a removable Euler spike",
            ));
        }
        let loop_id = first_value.loop_id;
        let previous = first_value.previous;
        let next = second_value.next;
        let only_spike = previous == second_use && next == first_use;
        let removed_names = [
            edge_value.name,
            self.working.vertex(vertex)?.name,
            first_value.name,
            second_value.name,
        ];

        if only_spike {
            self.working.loops.get_mut(loop_id)?.first = None;
        } else {
            self.working.coedges.get_mut(previous)?.next = next;
            self.working.coedges.get_mut(next)?.previous = previous;
            if matches!(self.working.loop_(loop_id)?.first, Some(id) if id == first_use || id == second_use)
            {
                self.working.loops.get_mut(loop_id)?.first = Some(next);
            }
        }
        self.working.coedges.remove(forward)?;
        self.working.coedges.remove(reverse)?;
        self.working.edges.remove(edge)?;
        self.working.vertices.remove(vertex)?;
        self.repair_outgoing(surviving_vertex)?;

        for name in removed_names {
            self.history
                .changed(name, ChangeKind::Deleted, std::iter::empty());
        }
        let survivor_name = self.working.vertex(surviving_vertex)?.name;
        self.history
            .changed(survivor_name, ChangeKind::Modified, [survivor_name]);
        Ok(KevResult {
            surviving_vertex,
            loop_id,
        })
    }

    /// Kill edge and face, the inverse of MEF for a face with one outer loop.
    pub fn kef(&mut self, edge: EdgeId, face_to_remove: FaceId) -> KernelResult<KefResult> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let result = self
            .kef_unchecked(edge, face_to_remove)
            .and_then(|changed| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| changed)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
        }
        result
    }

    fn kef_unchecked(&mut self, edge: EdgeId, face_to_remove: FaceId) -> KernelResult<KefResult> {
        let (edge_coedges, edge_name, vertices) = {
            let edge_value = self.working.edge(edge)?;
            (
                edge_value.coedges.clone(),
                edge_value.name,
                edge_value.vertices,
            )
        };
        if edge_coedges.len() != 2 {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KEF requires a two-sided edge",
            ));
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
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "basic KEF only removes a face with one outer loop; transfer holes first",
            ));
        }
        let remove_loop = remove_face_loops[0];
        let remove_use = *edge_coedges
            .iter()
            .find(|id| {
                self.working
                    .coedge(**id)
                    .map(|coedge| coedge.loop_id == remove_loop)
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::InvalidInput,
                    "edge is not incident to the face being removed",
                )
            })?;
        let keep_use = *edge_coedges
            .iter()
            .find(|id| **id != remove_use)
            .ok_or_else(|| {
                KernelError::new(DiagnosticKind::TopologyInvariant, "edge mate is missing")
            })?;
        let keep_loop = self.working.coedge(keep_use)?.loop_id;
        let keep_face = self.working.loop_(keep_loop)?.face;
        if keep_face == face_to_remove {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KEF cannot remove a seam edge from one face",
            ));
        }
        if self.working.face(keep_face)?.shell != shell {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KEF faces must belong to the same shell",
            ));
        }

        let keep_previous = self.working.coedge(keep_use)?.previous;
        let keep_next = self.working.coedge(keep_use)?.next;
        let remove_previous = self.working.coedge(remove_use)?.previous;
        let remove_next = self.working.coedge(remove_use)?.next;
        if keep_previous == keep_use || remove_previous == remove_use {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KEF would create an empty merged loop",
            ));
        }
        self.working.coedges.get_mut(keep_previous)?.next = remove_next;
        self.working.coedges.get_mut(remove_next)?.previous = keep_previous;
        self.working.coedges.get_mut(remove_previous)?.next = keep_next;
        self.working.coedges.get_mut(keep_next)?.previous = remove_previous;
        self.working.loops.get_mut(keep_loop)?.first = Some(keep_next);

        let mut current = keep_next;
        loop {
            self.working.coedges.get_mut(current)?.loop_id = keep_loop;
            current = self.working.coedge(current)?.next;
            if current == keep_next {
                break;
            }
            if current == keep_use || current == remove_use {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "KEF splice retained a removed coedge",
                ));
            }
        }

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
        for vertex in vertices {
            self.repair_outgoing(vertex)?;
        }

        for name in [
            edge_name,
            remove_face_name,
            remove_loop_name,
            use_names[0],
            use_names[1],
        ] {
            self.history
                .changed(name, ChangeKind::Deleted, std::iter::empty());
        }
        self.history
            .changed(keep_face_name, ChangeKind::Merged, [keep_face_name]);
        Ok(KefResult {
            surviving_face: keep_face,
            loop_id: keep_loop,
        })
    }

    fn repair_outgoing(&mut self, vertex: VertexId) -> KernelResult<()> {
        let replacement = self.working.coedges().find_map(|(id, _)| {
            self.working
                .coedge_vertices(id)
                .ok()
                .and_then(|vertices| (vertices[0] == vertex).then_some(id))
        });
        self.working.vertices.get_mut(vertex)?.outgoing = replacement;
        Ok(())
    }

    fn rename_vertex(&mut self, id: VertexId, name: super::PersistentId) -> KernelResult<()> {
        let old = self.working.vertex(id)?.name;
        self.working.vertices.get_mut(id)?.name = name;
        self.history.rename_entity(old, name);
        Ok(())
    }

    fn rename_edge(&mut self, id: EdgeId, name: super::PersistentId) -> KernelResult<()> {
        let old = self.working.edge(id)?.name;
        self.working.edges.get_mut(id)?.name = name;
        self.history.rename_entity(old, name);
        Ok(())
    }

    fn rename_coedge(&mut self, id: CoedgeId, name: super::PersistentId) -> KernelResult<()> {
        let old = self.working.coedge(id)?.name;
        self.working.coedges.get_mut(id)?.name = name;
        self.history.rename_entity(old, name);
        Ok(())
    }

    fn rename_face(&mut self, id: FaceId, name: super::PersistentId) -> KernelResult<()> {
        let old = self.working.face(id)?.name;
        self.working.faces.get_mut(id)?.name = name;
        self.history.rename_entity(old, name);
        Ok(())
    }

    fn rename_loop(&mut self, id: LoopId, name: super::PersistentId) -> KernelResult<()> {
        let old = self.working.loop_(id)?.name;
        self.working.loops.get_mut(id)?.name = name;
        self.history.rename_entity(old, name);
        Ok(())
    }

    fn name_mev(
        &mut self,
        result: MevResult,
        vertex_key: &str,
        edge_key: &str,
    ) -> KernelResult<()> {
        let vertex_name = self.names.named("vertex", vertex_key)?;
        let edge_name = self.names.named("edge", edge_key)?;
        let forward_name = self.names.named("coedge.forward", edge_key)?;
        let reverse_name = self.names.named("coedge.reverse", edge_key)?;
        self.rename_vertex(result.vertex, vertex_name)?;
        self.rename_edge(result.edge, edge_name)?;
        self.rename_coedge(result.forward, forward_name)?;
        self.rename_coedge(result.reverse, reverse_name)
    }

    fn name_mef(
        &mut self,
        result: MefResult,
        edge_key: &str,
        face_key: Option<&str>,
    ) -> KernelResult<()> {
        let edge_name = self.names.named("edge", edge_key)?;
        let forward_name = self.names.named("coedge.forward", edge_key)?;
        let reverse_name = self.names.named("coedge.reverse", edge_key)?;
        self.rename_edge(result.edge, edge_name)?;
        self.rename_coedge(result.forward, forward_name)?;
        self.rename_coedge(result.reverse, reverse_name)?;
        if let Some(face_key) = face_key {
            let face_name = self.names.named("face", face_key)?;
            let loop_name = self.names.named("loop.outer", face_key)?;
            self.rename_face(result.face, face_name)?;
            self.rename_loop(result.loop_id, loop_name)?;
        }
        Ok(())
    }

    /// Kill edge, make ring: remove a bridge edge and split one loop into an outer
    /// loop and a new inner loop on the same face.
    ///
    /// `inner_use` selects which removed coedge's successor begins the hole cycle.
    pub fn kemr(&mut self, edge: EdgeId, inner_use: CoedgeId) -> KernelResult<KemrResult> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let result = self.kemr_unchecked(edge, inner_use).and_then(|changed| {
            validate(
                &self.working,
                self.kernel.tolerance,
                ValidationMode::Construction,
            )
            .into_result()
            .map(|_| changed)
        });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
        }
        result
    }

    fn kemr_unchecked(&mut self, edge: EdgeId, inner_use: CoedgeId) -> KernelResult<KemrResult> {
        let (uses, edge_name, vertices) = {
            let edge = self.working.edge(edge)?;
            (edge.coedges.clone(), edge.name, edge.vertices)
        };
        if uses.len() != 2 || !uses.contains(&inner_use) {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KEMR requires one of the two coedges of a two-sided edge",
            ));
        }
        let other_use = uses
            .iter()
            .copied()
            .find(|id| *id != inner_use)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "KEMR edge mate is missing",
                )
            })?;
        let inner = self.working.coedge(inner_use)?;
        let other = self.working.coedge(other_use)?;
        if inner.loop_id != other.loop_id {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KEMR edge uses must belong to one loop",
            ));
        }
        if inner.next == other_use || other.next == inner_use {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "use KEV for a spike edge",
            ));
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
        self.working.face(face)?;
        self.working.faces.get_mut(face)?.loops.push(inner_loop);

        // Cycle selected as inner: other_previous -> inner_next.
        self.working.coedges.get_mut(other_previous)?.next = inner_next;
        self.working.coedges.get_mut(inner_next)?.previous = other_previous;
        // Remaining outer cycle: inner_previous -> other_next.
        self.working.coedges.get_mut(inner_previous)?.next = other_next;
        self.working.coedges.get_mut(other_next)?.previous = inner_previous;
        self.working.loops.get_mut(outer_loop)?.first = Some(other_next);
        self.working.loops.get_mut(inner_loop)?.first = Some(inner_next);
        self.assign_cycle(inner_loop, inner_next)?;
        self.assign_cycle(outer_loop, other_next)?;

        self.working.coedges.remove(inner_use)?;
        self.working.coedges.remove(other_use)?;
        self.working.edges.remove(edge)?;
        for vertex in vertices {
            self.repair_outgoing(vertex)?;
        }

        self.history.derived(
            loop_name,
            [self.working.face(face)?.name],
            "kemr.inner-loop",
        );
        for name in [edge_name, removed_use_names[0], removed_use_names[1]] {
            self.history
                .changed(name, ChangeKind::Deleted, std::iter::empty());
        }
        let face_name = self.working.face(face)?.name;
        self.history
            .changed(face_name, ChangeKind::Modified, [face_name]);
        Ok(KemrResult {
            face,
            outer_loop,
            inner_loop,
        })
    }

    /// Make edge, kill ring: bridge an inner loop to another loop on the same face.
    #[allow(clippy::too_many_arguments)]
    pub fn mekr(
        &mut self,
        outer: CoedgeId,
        inner: CoedgeId,
        curve: Curve3,
        curve_range: ParameterRange,
        forward_pcurve: Curve2,
        reverse_pcurve: Curve2,
        pcurve_range: ParameterRange,
    ) -> KernelResult<MekrResult> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let result = self
            .mekr_unchecked(
                outer,
                inner,
                curve,
                curve_range,
                forward_pcurve,
                reverse_pcurve,
                pcurve_range,
            )
            .and_then(|changed| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| changed)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn mekr_unchecked(
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
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "MEKR requires two distinct loops",
            ));
        }
        let face = self.working.loop_(outer_loop)?.face;
        if self.working.loop_(inner_loop)?.face != face
            || self.working.loop_(inner_loop)?.kind != LoopKind::Inner
        {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "MEKR loops must belong to the same face and the removed loop must be inner",
            ));
        }
        let [start, _] = self.working.coedge_vertices(outer)?;
        let [end, _] = self.working.coedge_vertices(inner)?;
        if start == end {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "MEKR may not create a zero-length bridge",
            ));
        }
        let outer_previous = self.working.coedge(outer)?.previous;
        let inner_previous = self.working.coedge(inner)?.previous;
        let edge_name = self.names.next_for("edge")?;
        let forward_name = self.names.next_for("coedge.forward")?;
        let reverse_name = self.names.next_for("coedge.reverse")?;
        let edge = self.working.edges.insert(Edge::new(
            edge_name,
            [start, end],
            curve,
            curve_range,
            self.kernel.tolerance.linear,
        ));
        let forward = self.working.coedges.insert_with(|self_id| {
            Coedge::new(
                forward_name,
                edge,
                outer_loop,
                true,
                forward_pcurve,
                pcurve_range,
                self_id,
            )
        });
        let reverse = self.working.coedges.insert_with(|self_id| {
            Coedge::new(
                reverse_name,
                edge,
                outer_loop,
                false,
                reverse_pcurve,
                pcurve_range.reversed(),
                self_id,
            )
        });

        self.working.coedges.get_mut(outer_previous)?.next = forward;
        {
            let value = self.working.coedges.get_mut(forward)?;
            value.previous = outer_previous;
            value.next = inner;
        }
        self.working.coedges.get_mut(inner)?.previous = forward;
        self.working.coedges.get_mut(inner_previous)?.next = reverse;
        {
            let value = self.working.coedges.get_mut(reverse)?;
            value.previous = inner_previous;
            value.next = outer;
        }
        self.working.coedges.get_mut(outer)?.previous = reverse;
        self.working
            .edges
            .get_mut(edge)?
            .coedges
            .extend([forward, reverse]);
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
            .derived(edge_name, [face_name], "mekr.bridge-edge");
        self.history
            .derived(forward_name, [edge_name], "mekr.forward-use");
        self.history
            .derived(reverse_name, [edge_name], "mekr.reverse-use");
        self.history
            .changed(inner_loop_name, ChangeKind::Deleted, std::iter::empty());
        self.history
            .changed(face_name, ChangeKind::Modified, [face_name]);
        Ok(MekrResult {
            edge,
            loop_id: outer_loop,
            forward,
            reverse,
        })
    }

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
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "coedge cycle does not close",
                ));
            }
        }
    }

    /// Kill face, make ring hole: transfer a simple face's outer loop into another
    /// face as an inner loop. Geometric validation guarantees the transferred
    /// p-curves actually lie on the surviving face's surface.
    pub fn kfmrh(
        &mut self,
        face_to_keep: FaceId,
        face_to_remove: FaceId,
    ) -> KernelResult<KfmrhResult> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let result = self
            .kfmrh_unchecked(face_to_keep, face_to_remove)
            .and_then(|changed| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| changed)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
        }
        result
    }

    fn kfmrh_unchecked(
        &mut self,
        face_to_keep: FaceId,
        face_to_remove: FaceId,
    ) -> KernelResult<KfmrhResult> {
        if face_to_keep == face_to_remove {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "KFMRH requires two faces",
            ));
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
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
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
        Ok(KfmrhResult {
            face: face_to_keep,
            inner_loop,
        })
    }

    /// Make face, kill ring hole: promote an inner loop to the outer loop of a new
    /// face in the same shell. This is the inverse of KFMRH.
    pub fn mfkrh(
        &mut self,
        inner_loop: LoopId,
        surface: Surface3,
        orientation: FaceOrientation,
    ) -> KernelResult<MfkrhResult> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let result = self
            .mfkrh_unchecked(inner_loop, surface, orientation)
            .and_then(|changed| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| changed)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
        }
        result
    }

    fn mfkrh_unchecked(
        &mut self,
        inner_loop: LoopId,
        surface: Surface3,
        orientation: FaceOrientation,
    ) -> KernelResult<MfkrhResult> {
        let (old_face, loop_name, kind) = {
            let loop_value = self.working.loop_(inner_loop)?;
            (loop_value.face, loop_value.name, loop_value.kind)
        };
        if kind != LoopKind::Inner {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "MFKRH requires an inner loop",
            ));
        }
        let shell = self.working.face(old_face)?.shell;
        let old_face_name = self.working.face(old_face)?.name;
        let face_name = self.names.next_for("face")?;
        let face = self.working.faces.insert(Face::new(
            face_name,
            shell,
            surface,
            orientation,
            self.kernel.tolerance.linear,
        ));
        self.working.faces.get_mut(face)?.loops.push(inner_loop);
        self.working.shells.get_mut(shell)?.faces.push(face);
        self.working
            .faces
            .get_mut(old_face)?
            .loops
            .retain(|id| *id != inner_loop);
        {
            let loop_value = self.working.loops.get_mut(inner_loop)?;
            loop_value.face = face;
            loop_value.kind = LoopKind::Outer;
        }
        self.history
            .derived(face_name, [old_face_name, loop_name], "mfkrh.face");
        self.history
            .changed(loop_name, ChangeKind::Modified, [loop_name]);
        self.history
            .changed(old_face_name, ChangeKind::Modified, [old_face_name]);
        Ok(MfkrhResult {
            face,
            outer_loop: inner_loop,
        })
    }

    /// Declare an open construction shell topologically closed.
    ///
    /// This does not bypass publication checks: commit still proves p-curve/edge
    /// consistency and physical-volume validity for the complete body.
    pub fn seal_shell(&mut self, shell: ShellId) -> KernelResult<()> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self.seal_shell_unchecked(shell).and_then(|_| {
            validate(
                &self.working,
                self.kernel.tolerance,
                ValidationMode::Construction,
            )
            .into_result()
        });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn seal_shell_unchecked(&mut self, shell: ShellId) -> KernelResult<()> {
        let (shell_name, face_ids, body) = {
            let shell_value = self.working.shell(shell)?;
            (
                shell_value.name,
                shell_value.faces.clone(),
                shell_value.body,
            )
        };
        if face_ids.is_empty() {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "a closed shell needs at least one face",
            ));
        }
        for face_id in &face_ids {
            let face = self.working.face(*face_id)?;
            if face.loops.is_empty()
                || face.loops.iter().any(|loop_id| {
                    self.working
                        .loop_(*loop_id)
                        .map(|loop_| loop_.first.is_none())
                        .unwrap_or(true)
                })
            {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "every face of a closed shell needs nonempty loops",
                ));
            }
        }
        let face_set: std::collections::BTreeSet<_> = face_ids.iter().copied().collect();
        let mut edge_ids = std::collections::BTreeSet::new();
        for face_id in &face_ids {
            for loop_id in &self.working.face(*face_id)?.loops {
                let first = self.working.loop_(*loop_id)?.first.ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "closed shell contains an empty loop",
                    )
                })?;
                let mut current = first;
                loop {
                    edge_ids.insert(self.working.coedge(current)?.edge);
                    current = self.working.coedge(current)?.next;
                    if current == first {
                        break;
                    }
                }
            }
        }
        for edge_id in edge_ids {
            let edge = self.working.edge(edge_id)?;
            if edge.coedges.len() != 2 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "every edge of a closed shell must have exactly two coedges",
                ));
            }
            for coedge_id in &edge.coedges {
                let loop_id = self.working.coedge(*coedge_id)?.loop_id;
                if !face_set.contains(&self.working.loop_(loop_id)?.face) {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "a closed shell edge may not be shared with a face outside the shell",
                    ));
                }
            }
        }
        self.working.shells.get_mut(shell)?.kind = ShellKind::Closed;
        if !self.touched_bodies.contains(&body) {
            self.touched_bodies.push(body);
        }
        self.history
            .changed(shell_name, ChangeKind::Modified, [shell_name]);
        Ok(())
    }

    /// Move the sole shell of `source` into `target` and remove the empty body.
    ///
    /// This is the body/shell Euler operator used to add a disconnected cavity
    /// boundary to a solid. Its inverse is [`Self::split_shell_from_body`].
    pub fn merge_shell_into_body(
        &mut self,
        target: BodyId,
        source: BodyId,
    ) -> KernelResult<ShellId> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self
            .merge_shell_into_body_unchecked(target, source)
            .and_then(|shell| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| shell)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn merge_shell_into_body_unchecked(
        &mut self,
        target: BodyId,
        source: BodyId,
    ) -> KernelResult<ShellId> {
        if target == source {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "body/shell merge requires distinct bodies",
            ));
        }
        let target_name = self.working.body(target)?.name;
        let source_value = self.working.body(source)?;
        if source_value.shells.len() != 1 {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
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
        if !self.touched_bodies.contains(&target) {
            self.touched_bodies.push(target);
        }
        self.history
            .changed(target_name, ChangeKind::Modified, [target_name]);
        self.history
            .changed(shell_name, ChangeKind::Modified, [shell_name]);
        self.history
            .changed(source_name, ChangeKind::Deleted, std::iter::empty());
        Ok(shell)
    }

    /// Sew two closed bodies across a geometrically coincident face pair.
    ///
    /// This is the high-level Euler composition used by an adjacent union: the
    /// two material-opposed faces are removed, their duplicate boundary graph
    /// is welded, and the remaining shells become one closed shell atomically.
    pub fn sew_bodies_across_faces(
        &mut self,
        target: BodyId,
        source: BodyId,
        target_face: FaceId,
        source_face: FaceId,
    ) -> KernelResult<BodyId> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let composition_snapshot = self.composes_imported_topology;
        let result = self
            .sew_bodies_across_faces_unchecked(target, source, target_face, source_face)
            .and_then(|body| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| body)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.touched_bodies = touched_snapshot;
            self.composes_imported_topology = composition_snapshot;
        }
        result
    }

    fn sew_bodies_across_faces_unchecked(
        &mut self,
        target: BodyId,
        source: BodyId,
        target_face: FaceId,
        source_face: FaceId,
    ) -> KernelResult<BodyId> {
        if target == source {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "face sewing requires two distinct bodies",
            ));
        }
        let target_shells = self.working.body(target)?.shells.clone();
        let source_shells = self.working.body(source)?.shells.clone();
        if target_shells.len() != 1 || source_shells.len() != 1 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "adjacent face sewing currently requires one shell per body",
            ));
        }
        let target_shell = target_shells[0];
        let source_shell = source_shells[0];
        if self.working.face(target_face)?.shell != target_shell
            || self.working.face(source_face)?.shell != source_shell
        {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "sewn faces must belong to their specified bodies",
            ));
        }
        let target_loops = self.working.face(target_face)?.loops.clone();
        let source_loops = self.working.face(source_face)?.loops.clone();
        if target_loops.is_empty() || source_loops.len() < target_loops.len() {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "adjacent face sewing requires every target loop to occur on the source patch",
            ));
        }
        let linear = self.kernel.tolerance.linear;
        let loops_match = |first: LoopId, second: LoopId| -> KernelResult<bool> {
            let first_boundary = self.collect_loop(first)?;
            let second_boundary = self.collect_loop(second)?;
            if first_boundary.len() != second_boundary.len() {
                return Ok(false);
            }
            let mut used = std::collections::BTreeSet::new();
            for first_use in first_boundary {
                let first_edge = self.working.coedge(first_use)?.edge;
                let first_value = self.working.edge(first_edge)?;
                let first_points = [
                    self.working.vertex(first_value.vertices[0])?.point,
                    self.working.vertex(first_value.vertices[1])?.point,
                ];
                let first_middle = first_value
                    .curve
                    .evaluate(first_value.range.start + first_value.range.span() * 0.5)?
                    .point;
                let Some(second_edge) = second_boundary.iter().find_map(|second_use| {
                    let edge = self.working.coedge(*second_use).ok()?.edge;
                    if used.contains(&edge) {
                        return None;
                    }
                    let value = self.working.edge(edge).ok()?;
                    let second_points = [
                        self.working.vertex(value.vertices[0]).ok()?.point,
                        self.working.vertex(value.vertices[1]).ok()?.point,
                    ];
                    let middle = value
                        .curve
                        .evaluate(value.range.start + value.range.span() * 0.5)
                        .ok()?
                        .point;
                    (first_middle.distance(middle) <= linear
                        && ((first_points[0].distance(second_points[0]) <= linear
                            && first_points[1].distance(second_points[1]) <= linear)
                            || (first_points[0].distance(second_points[1]) <= linear
                                && first_points[1].distance(second_points[0]) <= linear)))
                        .then_some(edge)
                }) else {
                    return Ok(false);
                };
                used.insert(second_edge);
            }
            Ok(true)
        };
        let mut used_source_loops = std::collections::BTreeSet::new();
        let mut loop_pairs = Vec::new();
        for target_loop in &target_loops {
            let mut matched = None;
            for source_loop in &source_loops {
                if !used_source_loops.contains(source_loop)
                    && loops_match(*target_loop, *source_loop)?
                {
                    matched = Some(*source_loop);
                    break;
                }
            }
            let Some(source_loop) = matched else {
                return Err(KernelError::new(
                    DiagnosticKind::InvalidInput,
                    "coincident sewn face patches do not have matching boundary loops",
                ));
            };
            used_source_loops.insert(source_loop);
            loop_pairs.push((*target_loop, source_loop));
        }
        let source_inner_loops = source_loops
            .iter()
            .copied()
            .filter(|loop_id| !used_source_loops.contains(loop_id))
            .collect::<Vec<_>>();
        if source_inner_loops.iter().any(|loop_id| {
            self.working
                .loop_(*loop_id)
                .map(|loop_value| loop_value.kind != LoopKind::Inner)
                .unwrap_or(true)
        }) {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "only unmatched source inner loops can survive face-patch sewing",
            ));
        }
        let (patch_surface, patch_orientation) = {
            let face = self.working.face(target_face)?;
            (face.surface.clone(), face.orientation)
        };
        for loop_id in &source_inner_loops {
            let patch = self.mfkrh_unchecked(*loop_id, patch_surface.clone(), patch_orientation)?;
            self.reparameterize_planar_face(patch.face)?;
        }
        let mut target_boundary = Vec::new();
        let mut source_boundary = Vec::new();
        for (target_loop, source_loop) in &loop_pairs {
            target_boundary.extend(self.collect_loop(*target_loop)?);
            source_boundary.extend(self.collect_loop(*source_loop)?);
        }

        let mut used_target_edges = std::collections::BTreeSet::new();
        let mut vertex_welds = std::collections::BTreeMap::<VertexId, VertexId>::new();
        let mut edge_welds = Vec::new();
        for source_use in &source_boundary {
            let source_edge = self.working.coedge(*source_use)?.edge;
            let source_vertices = self.working.edge(source_edge)?.vertices;
            let source_points = [
                self.working.vertex(source_vertices[0])?.point,
                self.working.vertex(source_vertices[1])?.point,
            ];
            let target_use = target_boundary
                .iter()
                .copied()
                .find(|candidate| {
                    let Ok(edge) = self.working.coedge(*candidate).map(|coedge| coedge.edge) else {
                        return false;
                    };
                    if used_target_edges.contains(&edge) {
                        return false;
                    }
                    let Ok(vertices) = self.working.edge(edge).map(|edge| edge.vertices) else {
                        return false;
                    };
                    let Ok(first) = self.working.vertex(vertices[0]).map(|vertex| vertex.point)
                    else {
                        return false;
                    };
                    let Ok(second) = self.working.vertex(vertices[1]).map(|vertex| vertex.point)
                    else {
                        return false;
                    };
                    let Ok(target_midpoint) = self
                        .working
                        .edge(edge)
                        .and_then(|edge| {
                            edge.curve
                                .evaluate(edge.range.start + edge.range.span() * 0.5)
                        })
                        .map(|evaluation| evaluation.point)
                    else {
                        return false;
                    };
                    let Ok(source_midpoint) = self
                        .working
                        .edge(source_edge)
                        .and_then(|edge| {
                            edge.curve
                                .evaluate(edge.range.start + edge.range.span() * 0.5)
                        })
                        .map(|evaluation| evaluation.point)
                    else {
                        return false;
                    };
                    target_midpoint.distance(source_midpoint) <= linear
                        && ((first.distance(source_points[0]) <= linear
                            && second.distance(source_points[1]) <= linear)
                            || (first.distance(source_points[1]) <= linear
                                && second.distance(source_points[0]) <= linear))
                })
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::InvalidInput,
                        "coincident sewn faces do not have matching boundary edges",
                    )
                })?;
            let target_edge = self.working.coedge(target_use)?.edge;
            used_target_edges.insert(target_edge);
            let target_vertices = self.working.edge(target_edge)?.vertices;
            for source_vertex in source_vertices {
                let point = self.working.vertex(source_vertex)?.point;
                let target_vertex = target_vertices
                    .iter()
                    .copied()
                    .find(|candidate| {
                        self.working
                            .vertex(*candidate)
                            .map(|vertex| vertex.point.distance(point) <= linear)
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::InvalidInput,
                            "coincident boundary vertices do not match",
                        )
                    })?;
                vertex_welds.insert(source_vertex, target_vertex);
            }
            let target_mate = self
                .working
                .edge(target_edge)?
                .coedges
                .iter()
                .copied()
                .find(|coedge| *coedge != target_use)
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "target sewn edge has no mate",
                    )
                })?;
            let source_mate = self
                .working
                .edge(source_edge)?
                .coedges
                .iter()
                .copied()
                .find(|coedge| *coedge != *source_use)
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "source sewn edge has no mate",
                    )
                })?;
            edge_welds.push((target_edge, target_mate, source_edge, source_mate));
        }

        let all_edges = self.working.edges().map(|(id, _)| id).collect::<Vec<_>>();
        for edge in all_edges {
            let vertices = self.working.edge(edge)?.vertices;
            self.working.edges.get_mut(edge)?.vertices = [
                vertex_welds
                    .get(&vertices[0])
                    .copied()
                    .unwrap_or(vertices[0]),
                vertex_welds
                    .get(&vertices[1])
                    .copied()
                    .unwrap_or(vertices[1]),
            ];
        }
        for (target_edge, target_mate, source_edge, source_mate) in &edge_welds {
            let target_vertices = self.working.edge(*target_edge)?.vertices;
            let source_vertices = self.working.edge(*source_edge)?.vertices;
            let circular = matches!(self.working.edge(*target_edge)?.curve, Curve3::Circle(_))
                && matches!(self.working.edge(*source_edge)?.curve, Curve3::Circle(_));
            let source_mate_face = self
                .working
                .loop_(self.working.coedge(*source_mate)?.loop_id)?
                .face;
            let cylindrical = circular
                && matches!(
                    self.working.face(source_mate_face)?.surface,
                    Surface3::Cylinder(_)
                );
            let forward = if cylindrical {
                !self.working.coedge(*target_mate)?.forward
            } else {
                let source_start = if self.working.coedge(*source_mate)?.forward {
                    source_vertices[0]
                } else {
                    source_vertices[1]
                };
                source_start == target_vertices[0]
            };
            let replacement_pcurve = cylindrical
                .then(|| self.pcurve_for_welded_circular_edge(*target_edge, *source_mate))
                .transpose()?;
            let source_name = self.working.edge(*source_edge)?.name;
            {
                let target_range = self.working.edge(*target_edge)?.range;
                let source_use = self.working.coedges.get_mut(*source_mate)?;
                source_use.edge = *target_edge;
                source_use.forward = forward;
                if let Some(pcurve) = replacement_pcurve {
                    source_use.pcurve = pcurve;
                    source_use.range = if forward {
                        target_range
                    } else {
                        target_range.reversed()
                    };
                }
            }
            self.working.edges.get_mut(*target_edge)?.coedges = vec![*target_mate, *source_mate];
            self.working.edges.remove(*source_edge)?;
            self.history.changed(
                source_name,
                ChangeKind::Merged,
                [self.working.edge(*target_edge)?.name],
            );
        }

        let removed_coedges = target_boundary
            .iter()
            .chain(&source_boundary)
            .copied()
            .collect::<Vec<_>>();
        let removed_names = removed_coedges
            .iter()
            .map(|coedge| self.working.coedge(*coedge).map(|value| value.name))
            .collect::<KernelResult<Vec<_>>>()?;
        for coedge in removed_coedges {
            self.working.coedges.remove(coedge)?;
        }
        let removed_loop_names = loop_pairs
            .iter()
            .flat_map(|(target_loop, source_loop)| [*target_loop, *source_loop])
            .map(|loop_id| {
                self.working
                    .loop_(loop_id)
                    .map(|loop_value| loop_value.name)
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let target_face_name = self.working.face(target_face)?.name;
        let source_face_name = self.working.face(source_face)?.name;
        for (target_loop, source_loop) in &loop_pairs {
            self.working.loops.remove(*target_loop)?;
            self.working.loops.remove(*source_loop)?;
        }
        self.working.faces.remove(target_face)?;
        self.working
            .shells
            .get_mut(target_shell)?
            .faces
            .retain(|face| *face != target_face);
        self.working.faces.remove(source_face)?;
        let source_faces = self.working.shell(source_shell)?.faces.clone();
        for face in source_faces.into_iter().filter(|face| *face != source_face) {
            self.working.faces.get_mut(face)?.shell = target_shell;
            self.working.shells.get_mut(target_shell)?.faces.push(face);
        }
        let source_shell_name = self.working.shell(source_shell)?.name;
        let source_body_name = self.working.body(source)?.name;
        self.working.shells.remove(source_shell)?;
        self.working.bodies.remove(source)?;

        for (edge, _, _, source_mate) in &edge_welds {
            let source_loop = self.working.coedge(*source_mate)?.loop_id;
            let source_face = self.working.loop_(source_loop)?.face;
            let adjacent = self.working.edge(*edge)?.coedges.clone();
            if adjacent.len() != 2 {
                continue;
            }
            let first_face = self
                .working
                .loop_(self.working.coedge(adjacent[0])?.loop_id)?
                .face;
            let second_face = self
                .working
                .loop_(self.working.coedge(adjacent[1])?.loop_id)?
                .face;
            if first_face == second_face {
                continue;
            }
            let (same_surface, reparameterize) = {
                let first = self.working.face(first_face)?;
                let second = self.working.face(second_face)?;
                let same_orientation = first.orientation == second.orientation;
                match (&first.surface, &second.surface) {
                    (Surface3::Plane(first_plane), Surface3::Plane(second_plane)) => {
                        let normals_parallel = first_plane
                            .normal()
                            .cross(second_plane.normal())
                            .magnitude()
                            <= self.kernel.tolerance.angular;
                        let coplanar = (second_plane.origin() - first_plane.origin())
                            .dot(first_plane.normal())
                            .abs()
                            <= self.kernel.tolerance.linear;
                        (same_orientation && normals_parallel && coplanar, true)
                    }
                    (Surface3::Cylinder(first_cylinder), Surface3::Cylinder(second_cylinder)) => (
                        same_orientation
                            && self.coincident_cylinders(*first_cylinder, *second_cylinder),
                        true,
                    ),
                    _ => (first.surface == second.surface && same_orientation, false),
                }
            };
            if same_surface {
                let remaining_face = if source_face == first_face {
                    second_face
                } else {
                    first_face
                };
                self.kef_unchecked(*edge, source_face)?;
                if reparameterize {
                    match self.working.face(remaining_face)?.surface {
                        Surface3::Plane(_) => self.reparameterize_planar_face(remaining_face)?,
                        Surface3::Cylinder(_) => {
                            self.reparameterize_cylindrical_face(remaining_face)?
                        }
                        _ => {}
                    }
                }
            }
        }

        for (source_vertex, target_vertex) in vertex_welds {
            let source_name = self.working.vertex(source_vertex)?.name;
            let target_name = self.working.vertex(target_vertex)?.name;
            self.working.vertices.remove(source_vertex)?;
            self.repair_outgoing(target_vertex)?;
            self.history
                .changed(source_name, ChangeKind::Merged, [target_name]);
        }
        let mut deleted_names = removed_loop_names;
        deleted_names.extend([
            target_face_name,
            source_face_name,
            source_shell_name,
            source_body_name,
        ]);
        for name in removed_names.into_iter().chain(deleted_names) {
            self.history
                .changed(name, ChangeKind::Deleted, std::iter::empty());
        }
        let target_body_name = self.working.body(target)?.name;
        let target_shell_name = self.working.shell(target_shell)?.name;
        self.history
            .changed(target_body_name, ChangeKind::Modified, [target_body_name]);
        self.history
            .changed(target_shell_name, ChangeKind::Modified, [target_shell_name]);
        self.touched_bodies.retain(|body| *body != source);
        if !self.touched_bodies.contains(&target) {
            self.touched_bodies.push(target);
        }
        self.composes_imported_topology = true;
        Ok(target)
    }

    /// Re-express a circular edge in the parameter space of the face that
    /// survives sewing. Endpoint matching is insufficient for circles because
    /// the discarded edge may use a different angular frame even when both
    /// curves describe the same geometric arc.
    fn pcurve_for_welded_circular_edge(
        &self,
        edge_id: EdgeId,
        coedge_id: CoedgeId,
    ) -> KernelResult<Curve2> {
        let edge = self.working.edge(edge_id)?;
        let Curve3::Circle(_) = &edge.curve else {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "circular edge reparameterization requires a circular edge",
            ));
        };
        let loop_id = self.working.coedge(coedge_id)?.loop_id;
        let face_id = self.working.loop_(loop_id)?.face;
        let surface = &self.working.face(face_id)?.surface;
        let Surface3::Cylinder(_) = surface else {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "circular edge reparameterization requires a cylindrical adjacent face",
            ));
        };

        let parameter = edge.range.start;
        let curve_evaluation = edge.curve.evaluate(parameter)?;
        let projection = surface.project(curve_evaluation.point, self.kernel.tolerance)?;
        let surface_evaluation =
            surface.evaluate(projection.parameters.0, projection.parameters.1)?;
        let direction = if curve_evaluation.first_derivative.dot(surface_evaluation.du) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let origin = Point2::new(
            projection.parameters.0 - direction * parameter,
            projection.parameters.1,
        );
        Ok(Line2::new(origin, Vec2::new(direction, 0.0))?.into())
    }

    fn coincident_cylinders(&self, first: Cylinder3, second: Cylinder3) -> bool {
        let first_axis = first.axis().normalize();
        let second_axis = second.axis().normalize();
        let axes_parallel =
            first_axis.cross(second_axis).magnitude() <= self.kernel.tolerance.angular;
        let origins_coaxial = (second.origin() - first.origin())
            .cross(first_axis)
            .magnitude()
            <= self.kernel.tolerance.linear;
        let radii_equal = (first.x_radius().magnitude() - second.x_radius().magnitude()).abs()
            <= self.kernel.tolerance.linear
            && (first.y_radius().magnitude() - second.y_radius().magnitude()).abs()
                <= self.kernel.tolerance.linear;
        axes_parallel && origins_coaxial && radii_equal
    }

    fn reparameterize_cylindrical_face(&mut self, face_id: FaceId) -> KernelResult<()> {
        let surface = self.working.face(face_id)?.surface.clone();
        let Surface3::Cylinder(_) = surface else {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "cylindrical reparameterization requires a cylindrical face",
            ));
        };
        let loops = self.working.face(face_id)?.loops.clone();
        for loop_id in loops {
            let coedges = self.collect_loop(loop_id)?;
            for coedge_id in &coedges {
                let edge_id = self.working.coedge(*coedge_id)?.edge;
                let edge = self.working.edge(edge_id)?.clone();
                let pcurve = match edge.curve {
                    Curve3::Circle(_) => {
                        self.pcurve_for_welded_circular_edge(edge_id, *coedge_id)?
                    }
                    Curve3::Line(_) => {
                        let start = edge.curve.evaluate(edge.range.start)?.point;
                        let end = edge.curve.evaluate(edge.range.end)?.point;
                        let start_uv = surface.project(start, self.kernel.tolerance)?.parameters;
                        let mut end_uv = surface.project(end, self.kernel.tolerance)?.parameters;
                        while end_uv.0 - start_uv.0 > std::f64::consts::PI {
                            end_uv.0 -= std::f64::consts::TAU;
                        }
                        while end_uv.0 - start_uv.0 < -std::f64::consts::PI {
                            end_uv.0 += std::f64::consts::TAU;
                        }
                        let direction = Vec2::new(end_uv.0 - start_uv.0, end_uv.1 - start_uv.1)
                            / edge.range.span();
                        let origin =
                            Point2::new(start_uv.0, start_uv.1) - direction * edge.range.start;
                        Line2::new(origin, direction)?.into()
                    }
                    Curve3::Nurbs(_) => {
                        return Err(KernelError::new(
                            DiagnosticKind::UnsupportedGeometry,
                            "a cylindrical face currently requires line or circle boundary edges",
                        ));
                    }
                };
                let coedge = self.working.coedges.get_mut(*coedge_id)?;
                coedge.pcurve = pcurve;
                coedge.range = if coedge.forward {
                    edge.range
                } else {
                    edge.range.reversed()
                };
            }

            // Cylinder u is periodic. Choose one continuous lift of the loop
            // into parameter space so area and winding are evaluated on the
            // intended strip instead of cancelling across the 2π seam.
            let mut previous_end = None;
            for coedge_id in coedges {
                let coedge = self.working.coedge(coedge_id)?;
                let start = coedge.pcurve.evaluate(coedge.range.start)?.point;
                let end = coedge.pcurve.evaluate(coedge.range.end)?.point;
                let shift = previous_end
                    .map(|previous: Point2| {
                        ((previous.x - start.x) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU
                    })
                    .unwrap_or(0.0);
                if shift != 0.0 {
                    let Curve2::Line(line) = self.working.coedge(coedge_id)?.pcurve else {
                        return Err(KernelError::new(
                            DiagnosticKind::UnsupportedGeometry,
                            "cylindrical boundary reparameterization requires linear p-curves",
                        ));
                    };
                    self.working.coedges.get_mut(coedge_id)?.pcurve =
                        Line2::new(line.origin() + Vec2::new(shift, 0.0), line.direction())?.into();
                }
                previous_end = Some(end + Vec2::new(shift, 0.0));
            }
        }
        Ok(())
    }

    pub(crate) fn sew_body_across_face_patch(
        &mut self,
        target: BodyId,
        source: BodyId,
        target_face: FaceId,
        source_face: FaceId,
    ) -> KernelResult<BodyId> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let composition_snapshot = self.composes_imported_topology;
        let result = self
            .sew_body_across_face_patch_unchecked(target, source, target_face, source_face)
            .and_then(|body| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| body)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.touched_bodies = touched_snapshot;
            self.composes_imported_topology = composition_snapshot;
        }
        result
    }

    fn sew_body_across_face_patch_unchecked(
        &mut self,
        target: BodyId,
        source: BodyId,
        target_face: FaceId,
        source_face: FaceId,
    ) -> KernelResult<BodyId> {
        if target == source {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "face-patch sewing requires two distinct bodies",
            ));
        }
        if self.working.face(source_face)?.loops.len() > 1 {
            return self.sew_bodies_across_faces_unchecked(
                target,
                source,
                target_face,
                source_face,
            );
        }
        let target_shells = self.working.body(target)?.shells.clone();
        let source_shells = self.working.body(source)?.shells.clone();
        if target_shells.len() != 1 || source_shells.len() != 1 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "face-patch sewing requires one shell per body",
            ));
        }
        let target_shell = target_shells[0];
        let source_shell = source_shells[0];
        if self.working.face(target_face)?.shell != target_shell
            || self.working.face(source_face)?.shell != source_shell
            || self.working.face(source_face)?.loops.len() != 1
        {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "the sewn patch must be a simply connected face on the source body",
            ));
        }

        let source_loop = self.working.face(source_face)?.loops[0];
        let circular_frame = self
            .collect_loop(source_loop)?
            .into_iter()
            .find_map(|coedge| {
                let edge = self.working.coedge(coedge).ok()?.edge();
                match self.working.edge(edge).ok()?.curve() {
                    Curve3::Circle(circle) => Some(*circle),
                    _ => None,
                }
            });
        let target_plane = match self.working.face(target_face)?.surface() {
            Surface3::Plane(plane) => *plane,
            _ => {
                return Err(KernelError::new(
                    DiagnosticKind::InvalidInput,
                    "a sewn face patch requires a planar target face",
                ));
            }
        };
        let parameter_plane = if let Some(circle) = circular_frame {
            Plane3::new(
                circle.center(),
                circle.x_radius().normalize(),
                circle.y_radius().normalize(),
                self.kernel.tolerance,
            )?
        } else {
            Plane3::from_origin_normal(target_plane.origin(), target_plane.normal())?
        };
        if parameter_plane.normal().dot(target_plane.normal()) < 0.0 {
            let orientation = self.working.face(target_face)?.orientation;
            self.working.faces.get_mut(target_face)?.orientation = match orientation {
                FaceOrientation::Forward => FaceOrientation::Reversed,
                FaceOrientation::Reversed => FaceOrientation::Forward,
            };
        }
        self.working.faces.get_mut(target_face)?.surface = parameter_plane.into();

        let source_faces = self.working.shell(source_shell)?.faces.clone();
        for face in source_faces {
            self.working.faces.get_mut(face)?.shell = target_shell;
            self.working.shells.get_mut(target_shell)?.faces.push(face);
        }
        let source_shell_name = self.working.shell(source_shell)?.name;
        let source_body_name = self.working.body(source)?.name;
        self.working.shells.remove(source_shell)?;
        self.working.bodies.remove(source)?;

        self.kfmrh_unchecked(target_face, source_face)?;
        self.reparameterize_planar_face(target_face)?;
        self.history
            .changed(source_shell_name, ChangeKind::Deleted, std::iter::empty());
        self.history
            .changed(source_body_name, ChangeKind::Deleted, std::iter::empty());
        let target_name = self.working.body(target)?.name;
        self.history
            .changed(target_name, ChangeKind::Modified, [target_name]);
        self.touched_bodies.retain(|body| *body != source);
        if !self.touched_bodies.contains(&target) {
            self.touched_bodies.push(target);
        }
        self.composes_imported_topology = true;
        Ok(target)
    }

    /// Move one shell into a newly created body, the inverse body/shell Euler
    /// operator of [`Self::merge_shell_into_body`].
    pub fn split_shell_from_body(&mut self, shell: ShellId) -> KernelResult<BodyId> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self
            .split_shell_from_body_unchecked(shell)
            .and_then(|body| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| body)
            });
        if result.is_err() {
            self.working = snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn split_shell_from_body_unchecked(&mut self, shell: ShellId) -> KernelResult<BodyId> {
        let source = self.working.shell(shell)?.body;
        let source_name = self.working.body(source)?.name;
        if self.working.body(source)?.shells.len() <= 1 {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "splitting a shell must leave at least one shell in the source body",
            ));
        }
        let body_name = self.names.next_for("body.split")?;
        let body = self.working.bodies.insert(Body::new(body_name));
        self.working.bodies.get_mut(body)?.shells.push(shell);
        self.working
            .bodies
            .get_mut(source)?
            .shells
            .retain(|candidate| *candidate != shell);
        self.working.shells.get_mut(shell)?.body = body;
        self.history.created(body_name);
        self.history
            .changed(source_name, ChangeKind::Modified, [source_name]);
        if !self.touched_bodies.contains(&body) {
            self.touched_bodies.push(body);
        }
        Ok(body)
    }

    /// Extrude a profile bounded by exact line, circular-arc, or NURBS curve
    /// uses. High-level callers normally use [`Profile::extrude`] instead.
    pub fn extrude_curve_profile(
        &mut self,
        plane: Plane3,
        profile: &Profile,
        vector: Vec3,
    ) -> KernelResult<BodyId> {
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self
            .extrude_curve_profile_unchecked(plane, profile, vector)
            .and_then(|body| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| body)
            });
        if result.is_err() {
            self.working = topology_snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    /// Build a closed triangular polyhedron through a shelling sequence of MEV
    /// and MEF operations. The input is exact construction geometry, not a render
    /// mesh, and publication still performs the complete physical certification.
    pub fn polyhedron(&mut self, mesh: &PolyhedralMesh) -> KernelResult<BodyId> {
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self.polyhedron_unchecked(mesh).and_then(|body| {
            validate(
                &self.working,
                self.kernel.tolerance,
                ValidationMode::Construction,
            )
            .into_result()
            .map(|_| body)
        });
        if result.is_err() {
            self.working = topology_snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    pub(crate) fn merged_polyhedron(&mut self, mesh: &PolyhedralMesh) -> KernelResult<BodyId> {
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self.polyhedron_unchecked(mesh).and_then(|body| {
            self.merge_coplanar_boolean_faces_unchecked(body)?;
            validate(
                &self.working,
                self.kernel.tolerance,
                ValidationMode::Construction,
            )
            .into_result()
            .map(|_| body)
        });
        if result.is_err() {
            self.working = topology_snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    pub(crate) fn hollow_polyhedron(
        &mut self,
        outer: &PolyhedralMesh,
        cavities: &[PolyhedralMesh],
    ) -> KernelResult<BodyId> {
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self.polyhedron_unchecked(outer).and_then(|body| {
            self.merge_coplanar_boolean_faces_unchecked(body)?;
            for (index, cavity) in cavities.iter().enumerate() {
                self.names.push_scope(format!("cavity{index}"))?;
                let cavity_body = self.polyhedron_unchecked(cavity)?;
                self.merge_coplanar_boolean_faces_unchecked(cavity_body)?;
                self.names.pop_scope()?;
                self.merge_shell_into_body_unchecked(body, cavity_body)?;
            }
            validate(
                &self.working,
                self.kernel.tolerance,
                ValidationMode::Construction,
            )
            .into_result()
            .map(|_| body)
        });
        if result.is_err() {
            self.working = topology_snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    /// Compute a planar BREP Boolean and rebuild its boundary through Euler
    /// operators. Curved operands and non-genus-zero outputs are rejected until
    /// their exact reconstruction paths are certified.
    pub fn boolean(
        &mut self,
        first: BodyId,
        second: BodyId,
        kind: super::BooleanKind,
    ) -> KernelResult<Vec<BodyId>> {
        self.boolean_rebuilding(first, second, kind)
    }

    fn boolean_rebuilding(
        &mut self,
        first: BodyId,
        second: BodyId,
        kind: super::BooleanKind,
    ) -> KernelResult<Vec<BodyId>> {
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let prism_axis = self.boolean_prism_axis(first, second)?;
        let result = super::boolean::boolean_planar_brep(
            &self.working,
            self.kernel.tolerance,
            first,
            second,
            kind,
        )
        .and_then(|meshes| {
            let mut bodies = Vec::with_capacity(meshes.len());
            for (index, mesh) in meshes.iter().enumerate() {
                self.names.push_scope(format!("boolean.component{index}"))?;
                let reconstructed = if mesh.outer.genus() > 0 {
                    self.prismatic_polyhedron_unchecked(&mesh.outer, prism_axis)?
                } else {
                    None
                };
                let (body, merge_faces) = if let Some(body) = reconstructed {
                    (body, false)
                } else {
                    (self.polyhedron_unchecked(&mesh.outer)?, true)
                };
                for (cavity_index, cavity) in mesh.cavities.iter().enumerate() {
                    self.names.push_scope(format!("cavity{cavity_index}"))?;
                    let cavity_body = self.polyhedron_unchecked(cavity)?;
                    self.names.pop_scope()?;
                    self.merge_shell_into_body_unchecked(body, cavity_body)?;
                }
                if merge_faces {
                    self.merge_coplanar_boolean_faces_unchecked(body)?;
                }
                self.names.pop_scope()?;
                bodies.push(body);
            }
            Ok(bodies)
        })
        .and_then(|bodies| {
            validate(
                &self.working,
                self.kernel.tolerance,
                ValidationMode::Construction,
            )
            .into_result()
            .map(|_| bodies)
        });
        if result.is_err() {
            self.working = topology_snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn boolean_prism_axis(&self, first: BodyId, second: BodyId) -> KernelResult<Option<Vec3>> {
        let mut candidate = None;
        for body in [first, second] {
            for shell in self.working.body(body)?.shells() {
                for face in self.working.shell(*shell)?.faces() {
                    let face_value = self.working.face(*face)?;
                    let (priority, axis) = match face_value.surface {
                        Surface3::Cylinder(cylinder) => (usize::MAX, cylinder.axis().normalize()),
                        Surface3::Plane(plane) => {
                            (self.working.face_edges(*face)?.len(), plane.normal())
                        }
                        _ => continue,
                    };
                    if candidate
                        .map(|(current, _)| priority > current)
                        .unwrap_or(true)
                    {
                        candidate = Some((priority, axis));
                    }
                }
            }
        }
        Ok(candidate.map(|(_, axis)| axis))
    }

    fn prismatic_polyhedron_unchecked(
        &mut self,
        mesh: &PolyhedralMesh,
        preferred_axis: Option<Vec3>,
    ) -> KernelResult<Option<BodyId>> {
        if mesh.orientation() != PolyhedralShellOrientation::Outer {
            return Ok(None);
        }
        let vertices = mesh.vertices();
        let allowed = self.kernel.tolerance.linear * 64.0;
        let mut candidate = preferred_axis.map(|axis| {
            let axis = axis.normalize();
            let projections = vertices
                .iter()
                .map(|vertex| axis.dot(vertex.point.to_vec()))
                .collect::<Vec<_>>();
            let minimum = projections.iter().copied().fold(f64::INFINITY, f64::min);
            let maximum = projections
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            (axis, projections, minimum, maximum)
        });
        for triangle in mesh.triangles() {
            if preferred_axis.is_some() {
                break;
            }
            let [a, b, c] = triangle.vertices;
            let normal = (vertices[b].point - vertices[a].point)
                .cross(vertices[c].point - vertices[a].point);
            if normal.magnitude2() <= allowed.powi(4) {
                continue;
            }
            let axis = normal.normalize();
            let projections = vertices
                .iter()
                .map(|vertex| axis.dot(vertex.point.to_vec()))
                .collect::<Vec<_>>();
            let minimum = projections.iter().copied().fold(f64::INFINITY, f64::min);
            let maximum = projections
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            if maximum - minimum <= allowed
                || projections.iter().any(|value| {
                    (*value - minimum).abs() > allowed && (*value - maximum).abs() > allowed
                })
            {
                continue;
            }
            if candidate
                .as_ref()
                .map(|(_, _, current_minimum, current_maximum)| {
                    maximum - minimum > *current_maximum - *current_minimum
                })
                .unwrap_or(true)
            {
                candidate = Some((axis, projections, minimum, maximum));
            }
        }
        let Some((axis, projections, minimum, maximum)) = candidate else {
            return Ok(None);
        };

        let cap_boundary = |level: f64| {
            let mut cap_edges = std::collections::BTreeMap::<(usize, usize), usize>::new();
            for triangle in mesh.triangles() {
                if !triangle
                    .vertices
                    .iter()
                    .all(|index| (projections[*index] - level).abs() <= allowed)
                {
                    continue;
                }
                let [a, b, c] = triangle.vertices;
                for (first, second) in [(a, b), (b, c), (c, a)] {
                    *cap_edges
                        .entry(super::polyhedron::ordered(first, second))
                        .or_default() += 1;
                }
            }
            cap_edges
                .into_iter()
                .filter_map(|(edge, uses)| (uses == 1).then_some(edge))
                .collect::<std::collections::BTreeSet<_>>()
        };
        let mut boundary = cap_boundary(minimum);
        if boundary.is_empty() {
            return Ok(None);
        }
        let mut adjacency = std::collections::BTreeMap::<usize, Vec<usize>>::new();
        for (first, second) in &boundary {
            adjacency.entry(*first).or_default().push(*second);
            adjacency.entry(*second).or_default().push(*first);
        }
        if adjacency.values().any(|neighbors| neighbors.len() != 2) {
            return Ok(None);
        }
        let origin_index = boundary.iter().next().expect("nonempty cap boundary").0;
        let origin = vertices[origin_index].point;
        let first_neighbor = adjacency[&origin_index][0];
        let u = (vertices[first_neighbor].point - origin).normalize();
        let v = axis.cross(u).normalize();
        let cap_points = |edges: &std::collections::BTreeSet<(usize, usize)>| {
            edges
                .iter()
                .flat_map(|edge| [edge.0, edge.1])
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|index| {
                    let relative = vertices[index].point - origin;
                    Point2::new(relative.dot(u), relative.dot(v))
                })
                .collect::<Vec<_>>()
        };
        let lower_points = cap_points(&boundary);
        let upper_points = cap_points(&cap_boundary(maximum));
        if lower_points.len() != upper_points.len()
            || lower_points.iter().any(|point| {
                upper_points
                    .iter()
                    .all(|candidate| candidate.distance(*point) > allowed * 16.0)
            })
        {
            return Ok(None);
        }
        let plane = Plane3::new(origin, u, v, self.kernel.tolerance)?;
        let mut rings = Vec::<(f64, Vec<Point2>)>::new();
        while let Some((start, next)) = boundary.iter().next().copied() {
            let mut indices = vec![start, next];
            boundary.remove(&super::polyhedron::ordered(start, next));
            let mut previous = start;
            let mut current = next;
            while current != start {
                let following = adjacency[&current]
                    .iter()
                    .copied()
                    .find(|value| {
                        *value != previous
                            && boundary.contains(&super::polyhedron::ordered(current, *value))
                    })
                    .ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::GeometryInvariant,
                            "prismatic Boolean cap boundary does not close",
                        )
                    })?;
                boundary.remove(&super::polyhedron::ordered(current, following));
                previous = current;
                current = following;
                if current != start {
                    indices.push(current);
                }
            }
            let mut points = indices
                .iter()
                .map(|index| {
                    let relative = vertices[*index].point - origin;
                    Point2::new(relative.dot(u), relative.dot(v))
                })
                .collect::<Vec<_>>();
            loop {
                let removable = (0..points.len()).find(|index| {
                    let previous = points[(*index + points.len() - 1) % points.len()];
                    let current = points[*index];
                    let next = points[(*index + 1) % points.len()];
                    let first = current - previous;
                    let second = next - current;
                    first.magnitude() <= allowed
                        || second.magnitude() <= allowed
                        || (first.x * second.y - first.y * second.x).abs()
                            <= allowed * (first.magnitude() + second.magnitude()).max(1.0)
                });
                if points.len() <= 3 || removable.is_none() {
                    break;
                }
                points.remove(removable.expect("checked removable profile point"));
            }
            let area = (0..points.len())
                .map(|index| {
                    let first = points[index];
                    let second = points[(index + 1) % points.len()];
                    first.x * second.y - second.x * first.y
                })
                .sum::<f64>()
                * 0.5;
            rings.push((area, points));
        }
        let outer_index = rings
            .iter()
            .enumerate()
            .max_by(|(_, first), (_, second)| first.0.abs().total_cmp(&second.0.abs()))
            .map(|(index, _)| index)
            .ok_or_else(|| {
                KernelError::new(DiagnosticKind::GeometryInvariant, "prism has no cap ring")
            })?;
        let outer = Wire2::polygon(rings[outer_index].1.clone(), self.kernel.tolerance)?;
        let holes = rings
            .into_iter()
            .enumerate()
            .filter_map(|(index, (_, points))| (index != outer_index).then_some(points))
            .map(|points| Wire2::polygon(points, self.kernel.tolerance))
            .collect::<KernelResult<Vec<_>>>()?;
        let profile = Profile::new(outer, holes, self.kernel.tolerance)?;
        let body =
            self.extrude_curve_profile_unchecked(plane, &profile, axis * (maximum - minimum))?;
        Ok(Some(body))
    }

    /// Remove triangulation edges introduced solely by the Boolean boundary
    /// reconstruction. The Boolean classifier already groups coplanar fragments;
    /// KEF turns each resulting triangle fan back into meaningful planar BREP
    /// faces while preserving the union of their source provenance.
    fn merge_coplanar_boolean_faces_unchecked(&mut self, body: BodyId) -> KernelResult<()> {
        loop {
            let mut candidate = None;
            let shell_ids = self.working.body(body)?.shells().to_vec();
            'search: for shell in shell_ids {
                let face_ids = self.working.shell(shell)?.faces().to_vec();
                for face in face_ids {
                    for edge in self.working.face_edges(face)? {
                        let adjacent = self.working.edge_faces(edge)?;
                        if adjacent.len() != 2 {
                            continue;
                        }
                        let other = if adjacent[0] == face {
                            adjacent[1]
                        } else {
                            adjacent[0]
                        };
                        if self.coplanar_faces(face, other)? {
                            candidate = Some((edge, other));
                            break 'search;
                        }
                    }
                }
            }
            let Some((edge, remove_face)) = candidate else {
                self.remove_boolean_triangulation_spikes_unchecked(body)?;
                return Ok(());
            };

            let adjacent = self.working.edge_faces(edge)?;
            let keep_face = *adjacent
                .iter()
                .find(|face| **face != remove_face)
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "coplanar merge edge lost its surviving face",
                    )
                })?;
            let mut sources = std::collections::BTreeSet::new();
            for face in [keep_face, remove_face] {
                let name = self.working.face(face)?.name;
                if let Some(super::Origin::Derived {
                    sources: face_sources,
                    ..
                }) = self.history.origin(name)
                {
                    sources.extend(face_sources.iter().copied());
                }
            }
            let merged = self.kef_unchecked(edge, remove_face)?;
            self.reparameterize_planar_face(merged.surviving_face)?;
            if !sources.is_empty() {
                let name = self.working.face(merged.surviving_face)?.name;
                self.history.derived(name, sources, "boolean.face");
            }
        }
    }

    fn remove_boolean_triangulation_spikes_unchecked(&mut self, body: BodyId) -> KernelResult<()> {
        loop {
            let mut candidate = None;
            let shell_ids = self.working.body(body)?.shells().to_vec();
            'search: for shell in shell_ids {
                for face in self.working.shell(shell)?.faces() {
                    for edge in self.working.face_edges(*face)? {
                        if self.working.edge_faces(edge)?.len() != 1 {
                            continue;
                        }
                        let edge_value = self.working.edge(edge)?;
                        let forward = edge_value.coedges().iter().find(|coedge| {
                            self.working
                                .coedge(**coedge)
                                .map(|value| value.is_forward())
                                .unwrap_or(false)
                        });
                        let reverse = edge_value.coedges().iter().find(|coedge| {
                            self.working
                                .coedge(**coedge)
                                .map(|value| !value.is_forward())
                                .unwrap_or(false)
                        });
                        if let (Some(forward), Some(reverse)) = (forward, reverse) {
                            for vertex in edge_value.vertices() {
                                let ordered = if vertex == edge_value.vertices()[1] {
                                    self.working.coedge(*forward)?.next() == *reverse
                                } else {
                                    self.working.coedge(*reverse)?.next() == *forward
                                };
                                if ordered && self.working.vertex_edges(vertex)?.len() == 1 {
                                    candidate = Some((edge, vertex, *face));
                                    break 'search;
                                }
                            }
                        }
                    }
                }
            }
            let Some((edge, vertex, face)) = candidate else {
                return Ok(());
            };
            self.kev_unchecked(edge, vertex)?;
            self.reparameterize_planar_face(face)?;
        }
    }

    fn reparameterize_planar_face(&mut self, face: FaceId) -> KernelResult<()> {
        let Surface3::Plane(plane) = self.working.face(face)?.surface().clone() else {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "coplanar face merge produced a non-planar survivor",
            ));
        };
        let loops = self.working.face(face)?.loops().to_vec();
        for loop_id in loops {
            for coedge_id in self.collect_loop(loop_id)? {
                let edge_id = self.working.coedge(coedge_id)?.edge();
                let edge = self.working.edge(edge_id)?;
                self.working.coedges.get_mut(coedge_id)?.pcurve = match edge.curve() {
                    Curve3::Line(_) => {
                        let vertices = self.working.coedge_vertices(coedge_id)?;
                        let start = plane
                            .project(self.working.vertex(vertices[0])?.point())?
                            .parameters;
                        let end = plane
                            .project(self.working.vertex(vertices[1])?.point())?
                            .parameters;
                        let range = self.working.coedge(coedge_id)?.range;
                        let start = Point2::new(start.0, start.1);
                        let end = Point2::new(end.0, end.1);
                        let direction = (end - start) / range.span();
                        Line2::new(start - direction * range.start, direction)?.into()
                    }
                    Curve3::Circle(circle) => {
                        let center = plane.project(circle.center())?.parameters;
                        let x = plane
                            .project(circle.center() + circle.x_radius())?
                            .parameters;
                        let y = plane
                            .project(circle.center() + circle.y_radius())?
                            .parameters;
                        let radius = circle.radius();
                        let achieved = (x.0 - center.0 - radius)
                            .abs()
                            .max((x.1 - center.1).abs())
                            .max((y.0 - center.0).abs())
                            .max((y.1 - center.1 - radius).abs());
                        if achieved > self.kernel.tolerance.linear_at_scale(radius) {
                            return Err(KernelError::new(
                                DiagnosticKind::UnsupportedGeometry,
                                "a circular planar edge requires an aligned face parameter frame",
                            ));
                        }
                        Circle2::new(Point2::new(center.0, center.1), radius)?.into()
                    }
                    Curve3::Nurbs(curve) => NurbsCurve2::new(
                        curve.degree(),
                        curve
                            .control_points()
                            .iter()
                            .map(|point| {
                                let projected = plane.project(*point)?.parameters;
                                Ok(Point2::new(projected.0, projected.1))
                            })
                            .collect::<KernelResult<Vec<_>>>()?,
                        curve.weights().to_vec(),
                        curve.knots().to_vec(),
                    )?
                    .into(),
                };
            }
        }
        Ok(())
    }

    fn coplanar_faces(&self, first: FaceId, second: FaceId) -> KernelResult<bool> {
        let first = self.working.face(first)?;
        let second = self.working.face(second)?;
        let (Surface3::Plane(first_plane), Surface3::Plane(second_plane)) =
            (first.surface(), second.surface())
        else {
            return Ok(false);
        };
        let first_normal = first_plane.normal();
        let second_normal = second_plane.normal();
        let same_orientation = first.orientation() == second.orientation();
        let parallel =
            first_normal.cross(second_normal).magnitude() <= self.kernel.tolerance.angular;
        let separation = (second_plane.origin() - first_plane.origin())
            .dot(first_normal)
            .abs();
        Ok(same_orientation
            && parallel
            && separation
                <= self
                    .kernel
                    .tolerance
                    .linear_at_scale(second_plane.origin().to_vec().magnitude()))
    }

    fn polyhedron_unchecked(&mut self, mesh: &PolyhedralMesh) -> KernelResult<BodyId> {
        let vertices = mesh.vertices();
        let triangles = mesh.triangles();
        let seed_triangle = &triangles[0];
        let [a, b, c] = seed_triangle.vertices;
        let seed_plane = Plane3::new(
            vertices[a].point,
            vertices[b].point - vertices[a].point,
            vertices[c].point - vertices[a].point,
            self.kernel.tolerance,
        )?;
        let range = ParameterRange::new(0.0, 1.0)?;
        let placeholder: Curve2 = Line2::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))?.into();
        let seed = self.mvfs_unchecked(
            vertices[a].point,
            seed_plane.into(),
            FaceOrientation::Forward,
        )?;
        self.names.push_scope("polyhedron")?;
        let seed_vertex_name = self.names.named("vertex", &vertices[a].key)?;
        self.rename_vertex(seed.vertex, seed_vertex_name)?;
        if !vertices[a].sources.is_empty() {
            self.history.derived(
                seed_vertex_name,
                vertices[a].sources.iter().copied(),
                "polyhedron.vertex",
            );
        }

        let first_edge = self.mev_unchecked(
            seed.loop_id,
            None,
            seed.vertex,
            vertices[b].point,
            Line3::through(vertices[a].point, vertices[b].point)?.into(),
            range,
            placeholder.clone(),
            placeholder.clone(),
            range,
        )?;
        self.name_mev(
            first_edge,
            &vertices[b].key,
            &polyhedral_edge_key(mesh, a, b),
        )?;
        self.derive_polyhedral_edge(first_edge.edge, mesh, a, b)?;
        if !vertices[b].sources.is_empty() {
            let name = self.working.vertex(first_edge.vertex)?.name;
            self.history.derived(
                name,
                vertices[b].sources.iter().copied(),
                "polyhedron.vertex",
            );
        }
        let second_edge = self.mev_unchecked(
            seed.loop_id,
            Some(first_edge.forward),
            first_edge.vertex,
            vertices[c].point,
            Line3::through(vertices[b].point, vertices[c].point)?.into(),
            range,
            placeholder.clone(),
            placeholder.clone(),
            range,
        )?;
        self.name_mev(
            second_edge,
            &vertices[c].key,
            &polyhedral_edge_key(mesh, b, c),
        )?;
        self.derive_polyhedral_edge(second_edge.edge, mesh, b, c)?;
        if !vertices[c].sources.is_empty() {
            let name = self.working.vertex(second_edge.vertex)?.name;
            self.history.derived(
                name,
                vertices[c].sources.iter().copied(),
                "polyhedron.vertex",
            );
        }
        let lamina = self.mef_unchecked(
            first_edge.forward,
            second_edge.reverse,
            Line3::through(vertices[a].point, vertices[c].point)?.into(),
            range,
            seed_plane.into(),
            FaceOrientation::Reversed,
            placeholder.clone(),
            placeholder.clone(),
            range,
        )?;
        self.name_mef(
            lamina,
            &polyhedral_edge_key(mesh, a, c),
            Some("construction"),
        )?;
        self.derive_polyhedral_edge(lamina.edge, mesh, a, c)?;
        self.assign_triangle_face(seed.face, seed_triangle, mesh)?;
        let seed_face_name = self.names.named("face", &seed_triangle.key)?;
        let seed_loop_name = self.names.named("loop.outer", &seed_triangle.key)?;
        self.rename_face(seed.face, seed_face_name)?;
        self.rename_loop(seed.loop_id, seed_loop_name)?;
        if !seed_triangle.sources.is_empty() {
            self.history.derived(
                seed_face_name,
                seed_triangle.sources.iter().copied(),
                "polyhedron.face",
            );
        }

        let mut topology_vertices = std::collections::BTreeMap::new();
        topology_vertices.insert(a, seed.vertex);
        topology_vertices.insert(b, first_edge.vertex);
        topology_vertices.insert(c, second_edge.vertex);
        let mut input_vertices = std::collections::BTreeMap::new();
        input_vertices.insert(seed.vertex, a);
        input_vertices.insert(first_edge.vertex, b);
        input_vertices.insert(second_edge.vertex, c);
        let mut built = std::collections::BTreeSet::from([0usize]);
        let work_face = lamina.face;
        let work_loop = lamina.loop_id;

        while built.len() < triangles.len() {
            let boundary = self.collect_loop(work_loop)?;
            let mut frontier = std::collections::BTreeMap::new();
            for coedge_id in &boundary {
                let endpoints = self.working.coedge_vertices(*coedge_id)?;
                let first = *input_vertices.get(&endpoints[0]).ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "polyhedron frontier lost an input vertex",
                    )
                })?;
                let second = *input_vertices.get(&endpoints[1]).ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "polyhedron frontier lost an input vertex",
                    )
                })?;
                frontier.insert(super::polyhedron::ordered(first, second), *coedge_id);
            }

            if boundary.len() == 3 {
                let mut boundary_vertices = std::collections::BTreeSet::new();
                for coedge in &boundary {
                    for vertex in self.working.coedge_vertices(*coedge)? {
                        let input = input_vertices.get(&vertex).copied().ok_or_else(|| {
                            KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "polyhedron final frontier lost an input vertex",
                            )
                        })?;
                        boundary_vertices.insert(input);
                    }
                }
                if let Some((triangle_index, triangle)) =
                    triangles.iter().enumerate().find(|(index, triangle)| {
                        !built.contains(index)
                            && triangle
                                .vertices
                                .iter()
                                .copied()
                                .collect::<std::collections::BTreeSet<_>>()
                                == boundary_vertices
                    })
                {
                    self.assign_triangle_face(work_face, triangle, mesh)?;
                    let face_name = self.names.named("face", &triangle.key)?;
                    let loop_name = self.names.named("loop.outer", &triangle.key)?;
                    self.rename_face(work_face, face_name)?;
                    self.rename_loop(work_loop, loop_name)?;
                    if !triangle.sources.is_empty() {
                        self.history.derived(
                            face_name,
                            triangle.sources.iter().copied(),
                            "polyhedron.face",
                        );
                    }
                    built.insert(triangle_index);
                    break;
                }
            }

            let mut progress = false;
            for (triangle_index, triangle) in triangles.iter().enumerate() {
                if built.contains(&triangle_index) {
                    continue;
                }
                let [ta, tb, tc] = triangle.vertices;
                let triangle_edges = [
                    super::polyhedron::ordered(ta, tb),
                    super::polyhedron::ordered(tb, tc),
                    super::polyhedron::ordered(tc, ta),
                ];
                let frontier_count = triangle_edges
                    .iter()
                    .filter(|edge| frontier.contains_key(edge))
                    .count();
                if frontier_count == 1 {
                    let edge = triangle_edges
                        .iter()
                        .find(|edge| frontier.contains_key(edge))
                        .copied()
                        .ok_or_else(|| {
                            KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "polyhedron frontier edge disappeared",
                            )
                        })?;
                    let third = triangle
                        .vertices
                        .iter()
                        .copied()
                        .find(|vertex| *vertex != edge.0 && *vertex != edge.1)
                        .ok_or_else(|| {
                            KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "polyhedron triangle has no third vertex",
                            )
                        })?;
                    if topology_vertices.contains_key(&third) {
                        continue;
                    }
                    let boundary_use = frontier[&edge];
                    let [start, end] = self.working.coedge_vertices(boundary_use)?;
                    let end_input = input_vertices[&end];
                    let start_input = input_vertices[&start];
                    let spike = self.mev_unchecked(
                        work_loop,
                        Some(boundary_use),
                        end,
                        vertices[third].point,
                        Line3::through(vertices[end_input].point, vertices[third].point)?.into(),
                        range,
                        placeholder.clone(),
                        placeholder.clone(),
                        range,
                    )?;
                    self.name_mev(
                        spike,
                        &vertices[third].key,
                        &polyhedral_edge_key(mesh, end_input, third),
                    )?;
                    self.derive_polyhedral_edge(spike.edge, mesh, end_input, third)?;
                    if !vertices[third].sources.is_empty() {
                        let name = self.working.vertex(spike.vertex)?.name;
                        self.history.derived(
                            name,
                            vertices[third].sources.iter().copied(),
                            "polyhedron.vertex",
                        );
                    }
                    topology_vertices.insert(third, spike.vertex);
                    input_vertices.insert(spike.vertex, third);
                    let made = self.mef_unchecked(
                        spike.reverse,
                        boundary_use,
                        Line3::through(vertices[third].point, vertices[start_input].point)?.into(),
                        range,
                        self.triangle_plane(triangle, mesh)?.into(),
                        FaceOrientation::Forward,
                        placeholder.clone(),
                        placeholder.clone(),
                        range,
                    )?;
                    self.name_mef(
                        made,
                        &polyhedral_edge_key(mesh, third, start_input),
                        Some(&triangle.key),
                    )?;
                    self.derive_polyhedral_edge(made.edge, mesh, third, start_input)?;
                    self.assign_triangle_face(made.face, triangle, mesh)?;
                    if !triangle.sources.is_empty() {
                        let name = self.working.face(made.face)?.name;
                        self.history.derived(
                            name,
                            triangle.sources.iter().copied(),
                            "polyhedron.face",
                        );
                    }
                    built.insert(triangle_index);
                    progress = true;
                    break;
                }
                if frontier_count == 2 {
                    let triangle_set: std::collections::BTreeSet<_> =
                        triangle.vertices.iter().copied().collect();
                    let mut chain = None;
                    for boundary_use in &boundary {
                        let next = self.working.coedge(*boundary_use)?.next;
                        let endpoints = [
                            self.working.coedge_vertices(*boundary_use)?[0],
                            self.working.coedge_vertices(*boundary_use)?[1],
                            self.working.coedge_vertices(next)?[1],
                        ];
                        let inputs: std::collections::BTreeSet<_> = endpoints
                            .iter()
                            .filter_map(|vertex| input_vertices.get(vertex).copied())
                            .collect();
                        if inputs == triangle_set {
                            chain = Some((*boundary_use, next));
                            break;
                        }
                    }
                    let Some((chain_start, chain_second)) = chain else {
                        continue;
                    };
                    let after_chain = self.working.coedge(chain_second)?.next;
                    let [new_start, _] = self.working.coedge_vertices(after_chain)?;
                    let [new_end, _] = self.working.coedge_vertices(chain_start)?;
                    let start_input = input_vertices[&new_start];
                    let end_input = input_vertices[&new_end];
                    let made = self.mef_unchecked(
                        after_chain,
                        chain_start,
                        Line3::through(vertices[start_input].point, vertices[end_input].point)?
                            .into(),
                        range,
                        self.triangle_plane(triangle, mesh)?.into(),
                        FaceOrientation::Forward,
                        placeholder.clone(),
                        placeholder.clone(),
                        range,
                    )?;
                    self.name_mef(
                        made,
                        &polyhedral_edge_key(mesh, start_input, end_input),
                        Some(&triangle.key),
                    )?;
                    self.derive_polyhedral_edge(made.edge, mesh, start_input, end_input)?;
                    self.assign_triangle_face(made.face, triangle, mesh)?;
                    if !triangle.sources.is_empty() {
                        let name = self.working.face(made.face)?.name;
                        self.history.derived(
                            name,
                            triangle.sources.iter().copied(),
                            "polyhedron.face",
                        );
                    }
                    built.insert(triangle_index);
                    progress = true;
                    break;
                }
            }
            if !progress {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "triangular polyhedron has no Euler shelling step compatible with the current frontier",
                ));
            }
        }
        if built.len() != triangles.len() {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "polyhedron Euler shelling did not consume every triangle",
            ));
        }
        self.names.pop_scope()?;
        self.seal_shell_unchecked(seed.shell)?;
        Ok(seed.body)
    }

    fn triangle_plane(
        &self,
        triangle: &PolyhedralTriangle,
        mesh: &PolyhedralMesh,
    ) -> KernelResult<Plane3> {
        let [a, b, c] = triangle.vertices;
        Plane3::new(
            mesh.vertices()[a].point,
            mesh.vertices()[b].point - mesh.vertices()[a].point,
            mesh.vertices()[c].point - mesh.vertices()[a].point,
            self.kernel.tolerance,
        )
    }

    fn derive_polyhedral_edge(
        &mut self,
        edge: EdgeId,
        mesh: &PolyhedralMesh,
        first: usize,
        second: usize,
    ) -> KernelResult<()> {
        let sources = mesh.edge_sources(first, second);
        if !sources.is_empty() {
            let name = self.working.edge(edge)?.name;
            self.history
                .derived(name, sources.iter().copied(), "polyhedron.edge");
        }
        Ok(())
    }

    fn assign_triangle_face(
        &mut self,
        face_id: FaceId,
        triangle: &PolyhedralTriangle,
        mesh: &PolyhedralMesh,
    ) -> KernelResult<()> {
        let plane = self.triangle_plane(triangle, mesh)?;
        let loop_ids = self.working.face(face_id)?.loops.clone();
        if loop_ids.len() != 1 {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "polyhedron triangle face must have exactly one loop",
            ));
        }
        self.working.faces.get_mut(face_id)?.surface = plane.into();
        self.working.faces.get_mut(face_id)?.orientation = FaceOrientation::Forward;
        for coedge_id in self.collect_loop(loop_ids[0])? {
            let coedge = self.working.coedge(coedge_id)?;
            let edge = self.working.edge(coedge.edge)?;
            let start = plane
                .project(self.working.vertex(edge.vertices[0])?.point)?
                .parameters;
            let end = plane
                .project(self.working.vertex(edge.vertices[1])?.point)?
                .parameters;
            self.working.coedges.get_mut(coedge_id)?.pcurve = Line2::new(
                Point2::new(start.0, start.1),
                Point2::new(end.0, end.1) - Point2::new(start.0, start.1),
            )?
            .into();
        }
        Ok(())
    }

    /// Construct a closed right circular cylinder from Euler operators.
    pub fn cylinder(
        &mut self,
        center: Point3,
        x_radius: Vec3,
        y_radius: Vec3,
        axis: Vec3,
    ) -> KernelResult<BodyId> {
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self
            .cylinder_unchecked(center, x_radius, y_radius, axis)
            .and_then(|body| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| body)
            });
        if result.is_err() {
            self.working = topology_snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn cylinder_unchecked(
        &mut self,
        center: Point3,
        x_radius: Vec3,
        y_radius: Vec3,
        axis: Vec3,
    ) -> KernelResult<BodyId> {
        let circle = Circle3::new(center, x_radius, y_radius, self.kernel.tolerance)?;
        let cylinder = Cylinder3::new(center, axis, x_radius, y_radius, self.kernel.tolerance)?;
        let plane = Plane3::new(center, x_radius, y_radius, self.kernel.tolerance)?;
        let circle2: Curve2 = Circle2::new(Point2::new(0.0, 0.0), 1.0)?.into();
        let first_half = ParameterRange::new(0.0, std::f64::consts::PI)?;
        let second_half_reversed =
            ParameterRange::new(std::f64::consts::TAU, std::f64::consts::PI)?;
        let first = circle.evaluate(0.0)?.point;
        let second = circle.evaluate(std::f64::consts::PI)?.point;
        let seed = self.mvfs_unchecked(first, plane.into(), FaceOrientation::Forward)?;
        self.names.push_scope("circle_profile")?;
        let seed_vertex_name = self.names.named("vertex", "seam0")?;
        self.rename_vertex(seed.vertex, seed_vertex_name)?;
        let half = self.mev_unchecked(
            seed.loop_id,
            None,
            seed.vertex,
            second,
            circle.into(),
            first_half,
            circle2.clone(),
            circle2.clone(),
            first_half,
        )?;
        self.name_mev(half, "seam1", "arc0")?;
        let lamina = self.mef_unchecked(
            half.forward,
            half.reverse,
            circle.into(),
            second_half_reversed,
            plane.into(),
            FaceOrientation::Reversed,
            circle2.clone(),
            circle2,
            second_half_reversed,
        )?;
        self.name_mef(lamina, "arc1", Some("opposite_cap"))?;
        self.names.pop_scope()?;

        let height = axis.dot(plane.normal());
        let (sweep_face, sweep_loop, opposite_face) = if height > 0.0 {
            (seed.face, seed.loop_id, lamina.face)
        } else {
            (lamina.face, lamina.loop_id, seed.face)
        };
        let translated_plane =
            Plane3::new(center + axis, x_radius, y_radius, self.kernel.tolerance)?;
        self.working.faces.get_mut(sweep_face)?.surface = translated_plane.into();
        self.names.push_scope("cylinder_sweep")?;
        self.sweep_circle_loop_unchecked(
            sweep_loop,
            axis,
            cylinder,
            if height > 0.0 {
                FaceOrientation::Forward
            } else {
                FaceOrientation::Reversed
            },
            circle,
        )?;
        self.names.pop_scope()?;

        self.names.push_scope("caps")?;
        let start_face_name = self.names.named("face", "start")?;
        let end_face_name = self.names.named("face", "end")?;
        self.rename_face(opposite_face, start_face_name)?;
        self.rename_face(sweep_face, end_face_name)?;
        let start_loop_name = self.names.named("loop.outer", "start")?;
        let end_loop_name = self.names.named("loop.outer", "end")?;
        let start_loop = self
            .working
            .face(opposite_face)?
            .loops
            .first()
            .copied()
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "cylinder start cap has no loop",
                )
            })?;
        let end_loop = self
            .working
            .face(sweep_face)?
            .loops
            .first()
            .copied()
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "cylinder end cap has no loop",
                )
            })?;
        self.rename_loop(start_loop, start_loop_name)?;
        self.rename_loop(end_loop, end_loop_name)?;
        self.names.pop_scope()?;
        self.seal_shell_unchecked(seed.shell)?;
        Ok(seed.body)
    }

    /// Construct a closed sphere as four spherical lunes joined by four meridian
    /// edges. The two poles are ordinary topological vertices; only the surface
    /// parameterization is singular there.
    pub fn sphere(
        &mut self,
        center: Point3,
        x_radius: Vec3,
        y_radius: Vec3,
        z_radius: Vec3,
    ) -> KernelResult<BodyId> {
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self
            .sphere_unchecked(center, x_radius, y_radius, z_radius)
            .and_then(|body| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| body)
            });
        if result.is_err() {
            self.working = topology_snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn sphere_unchecked(
        &mut self,
        center: Point3,
        x_radius: Vec3,
        y_radius: Vec3,
        z_radius: Vec3,
    ) -> KernelResult<BodyId> {
        let sphere = Sphere3::new(center, x_radius, y_radius, z_radius, self.kernel.tolerance)?;
        let north = center + z_radius;
        let south = center - z_radius;
        let meridian_range =
            ParameterRange::new(std::f64::consts::FRAC_PI_2, -std::f64::consts::FRAC_PI_2)?;
        let tolerance = self.kernel.tolerance;
        let meridian = |u: f64| -> KernelResult<(Curve3, Curve2)> {
            let radial = x_radius * u.cos() + y_radius * u.sin();
            let curve: Curve3 = Circle3::new(center, radial, z_radius, tolerance)?.into();
            let pcurve: Curve2 = Line2::new(Point2::new(u, 0.0), Vec2::new(0.0, 1.0))?.into();
            Ok((curve, pcurve))
        };

        let seed = self.mvfs_unchecked(north, sphere.into(), FaceOrientation::Forward)?;
        self.names.push_scope("sphere")?;
        let north_name = self.names.named("vertex", "north")?;
        self.rename_vertex(seed.vertex, north_name)?;

        let (curve0, pcurve0) = meridian(0.0)?;
        let (_, pcurve2pi) = meridian(std::f64::consts::TAU)?;
        let first = self.mev_unchecked(
            seed.loop_id,
            None,
            seed.vertex,
            south,
            curve0,
            meridian_range,
            pcurve0,
            pcurve2pi,
            meridian_range,
        )?;
        self.name_mev(first, "south", "meridian.0")?;

        let (curve_pi, pcurve_pi) = meridian(std::f64::consts::PI)?;
        let opposite = self.mef_unchecked(
            first.forward,
            first.reverse,
            curve_pi,
            meridian_range,
            sphere.into(),
            FaceOrientation::Forward,
            pcurve_pi.clone(),
            pcurve_pi,
            meridian_range,
        )?;
        self.name_mef(opposite, "meridian.2", Some("lune.2"))?;

        let (curve_quarter, pcurve_quarter) = meridian(std::f64::consts::FRAC_PI_2)?;
        let quarter = self.mef_unchecked(
            first.forward,
            opposite.reverse,
            curve_quarter,
            meridian_range,
            sphere.into(),
            FaceOrientation::Forward,
            pcurve_quarter.clone(),
            pcurve_quarter,
            meridian_range,
        )?;
        self.name_mef(quarter, "meridian.1", Some("lune.1"))?;

        let u_three_quarters = 3.0 * std::f64::consts::FRAC_PI_2;
        let (curve_three_quarters, pcurve_three_quarters) = meridian(u_three_quarters)?;
        let three_quarters = self.mef_unchecked(
            opposite.forward,
            first.reverse,
            curve_three_quarters,
            meridian_range,
            sphere.into(),
            FaceOrientation::Forward,
            pcurve_three_quarters.clone(),
            pcurve_three_quarters,
            meridian_range,
        )?;
        self.name_mef(three_quarters, "meridian.3", Some("lune.3"))?;
        let first_face_name = self.names.named("face", "lune.0")?;
        let first_loop_name = self.names.named("loop.outer", "lune.0")?;
        self.rename_face(seed.face, first_face_name)?;
        self.rename_loop(seed.loop_id, first_loop_name)?;
        self.names.pop_scope()?;
        self.seal_shell_unchecked(seed.shell)?;
        Ok(seed.body)
    }

    /// Construct a closed right circular cone from one circular lamina and one
    /// apex spike. A final MEF divides the construction face into two conical
    /// patches while retaining a single topological apex vertex.
    pub fn cone(
        &mut self,
        apex: Point3,
        axis: Vec3,
        x_radius: Vec3,
        y_radius: Vec3,
    ) -> KernelResult<BodyId> {
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self
            .cone_unchecked(apex, axis, x_radius, y_radius)
            .and_then(|body| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| body)
            });
        if result.is_err() {
            self.working = topology_snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn cone_unchecked(
        &mut self,
        apex: Point3,
        axis: Vec3,
        x_radius: Vec3,
        y_radius: Vec3,
    ) -> KernelResult<BodyId> {
        if !axis.x.is_finite() || !axis.y.is_finite() || !axis.z.is_finite() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "cone axis must be finite",
            ));
        }
        let frame_normal = x_radius.cross(y_radius);
        if frame_normal.magnitude2() == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "cone radial frame must be nondegenerate",
            ));
        }
        // Canonicalize the analytic parameterization so its forward normal is
        // outward, independent of the handedness supplied by the caller.
        let y_radius = if axis.dot(frame_normal) >= 0.0 {
            y_radius
        } else {
            -y_radius
        };
        let base_center = apex + axis;
        let cone = Cone3::new(apex, axis, x_radius, y_radius, self.kernel.tolerance)?;
        let circle = Circle3::new(base_center, x_radius, y_radius, self.kernel.tolerance)?;
        let plane = Plane3::new(base_center, x_radius, y_radius, self.kernel.tolerance)?;
        let circle2: Curve2 = Circle2::new(Point2::new(0.0, 0.0), 1.0)?.into();
        let first_half = ParameterRange::new(0.0, std::f64::consts::PI)?;
        let second_half_reversed =
            ParameterRange::new(std::f64::consts::TAU, std::f64::consts::PI)?;
        let unit_range = ParameterRange::new(0.0, 1.0)?;
        let first_point = circle.evaluate(0.0)?.point;
        let second_point = circle.evaluate(std::f64::consts::PI)?.point;

        let seed = self.mvfs_unchecked(first_point, plane.into(), FaceOrientation::Forward)?;
        self.names.push_scope("cone")?;
        let first_vertex_name = self.names.named("vertex", "base.seam0")?;
        self.rename_vertex(seed.vertex, first_vertex_name)?;
        let half = self.mev_unchecked(
            seed.loop_id,
            None,
            seed.vertex,
            second_point,
            circle.into(),
            first_half,
            circle2.clone(),
            circle2.clone(),
            first_half,
        )?;
        self.name_mev(half, "base.seam1", "base.arc0")?;
        let lamina = self.mef_unchecked(
            half.forward,
            half.reverse,
            circle.into(),
            second_half_reversed,
            plane.into(),
            FaceOrientation::Reversed,
            circle2.clone(),
            circle2,
            second_half_reversed,
        )?;
        self.name_mef(lamina, "base.arc1", Some("construction"))?;

        let work_loop = lamina.loop_id;
        self.working.faces.get_mut(lamina.face)?.surface = cone.into();
        self.working.faces.get_mut(lamina.face)?.orientation = FaceOrientation::Forward;
        let boundary = self.collect_loop(work_loop)?;
        if boundary.len() != 2 {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "cone lamina must contain two semicircular edges",
            ));
        }
        let first_base_vertex = self.working.coedge_vertices(boundary[0])?[1];
        let generator0_forward: Curve2 =
            Line2::new(Point2::new(0.0, 1.0), Vec2::new(0.0, -1.0))?.into();
        let generator0_reverse: Curve2 = Line2::new(
            Point2::new(std::f64::consts::TAU, 1.0),
            Vec2::new(0.0, -1.0),
        )?
        .into();
        let apex_spike = self.mev_unchecked(
            work_loop,
            Some(boundary[0]),
            first_base_vertex,
            apex,
            Line3::through(first_point, apex)?.into(),
            unit_range,
            generator0_forward,
            generator0_reverse,
            unit_range,
        )?;
        self.name_mev(apex_spike, "apex", "generator.0")?;

        let side_boundary: Curve2 = Line2::new(Point2::new(0.0, 1.0), Vec2::new(1.0, 0.0))?.into();
        self.working.coedges.get_mut(boundary[0])?.pcurve = side_boundary.clone();
        self.working.coedges.get_mut(boundary[1])?.pcurve = side_boundary;
        let generator1_pcurve: Curve2 =
            Line2::new(Point2::new(std::f64::consts::PI, 1.0), Vec2::new(0.0, -1.0))?.into();
        let split = self.mef_unchecked(
            boundary[0],
            apex_spike.reverse,
            Line3::through(second_point, apex)?.into(),
            unit_range,
            cone.into(),
            FaceOrientation::Forward,
            generator1_pcurve.clone(),
            generator1_pcurve,
            unit_range,
        )?;
        self.name_mef(split, "generator.1", Some("side.1"))?;

        let cap_name = self.names.named("face", "base")?;
        let cap_loop_name = self.names.named("loop.outer", "base")?;
        let side_name = self.names.named("face", "side.0")?;
        let side_loop_name = self.names.named("loop.outer", "side.0")?;
        self.rename_face(seed.face, cap_name)?;
        self.rename_loop(seed.loop_id, cap_loop_name)?;
        self.rename_face(lamina.face, side_name)?;
        self.rename_loop(lamina.loop_id, side_loop_name)?;
        self.names.pop_scope()?;
        self.seal_shell_unchecked(seed.shell)?;
        Ok(seed.body)
    }

    /// Construct a regular torus using the genus-changing Euler operators.
    ///
    /// KEMR/KFMRH first turn a pair of coincident disks into two annular faces;
    /// MEKR then cuts each periodic half at a meridian seam. No connectivity is
    /// patched directly, and every intermediate state remains a valid Euler model.
    pub fn torus(
        &mut self,
        center: Point3,
        x_major: Vec3,
        y_major: Vec3,
        minor_radius: f64,
    ) -> KernelResult<BodyId> {
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self
            .torus_unchecked(center, x_major, y_major, minor_radius)
            .and_then(|body| {
                validate(
                    &self.working,
                    self.kernel.tolerance,
                    ValidationMode::Construction,
                )
                .into_result()
                .map(|_| body)
            });
        if result.is_err() {
            self.working = topology_snapshot;
            self.history = history_snapshot;
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn torus_unchecked(
        &mut self,
        center: Point3,
        x_major: Vec3,
        y_major: Vec3,
        minor_radius: f64,
    ) -> KernelResult<BodyId> {
        let torus = Torus3::new(
            center,
            x_major,
            y_major,
            minor_radius,
            self.kernel.tolerance,
        )?;
        let major_radius = torus.major_radius();
        let x_unit = x_major / major_radius;
        let y_unit = y_major / major_radius;
        let normal = torus.normal();
        let outer_radius = major_radius + minor_radius;
        let inner_radius = major_radius - minor_radius;
        let outer_circle = Circle3::new(
            center,
            x_unit * outer_radius,
            y_unit * outer_radius,
            self.kernel.tolerance,
        )?;
        let inner_circle = Circle3::new(
            center,
            x_unit * inner_radius,
            y_unit * inner_radius,
            self.kernel.tolerance,
        )?;
        let plane = Plane3::new(center, x_major, y_major, self.kernel.tolerance)?;
        let outer_pcurve: Curve2 = Circle2::new(Point2::new(0.0, 0.0), outer_radius)?.into();
        let inner_pcurve: Curve2 = Circle2::new(Point2::new(0.0, 0.0), inner_radius)?.into();
        let first_half = ParameterRange::new(0.0, std::f64::consts::PI)?;
        let second_half_reversed =
            ParameterRange::new(std::f64::consts::TAU, std::f64::consts::PI)?;
        let first_outer = outer_circle.evaluate(0.0)?.point;
        let second_outer = outer_circle.evaluate(std::f64::consts::PI)?.point;
        let first_inner = inner_circle.evaluate(0.0)?.point;
        let second_inner = inner_circle.evaluate(std::f64::consts::PI)?.point;

        let seed = self.mvfs_unchecked(first_outer, plane.into(), FaceOrientation::Forward)?;
        self.names.push_scope("torus")?;
        let outer0_name = self.names.named("vertex", "outer.seam0")?;
        self.rename_vertex(seed.vertex, outer0_name)?;
        let outer_half = self.mev_unchecked(
            seed.loop_id,
            None,
            seed.vertex,
            second_outer,
            outer_circle.into(),
            first_half,
            outer_pcurve.clone(),
            outer_pcurve.clone(),
            first_half,
        )?;
        self.name_mev(outer_half, "outer.seam1", "outer.arc0")?;
        let lamina = self.mef_unchecked(
            outer_half.forward,
            outer_half.reverse,
            outer_circle.into(),
            second_half_reversed,
            plane.into(),
            FaceOrientation::Reversed,
            outer_pcurve.clone(),
            outer_pcurve,
            second_half_reversed,
        )?;
        self.name_mef(lamina, "outer.arc1", Some("lower.annulus"))?;

        // Build a circular disk on a temporary bridge, then use KEMR and KFMRH
        // to transfer its boundary into paired annular holes.
        let outer_first = outer_half.forward;
        let outer_previous = self.working.coedge(outer_first)?.previous;
        let bridge = self.mev_unchecked(
            seed.loop_id,
            Some(outer_previous),
            seed.vertex,
            first_inner,
            Line3::through(first_outer, first_inner)?.into(),
            ParameterRange::new(0.0, 1.0)?,
            Line2::new(
                Point2::new(outer_radius, 0.0),
                Vec2::new(inner_radius - outer_radius, 0.0),
            )?
            .into(),
            Line2::new(
                Point2::new(outer_radius, 0.0),
                Vec2::new(inner_radius - outer_radius, 0.0),
            )?
            .into(),
            ParameterRange::new(0.0, 1.0)?,
        )?;
        self.name_mev(bridge, "inner.seam0", "temporary.bridge")?;
        let inner_half = self.mev_unchecked(
            seed.loop_id,
            Some(bridge.forward),
            bridge.vertex,
            second_inner,
            inner_circle.into(),
            first_half,
            inner_pcurve.clone(),
            inner_pcurve.clone(),
            first_half,
        )?;
        self.name_mev(inner_half, "inner.seam1", "inner.arc0")?;
        let disk = self.mef_unchecked(
            inner_half.reverse,
            inner_half.forward,
            inner_circle.into(),
            ParameterRange::new(std::f64::consts::PI, std::f64::consts::TAU)?,
            plane.into(),
            FaceOrientation::Forward,
            inner_pcurve.clone(),
            inner_pcurve,
            ParameterRange::new(std::f64::consts::PI, std::f64::consts::TAU)?,
        )?;
        self.name_mef(disk, "inner.arc1", Some("temporary.disk"))?;
        self.kemr_unchecked(bridge.edge, bridge.forward)?;
        self.kfmrh_unchecked(lamina.face, disk.face)?;

        self.working.faces.get_mut(seed.face)?.surface = torus.into();
        self.working.faces.get_mut(seed.face)?.orientation = FaceOrientation::Forward;
        self.working.faces.get_mut(lamina.face)?.surface = torus.into();
        self.working.faces.get_mut(lamina.face)?.orientation = FaceOrientation::Forward;
        let horizontal = |v: f64| -> KernelResult<Curve2> {
            Ok(Line2::new(Point2::new(0.0, v), Vec2::new(1.0, 0.0))?.into())
        };
        for coedge in [outer_half.forward, lamina.reverse] {
            self.working.coedges.get_mut(coedge)?.pcurve = horizontal(0.0)?;
        }
        for coedge in [disk.reverse, inner_half.reverse] {
            self.working.coedges.get_mut(coedge)?.pcurve = horizontal(std::f64::consts::PI)?;
        }
        for coedge in [outer_half.reverse, lamina.forward] {
            self.working.coedges.get_mut(coedge)?.pcurve = horizontal(std::f64::consts::TAU)?;
        }
        for coedge in [inner_half.forward, disk.forward] {
            self.working.coedges.get_mut(coedge)?.pcurve = horizontal(std::f64::consts::PI)?;
        }

        let tube_center = center + x_major;
        let tube_circle = Circle3::new(
            tube_center,
            x_unit * minor_radius,
            normal * minor_radius,
            self.kernel.tolerance,
        )?;
        let upper_seam = self.mekr_unchecked(
            outer_half.forward,
            disk.reverse,
            tube_circle.into(),
            ParameterRange::new(0.0, std::f64::consts::PI)?,
            Line2::new(Point2::new(std::f64::consts::TAU, 0.0), Vec2::new(0.0, 1.0))?.into(),
            Line2::new(Point2::new(0.0, 0.0), Vec2::new(0.0, 1.0))?.into(),
            ParameterRange::new(0.0, std::f64::consts::PI)?,
        )?;
        let upper_edge_name = self.names.named("edge", "meridian.upper")?;
        let upper_forward_name = self.names.named("coedge.forward", "meridian.upper")?;
        let upper_reverse_name = self.names.named("coedge.reverse", "meridian.upper")?;
        self.rename_edge(upper_seam.edge, upper_edge_name)?;
        self.rename_coedge(upper_seam.forward, upper_forward_name)?;
        self.rename_coedge(upper_seam.reverse, upper_reverse_name)?;

        let lower_seam = self.mekr_unchecked(
            lamina.forward,
            inner_half.forward,
            tube_circle.into(),
            ParameterRange::new(std::f64::consts::TAU, std::f64::consts::PI)?,
            Line2::new(Point2::new(0.0, 0.0), Vec2::new(0.0, 1.0))?.into(),
            Line2::new(Point2::new(std::f64::consts::TAU, 0.0), Vec2::new(0.0, 1.0))?.into(),
            ParameterRange::new(std::f64::consts::TAU, std::f64::consts::PI)?,
        )?;
        let lower_edge_name = self.names.named("edge", "meridian.lower")?;
        let lower_forward_name = self.names.named("coedge.forward", "meridian.lower")?;
        let lower_reverse_name = self.names.named("coedge.reverse", "meridian.lower")?;
        self.rename_edge(lower_seam.edge, lower_edge_name)?;
        self.rename_coedge(lower_seam.forward, lower_forward_name)?;
        self.rename_coedge(lower_seam.reverse, lower_reverse_name)?;
        let upper_name = self.names.named("face", "upper")?;
        let upper_loop_name = self.names.named("loop.outer", "upper")?;
        let lower_name = self.names.named("face", "lower")?;
        let lower_loop_name = self.names.named("loop.outer", "lower")?;
        self.rename_face(seed.face, upper_name)?;
        self.rename_loop(upper_seam.loop_id, upper_loop_name)?;
        self.rename_face(lamina.face, lower_name)?;
        self.rename_loop(lower_seam.loop_id, lower_loop_name)?;
        self.names.pop_scope()?;
        self.seal_shell_unchecked(seed.shell)?;
        Ok(seed.body)
    }

    fn sweep_circle_loop_unchecked(
        &mut self,
        sweep_loop: LoopId,
        axis: Vec3,
        cylinder: Cylinder3,
        side_orientation: FaceOrientation,
        circle: Circle3,
    ) -> KernelResult<()> {
        let unit_range = ParameterRange::new(0.0, 1.0)?;
        let boundary = self.collect_loop(sweep_loop)?;
        if boundary.len() != 2 {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "circular lamina must contain two semicircular edges",
            ));
        }
        let mut vertices = Vec::new();
        let mut angular_ranges = Vec::new();
        let mut boundary_keys = Vec::new();
        for coedge_id in &boundary {
            let coedge = self.working.coedge(*coedge_id)?;
            vertices.push(self.working.coedge_vertices(*coedge_id)?[0]);
            angular_ranges.push(coedge.range);
            boundary_keys.push(format!(
                "{:016x}",
                self.working.edge(coedge.edge)?.name.local
            ));
        }
        let placeholder: Curve2 = Line2::new(Point2::new(0.0, 0.0), Vec2::new(0.0, 1.0))?.into();
        let mut up = Vec::new();
        let mut down = Vec::new();
        for index in 0..2 {
            let vertex = vertices[index];
            let bottom = self.working.vertex(vertex)?.point;
            let source_key = format!("{:016x}", self.working.vertex(vertex)?.name.local);
            let made = self.mev_unchecked(
                sweep_loop,
                Some(boundary[(index + 1) % 2]),
                vertex,
                bottom + axis,
                Line3::new(bottom, axis)?.into(),
                unit_range,
                placeholder.clone(),
                placeholder.clone(),
                unit_range,
            )?;
            self.name_mev(
                made,
                &format!("top-{source_key}"),
                &format!("vertical-{source_key}"),
            )?;
            up.push(made.forward);
            down.push(made.reverse);
        }
        let mut anchor = None;
        for index in 0..2 {
            let next = (index + 1) % 2;
            let first = if index == 1 {
                anchor.ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "cylinder sweep lost its first top arc",
                    )
                })?
            } else {
                down[next]
            };
            let second = down[index];
            let angular = angular_ranges[index];
            let top_range = ParameterRange::new(angular.end, angular.start)?;
            let side_top: Curve2 = Line2::new(Point2::new(0.0, 1.0), Vec2::new(1.0, 0.0))?.into();
            let side = self.mef_unchecked(
                first,
                second,
                Circle3::new(
                    circle.center() + axis,
                    circle.x_radius(),
                    circle.y_radius(),
                    self.kernel.tolerance,
                )?
                .into(),
                top_range,
                cylinder.into(),
                side_orientation,
                side_top,
                Circle2::new(Point2::new(0.0, 0.0), 1.0)?.into(),
                top_range,
            )?;
            self.name_mef(
                side,
                &format!("top-{}", boundary_keys[index]),
                Some(&format!("side-{}", boundary_keys[index])),
            )?;
            {
                let coedge = self.working.coedges.get_mut(boundary[index])?;
                coedge.pcurve = Line2::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))?.into();
            }
            {
                let coedge = self.working.coedges.get_mut(down[index])?;
                coedge.pcurve =
                    Line2::new(Point2::new(angular.start, 0.0), Vec2::new(0.0, 1.0))?.into();
            }
            let next_angle = angular.end;
            {
                let coedge = self.working.coedges.get_mut(up[next])?;
                coedge.pcurve =
                    Line2::new(Point2::new(next_angle, 0.0), Vec2::new(0.0, 1.0))?.into();
            }
            if index == 0 {
                anchor = Some(side.reverse);
            }
        }
        Ok(())
    }

    fn extrude_curve_profile_unchecked(
        &mut self,
        plane: Plane3,
        profile: &Profile,
        vector: Vec3,
    ) -> KernelResult<BodyId> {
        if !vector.x.is_finite() || !vector.y.is_finite() || !vector.z.is_finite() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "extrusion vector must be finite",
            ));
        }
        let height = vector.dot(plane.normal());
        if height.abs() <= self.kernel.tolerance.linear_at_scale(vector.magnitude()) {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "extrusion vector must have a nonzero component normal to its profile plane",
            ));
        }
        let outer = prepared_wire_segments(profile.outer())?;
        let first_position =
            self.curve2_point_on_plane(plane, &outer[0], outer[0].range().start)?;
        let seed = self.mvfs_unchecked(first_position, plane.into(), FaceOrientation::Forward)?;
        self.names.push_scope("profile")?;
        self.names.push_scope("outer")?;
        let seed_name = self.names.named("vertex", "segment0:start")?;
        self.rename_vertex(seed.vertex, seed_name)?;
        let mut vertices = vec![seed.vertex];
        let mut forwards = Vec::new();
        let mut reverses = Vec::new();
        for (index, segment) in outer.iter().take(outer.len() - 1).enumerate() {
            let end = self.curve2_point_on_plane(plane, segment, segment.range().end)?;
            let curve = self.curve2_on_plane(plane, segment.curve())?;
            let made = self.mev_unchecked(
                seed.loop_id,
                (index != 0).then(|| forwards[index - 1]),
                vertices[index],
                end,
                curve,
                segment.range(),
                segment.curve().clone(),
                segment.curve().clone(),
                segment.range(),
            )?;
            self.name_mev(
                made,
                &format!("segment{index}:end"),
                &format!("segment{index}"),
            )?;
            vertices.push(made.vertex);
            forwards.push(made.forward);
            reverses.push(made.reverse);
        }
        let closing = outer.last().ok_or_else(|| {
            KernelError::new(DiagnosticKind::TopologyInvariant, "profile wire is empty")
        })?;
        let lamina = self.mef_unchecked(
            forwards[0],
            reverses[reverses.len() - 1],
            self.curve2_on_plane(plane, closing.curve())?,
            closing.range().reversed(),
            plane.into(),
            FaceOrientation::Reversed,
            closing.curve().clone(),
            closing.curve().clone(),
            closing.range().reversed(),
        )?;
        self.name_mef(
            lamina,
            &format!("segment{}", outer.len() - 1),
            Some("opposite_cap"),
        )?;
        self.names.pop_scope()?;
        self.names.pop_scope()?;

        let (sweep_face, sweep_loop) = if height > 0.0 {
            (seed.face, seed.loop_id)
        } else {
            (lamina.face, lamina.loop_id)
        };
        let opposite_face = if sweep_face == seed.face {
            lamina.face
        } else {
            seed.face
        };
        let sweep_orientation = self.working.face(sweep_face)?.orientation;
        for (hole_index, hole) in profile.holes().iter().enumerate() {
            self.names.push_scope("profile")?;
            self.names.push_scope(format!("hole{hole_index}"))?;
            self.add_curve_lamina_hole_unchecked(
                sweep_face,
                opposite_face,
                sweep_loop,
                plane,
                hole,
                sweep_orientation,
            )?;
            self.names.pop_scope()?;
            self.names.pop_scope()?;
        }
        let loops_to_sweep = self.working.face(sweep_face)?.loops.clone();
        let translated_plane = Plane3::new(
            plane.origin() + vector,
            plane.u(),
            plane.v(),
            self.kernel.tolerance,
        )?;
        self.working.faces.get_mut(sweep_face)?.surface = translated_plane.into();
        for (index, loop_id) in loops_to_sweep.into_iter().enumerate() {
            self.names.push_scope(if index == 0 {
                "outer_sweep".to_owned()
            } else {
                format!("hole{}_sweep", index - 1)
            })?;
            let side_faces = self.sweep_curve_loop_unchecked(loop_id, vector)?;
            let boundary = if index == 0 {
                super::ProfileBoundary::Outer
            } else {
                super::ProfileBoundary::Hole(index - 1)
            };
            let source_parts = prepared_wire_segment_sources(if index == 0 {
                profile.outer()
            } else {
                &profile.holes()[index - 1]
            });
            for (face, (segment, part)) in side_faces.into_iter().zip(source_parts) {
                let name = self.working.face(face)?.name;
                self.history.created_as(
                    name,
                    super::CreationRole::ExtrusionSide {
                        boundary,
                        segment,
                        part,
                    },
                );
            }
            self.names.pop_scope()?;
        }
        self.names.push_scope("caps")?;
        let start_face_name = self.names.named("face", "start")?;
        let end_face_name = self.names.named("face", "end")?;
        self.rename_face(opposite_face, start_face_name)?;
        self.rename_face(sweep_face, end_face_name)?;
        self.history
            .created_as(start_face_name, super::CreationRole::ExtrusionStart);
        self.history
            .created_as(end_face_name, super::CreationRole::ExtrusionEnd);
        let start_loops = self.working.face(opposite_face)?.loops.clone();
        let end_loops = self.working.face(sweep_face)?.loops.clone();
        for (index, loop_id) in start_loops.into_iter().enumerate() {
            let name = self.names.named(
                if index == 0 {
                    "loop.outer"
                } else {
                    "loop.inner"
                },
                &format!("start{index}"),
            )?;
            self.rename_loop(loop_id, name)?;
        }
        for (index, loop_id) in end_loops.into_iter().enumerate() {
            let name = self.names.named(
                if index == 0 {
                    "loop.outer"
                } else {
                    "loop.inner"
                },
                &format!("end{index}"),
            )?;
            self.rename_loop(loop_id, name)?;
        }
        self.names.pop_scope()?;
        self.seal_shell_unchecked(seed.shell)?;
        Ok(seed.body)
    }

    fn add_curve_lamina_hole_unchecked(
        &mut self,
        sweep_face: FaceId,
        opposite_face: FaceId,
        outer_loop: LoopId,
        plane: Plane3,
        wire: &Wire2,
        orientation: FaceOrientation,
    ) -> KernelResult<()> {
        // `Profile` stores holes clockwise, while the temporary disk used by
        // KEMR/KFMRH must follow the owning face's positive parameter-space
        // orientation.  The Euler operators subsequently turn that disk boundary
        // into an inner loop on each cap.
        let normalized = if orientation == FaceOrientation::Forward {
            wire.reversed()
        } else {
            wire.clone()
        };
        let segments = prepared_wire_segments(&normalized)?;
        let first_outer = self.working.loop_(outer_loop)?.first.ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "cannot attach a hole to an empty outer loop",
            )
        })?;
        let after = self.working.coedge(first_outer)?.previous;
        let outer_vertex = self.working.coedge_vertices(first_outer)?[0];
        let outer_point = self.working.vertex(outer_vertex)?.point;
        let outer_uv = self
            .working
            .coedge(first_outer)?
            .pcurve
            .evaluate(self.working.coedge(first_outer)?.range.start)?
            .point;
        let first_uv = segments[0].start()?;
        let first_point =
            self.curve2_point_on_plane(plane, &segments[0], segments[0].range().start)?;
        let unit = ParameterRange::new(0.0, 1.0)?;
        let bridge_pcurve: Curve2 = Line2::new(outer_uv, first_uv - outer_uv)?.into();
        let bridge = self.mev_unchecked(
            outer_loop,
            Some(after),
            outer_vertex,
            first_point,
            Line3::through(outer_point, first_point)?.into(),
            unit,
            bridge_pcurve.clone(),
            bridge_pcurve,
            unit,
        )?;
        self.name_mev(bridge, "segment0:start", "bridge->segment0")?;
        let mut vertices = vec![bridge.vertex];
        let mut forwards = Vec::new();
        let mut reverses = Vec::new();
        for (index, segment) in segments.iter().take(segments.len() - 1).enumerate() {
            let end = self.curve2_point_on_plane(plane, segment, segment.range().end)?;
            let made = self.mev_unchecked(
                outer_loop,
                Some(if index == 0 {
                    bridge.forward
                } else {
                    forwards[index - 1]
                }),
                vertices[index],
                end,
                self.curve2_on_plane(plane, segment.curve())?,
                segment.range(),
                segment.curve().clone(),
                segment.curve().clone(),
                segment.range(),
            )?;
            self.name_mev(
                made,
                &format!("segment{index}:end"),
                &format!("segment{index}"),
            )?;
            vertices.push(made.vertex);
            forwards.push(made.forward);
            reverses.push(made.reverse);
        }
        let closing = segments.last().ok_or_else(|| {
            KernelError::new(DiagnosticKind::TopologyInvariant, "profile hole is empty")
        })?;
        let disk = self.mef_unchecked(
            reverses[reverses.len() - 1],
            forwards[0],
            self.curve2_on_plane(plane, closing.curve())?,
            closing.range(),
            plane.into(),
            orientation,
            closing.curve().clone(),
            closing.curve().clone(),
            closing.range(),
        )?;
        self.name_mef(
            disk,
            &format!("segment{}", segments.len() - 1),
            Some("temporary_disk"),
        )?;
        let hole_result = self.kemr_unchecked(bridge.edge, bridge.forward)?;
        let sweep_loop_name = self.names.named("loop.inner", "sweep_cap")?;
        self.rename_loop(hole_result.inner_loop, sweep_loop_name)?;
        let opposite_result = self.kfmrh_unchecked(opposite_face, disk.face)?;
        let opposite_loop_name = self.names.named("loop.inner", "opposite_cap")?;
        self.rename_loop(opposite_result.inner_loop, opposite_loop_name)?;
        if self.working.face(sweep_face)?.loops.len() < 2
            || self.working.face(opposite_face)?.loops.len() < 2
        {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "Euler hole construction did not produce paired cap loops",
            ));
        }
        Ok(())
    }

    fn sweep_curve_loop_unchecked(
        &mut self,
        sweep_loop: LoopId,
        vector: Vec3,
    ) -> KernelResult<Vec<FaceId>> {
        let unit = ParameterRange::new(0.0, 1.0)?;
        let boundary = self.collect_loop(sweep_loop)?;
        let loop_kind = self.working.loop_(sweep_loop)?.kind;
        let side_orientation = match loop_kind {
            LoopKind::Outer => FaceOrientation::Forward,
            LoopKind::Inner => FaceOrientation::Reversed,
        };
        if boundary.len() < 2 {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "a swept curve loop needs at least two edges",
            ));
        }
        let mut boundary_vertices = Vec::new();
        let mut boundary_curves = Vec::new();
        let mut boundary_ranges = Vec::new();
        let mut boundary_keys = Vec::new();
        let mut vertex_keys = Vec::new();
        for coedge_id in &boundary {
            let coedge = self.working.coedge(*coedge_id)?;
            let vertex = self.working.coedge_vertices(*coedge_id)?[0];
            let edge = self.working.edge(coedge.edge)?;
            boundary_vertices.push(vertex);
            boundary_curves.push(edge.curve.clone());
            boundary_ranges.push(if coedge.forward {
                edge.range
            } else {
                edge.range.reversed()
            });
            boundary_keys.push(format!("{:016x}", edge.name.local));
            vertex_keys.push(format!("{:016x}", self.working.vertex(vertex)?.name.local));
        }
        let placeholder: Curve2 = Line2::new(Point2::new(0.0, 0.0), Vec2::new(0.0, 1.0))?.into();
        let mut up = Vec::new();
        let mut down = Vec::new();
        for index in 0..boundary.len() {
            let vertex = boundary_vertices[index];
            let bottom = self.working.vertex(vertex)?.point;
            let made = self.mev_unchecked(
                sweep_loop,
                Some(boundary[(index + boundary.len() - 1) % boundary.len()]),
                vertex,
                bottom + vector,
                Line3::new(bottom, vector)?.into(),
                unit,
                placeholder.clone(),
                placeholder.clone(),
                unit,
            )?;
            self.name_mev(
                made,
                &format!("top-{}", vertex_keys[index]),
                &format!("vertical-{}", vertex_keys[index]),
            )?;
            up.push(made.forward);
            down.push(made.reverse);
        }
        let mut anchor = None;
        let mut side_faces = Vec::with_capacity(boundary.len());
        for index in 0..boundary.len() {
            let next = (index + 1) % boundary.len();
            let first = if index + 1 == boundary.len() {
                anchor.ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "extrusion did not retain its first top edge",
                    )
                })?
            } else {
                down[next]
            };
            let second = down[index];
            let curve = &boundary_curves[index];
            let range = boundary_ranges[index];
            let top_curve = translate_curve(curve, vector, self.kernel.tolerance)?;
            let surface = extrusion_surface(curve, vector, self.kernel.tolerance)?;
            let face_orientation = match curve {
                Curve3::Circle(_) if loop_kind == LoopKind::Outer && range.span() < 0.0 => {
                    side_orientation.reversed()
                }
                Curve3::Circle(_) => side_orientation,
                _ if range.span() < 0.0 => FaceOrientation::Reversed,
                _ => FaceOrientation::Forward,
            };
            let side_top: Curve2 = Line2::new(Point2::new(0.0, 1.0), Vec2::new(1.0, 0.0))?.into();
            let cap_pcurve = self.working.coedge(boundary[index])?.pcurve.clone();
            let side = self.mef_unchecked(
                first,
                second,
                top_curve,
                range.reversed(),
                surface,
                face_orientation,
                side_top,
                cap_pcurve,
                range.reversed(),
            )?;
            self.name_mef(
                side,
                &format!("top-{}", boundary_keys[index]),
                Some(&format!("side-{}", boundary_keys[index])),
            )?;
            side_faces.push(side.face);
            self.working.coedges.get_mut(boundary[index])?.pcurve =
                Line2::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))?.into();
            self.working.coedges.get_mut(boundary[index])?.range = range;
            self.working.coedges.get_mut(down[index])?.pcurve =
                Line2::new(Point2::new(range.start, 0.0), Vec2::new(0.0, 1.0))?.into();
            self.working.coedges.get_mut(up[next])?.pcurve =
                Line2::new(Point2::new(range.end, 0.0), Vec2::new(0.0, 1.0))?.into();
            if index == 0 {
                anchor = Some(side.reverse);
            }
        }
        Ok(side_faces)
    }

    fn curve2_point_on_plane(
        &self,
        plane: Plane3,
        segment: &TrimmedCurve2,
        parameter: f64,
    ) -> KernelResult<Point3> {
        let point = segment.curve().evaluate(parameter)?.point;
        Ok(plane.evaluate(point.x, point.y)?.point)
    }

    fn curve2_on_plane(&self, plane: Plane3, curve: &Curve2) -> KernelResult<Curve3> {
        Ok(match curve {
            Curve2::Line(line) => Line3::new(
                plane.evaluate(line.origin().x, line.origin().y)?.point,
                plane.u() * line.direction().x + plane.v() * line.direction().y,
            )?
            .into(),
            Curve2::Circle(circle) => Circle3::new(
                plane.evaluate(circle.center().x, circle.center().y)?.point,
                plane.u() * circle.radius(),
                plane.v() * circle.radius(),
                self.kernel.tolerance,
            )?
            .into(),
            Curve2::Nurbs(curve) => NurbsCurve3::new(
                curve.degree(),
                curve
                    .control_points()
                    .iter()
                    .map(|point| plane.evaluate(point.x, point.y).map(|value| value.point))
                    .collect::<KernelResult<Vec<_>>>()?,
                curve.weights().to_vec(),
                curve.knots().to_vec(),
            )?
            .into(),
        })
    }

    fn collect_loop(&self, loop_id: LoopId) -> KernelResult<Vec<CoedgeId>> {
        let first = self.working.loop_(loop_id)?.first.ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "cannot collect an empty Euler loop",
            )
        })?;
        let mut output = Vec::new();
        let mut current = first;
        loop {
            output.push(current);
            current = self.working.coedge(current)?.next;
            if current == first {
                return Ok(output);
            }
            if output.len() > self.working.coedges.len() {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "Euler loop does not close",
                ));
            }
        }
    }

    /// Publish the result. This is the only path by which an Euler workspace can
    /// replace kernel state.
    pub fn commit(self) -> KernelResult<OperationHistory> {
        self.finish().map(|committed| committed.history)
    }

    pub(crate) fn finish(self) -> KernelResult<CommittedOperation> {
        validate(
            &self.working,
            self.kernel.tolerance,
            ValidationMode::Published,
        )
        .into_result()?;
        let all_certificates =
            super::physical::certify_topology(&self.working, self.kernel.tolerance)
                .map_err(KernelError::from_diagnostics)?;
        let certificates = self
            .touched_bodies
            .iter()
            .filter_map(|body| {
                all_certificates
                    .get(body)
                    .cloned()
                    .map(|certificate| (*body, certificate))
            })
            .collect();
        let mut current_names = std::collections::BTreeSet::new();
        for body in &self.touched_bodies {
            current_names.extend(self.working.body_entity_names(*body)?);
        }
        if !self.composes_imported_topology
            && current_names
                .iter()
                .any(|name| name.namespace != self.namespace)
        {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "a transaction may publish only topology owned by its operation namespace",
            ));
        }
        self.kernel.topology = self.working;
        Ok(CommittedOperation {
            operation: self.namespace,
            bodies: self.touched_bodies,
            history: self.history,
            certificates,
        })
    }
}

fn prepared_wire_segments(wire: &Wire2) -> KernelResult<Vec<TrimmedCurve2>> {
    if wire.segments().len() != 1 {
        return Ok(wire.segments().to_vec());
    }
    let segment = &wire.segments()[0];
    let middle = (segment.range().start + segment.range().end) * 0.5;
    Ok(vec![
        TrimmedCurve2::new(
            segment.curve().clone(),
            ParameterRange::new(segment.range().start, middle)?,
        )?,
        TrimmedCurve2::new(
            segment.curve().clone(),
            ParameterRange::new(middle, segment.range().end)?,
        )?,
    ])
}

fn prepared_wire_segment_sources(wire: &Wire2) -> Vec<(usize, usize)> {
    if wire.segments().len() == 1 {
        vec![(0, 0), (0, 1)]
    } else {
        wire.segments()
            .iter()
            .enumerate()
            .map(|(segment, _)| (segment, 0))
            .collect()
    }
}

fn translate_curve(
    curve: &Curve3,
    vector: Vec3,
    tolerance: super::ModelTolerance,
) -> KernelResult<Curve3> {
    Ok(match curve {
        Curve3::Line(line) => Line3::new(line.origin() + vector, line.direction())?.into(),
        Curve3::Circle(circle) => Circle3::new(
            circle.center() + vector,
            circle.x_radius(),
            circle.y_radius(),
            tolerance,
        )?
        .into(),
        Curve3::Nurbs(curve) => NurbsCurve3::new(
            curve.degree(),
            curve
                .control_points()
                .iter()
                .map(|point| *point + vector)
                .collect(),
            curve.weights().to_vec(),
            curve.knots().to_vec(),
        )?
        .into(),
    })
}

fn extrusion_surface(
    curve: &Curve3,
    vector: Vec3,
    tolerance: super::ModelTolerance,
) -> KernelResult<Surface3> {
    Ok(match curve {
        Curve3::Line(line) => {
            Plane3::new(line.origin(), line.direction(), vector, tolerance)?.into()
        }
        Curve3::Circle(circle) => super::Cylinder3::new(
            circle.center(),
            vector,
            circle.x_radius(),
            circle.y_radius(),
            tolerance,
        )?
        .into(),
        Curve3::Nurbs(curve) => {
            let bottom = curve.control_points().to_vec();
            let top = bottom.iter().map(|point| *point + vector).collect();
            NurbsSurface3::new(
                (curve.degree(), 1),
                vec![bottom, top],
                vec![curve.weights().to_vec(), curve.weights().to_vec()],
                (curve.knots().to_vec(), vec![0.0, 0.0, 1.0, 1.0]),
            )?
            .into()
        }
    })
}

fn polyhedral_edge_key(mesh: &PolyhedralMesh, first: usize, second: usize) -> String {
    let first = &mesh.vertices()[first].key;
    let second = &mesh.vertices()[second].key;
    if first < second {
        format!("{first}<->{second}")
    } else {
        format!("{second}<->{first}")
    }
}
