use std::collections::{BTreeMap, BTreeSet};

use uuid::Uuid;

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3};

use super::history::ChangeKind;
use super::naming::NameSource;
use super::polyhedron::PolyhedralTriangle;
use super::topology::{Body, Coedge, Edge, Face, Loop, Shell, Vertex};
use super::validation::{validate, ValidationMode};
use super::{
    BodyId, Circle2, Circle3, CoedgeId, Cone3, Curve2, Curve3, Cylinder3, DiagnosticKind, EdgeId,
    FaceId, FaceOrientation, Kernel, KernelError, KernelResult, Line2, Line3, LoopId, LoopKind,
    ModelTolerance, NurbsCurve3, NurbsSurface3, OperationHistory, ParameterRange, Plane3,
    PersistentId, PolyhedralMesh, Profile, ShellId, ShellKind, Sphere3, Surface3, Topology,
    Torus3, TrimmedCurve2, VertexId, Wire2,
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

/// One oriented exact boundary segment used by the generic BREP shell builder.
///
/// The 3D curve and the p-curve share the same parameter. `range` is directed
/// in loop order; the shell builder owns edge sharing and coedge orientation.
#[derive(Debug, Clone)]
pub(crate) struct BrepPatchSegment {
    pub curve: Curve3,
    pub pcurve: Curve2,
    pub range: ParameterRange,
    pub provenance: SegmentProvenance,
}

/// Correspondence attached before intersection, trimming or reparameterization.
#[derive(Debug, Clone, Default)]
pub(crate) struct SegmentProvenance {
    pub edges: Vec<super::history::EntitySource>,
    pub vertices: [Vec<super::history::EntitySource>; 2],
    pub boundaries: Vec<(super::PersistentId, ParameterRange, ParameterRange)>,
}

impl SegmentProvenance {
    pub(crate) fn from_coedge(topology: &Topology, id: CoedgeId) -> KernelResult<Self> {
        let coedge = topology.coedge(id)?;
        let edge = topology.edge(coedge.edge())?;
        let vertices = topology.coedge_vertices(id)?;
        Ok(Self {
            edges: vec![super::history::EntitySource::preserved(edge.name())],
            vertices: [
                vec![super::history::EntitySource::preserved(
                    topology.vertex(vertices[0])?.name(),
                )],
                vec![super::history::EntitySource::preserved(
                    topology.vertex(vertices[1])?.name(),
                )],
            ],
            boundaries: vec![(coedge.name(), coedge.range(), coedge.range())],
        })
    }

    pub(crate) fn trimmed(&self, original: ParameterRange, retained: ParameterRange) -> Self {
        if original == retained.reversed() {
            return self.reversed();
        }
        if original.span().signum() != retained.span().signum() {
            return self.reversed().trimmed(original.reversed(), retained);
        }
        let mut result = self.clone();
        if original != retained {
            for edge in &mut result.edges {
                edge.unchanged = false;
            }
            if original.start != retained.start {
                result.vertices[0].clear();
            }
            if original.end != retained.end {
                result.vertices[1].clear();
            }
            for (_, _, range) in &mut result.boundaries {
                let start = if retained.start == original.start {
                    range.start
                } else {
                    range.start
                        + range.span() * ((retained.start - original.start) / original.span())
                };
                let end = if retained.end == original.end {
                    range.end
                } else {
                    range.start + range.span() * ((retained.end - original.start) / original.span())
                };
                *range = ParameterRange { start, end };
            }
        }
        result
    }

    pub(crate) fn reversed(&self) -> Self {
        Self {
            edges: self.edges.clone(),
            vertices: [self.vertices[1].clone(), self.vertices[0].clone()],
            boundaries: self
                .boundaries
                .iter()
                .map(|(id, original, range)| (*id, *original, range.reversed()))
                .collect(),
        }
    }

    pub(crate) fn modified(&self) -> Self {
        let mut result = self.clone();
        for source in result
            .edges
            .iter_mut()
            .chain(result.vertices.iter_mut().flatten())
        {
            source.unchanged = false;
        }
        result.boundaries.clear();
        result
    }

    pub(crate) fn weld(&mut self, other: &Self) {
        for source in &other.edges {
            if !self.edges.contains(source) {
                self.edges.push(*source);
            }
        }
        for end in 0..2 {
            for source in &other.vertices[end] {
                if !self.vertices[end].contains(source) {
                    self.vertices[end].push(*source);
                }
            }
        }
        for boundary in &other.boundaries {
            if !self.boundaries.contains(boundary) {
                self.boundaries.push(*boundary);
            }
        }
    }

    pub(crate) fn through(&mut self, history: &OperationHistory) {
        for records in std::iter::once(&mut self.edges).chain(self.vertices.iter_mut()) {
            *records = records
                .iter()
                .flat_map(|record| {
                    history.construction.get(&record.name).map_or_else(
                        || vec![*record],
                        |sources| {
                            sources
                                .iter()
                                .map(|source| super::history::EntitySource {
                                    name: source.name,
                                    unchanged: record.unchanged && source.unchanged,
                                })
                                .collect()
                        },
                    )
                })
                .collect();
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BrepPatchLoop {
    pub kind: LoopKind,
    pub segments: Vec<BrepPatchSegment>,
}

#[derive(Debug, Clone)]
pub(crate) struct BrepPatch {
    pub sources: Vec<super::PersistentId>,
    pub preserved: Vec<super::PersistentId>,
    pub surface: Surface3,
    pub orientation: FaceOrientation,
    pub loops: Vec<BrepPatchLoop>,
}

impl BrepPatch {
    /// Test coverage of construction tokens, not geometric similarity. Each
    /// directed source coedge must survive in full, with no added boundary.
    pub(crate) fn retains_boundary(&self, topology: &Topology, face: FaceId) -> KernelResult<bool> {
        let mut expected = BTreeMap::new();
        for loop_id in topology.face(face)?.loops() {
            for id in topology.loop_coedges(*loop_id)? {
                let coedge = topology.coedge(id)?;
                expected.insert(coedge.name(), coedge.range());
            }
        }
        let mut intervals = BTreeMap::<super::PersistentId, Vec<(f64, f64)>>::new();
        for segment in self.loops.iter().flat_map(|loop_| &loop_.segments) {
            let mut covered = false;
            for (name, original, range) in &segment.provenance.boundaries {
                if expected.get(name) != Some(original) {
                    continue;
                }
                let start = (range.start - original.start) / original.span();
                let end = (range.end - original.start) / original.span();
                if end <= start {
                    continue;
                }
                intervals.entry(*name).or_default().push((start, end));
                covered = true;
            }
            if !covered {
                return Ok(false);
            }
        }
        for name in expected.keys() {
            let Some(spans) = intervals.get_mut(name) else {
                return Ok(false);
            };
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut end: f64 = 0.0;
            for &(a, b) in spans.iter() {
                if a > end {
                    return Ok(false);
                }
                end = end.max(b);
            }
            if end != 1.0 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// A fully prepared join; discarded candidates leave topology untouched.
struct EdgeMerge {
    vertex: VertexId,
    survivor: EdgeId,
    removed: EdgeId,
    prepend: bool,
    range: ParameterRange,
    coedges: Vec<CoedgeMerge>,
}

struct CoedgeMerge {
    survivor: CoedgeId,
    removed: CoedgeId,
    pcurve: Curve2,
    range: ParameterRange,
}

/// Retain an exact curve and extend its trim across a compatible subdivision.
/// Non-analytic curves currently require identical supports and join parameters.
fn same_domain_edge_range(
    first: &Edge,
    second: &Edge,
    vertex: VertexId,
    tol: ModelTolerance,
) -> KernelResult<Option<ParameterRange>> {
    let same = match (&first.curve, &second.curve) {
        (Curve3::Line(a), Curve3::Line(b)) => {
            a.direction()
                .normalize()
                .cross(b.direction().normalize())
                .magnitude()
                <= tol.angular
                && (a.origin() - b.origin())
                    .cross(a.direction().normalize())
                    .magnitude()
                    <= tol.linear
        }
        (Curve3::Circle(a), Curve3::Circle(b)) => {
            a.center().distance(b.center()) <= tol.linear
                && (a.radius() - b.radius()).abs() <= tol.linear
                && a.normal().cross(b.normal()).magnitude() <= tol.angular
        }
        (a, b) => a == b,
    };
    if !same {
        return Ok(None);
    }
    let prepend = first.vertices[0] == vertex;
    let first_join = if prepend {
        first.range.start
    } else {
        first.range.end
    };
    let second_join = if second.vertices[0] == vertex {
        second.range.start
    } else {
        second.range.end
    };
    let second_end = if second.vertices[0] == vertex {
        second.range.end
    } else {
        second.range.start
    };
    if !matches!(first.curve, Curve3::Line(_) | Curve3::Circle(_))
        && (first_join - second_join).abs() > tol.parameter
    {
        return Ok(None);
    }
    let a = first.curve.evaluate(first_join)?.first_derivative;
    let b = second.curve.evaluate(second_join)?.first_derivative;
    let scale = b.dot(a) / a.magnitude2();
    let extension = (second_end - second_join) * scale;
    if !extension.is_finite() || (extension.signum() == first.range.span().signum()) == prepend {
        return Ok(None);
    }
    let agrees = (0..=4).all(|i| {
        let fraction = i as f64 / 4.0;
        let a = first.curve.evaluate(first_join + extension * fraction);
        let b = second
            .curve
            .evaluate(second_join + (second_end - second_join) * fraction);
        matches!((a, b), (Ok(a), Ok(b)) if a.point.distance(b.point) <= tol.linear)
    });
    if !agrees {
        return Ok(None);
    }
    let range = ParameterRange::new(
        if prepend {
            first_join + extension
        } else {
            first.range.start
        },
        if prepend {
            first.range.end
        } else {
            first_join + extension
        },
    )?;
    if let Some(period) = first.curve.domain().period {
        if range.span().abs() > period + tol.parameter * 32.0 {
            return Ok(None);
        }
    }
    if !first.curve.domain().contains_range(range, tol.parameter) {
        return Ok(None);
    }
    Ok(Some(range))
}

/// Imprint existing boundary vertices onto unpaired exact trims before sewing.
/// Adjacent faces may describe one physical edge with different subdivisions.
/// Only parameter ranges change; supports, curves, and p-curves remain exact.
pub(crate) fn conform_brep_boundaries(
    patches: &mut [BrepPatch],
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    conform_brep_boundaries_impl(patches, tolerance, true)
}

// Cells already have paired edges inside each shell. Match subdivisions across
// those shells as well before removing their common faces and sewing them.
pub(crate) fn conform_brep_interfaces(
    patches: &mut [BrepPatch],
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    conform_brep_boundaries_impl(patches, tolerance, false)
}

fn conform_brep_boundaries_impl(
    patches: &mut [BrepPatch],
    tolerance: ModelTolerance,
    preserve_paired: bool,
) -> KernelResult<()> {
    let segments = patches
        .iter()
        .flat_map(|p| &p.loops)
        .flat_map(|l| &l.segments)
        .collect::<Vec<_>>();
    let ends = segments
        .iter()
        .map(|s| {
            Ok([
                s.curve.evaluate(s.range.start)?.point,
                s.curve.evaluate(s.range.end)?.point,
            ])
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let close = |a: Point3, b: Point3| {
        a.distance(b)
            <= tolerance
                .linear_at_scale(a.to_vec().magnitude().max(b.to_vec().magnitude()).max(1.0))
                * 8.0
    };
    let middles = segments
        .iter()
        .map(|s| {
            s.curve
                .evaluate((s.range.start + s.range.end) * 0.5)
                .map(|v| v.point)
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let paired = ends
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if !preserve_paired {
                return false;
            }
            ends.iter().enumerate().any(|(j, b)| {
                if i == j {
                    return false;
                }
                if (close(a[0], b[0]) && close(a[1], b[1]))
                    || (close(a[0], b[1]) && close(a[1], b[0]))
                {
                    return close(middles[i], middles[j])
                        || segments[i]
                            .curve
                            .project_on_trim(middles[j], segments[i].range, tolerance)
                            .is_ok_and(|p| {
                                let mut t = p.parameter;
                                if let Some(period) = segments[i].curve.domain().period {
                                    t += (((segments[i].range.start + segments[i].range.end)
                                        * 0.5
                                        - t)
                                        / period)
                                        .round()
                                        * period;
                                }
                                close(p.point, middles[j])
                                    && segments[i].range.contains(t, tolerance)
                            });
                }
                match (&segments[i].curve, &segments[j].curve) {
                    (Curve3::Circle(first), Curve3::Circle(second)) => {
                        close(a[0], a[1])
                            && close(b[0], b[1])
                            && close(first.center(), second.center())
                            && (first.radius() - second.radius()).abs() <= tolerance.linear * 8.0
                            && first.normal().dot(second.normal()).abs()
                                >= 1.0 - tolerance.angular * 8.0
                    }
                    _ => false,
                }
            })
        })
        .collect::<Vec<_>>();
    if preserve_paired && paired.iter().all(|p| *p) {
        return Ok(());
    }
    let mut vertices = Vec::new();
    for point in ends.iter().flatten() {
        if !vertices.iter().any(|candidate| close(*point, *candidate)) {
            vertices.push(*point);
        }
    }
    let mut index = 0;
    for loop_ in patches.iter_mut().flat_map(|p| &mut p.loops) {
        let mut conformed = Vec::new();
        for segment in std::mem::take(&mut loop_.segments) {
            let current = index;
            index += 1;
            if preserve_paired && paired[current] {
                conformed.push(segment);
                continue;
            }
            let mut parameters = vec![segment.range.start, segment.range.end];
            for point in &vertices {
                if ends[current].iter().any(|end| close(*end, *point)) {
                    continue;
                }
                let Ok(projection) =
                    segment
                        .curve
                        .project_on_trim(*point, segment.range, tolerance)
                else {
                    continue;
                };
                if !close(projection.point, *point) {
                    continue;
                }
                let mut parameter = projection.parameter;
                if let Some(period) = segment.curve.domain().period {
                    parameter += (((segment.range.start + segment.range.end) * 0.5 - parameter)
                        / period)
                        .round()
                        * period;
                }
                if segment.range.contains(parameter, tolerance)
                    && !parameters
                        .iter()
                        .any(|p| (*p - parameter).abs() <= tolerance.parameter * 64.0)
                {
                    parameters.push(parameter.clamp(
                        segment.range.start.min(segment.range.end),
                        segment.range.start.max(segment.range.end),
                    ));
                }
            }
            parameters.sort_by(f64::total_cmp);
            if segment.range.span() < 0.0 {
                parameters.reverse();
            }
            for pair in parameters.windows(2) {
                conformed.push(BrepPatchSegment {
                    range: ParameterRange::new(pair[0], pair[1])?,
                    provenance: segment
                        .provenance
                        .trimmed(segment.range, ParameterRange::new(pair[0], pair[1])?),
                    ..segment.clone()
                });
            }
        }
        loop_.segments = conformed;
    }
    Ok(())
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
    pub(super) kernel: &'kernel mut Kernel,
    pub(super) working: Topology,
    pub(super) names: NameSource,
    pub(super) history: OperationHistory,
    pub(super) touched_bodies: Vec<BodyId>,
    namespace: Uuid,
    pub(super) composes_imported_topology: bool,
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

    fn record_loop_edit(&mut self, id: LoopId) -> KernelResult<()> {
        let loop_ = self.working.loop_(id)?;
        let face = self.working.face(loop_.face)?;
        let shell = self.working.shell(face.shell)?;
        let body = self.working.body(shell.body)?;
        for name in [loop_.name, face.name, shell.name, body.name] {
            self.history.modified(name);
        }
        Ok(())
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
            .derived(vertex_name, [source_name], super::DerivationRole::MevVertex);
        self.history
            .derived(edge_name, [source_name, vertex_name], super::DerivationRole::MevEdge);
        self.history
            .derived(forward_name, [edge_name], super::DerivationRole::MevForwardUse);
        self.history
            .derived(reverse_name, [edge_name], super::DerivationRole::MevReverseUse);
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
            .derived(edge_name, [start_name, end_name], super::DerivationRole::MefEdge);
        self.history
            .derived(forward_name, [edge_name], super::DerivationRole::MefNewFaceUse);
        self.history
            .derived(reverse_name, [edge_name], super::DerivationRole::MefOldFaceUse);
        self.history.derived(face_name, [old_face_name], super::DerivationRole::MefFace);
        self.history
            .derived(loop_name, [face_name], super::DerivationRole::MefOuterLoop);
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
        self.record_loop_edit(loop_id)?;
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
            self.history.derived(keep_face_name, sources, super::DerivationRole::KefFace);
        }
        self.record_loop_edit(keep_loop)?;
        Ok(KefResult {
            surviving_face: keep_face,
            loop_id: keep_loop,
        })
    }

    /// Normalize redundant partitions when an operation may have created them.
    /// Removing face partitions can expose degree-two edge subdivisions.
    pub(crate) fn unify_same_domain(&mut self) -> KernelResult<()> {
        self.merge_periodic_face_partitions()?;
        self.unify_same_domain_edges()
    }

    /// Remove Boolean partition edges between patches in the same periodic
    /// surface chart. Keep actual seams (two uses on one face), distinct
    /// supports, and disconnected face regions intact.
    fn merge_periodic_face_partitions(&mut self) -> KernelResult<()> {
        loop {
            let mut candidate = None;
            for (edge_id, edge) in self.working.edges() {
                if edge.coedges.len() != 2 {
                    continue;
                }
                let first_use = self.working.coedge(edge.coedges[0])?;
                let second_use = self.working.coedge(edge.coedges[1])?;
                let first_loop = self.working.loop_(first_use.loop_id)?;
                let second_loop = self.working.loop_(second_use.loop_id)?;
                if first_loop.face == second_loop.face
                    || first_loop.kind != LoopKind::Outer
                    || second_loop.kind != LoopKind::Outer
                {
                    continue;
                }
                let first = self.working.face(first_loop.face)?;
                let second = self.working.face(second_loop.face)?;
                if first.surface != second.surface
                    || first.orientation != second.orientation
                    || second.loops.len() != 1
                    || !matches!(first.surface, Surface3::Cylinder(_) | Surface3::Torus(_))
                {
                    continue;
                }
                // Put the removed patch in the survivor's periodic chart
                // before splicing its boundary. A complete period is the only
                // permissible change to these exact p-curves.
                let first_uv = first_use.pcurve.evaluate(first_use.range.start)?.point;
                let second_uv = second_use.pcurve.evaluate(second_use.range.end)?.point;
                let domains = first.surface.domains();
                let delta = first_uv - second_uv;
                let offset = Vec2::new(
                    domains[0]
                        .period
                        .map_or(0.0, |period| (delta.x / period).round() * period),
                    domains[1]
                        .period
                        .map_or(0.0, |period| (delta.y / period).round() * period),
                );
                if (delta - offset).magnitude() > self.kernel.tolerance.parameter * 16.0 {
                    continue;
                }
                candidate = Some((edge_id, second_loop.face, second_use.loop_id, offset));
                break;
            }
            let Some((edge, removed_face, removed_loop, offset)) = candidate else {
                break;
            };
            if offset.magnitude2() > 0.0 {
                for coedge in self.collect_loop(removed_loop)? {
                    let translated = super::trim::translate_pcurve(
                        &self.working.coedge(coedge)?.pcurve,
                        offset,
                        self.kernel.tolerance,
                    )?;
                    self.working.coedges.get_mut(coedge)?.pcurve = translated;
                }
            }
            let removed_name = self.working.face(removed_face)?.name;
            let survivor = self.kef_unchecked(edge, removed_face)?.surviving_face;
            let survivor_name = self.working.face(survivor)?.name;
            self.history
                .changed(removed_name, ChangeKind::Merged, [survivor_name]);
        }
        Ok(())
    }

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

    /// Remove degree-two subdivisions using a local incidence index. Only the
    /// surviving endpoints need reconsideration after each merge.
    pub(crate) fn unify_same_domain_edges(&mut self) -> KernelResult<()> {
        let mut incidence = BTreeMap::<VertexId, BTreeSet<EdgeId>>::new();
        for (id, edge) in self.working.edges() {
            for vertex in edge.vertices {
                incidence.entry(vertex).or_default().insert(id);
            }
        }
        let mut pending = incidence
            .iter()
            .filter_map(|(vertex, edges)| (edges.len() == 2).then_some(*vertex))
            .collect::<BTreeSet<_>>();
        while let Some(vertex) = pending.pop_first() {
            let Some(edges) = incidence.get(&vertex) else {
                continue;
            };
            if edges.len() != 2 {
                continue;
            }
            let mut edges = edges.iter().copied();
            let edges = [edges.next().unwrap(), edges.next().unwrap()];
            let Some(candidate) = self.prepare_edge_merge(vertex, edges)? else {
                continue;
            };
            let survivor = candidate.survivor;
            let removed = candidate.removed;
            let endpoints = self.apply_edge_merge(candidate)?;
            incidence.remove(&vertex);
            for endpoint in endpoints {
                let edges = incidence
                    .get_mut(&endpoint)
                    .expect("surviving endpoint is indexed");
                edges.remove(&removed);
                edges.insert(survivor);
                pending.insert(endpoint);
            }
        }
        Ok(())
    }

    /// Prepare both face uses before changing any connectivity. Geometrically
    /// equivalent supports are insufficient without compatible loop adjacency.
    fn prepare_edge_merge(
        &self,
        vertex: VertexId,
        edges: [EdgeId; 2],
    ) -> KernelResult<Option<EdgeMerge>> {
        let first = self.working.edge(edges[0])?;
        let second = self.working.edge(edges[1])?;
        if first.coedges.len() != 2
            || second.coedges.len() != 2
            || first.vertices[0] == first.vertices[1]
            || second.vertices[0] == second.vertices[1]
            || self.working.edge_faces(edges[0])?.len() != 2
            || self.working.edge_faces(edges[1])?.len() != 2
        {
            return Ok(None);
        }
        let Some(range) = same_domain_edge_range(first, second, vertex, self.kernel.tolerance)? else {
            return Ok(None);
        };
        let tol = self.kernel.tolerance;
        let mut coedges = Vec::new();
        for id in &first.coedges {
            let keep = self.working.coedge(*id)?;
            let other = second.coedges.iter().copied().find(|other| {
                self.working.coedge(*other).is_ok_and(|value| {
                    value.loop_id == keep.loop_id && (keep.next == *other || keep.previous == *other)
                })
            });
            let Some(other) = other else {
                break;
            };
            let map = |t: f64| {
                let fraction = (t - first.range.start) / first.range.span();
                keep.range.start
                    + keep.range.span()
                        * if keep.forward {
                            fraction
                        } else {
                            1.0 - fraction
                        }
            };
            let start = map(if keep.forward { range.start } else { range.end });
            let end = map(if keep.forward { range.end } else { range.start });
            let use_range = ParameterRange::new(start, end)?;
            let Ok(pcurve) = keep
                .pcurve
                .extended(ParameterRange::new(start.min(end), start.max(end))?)
            else {
                break;
            };
            let surface = &self
                .working
                .face(self.working.loop_(keep.loop_id)?.face)?
                .surface;
            let valid = (0..=16).all(|i| {
                let fraction = i as f64 / 16.0;
                let t = range.start + range.span() * fraction;
                let uv = pcurve.evaluate(map(t));
                let point = first.curve.evaluate(t);
                match (uv, point) {
                    (Ok(uv), Ok(point)) => {
                        surface.evaluate(uv.point.x, uv.point.y).is_ok_and(|value| {
                            value.point.distance(point.point)
                                <= tol.linear_at_scale(point.point.to_vec().magnitude()) * 8.0
                        })
                    }
                    _ => false,
                }
            });
            if !valid {
                break;
            }
            coedges.push(CoedgeMerge {
                survivor: *id,
                removed: other,
                pcurve,
                range: use_range,
            });
        }
        if coedges.len() != 2 || coedges[0].removed == coedges[1].removed {
            return Ok(None);
        }
        Ok(Some(EdgeMerge {
            vertex,
            survivor: edges[0],
            removed: edges[1],
            prepend: first.vertices[0] == vertex,
            range,
            coedges,
        }))
    }

    /// Apply a prepared edge join, retaining the surviving edge's identity.
    fn apply_edge_merge(&mut self, candidate: EdgeMerge) -> KernelResult<[VertexId; 2]> {
        let EdgeMerge {
            vertex,
            survivor,
            removed,
            prepend,
            range,
            coedges,
        } = candidate;
        let removed_edge = self.working.edge(removed)?.clone();
        let other_vertex = removed_edge
            .vertices
            .iter()
            .copied()
            .find(|v| *v != vertex)
            .unwrap();
        for CoedgeMerge {
            survivor: keep_id,
            removed: remove_id,
            pcurve,
            range: use_range,
        } in coedges
        {
            let removed_use = self.working.coedge(remove_id)?.clone();
            self.working.coedges.get_mut(removed_use.previous)?.next = removed_use.next;
            self.working.coedges.get_mut(removed_use.next)?.previous = removed_use.previous;
            if self.working.loop_(removed_use.loop_id)?.first == Some(remove_id) {
                self.working.loops.get_mut(removed_use.loop_id)?.first = Some(keep_id);
            }
            let keep = self.working.coedges.get_mut(keep_id)?;
            keep.pcurve = pcurve;
            keep.range = use_range;
            let name = keep.name;
            self.working.coedges.remove(remove_id)?;
            self.history
                .changed(removed_use.name, ChangeKind::Merged, [name]);
            self.history.modified(name);
            self.record_loop_edit(removed_use.loop_id)?;
        }
        let edge = self.working.edges.get_mut(survivor)?;
        edge.vertices[if prepend { 0 } else { 1 }] = other_vertex;
        edge.range = range;
        let name = edge.name;
        self.working.edges.remove(removed)?;
        self.history.merge_construction(removed_edge.name, name);
        self.history
            .changed(removed_edge.name, ChangeKind::Merged, [name]);
        self.history.changed(name, ChangeKind::Modified, [name]);
        let vertex_name = self.working.vertex(vertex)?.name;
        self.working.vertices.remove(vertex)?;
        self.history.changed(vertex_name, ChangeKind::Deleted, []);
        let edge = self.working.edge(survivor)?;
        let endpoints = edge.vertices;
        let uses = edge.coedges.clone();
        for endpoint in endpoints {
            let outgoing = uses.iter().copied().find(|id| {
                self.working
                    .coedge_vertices(*id)
                    .is_ok_and(|vertices| vertices[0] == endpoint)
            });
            self.working.vertices.get_mut(endpoint)?.outgoing = outgoing;
        }
        Ok(endpoints)
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
            super::DerivationRole::KemrInnerLoop,
        );
        for name in [edge_name, removed_use_names[0], removed_use_names[1]] {
            self.history
                .changed(name, ChangeKind::Deleted, std::iter::empty());
        }
        let face_name = self.working.face(face)?.name;
        self.history
            .changed(face_name, ChangeKind::Modified, [face_name]);
        self.record_loop_edit(outer_loop)?;
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
            .derived(edge_name, [face_name], super::DerivationRole::MekrBridgeEdge);
        self.history
            .derived(forward_name, [edge_name], super::DerivationRole::MekrForwardUse);
        self.history
            .derived(reverse_name, [edge_name], super::DerivationRole::MekrReverseUse);
        self.history
            .changed(inner_loop_name, ChangeKind::Deleted, std::iter::empty());
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
        self.record_loop_edit(inner_loop)?;
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
            .derived(face_name, [old_face_name, loop_name], super::DerivationRole::MfkrhFace);
        self.history
            .changed(loop_name, ChangeKind::Modified, [loop_name]);
        self.history
            .changed(old_face_name, ChangeKind::Modified, [old_face_name]);
        self.record_loop_edit(inner_loop)?;
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
                    Curve3::SurfaceIntersection(curve) => {
                        curve.pcurve_for_surface(&surface).cloned().ok_or_else(|| {
                            KernelError::new(
                                DiagnosticKind::GeometryInvariant,
                                "implicit intersection edge does not retain the cylindrical face",
                            )
                        })?
                    }
                    Curve3::SurfaceCurve(curve) => {
                        if curve.surface() != &surface {
                            return Err(KernelError::new(
                                DiagnosticKind::GeometryInvariant,
                                "surface curve does not retain the cylindrical face",
                            ));
                        }
                        curve.pcurve().clone()
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

    /// Assemble analytical face patches into one closed, manifold shell.
    ///
    /// This is the Euler composition used after a generic Boolean has imprinted
    /// and classified the operand faces.  It deliberately knows nothing about
    /// the operation which produced the patches.  Curves are welded by their
    /// certified endpoints and midpoint, then ordinary published-topology
    /// validation proves that every edge has exactly two opposite uses.
    pub(crate) fn brep_shell(&mut self, patches: &[BrepPatch]) -> KernelResult<BodyId> {
        let mut patches = patches.to_vec();
        conform_brep_boundaries(&mut patches, self.kernel.tolerance)?;
        let topology_snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self.brep_shell_unchecked(&patches).and_then(|body| {
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

    /// Assemble newly created components using signed BREP volume and trimmed
    /// surface classification. A containment witness is sufficient only after
    /// establishing that the two closed boundaries are disjoint.
    pub(crate) fn nest_brep_shells(&mut self, bodies: &[BodyId]) -> KernelResult<()> {
        if bodies.len() < 2 {
            return Ok(());
        }
        let topology = self.working.clone();
        let tolerance = self.kernel.tolerance;
        let shells = bodies.iter().map(|body| {
            let value = topology.body(*body)?;
            if value.shells().len() != 1 {
                return Err(KernelError::new(DiagnosticKind::TopologyInvariant,
                    "a newly assembled BREP component must contain exactly one shell"));
            }
            let shell = value.shells()[0];
            let volume = super::physical::shell_signed_volume(&topology, shell, tolerance)?;
            if volume.abs() <= tolerance.linear.powi(3) {
                return Err(KernelError::new(DiagnosticKind::GeometryInvariant,
                    "an analytical BREP shell has zero volume at model tolerance"));
            }
            Ok((*body, shell, volume))
        }).collect::<KernelResult<Vec<_>>>()?;
        let mut containers = Vec::new();
        for (body, shell, volume) in &shells {
            if *volume > 0.0 {
                containers.push((*body, *shell, *volume,
                    super::brep_classification::BrepClassifier::new(&topology, *shell, tolerance)?));
            }
        }
        containers.sort_by(|a, b| a.2.total_cmp(&b.2));
        let mut attachments = Vec::new();
        for (body, cavity, volume) in &shells {
            if *volume > 0.0 { continue; }
            let witness = shell_witness(&topology, *cavity)?;
            let mut parent = None;
            for (outer_body, outer, _, classifier) in &containers {
                if !shells_intersect(&topology, *outer, *cavity, tolerance, None)?
                    && classifier.contains(witness)? {
                    parent = Some(*outer_body);
                    break;
                }
            }
            let parent = parent.ok_or_else(|| KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "an inward-oriented analytical BREP shell is not contained by an outer shell"))?;
            attachments.push((parent, *body, *cavity));
        }
        for (index, (parent, _, shell)) in attachments.iter().enumerate() {
            for (other_parent, _, other) in &attachments[..index] {
                if parent == other_parent
                    && shells_intersect(&topology, *shell, *other, tolerance, None)? {
                    return Err(KernelError::new(DiagnosticKind::GeometryInvariant,
                        "cavity boundaries intersect"));
                }
            }
        }
        for (parent, body, _) in attachments {
            self.merge_shell_into_body_unchecked(parent, body)?;
        }
        Ok(())
    }

    /// Support reconstruction retains the original body/shell relationship.
    /// Only changed boundaries need collision checks; they cannot be reassigned
    /// to another solid merely because the edited cavity now lies inside it.
    pub(crate) fn restore_brep_shells(
        &mut self,
        source: &Topology,
        rebuilt: &BTreeMap<ShellId, BodyId>,
        affected: &BTreeSet<PersistentId>,
    ) -> KernelResult<()> {
        let topology = self.working.clone();
        let tolerance = self.kernel.tolerance;
        let changed_faces = topology.faces().filter_map(|(id, face)|
            self.history.construction.get(&face.name()).is_some_and(|sources|
                sources.iter().any(|source| affected.contains(&source.name))).then_some(id)
        ).collect::<BTreeSet<_>>();
        let source_bodies = rebuilt.keys().map(|shell| source.shell(*shell).map(|value| value.body()))
            .collect::<KernelResult<BTreeSet<_>>>()?;
        for body in source_bodies {
            let body = source.body(body)?;
            // Construction and shell merge keep the outward shell first.
            let outer_body = rebuilt[&body.shells()[0]];
            let outer = topology.body(outer_body)?.shells()[0];
            let mut shells = vec![outer];
            for source_shell in &body.shells()[1..] {
                let cavity_body = rebuilt[source_shell];
                let cavity = topology.body(cavity_body)?.shells()[0];
                for other in &shells {
                    if shells_intersect(&topology, cavity, *other, tolerance, Some(&changed_faces))? {
                        return Err(KernelError::new(DiagnosticKind::GeometryInvariant,
                            "an edited cavity boundary intersects another shell"));
                    }
                }
                let changed = [outer, cavity].iter().any(|shell|
                    topology.shell(*shell).unwrap().faces().iter().any(|face| changed_faces.contains(face)));
                if changed && !super::brep_classification::BrepClassifier::new(
                    &topology, outer, tolerance)?.contains(shell_witness(&topology, cavity)?)? {
                    return Err(KernelError::new(DiagnosticKind::GeometryInvariant,
                        "an edited cavity lies outside its original outer shell"));
                }
                shells.push(cavity);
                self.merge_shell_into_body_unchecked(outer_body, cavity_body)?;
            }
        }
        Ok(())
    }

    fn brep_shell_unchecked(&mut self, patches: &[BrepPatch]) -> KernelResult<BodyId> {
        if patches.is_empty() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "an analytical BREP shell needs at least one face patch",
            ));
        }
        if patches.iter().any(|patch| {
            patch.loops.is_empty()
                || patch.loops.iter().any(|loop_| loop_.segments.is_empty())
                || patch
                    .loops
                    .iter()
                    .filter(|loop_| loop_.kind == LoopKind::Outer)
                    .count()
                    != 1
        }) {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "every analytical BREP face patch needs one nonempty outer loop",
            ));
        }

        self.names.push_scope("brep")?;
        let body_name = self.names.next_for("body")?;
        let shell_name = self.names.next_for("shell")?;
        let body = self.working.bodies.insert(Body::new(body_name));
        let shell = self
            .working
            .shells
            .insert(Shell::new(shell_name, body, ShellKind::Open));
        self.working.bodies.get_mut(body)?.shells.push(shell);

        #[derive(Clone)]
        struct WeldedEdge {
            edge: EdgeId,
            vertices: [VertexId; 2],
            curve: Curve3,
            range: ParameterRange,
            start: Point3,
            end: Point3,
            middle: Point3,
        }

        let mut vertices = Vec::<(Point3, VertexId)>::new();
        let mut edges = Vec::<WeldedEdge>::new();
        let linear = self.kernel.tolerance.linear;
        let weld_vertex = |transaction: &mut Self,
                           vertices: &mut Vec<(Point3, VertexId)>,
                           point: Point3|
         -> KernelResult<VertexId> {
            if let Some((_, vertex)) = vertices
                .iter()
                .find(|(candidate, _)| candidate.distance(point) <= linear * 8.0)
            {
                return Ok(*vertex);
            }
            let name = transaction.names.next_for("vertex")?;
            let vertex = transaction.make_vertex(name, point);
            vertices.push((point, vertex));
            Ok(vertex)
        };

        for patch in patches {
            let face_name = self.names.next_for("face")?;
            let outer_name = self.names.next_for("loop.outer")?;
            let (face, initial_outer) = self.make_face_loop(
                shell,
                face_name,
                outer_name,
                patch.surface.clone(),
                patch.orientation,
            )?;
            if !patch.sources.is_empty() {
                self.history
                    .derived(face_name, patch.sources.iter().copied(), super::DerivationRole::BrepFace);
                self.history.record_construction(
                    face_name,
                    patch
                        .sources
                        .iter()
                        .map(|name| super::history::EntitySource {
                            name: *name,
                            unchanged: patch.preserved.contains(name),
                        }),
                );
            }
            let outer_index = patch
                .loops
                .iter()
                .position(|loop_| loop_.kind == LoopKind::Outer)
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "analytical BREP face patch lost its outer loop",
                    )
                })?;

            for (loop_index, patch_loop) in patch.loops.iter().enumerate() {
                let loop_id = if loop_index == outer_index {
                    initial_outer
                } else {
                    let name = self.names.next_for("loop.inner")?;
                    let loop_id = self
                        .working
                        .loops
                        .insert(Loop::new(name, face, LoopKind::Inner));
                    self.working.faces.get_mut(face)?.loops.push(loop_id);
                    loop_id
                };
                let mut uses = Vec::with_capacity(patch_loop.segments.len());
                for segment in &patch_loop.segments {
                    let start = segment.curve.evaluate(segment.range.start)?.point;
                    let end = segment.curve.evaluate(segment.range.end)?.point;
                    let middle = segment
                        .curve
                        .evaluate(segment.range.start + segment.range.span() * 0.5)?
                        .point;
                    let start_vertex = weld_vertex(self, &mut vertices, start)?;
                    let end_vertex = weld_vertex(self, &mut vertices, end)?;
                    self.history.record_construction(
                        self.working.vertex(start_vertex)?.name(),
                        segment.provenance.vertices[0].iter().copied(),
                    );
                    self.history.record_construction(
                        self.working.vertex(end_vertex)?.name(),
                        segment.provenance.vertices[1].iter().copied(),
                    );
                    let matched = edges.iter().find(|candidate| {
                        let closed_circles_match = match (&candidate.curve, &segment.curve) {
                            (Curve3::Circle(first), Curve3::Circle(second)) => {
                                candidate.start.distance(candidate.end) <= linear * 8.0
                                    && start.distance(end) <= linear * 8.0
                                    && first.center().distance(second.center()) <= linear * 8.0
                                    && (first.radius() - second.radius()).abs() <= linear * 8.0
                                    && first.normal().dot(second.normal()).abs()
                                        >= 1.0 - self.kernel.tolerance.angular * 8.0
                            }
                            _ => false,
                        };
                        if closed_circles_match { return true; }
                        // Endpoint agreement is necessary before an exact
                        // midpoint projection can identify this shared edge.
                        if !((candidate.start.distance(start) <= linear * 8.0
                            && candidate.end.distance(end) <= linear * 8.0)
                            || (candidate.start.distance(end) <= linear * 8.0
                                && candidate.end.distance(start) <= linear * 8.0))
                        {
                            return false;
                        }
                        candidate.middle.distance(middle) <= linear * 64.0
                            || candidate
                                .curve
                                .project_on_trim(middle, candidate.range, self.kernel.tolerance)
                                .is_ok_and(|projection| {
                                    projection.distance <= linear * 64.0
                                        && candidate
                                            .range
                                            .contains(projection.parameter, self.kernel.tolerance)
                                })
                    }).cloned();
                    let (edge, forward, pcurve) = if let Some(candidate) = matched {
                        let closed = candidate.start.distance(candidate.end) <= linear * 8.0
                            && start.distance(end) <= linear * 8.0;
                        let forward = if closed {
                            match (&candidate.curve, &segment.curve) {
                                (Curve3::Circle(first), Curve3::Circle(second)) => {
                                    first.normal().dot(second.normal())
                                        * candidate.range.span().signum()
                                        * segment.range.span().signum()
                                        > 0.0
                                }
                                _ => {
                                    let parameter = segment
                                        .curve
                                        .project(candidate.start, self.kernel.tolerance)?
                                        .parameter;
                                    let first = candidate
                                        .curve
                                        .evaluate(candidate.range.start)?
                                        .first_derivative
                                        * candidate.range.span().signum();
                                    let second =
                                        segment.curve.evaluate(parameter)?.first_derivative
                                            * segment.range.span().signum();
                                    first.dot(second) > 0.0
                                }
                            }
                        } else {
                            candidate.vertices == [start_vertex, end_vertex]
                        };
                        if !closed && !forward && candidate.vertices != [end_vertex, start_vertex] {
                            return Err(KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "welded analytical edge endpoints disagree",
                            ));
                        }
                        let coedge_range = if forward {
                            candidate.range
                        } else {
                            candidate.range.reversed()
                        };
                        let pcurve = if coedge_range == segment.range
                            && candidate.curve == segment.curve
                        {
                            segment.pcurve.clone()
                        } else if let (
                            Curve3::Line(line),
                            Curve3::Line(source),
                            Curve2::Line(pcurve),
                            Surface3::Cone(_),
                        ) = (
                            &candidate.curve,
                            &segment.curve,
                            &segment.pcurve,
                            &patch.surface,
                        ) {
                            // Reparameterize exact generator lines algebraically.
                            // Projection is undefined at a cone/sphere pole.
                            let scale = line.direction().dot(source.direction())
                                / source.direction().magnitude2();
                            let offset = (line.origin() - source.origin()).dot(source.direction())
                                / source.direction().magnitude2();
                            Line2::new(
                                pcurve.origin() + pcurve.direction() * offset,
                                pcurve.direction() * scale,
                            )?
                            .into()
                        } else if let (
                            Curve3::Circle(circle), Curve3::Circle(source), Curve2::Line(pcurve),
                            Surface3::Sphere(_) | Surface3::Revolved(_),
                        ) = (&candidate.curve, &segment.curve, &segment.pcurve, &patch.surface)
                        {
                            // Circle charts differ only by phase and orientation.
                            // Retain the exact meridian through a revolution pole,
                            // where surface projection has no unique derivative.
                            let scale = circle.normal().dot(source.normal()).signum();
                            let anchor = coedge_range.start.min(coedge_range.end);
                            let parameter = segment.curve.project(
                                candidate.curve.evaluate(anchor)?.point, self.kernel.tolerance,
                            )?.parameter;
                            let reference = segment.range.start + segment.range.span()
                                * (anchor - coedge_range.start) / coedge_range.span();
                            let parameter = parameter + ((reference - parameter) / std::f64::consts::TAU).round()
                                * std::f64::consts::TAU;
                            Line2::new(
                                pcurve.origin() + pcurve.direction() * (parameter - scale * anchor),
                                pcurve.direction() * scale,
                            )?.into()
                        } else if let (Curve3::Line(line), Surface3::Plane(plane)) =
                            (&candidate.curve, &patch.surface)
                        {
                            // A line on a plane has an unbounded exact p-curve.
                            // Keep that simple representation so later Euler
                            // edits may trim or extend the edge without leaving
                            // an artificial projection-guide domain.
                            let origin = plane.project(line.origin())?.parameters;
                            let next = plane.project(line.origin() + line.direction())?.parameters;
                            Line2::new(
                                Point2::new(origin.0, origin.1),
                                Vec2::new(next.0 - origin.0, next.1 - origin.1),
                            )?
                            .into()
                        } else {
                            let guide_range = ParameterRange::new(
                                candidate.range.start.min(candidate.range.end),
                                candidate.range.start.max(candidate.range.end),
                            )?;
                            let mut guide_points = Vec::new();
                            for index in 0..=32 {
                                let parameter =
                                    guide_range.start + guide_range.span() * index as f64 / 32.0;
                                let point = candidate.curve.evaluate(parameter)?.point;
                                // Reparameterize the retained trim in its own
                                // chart. Projecting onto the surface afresh can
                                // choose another turn of a closed blend support.
                                let fraction = (parameter - coedge_range.start)
                                    / coedge_range.span();
                                let reference = segment.range.start + segment.range.span() * fraction;
                                let mut source_parameter = segment.curve.project_on_trim(
                                    point, segment.range, self.kernel.tolerance,
                                )?.parameter;
                                if let Some(period) = segment.curve.domain().period {
                                    source_parameter += ((reference - source_parameter) / period).round() * period;
                                } else if index == 0 || index == 32 {
                                    source_parameter = reference;
                                }
                                guide_points.push(segment.pcurve.evaluate(source_parameter)?.point);
                            }
                            super::intersection::unwrap_periodic_points(
                                &mut guide_points,
                                patch.surface.domains(),
                            );
                            let guide: Curve2 = super::intersection::smooth_nurbs_guide(
                                &guide_points,
                                guide_range,
                            )?
                            .into();
                            super::SurfaceProjectionPcurve2::new(
                                candidate.curve.clone(),
                                patch.surface.clone(),
                                guide,
                                guide_range,
                                self.kernel.tolerance,
                            )?
                            .into()
                        };
                        (candidate.edge, forward, pcurve)
                    } else {
                        let edge_name = self.names.next_for("edge")?;
                        let edge = self.make_edge(
                            edge_name,
                            [start_vertex, end_vertex],
                            segment.curve.clone(),
                            segment.range,
                        )?;
                        edges.push(WeldedEdge {
                            edge,
                            vertices: [start_vertex, end_vertex],
                            curve: segment.curve.clone(),
                            range: segment.range,
                            start,
                            end,
                            middle,
                        });
                        (edge, true, segment.pcurve.clone())
                    };
                    let edge_range = self.working.edge(edge)?.range;
                    self.history.record_construction(
                        self.working.edge(edge)?.name(),
                        segment.provenance.edges.iter().copied(),
                    );
                    let coedge_name = self.names.next_for("coedge")?;
                    let coedge = self.make_coedge(
                        coedge_name,
                        edge,
                        loop_id,
                        forward,
                        pcurve,
                        if forward {
                            edge_range
                        } else {
                            edge_range.reversed()
                        },
                    )?;
                    uses.push(coedge);
                    if self.working.vertex(start_vertex)?.outgoing.is_none() {
                        self.working.vertices.get_mut(start_vertex)?.outgoing = Some(coedge);
                    }
                }
                for index in 0..uses.len() {
                    let previous = uses[(index + uses.len() - 1) % uses.len()];
                    let next = uses[(index + 1) % uses.len()];
                    let value = self.working.coedges.get_mut(uses[index])?;
                    value.previous = previous;
                    value.next = next;
                }
                self.working.loops.get_mut(loop_id)?.first = uses.first().copied();
            }
        }
        self.names.pop_scope()?;
        if let Some(edge) = edges.iter().find(|edge| {
            self.working
                .edge(edge.edge)
                .is_ok_and(|value| value.coedges.len() != 2)
        }) {
            let uses = self.working.edge(edge.edge)?.coedges.len();
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                format!(
                    "analytical BREP edge has {uses} uses instead of two: {:?} -> {:?} via {:?}",
                    edge.start, edge.end, edge.middle
                ),
            ));
        }
        // Spatial welding identifies edge mates, but a point contact can have
        // several disconnected face fans. Each fan owns a topological vertex.
        let mut corners = BTreeMap::<VertexId, BTreeSet<CoedgeId>>::new();
        for edge in &edges {
            for coedge in self.working.edge(edge.edge)?.coedges() {
                let vertex = self.working.coedge_vertices(*coedge)?[0];
                corners.entry(vertex).or_default().insert(*coedge);
            }
        }
        for (vertex, mut remaining) in corners {
            let mut first = true;
            while let Some(start) = remaining.first().copied() {
                let target = if first {
                    first = false;
                    vertex
                } else {
                    let name = self.names.next_for("vertex")?;
                    self.make_vertex(name, self.working.vertex(vertex)?.point())
                };
                self.working.vertices.get_mut(target)?.outgoing = Some(start);
                let mut current = start;
                while remaining.remove(&current) {
                    let coedge = self.working.coedge(current)?.clone();
                    let previous = self.working.coedge(coedge.previous)?;
                    let next = *self.working.edge(previous.edge)?.coedges().iter()
                        .find(|id| **id != coedge.previous).expect("sewn edge has two uses");
                    self.working.edges.get_mut(coedge.edge)?.vertices[usize::from(!coedge.forward)] = target;
                    current = next;
                }
            }
        }
        self.seal_shell_unchecked(shell)?;
        if !self.touched_bodies.contains(&body) {
            self.touched_bodies.push(body);
        }
        Ok(body)
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
        self.assign_triangle_face(seed.face, seed_triangle, mesh)?;
        let seed_face_name = self.names.named("face", &seed_triangle.key)?;
        let seed_loop_name = self.names.named("loop.outer", &seed_triangle.key)?;
        self.rename_face(seed.face, seed_face_name)?;
        self.rename_loop(seed.loop_id, seed_loop_name)?;

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
                    self.assign_triangle_face(made.face, triangle, mesh)?;
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
                    self.assign_triangle_face(made.face, triangle, mesh)?;
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
        self.history.construction_roles.insert(self.working.face(seed.face)?.name(), vec![super::CreationRole::ProfileSide {
            generators: vec![[0, 0, 0]], chart: Some(super::SurfaceChart::SectionPositive),
        }]);
        self.history.construction_roles.insert(self.working.face(lamina.face)?.name(), vec![super::CreationRole::ProfileSide {
            generators: vec![[0, 0, 0]], chart: Some(super::SurfaceChart::SectionNegative),
        }]);
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
            for (face, (segment, part)) in side_faces.iter().copied().zip(source_parts) {
                let name = self.working.face(face)?.name;
                self.history.created_as(
                    name,
                    super::CreationRole::ExtrusionSide {
                        boundary,
                        segment: profile.generator(index, segment),
                        part,
                    },
                );
            }
            let source_wire = if index == 0 {
                profile.outer()
            } else {
                &profile.holes()[index - 1]
            };
            if is_periodic_circle_wire(source_wire, self.kernel.tolerance) {
                self.merge_periodic_cylinder_sides_unchecked(&side_faces)?;
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

    /// A full circular profile is temporarily partitioned into arcs so the
    /// elementary Euler sweep can seed distinct vertices. Remove the internal
    /// joins afterwards, leaving one periodic cylinder face and one remaining
    /// join as its topological seam.
    fn merge_periodic_cylinder_sides_unchecked(
        &mut self,
        side_faces: &[FaceId],
    ) -> KernelResult<()> {
        if side_faces.len() < 2 {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "a periodic circular sweep must initially have multiple side faces",
            ));
        }
        let first = side_faces[0];
        let Surface3::Cylinder(first_surface) = self.working.face(first)?.surface.clone() else {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "a periodic circular sweep did not produce cylindrical sides",
            ));
        };
        let first_axis = first_surface.axis().normalize();
        let mut survivor = first;
        for face in side_faces.iter().copied().skip(1) {
            let Surface3::Cylinder(surface) = self.working.face(face)?.surface.clone() else {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "a periodic circular sweep did not produce cylindrical sides",
                ));
            };
            if first_axis.cross(surface.axis().normalize()).magnitude()
                > self.kernel.tolerance.angular
                || (surface.origin() - first_surface.origin())
                    .cross(first_axis)
                    .magnitude()
                    > self.kernel.tolerance.linear
                || (first_surface.radius() - surface.radius()).abs() > self.kernel.tolerance.linear
            {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "periodic circular sweep sides do not share one cylinder",
                ));
            }
            let survivor_edges = self
                .working
                .face_edges(survivor)?
                .into_iter()
                .collect::<BTreeSet<_>>();
            let join = self
                .working
                .face_edges(face)?
                .into_iter()
                .find(|edge| {
                    survivor_edges.contains(edge)
                        && self
                            .working
                            .edge(*edge)
                            .is_ok_and(|value| matches!(value.curve(), Curve3::Line(_)))
                })
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "periodic cylinder patches have no shared sweep edge",
                    )
                })?;
            survivor = self.kef_unchecked(join, face)?.surviving_face;
            self.working.faces.get_mut(survivor)?.surface = Surface3::Cylinder(first_surface);
        }
        let mut circle_groups = Vec::<Vec<EdgeId>>::new();
        for edge_id in self.working.face_edges(survivor)? {
            let Curve3::Circle(circle) = self.working.edge(edge_id)?.curve.clone() else {
                continue;
            };
            if let Some(group) = circle_groups.iter_mut().find(|group| {
                matches!(
                    self.working.edge(group[0]).map(Edge::curve),
                    Ok(Curve3::Circle(candidate)) if *candidate == circle
                )
            }) {
                group.push(edge_id);
            } else {
                circle_groups.push(vec![edge_id]);
            }
        }
        for group in circle_groups {
            if group.len() > 1 {
                self.merge_periodic_circle_edge_cycle_unchecked(&group)?;
            }
        }
        self.reparameterize_cylindrical_face(survivor)?;
        Ok(())
    }

    /// Collapse the temporary arc cycle of a full circle into one periodic
    /// edge. This is the inverse Euler simplification of splitting an edge with
    /// degree-two vertices: one edge and one vertex are removed per join while
    /// both incident face loops retain their exact p-curves.
    fn merge_periodic_circle_edge_cycle_unchecked(
        &mut self,
        edges: &[EdgeId],
    ) -> KernelResult<EdgeId> {
        if edges.len() < 2 {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "a periodic edge cycle needs at least two arcs",
            ));
        }
        let edge_set = edges.iter().copied().collect::<BTreeSet<_>>();
        let reference = self.working.edge(edges[0])?.clone();
        let Curve3::Circle(reference_circle) = reference.curve else {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "periodic edge consolidation requires circular arcs",
            ));
        };
        let mut incidence = BTreeMap::<VertexId, Vec<EdgeId>>::new();
        for edge_id in edges {
            let edge = self.working.edge(*edge_id)?;
            if !matches!(&edge.curve, Curve3::Circle(circle) if *circle == reference_circle)
                || edge.coedges.len() != 2
            {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "periodic edge cycle does not share one exact circle and two face uses",
                ));
            }
            for vertex in edge.vertices {
                incidence.entry(vertex).or_default().push(*edge_id);
            }
        }
        if incidence.values().any(|incident| incident.len() != 2) {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "periodic circular arcs do not form a closed degree-two cycle",
            ));
        }
        let seam = incidence
            .keys()
            .copied()
            .find(|vertex| {
                self.working
                    .vertex_edges(*vertex)
                    .is_ok_and(|incident| incident.iter().any(|edge| !edge_set.contains(edge)))
            })
            .or_else(|| incidence.keys().copied().next())
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "periodic edge cycle has no seam vertex",
                )
            })?;

        let mut ordered = Vec::<(EdgeId, bool)>::with_capacity(edges.len());
        let mut used = BTreeSet::new();
        let mut vertex = seam;
        let mut edge_id = incidence[&seam][0];
        loop {
            let edge = self.working.edge(edge_id)?;
            let forward = if edge.vertices[0] == vertex {
                true
            } else if edge.vertices[1] == vertex {
                false
            } else {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "ordered circular arc is disconnected from its predecessor",
                ));
            };
            vertex = if forward {
                edge.vertices[1]
            } else {
                edge.vertices[0]
            };
            ordered.push((edge_id, forward));
            used.insert(edge_id);
            if vertex == seam {
                break;
            }
            edge_id = incidence[&vertex]
                .iter()
                .copied()
                .find(|candidate| !used.contains(candidate))
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "periodic circular arc cycle terminates before its seam",
                    )
                })?;
            if ordered.len() >= edges.len() {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "periodic circular arc ordering does not close",
                ));
            }
        }
        if ordered.len() != edges.len() {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "periodic circular arc cycle has disconnected components",
            ));
        }

        let survivor = ordered[0].0;
        let first = self.working.edge(survivor)?.clone();
        let start = if ordered[0].1 {
            first.range.start
        } else {
            first.range.end
        };
        let span = ordered.iter().try_fold(0.0, |span, (edge_id, forward)| {
            let range = self.working.edge(*edge_id)?.range;
            Ok::<_, KernelError>(
                span + if *forward {
                    range.span()
                } else {
                    -range.span()
                },
            )
        })?;
        if (span.abs() - std::f64::consts::TAU).abs() > self.kernel.tolerance.parameter * 32.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "periodic circular arc cycle does not cover one full turn",
            ));
        }
        let range = ParameterRange::new(start, start + span)?;
        let survivor_uses = first.coedges.clone();
        let mut removed_uses = Vec::new();
        for survivor_use in &survivor_uses {
            let loop_id = self.working.coedge(*survivor_use)?.loop_id;
            let loop_uses = edges
                .iter()
                .map(|edge_id| {
                    self.working
                        .edge(*edge_id)?
                        .coedges
                        .iter()
                        .copied()
                        .find(|coedge| {
                            self.working
                                .coedge(*coedge)
                                .is_ok_and(|value| value.loop_id == loop_id)
                        })
                        .ok_or_else(|| {
                            KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "periodic circular arcs do not share both incident loops",
                            )
                        })
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let original_range = self.working.coedge(*survivor_use)?.range;
            for use_id in loop_uses
                .into_iter()
                .filter(|use_id| use_id != survivor_use)
            {
                let use_value = self.working.coedge(use_id)?.clone();
                self.working.coedges.get_mut(use_value.previous)?.next = use_value.next;
                self.working.coedges.get_mut(use_value.next)?.previous = use_value.previous;
                if self.working.loop_(loop_id)?.first == Some(use_id) {
                    self.working.loops.get_mut(loop_id)?.first = Some(use_value.next);
                }
                removed_uses.push((use_id, use_value.name));
            }
            let forward = original_range.span().signum() == span.signum();
            let value = self.working.coedges.get_mut(*survivor_use)?;
            value.forward = forward;
            value.range = if forward { range } else { range.reversed() };
        }

        {
            let value = self.working.edges.get_mut(survivor)?;
            value.vertices = [seam, seam];
            value.curve = reference_circle.into();
            value.range = range;
        }
        for (use_id, name) in removed_uses {
            self.working.coedges.remove(use_id)?;
            self.history
                .changed(name, ChangeKind::Deleted, std::iter::empty());
        }
        for edge_id in edges.iter().copied().filter(|edge_id| *edge_id != survivor) {
            let name = self.working.edge(edge_id)?.name;
            self.working.edges.remove(edge_id)?;
            self.history
                .changed(name, ChangeKind::Deleted, std::iter::empty());
        }
        for vertex_id in incidence.keys().copied().filter(|vertex| *vertex != seam) {
            if !self.working.vertex_edges(vertex_id)?.is_empty() {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "periodic arc split vertex remains incident after edge consolidation",
                ));
            }
            let name = self.working.vertex(vertex_id)?.name;
            self.working.vertices.remove(vertex_id)?;
            self.history
                .changed(name, ChangeKind::Deleted, std::iter::empty());
        }
        self.repair_outgoing(seam)?;
        let survivor_name = self.working.edge(survivor)?.name;
        self.history
            .changed(survivor_name, ChangeKind::Modified, [survivor_name]);
        Ok(survivor)
    }

    /// Merge an exact periodic rolling sweep into one toroidal BREP face. The
    /// remaining meridian is a one-face parameter seam, not a geometric split.
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

    /// Construct an exact end-cap rolling blend. The first sweep stops one
    /// radius below the cap; this second Euler sweep follows quarter-circle
    /// vertex paths. Line rails produce cylinders and circular rails tori.
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
            Curve2::SurfaceIntersection(curve) if curve.lies_on(&Surface3::Plane(plane)) => {
                curve.spatial_curve()
            }
            Curve2::SurfaceProjection(curve) if curve.surface() == &Surface3::Plane(plane) => {
                curve.curve().clone()
            }
            Curve2::SurfaceIntersection(_) | Curve2::SurfaceProjection(_) => {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "an implicit p-curve can only be lifted onto its retained plane",
                ))
            }
        })
    }

    pub(super) fn make_vertex(&mut self, name: super::PersistentId, point: Point3) -> VertexId {
        self.working
            .vertices
            .insert(Vertex::new(name, point, self.kernel.tolerance.linear))
    }

    /// Create a face and its initial outer loop in one topologically consistent
    /// Euler transaction step.
    pub(super) fn make_face_loop(
        &mut self,
        shell: ShellId,
        face_name: super::PersistentId,
        loop_name: super::PersistentId,
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
    /// surrounding Euler rewrite is assembled.

    /// Create an exact edge without uses. Uses are attached explicitly while a
    /// surrounding Euler rewrite is assembled.
    pub(super) fn make_edge(
        &mut self,
        name: super::PersistentId,
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
        name: super::PersistentId,
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
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "Euler edge deletion did not receive the edge's complete use set",
            ));
        }
        for use_id in uses {
            let name = self.working.coedge(*use_id)?.name;
            self.working.coedges.remove(*use_id)?;
            self.history
                .changed(name, ChangeKind::Deleted, std::iter::empty());
        }
        self.working.edges.remove(edge)?;
        self.history
            .changed(edge_value.name, ChangeKind::Deleted, std::iter::empty());
        Ok(())
    }

    pub(super) fn delete_split_vertex(
        &mut self,
        vertex: VertexId,
        outputs: impl IntoIterator<Item = super::PersistentId>,
    ) -> KernelResult<()> {
        let name = self.working.vertex(vertex)?.name;
        self.working.vertices.remove(vertex)?;
        self.history.changed(name, ChangeKind::Split, outputs);
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
        self.retarget_edge_endpoint_impl(edge_id, old_vertex, new_vertex, point, false)
    }

    /// Retarget one endpoint while permitting its exact curve and pcurves to
    /// extend beyond their previous trims. Fillet termination on a concave
    /// corner needs this when the terminating face grows into the removed
    /// material instead of merely trimming an existing boundary.
    pub(super) fn retarget_edge_endpoint_allow_extension(
        &mut self,
        edge_id: EdgeId,
        old_vertex: VertexId,
        new_vertex: VertexId,
        point: Point3,
    ) -> KernelResult<()> {
        self.retarget_edge_endpoint_impl(edge_id, old_vertex, new_vertex, point, true)
    }

    fn retarget_edge_endpoint_impl(
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
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "edge does not contain the endpoint being retargeted",
                )
            })?;
        let mut projection = edge.curve.project(point, self.kernel.tolerance)?;
        if let Some(period) = edge.curve.domain().period {
            let reference = if endpoint == 0 {
                edge.range.start
            } else {
                edge.range.end
            };
            projection.parameter += ((reference - projection.parameter) / period).round() * period;
        }
        let allowed = self
            .kernel
            .tolerance
            .linear_at_scale(point.to_vec().magnitude().max(1.0));
        if projection.distance > allowed {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                format!(
                    "replacement endpoint misses its exact edge by {}",
                    projection.distance
                ),
            ));
        }
        let remaining_span = if endpoint == 0 {
            edge.range.end - projection.parameter
        } else {
            projection.parameter - edge.range.start
        };
        let preserves_direction = remaining_span.signum() == edge.range.span().signum()
            && remaining_span.abs() > self.kernel.tolerance.parameter;
        if !preserves_direction {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "replacement endpoint would collapse or reverse its exact edge",
            ));
        }
        if !allow_extension
            && !edge
                .range
                .contains(projection.parameter, self.kernel.tolerance)
        {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                format!(
                    "replacement parameter {} lies outside edge range {:?}",
                    projection.parameter, edge.range,
                ),
            ));
        }
        let edge_fraction = (projection.parameter - edge.range.start) / edge.range.span();
        for coedge_id in edge.coedges.iter().copied() {
            let coedge = self.working.coedge(coedge_id)?.clone();
            let traversal_vertices = self.working.coedge_vertices(coedge_id)?;
            let traversal_fraction = if coedge.forward {
                edge_fraction
            } else {
                1.0 - edge_fraction
            };
            let parameter = coedge.range.start + coedge.range.span() * traversal_fraction;
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
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "edge use lost the endpoint being retargeted",
                ));
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
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "Euler insertion endpoints are not adjacent",
            ));
        }
        let loop_id = self.working.coedge(previous)?.loop_id;
        self.record_loop_edit(loop_id)?;
        if self.working.coedge(next)?.loop_id != loop_id {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "Euler insertion endpoints belong to different loops",
            ));
        }
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
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "a face loop needs at least three boundary uses",
            ));
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
                        .map(|vertices| vertices[0] == end)
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "boundary uses do not form an oriented cycle",
                    )
                })?;
            remaining.remove(&next);
            ordered.push(next);
        }
        if self.working.coedge_vertices(*ordered.last().unwrap())?[1]
            != self.working.coedge_vertices(first)?[0]
        {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "boundary cycle does not close",
            ));
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
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "open boundary has no end vertex",
                )
            })?;
        let end = balance
            .iter()
            .find_map(|(vertex, value)| (*value == 1).then_some(*vertex))
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "open boundary has no start vertex",
                )
            })?;
        let edge = self.working.coedge(closing)?.edge;
        let edge_vertices = self.working.edge(edge)?.vertices;
        let forward = if edge_vertices == [start, end] {
            true
        } else if edge_vertices == [end, start] {
            false
        } else {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "closing boundary has the wrong vertices",
            ));
        };
        let range = self.working.edge(edge)?.range;
        let value = self.working.coedges.get_mut(closing)?;
        value.forward = forward;
        value.range = if forward { range } else { range.reversed() };
        Ok(())
    }

    pub(super) fn collect_loop(&self, loop_id: LoopId) -> KernelResult<Vec<CoedgeId>> {
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

    pub(crate) fn finish(mut self) -> KernelResult<CommittedOperation> {
        super::validation::validate_structure(
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
        self.history
            .complete_retained(&self.kernel.topology, &self.working);
        self.kernel.topology = self.working;
        Ok(CommittedOperation {
            operation: self.namespace,
            bodies: self.touched_bodies,
            history: self.history,
            certificates,
        })
    }
}

fn shell_witness(topology: &Topology, shell: ShellId) -> KernelResult<Point3> {
    let face = topology.shell(shell)?.faces()[0];
    let edge = topology.face_edges(face)?[0];
    topology.edge(edge)?.curve().evaluate(topology.edge(edge)?.range().start).map(|value| value.point)
}

fn shells_intersect(
    topology: &Topology,
    first: ShellId,
    second: ShellId,
    tolerance: ModelTolerance,
    affected: Option<&BTreeSet<FaceId>>,
) -> KernelResult<bool> {
    for a in topology.shell(first)?.faces() {
        for b in topology.shell(second)?.faces() {
            if affected.is_some_and(|changed| !changed.contains(a) && !changed.contains(b)) {
                continue;
            }
            if super::trim::trimmed_faces_intersect(topology, *a, *b, tolerance)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn prepared_wire_segments(wire: &Wire2) -> KernelResult<Vec<TrimmedCurve2>> {
    if wire.segments().len() != 1 {
        return Ok(wire.segments().to_vec());
    }
    let segment = &wire.segments()[0];
    let part_count = if matches!(segment.curve(), Curve2::Circle(_))
        && (segment.range().span().abs() - std::f64::consts::TAU).abs()
            <= f64::EPSILON * std::f64::consts::TAU * 128.0
    {
        4
    } else {
        2
    };
    (0..part_count)
        .map(|part| {
            let start =
                segment.range().start + segment.range().span() * part as f64 / part_count as f64;
            let end = segment.range().start
                + segment.range().span() * (part + 1) as f64 / part_count as f64;
            TrimmedCurve2::new(segment.curve().clone(), ParameterRange::new(start, end)?)
        })
        .collect()
}

fn is_periodic_circle_wire(wire: &Wire2, tolerance: ModelTolerance) -> bool {
    let Some(Curve2::Circle(reference)) = wire.segments().first().map(TrimmedCurve2::curve) else {
        return false;
    };
    let total_turn = wire
        .segments()
        .iter()
        .map(|segment| segment.range().span())
        .sum::<f64>();
    wire.segments().iter().all(|segment| {
        matches!(segment.curve(), Curve2::Circle(circle)
            if circle.center().distance(reference.center()) <= tolerance.linear
                && (circle.radius() - reference.radius()).abs() <= tolerance.linear
                && segment.range().span().signum() == total_turn.signum())
    }) && (total_turn.abs() - std::f64::consts::TAU).abs() <= tolerance.parameter
}

pub(super) fn prepared_wire_segment_sources(wire: &Wire2) -> Vec<(usize, usize)> {
    if wire.segments().len() == 1 {
        let part_count = if matches!(wire.segments()[0].curve(), Curve2::Circle(_))
            && (wire.segments()[0].range().span().abs() - std::f64::consts::TAU).abs()
                <= f64::EPSILON * std::f64::consts::TAU * 128.0
        {
            4
        } else {
            2
        };
        (0..part_count).map(|part| (0, part)).collect()
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
        Curve3::SurfaceIntersection(_) | Curve3::SurfaceCurve(_) => {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "translation of an implicit surface-intersection curve requires transforming both retained supports",
            ));
        }
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
        Curve3::SurfaceIntersection(_) | Curve3::SurfaceCurve(_) => {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "an implicit surface-intersection curve has no exact translational sweep representation",
            ));
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
