//! Assembly of exact face patches into closed, manifold shells.
//!
//! Global operations (Booleans, offsets, sweeps, lofts, face edits) describe
//! their result as trimmed face patches carrying provenance. Sewing welds
//! their boundaries into shared edges and vertices; it knows nothing about the
//! operation that produced the patches.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cgmath::prelude::*;

use crate::{Point2, Point3};

use super::history::EntitySource;
use super::topology::{Body, Loop, Shell};
use super::transaction::Transaction;
use super::{
    geometry_error, invalid, topology_error, BodyId, Curve2, Curve3, EdgeId, FaceId,
    FaceOrientation, KernelResult, Line2, LoopKind, ModelTolerance, OperationHistory,
    ParameterRange, PersistentId, ShellId, ShellKind, Surface3, Topology, VertexId,
};

/// One oriented exact boundary segment of a face patch.
///
/// The 3D curve and the p-curve share the same parameter. `range` is directed
/// in loop order; sewing owns edge sharing and coedge orientation.
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
    pub edges: Vec<EntitySource>,
    pub vertices: [Vec<EntitySource>; 2],
    pub boundaries: Vec<(PersistentId, ParameterRange, ParameterRange)>,
}

impl SegmentProvenance {
    pub(crate) fn from_coedge(topology: &Topology, id: super::CoedgeId) -> KernelResult<Self> {
        let coedge = topology.coedge(id)?;
        let edge = topology.edge(coedge.edge())?;
        let vertices = topology.coedge_vertices(id)?;
        Ok(Self {
            edges: vec![EntitySource::preserved(edge.name())],
            vertices: [
                vec![EntitySource::preserved(topology.vertex(vertices[0])?.name())],
                vec![EntitySource::preserved(topology.vertex(vertices[1])?.name())],
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
                                .map(|source| EntitySource {
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

impl BrepPatchSegment {
    /// The exact trim of one existing coedge.
    pub(crate) fn from_coedge(topology: &Topology, id: super::CoedgeId) -> KernelResult<Self> {
        let coedge = topology.coedge(id)?;
        Ok(Self {
            provenance: SegmentProvenance::from_coedge(topology, id)?,
            curve: topology.edge(coedge.edge())?.curve().clone(),
            pcurve: coedge.pcurve().clone(),
            range: coedge.range(),
        })
    }
}

impl BrepPatchLoop {
    /// The exact trims of one existing loop, in its own direction and kind.
    pub(crate) fn from_loop(topology: &Topology, id: super::LoopId) -> KernelResult<Self> {
        Ok(Self {
            kind: topology.loop_(id)?.kind(),
            segments: topology
                .loop_coedges(id)?
                .into_iter()
                .map(|coedge| BrepPatchSegment::from_coedge(topology, coedge))
                .collect::<KernelResult<_>>()?,
        })
    }

    /// Traverse the loop in the opposite direction.
    pub(crate) fn reverse(&mut self) {
        self.segments.reverse();
        for segment in &mut self.segments {
            segment.range = segment.range.reversed();
            segment.provenance = segment.provenance.reversed();
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BrepPatch {
    pub sources: Vec<PersistentId>,
    pub preserved: Vec<PersistentId>,
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
        let mut intervals = BTreeMap::<PersistentId, Vec<(f64, f64)>>::new();
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

    /// Flip the material side, keeping every boundary on the same support.
    pub(crate) fn reverse(&mut self) {
        self.orientation = self.orientation.reversed();
        for loop_ in &mut self.loops {
            loop_.reverse();
        }
    }
}

/// Imprint existing boundary vertices onto unpaired exact trims before sewing.
/// Adjacent faces may describe one physical edge with different subdivisions.
/// Only parameter ranges change; supports, curves, and p-curves remain exact.
pub(crate) fn conform_brep_boundaries(
    patches: &mut [BrepPatch],
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    conform_boundaries(patches, tolerance, true)
}

/// Cells already have paired edges inside each shell. Match subdivisions across
/// those shells as well before removing their common faces and sewing them.
pub(crate) fn conform_brep_interfaces(
    patches: &mut [BrepPatch],
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    conform_boundaries(patches, tolerance, false)
}

fn conform_boundaries(
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
                                let t = segments[i].curve.domain().nearest(
                                    p.parameter,
                                    (segments[i].range.start + segments[i].range.end) * 0.5,
                                );
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
                let parameter = segment.curve.domain().nearest(
                    projection.parameter,
                    (segment.range.start + segment.range.end) * 0.5,
                );
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
                let range = ParameterRange::new(pair[0], pair[1])?;
                conformed.push(BrepPatchSegment {
                    range,
                    provenance: segment.provenance.trimmed(segment.range, range),
                    ..segment.clone()
                });
            }
        }
        loop_.segments = conformed;
    }
    Ok(())
}

/// Points welded within a fixed radius, found in constant expected time.
/// Among several candidates the earliest inserted wins, as in a linear scan.
struct PointWeld<T> {
    radius: f64,
    cells: HashMap<[i64; 3], Vec<(usize, Point3, T)>>,
    count: usize,
}

impl<T: Copy> PointWeld<T> {
    fn new(radius: f64) -> Self {
        Self {
            radius,
            cells: HashMap::new(),
            count: 0,
        }
    }

    fn cell(&self, point: Point3) -> [i64; 3] {
        [point.x, point.y, point.z].map(|value| (value / self.radius).floor() as i64)
    }

    fn find(&self, point: Point3) -> Option<T> {
        let [x, y, z] = self.cell(point);
        let mut best = None::<(usize, T)>;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for (index, candidate, value) in
                        self.cells.get(&[x + dx, y + dy, z + dz]).into_iter().flatten()
                    {
                        if candidate.distance(point) <= self.radius
                            && best.is_none_or(|(best, _)| *index < best)
                        {
                            best = Some((*index, *value));
                        }
                    }
                }
            }
        }
        best.map(|(_, value)| value)
    }

    fn insert(&mut self, point: Point3, value: T) {
        let cell = self.cell(point);
        self.cells
            .entry(cell)
            .or_default()
            .push((self.count, point, value));
        self.count += 1;
    }
}

/// An edge created while sewing, with its sampled geometry for mating.
#[derive(Clone)]
struct SewnEdge {
    edge: EdgeId,
    vertices: [VertexId; 2],
    curve: Curve3,
    range: ParameterRange,
    start: Point3,
    end: Point3,
    middle: Point3,
}

impl SewnEdge {
    fn is_closed(&self, linear: f64) -> bool {
        self.start.distance(self.end) <= linear * 8.0
    }
}

impl Transaction<'_> {
    /// Assemble exact face patches into one closed, manifold shell of a new
    /// body. Boundaries are welded by their endpoints and midpoint; publication
    /// later proves that every edge has exactly two opposite uses.
    pub(crate) fn brep_shell(&mut self, patches: &[BrepPatch]) -> KernelResult<BodyId> {
        let mut patches = patches.to_vec();
        conform_brep_boundaries(&mut patches, self.kernel.tolerance)?;
        if patches.is_empty() {
            return Err(invalid("an analytical BREP shell needs at least one face patch"));
        }
        if patches.iter().any(|patch| {
            patch.loops.iter().any(|loop_| loop_.segments.is_empty())
                || patch
                    .loops
                    .iter()
                    .filter(|loop_| loop_.kind == LoopKind::Outer)
                    .count()
                    != 1
        }) {
            return Err(topology_error(
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

        let linear = self.kernel.tolerance.linear;
        let mut vertices = PointWeld::<VertexId>::new(linear * 8.0);
        let mut edges = Vec::<SewnEdge>::new();
        let mut edges_by_vertices = BTreeMap::<[VertexId; 2], Vec<usize>>::new();
        let mut closed_circles = Vec::<usize>::new();

        for patch in &patches {
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
                self.history.derived(
                    face_name,
                    patch.sources.iter().copied(),
                    super::DerivationRole::BrepFace,
                );
                self.history.record_construction(
                    face_name,
                    patch.sources.iter().map(|name| EntitySource {
                        name: *name,
                        unchanged: patch.preserved.contains(name),
                    }),
                );
            }
            let outer_index = patch
                .loops
                .iter()
                .position(|loop_| loop_.kind == LoopKind::Outer)
                .expect("patch outer loop was validated");

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
                    let start_vertex = self.weld_vertex(&mut vertices, start)?;
                    let end_vertex = self.weld_vertex(&mut vertices, end)?;
                    self.history.record_construction(
                        self.working.vertex(start_vertex)?.name(),
                        segment.provenance.vertices[0].iter().copied(),
                    );
                    self.history.record_construction(
                        self.working.vertex(end_vertex)?.name(),
                        segment.provenance.vertices[1].iter().copied(),
                    );
                    let closed_circle = matches!(segment.curve, Curve3::Circle(_))
                        && start.distance(end) <= linear * 8.0;
                    let key = ordered_vertices(start_vertex, end_vertex);
                    let mut candidates = edges_by_vertices
                        .get(&key)
                        .into_iter()
                        .flatten()
                        .chain(closed_circle.then_some(&closed_circles).into_iter().flatten())
                        .copied()
                        .collect::<Vec<_>>();
                    candidates.sort_unstable();
                    candidates.dedup();
                    let matched = candidates
                        .into_iter()
                        .map(|index| &edges[index])
                        .find(|candidate| self.mates(candidate, segment, start, end, middle))
                        .cloned();
                    let (edge, forward, pcurve) = if let Some(candidate) = matched {
                        let (forward, pcurve) = self.mate_segment(
                            &candidate,
                            segment,
                            &patch.surface,
                            [start_vertex, end_vertex],
                            start.distance(end) <= linear * 8.0,
                        )?;
                        (candidate.edge, forward, pcurve)
                    } else {
                        let edge_name = self.names.next_for("edge")?;
                        let edge = self.make_edge(
                            edge_name,
                            [start_vertex, end_vertex],
                            segment.curve.clone(),
                            segment.range,
                        )?;
                        let sewn = SewnEdge {
                            edge,
                            vertices: [start_vertex, end_vertex],
                            curve: segment.curve.clone(),
                            range: segment.range,
                            start,
                            end,
                            middle,
                        };
                        edges_by_vertices.entry(key).or_default().push(edges.len());
                        if closed_circle {
                            closed_circles.push(edges.len());
                        }
                        edges.push(sewn);
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
        for edge in &edges {
            let uses = self.working.edge(edge.edge)?.coedges.len();
            if uses != 2 {
                return Err(topology_error(format!(
                    "analytical BREP edge has {uses} uses instead of two: {:?} -> {:?} via {:?}",
                    edge.start, edge.end, edge.middle
                )));
            }
        }
        self.separate_vertex_fans(&edges)?;
        self.seal_shell(shell)?;
        Ok(body)
    }

    fn weld_vertex(
        &mut self,
        vertices: &mut PointWeld<VertexId>,
        point: Point3,
    ) -> KernelResult<VertexId> {
        if let Some(vertex) = vertices.find(point) {
            return Ok(vertex);
        }
        let name = self.names.next_for("vertex")?;
        let vertex = self.make_vertex(name, point);
        vertices.insert(point, vertex);
        Ok(vertex)
    }

    /// Whether `segment` describes the same physical edge as `candidate`.
    fn mates(
        &self,
        candidate: &SewnEdge,
        segment: &BrepPatchSegment,
        start: Point3,
        end: Point3,
        middle: Point3,
    ) -> bool {
        let linear = self.kernel.tolerance.linear;
        if let (Curve3::Circle(first), Curve3::Circle(second)) = (&candidate.curve, &segment.curve)
        {
            if candidate.is_closed(linear)
                && start.distance(end) <= linear * 8.0
                && first.center().distance(second.center()) <= linear * 8.0
                && (first.radius() - second.radius()).abs() <= linear * 8.0
                && first.normal().dot(second.normal()).abs()
                    >= 1.0 - self.kernel.tolerance.angular * 8.0
            {
                return true;
            }
        }
        // Endpoint agreement is necessary before an exact midpoint projection
        // can identify this shared edge.
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
    }

    /// The orientation and p-curve of a second use of an already sewn edge,
    /// expressed on the retained edge curve and parameterization.
    fn mate_segment(
        &self,
        candidate: &SewnEdge,
        segment: &BrepPatchSegment,
        surface: &Surface3,
        vertices: [VertexId; 2],
        closed_segment: bool,
    ) -> KernelResult<(bool, Curve2)> {
        let tolerance = self.kernel.tolerance;
        let closed = candidate.is_closed(tolerance.linear) && closed_segment;
        let forward = if closed {
            match (&candidate.curve, &segment.curve) {
                (Curve3::Circle(first), Curve3::Circle(second)) => {
                    first.normal().dot(second.normal())
                        * candidate.range.span().signum()
                        * segment.range.span().signum()
                        > 0.0
                }
                _ => {
                    let parameter = segment.curve.project(candidate.start, tolerance)?.parameter;
                    let first = candidate
                        .curve
                        .evaluate(candidate.range.start)?
                        .first_derivative
                        * candidate.range.span().signum();
                    let second = segment.curve.evaluate(parameter)?.first_derivative
                        * segment.range.span().signum();
                    first.dot(second) > 0.0
                }
            }
        } else {
            candidate.vertices == vertices
        };
        if !closed && !forward && candidate.vertices != [vertices[1], vertices[0]] {
            return Err(topology_error("welded analytical edge endpoints disagree"));
        }
        let coedge_range = if forward {
            candidate.range
        } else {
            candidate.range.reversed()
        };
        if coedge_range == segment.range && candidate.curve == segment.curve {
            return Ok((forward, segment.pcurve.clone()));
        }
        let pcurve = match (&candidate.curve, &segment.curve, &segment.pcurve, surface) {
            (Curve3::Line(line), Curve3::Line(source), Curve2::Line(pcurve), Surface3::Cone(_)) => {
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
            }
            (
                Curve3::Circle(circle),
                Curve3::Circle(source),
                Curve2::Line(pcurve),
                Surface3::Sphere(_) | Surface3::Revolved(_),
            ) => {
                // Circle charts differ only by phase and orientation. Retain
                // the exact meridian through a revolution pole, where surface
                // projection has no unique derivative.
                let scale = circle.normal().dot(source.normal()).signum();
                let anchor = coedge_range.start.min(coedge_range.end);
                let parameter = segment
                    .curve
                    .project(candidate.curve.evaluate(anchor)?.point, tolerance)?
                    .parameter;
                let reference = segment.range.start
                    + segment.range.span() * (anchor - coedge_range.start) / coedge_range.span();
                let parameter = super::ParameterDomain::ANGLE.nearest(parameter, reference);
                Line2::new(
                    pcurve.origin() + pcurve.direction() * (parameter - scale * anchor),
                    pcurve.direction() * scale,
                )?
                .into()
            }
            // A line on a plane has an unbounded exact p-curve. Keeping that
            // representation lets later edits trim or extend the edge without
            // an artificial projection-guide domain.
            (Curve3::Line(_), _, _, Surface3::Plane(plane)) => plane
                .flatten_curve(&candidate.curve, tolerance)?
                .expect("a line has an exact plane image"),
            _ => self.projected_mate_pcurve(candidate, segment, surface, coedge_range)?,
        };
        Ok((forward, pcurve))
    }

    /// Reparameterize the retained trim in the segment's own chart. Projecting
    /// onto the surface afresh can choose another turn of a closed support.
    fn projected_mate_pcurve(
        &self,
        candidate: &SewnEdge,
        segment: &BrepPatchSegment,
        surface: &Surface3,
        coedge_range: ParameterRange,
    ) -> KernelResult<Curve2> {
        let tolerance = self.kernel.tolerance;
        let guide_range = ParameterRange::new(
            candidate.range.start.min(candidate.range.end),
            candidate.range.start.max(candidate.range.end),
        )?;
        let mut guide_points = Vec::<Point2>::new();
        for index in 0..=32 {
            let parameter = guide_range.start + guide_range.span() * index as f64 / 32.0;
            let point = candidate.curve.evaluate(parameter)?.point;
            let fraction = (parameter - coedge_range.start) / coedge_range.span();
            let reference = segment.range.start + segment.range.span() * fraction;
            let mut source_parameter = segment
                .curve
                .project_on_trim(point, segment.range, tolerance)?
                .parameter;
            if segment.curve.domain().period.is_some() {
                source_parameter = segment.curve.domain().nearest(source_parameter, reference);
            } else if index == 0 || index == 32 {
                source_parameter = reference;
            }
            guide_points.push(segment.pcurve.evaluate(source_parameter)?.point);
        }
        super::intersection::unwrap_periodic_points(&mut guide_points, surface.domains());
        let guide: Curve2 =
            super::intersection::smooth_nurbs_guide(&guide_points, guide_range)?.into();
        Ok(super::SurfaceProjectionPcurve2::new(
            candidate.curve.clone(),
            surface.clone(),
            guide,
            guide_range,
            tolerance,
        )?
        .into())
    }

    /// Spatial welding identifies edge mates, but a point contact can have
    /// several disconnected face fans. Each fan owns a topological vertex.
    fn separate_vertex_fans(&mut self, edges: &[SewnEdge]) -> KernelResult<()> {
        let mut corners = BTreeMap::<VertexId, BTreeSet<super::CoedgeId>>::new();
        for edge in edges {
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
                let mut current = start;
                while remaining.remove(&current) {
                    let coedge = self.working.coedge(current)?.clone();
                    let previous = self.working.coedge(coedge.previous)?;
                    let next = *self
                        .working
                        .edge(previous.edge)?
                        .coedges()
                        .iter()
                        .find(|id| **id != coedge.previous)
                        .expect("sewn edge has two uses");
                    self.working.edges.get_mut(coedge.edge)?.vertices
                        [usize::from(!coedge.forward)] = target;
                    current = next;
                }
            }
        }
        Ok(())
    }

    /// Assemble newly created components using signed BREP volume and trimmed
    /// surface classification. A containment witness is sufficient only after
    /// establishing that the two closed boundaries are disjoint.
    pub(crate) fn nest_brep_shells(&mut self, bodies: &[BodyId]) -> KernelResult<()> {
        if bodies.len() < 2 {
            return Ok(());
        }
        let topology = &self.working;
        let tolerance = self.kernel.tolerance;
        let shells = bodies
            .iter()
            .map(|body| {
                let value = topology.body(*body)?;
                if value.shells().len() != 1 {
                    return Err(topology_error(
                        "a newly assembled BREP component must contain exactly one shell",
                    ));
                }
                let shell = value.shells()[0];
                let volume = super::physical::shell_signed_volume(topology, shell, tolerance)?;
                if volume.abs() <= tolerance.linear.powi(3) {
                    return Err(geometry_error(
                        "an analytical BREP shell has zero volume at model tolerance",
                    ));
                }
                Ok((*body, shell, volume))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let mut containers = Vec::new();
        for (body, shell, volume) in &shells {
            if *volume > 0.0 {
                containers.push((
                    *body,
                    *shell,
                    *volume,
                    super::brep_classification::BrepClassifier::new(topology, *shell, tolerance)?,
                ));
            }
        }
        containers.sort_by(|a, b| a.2.total_cmp(&b.2));
        let mut attachments = Vec::new();
        for (body, cavity, volume) in &shells {
            if *volume > 0.0 {
                continue;
            }
            let witness = shell_witness(topology, *cavity)?;
            let mut parent = None;
            for (outer_body, outer, _, classifier) in &containers {
                if !shells_intersect(topology, *outer, *cavity, tolerance, None)?
                    && classifier.contains(witness)?
                {
                    parent = Some(*outer_body);
                    break;
                }
            }
            let parent = parent.ok_or_else(|| {
                topology_error(
                    "an inward-oriented analytical BREP shell is not contained by an outer shell",
                )
            })?;
            attachments.push((parent, *body, *cavity));
        }
        for (index, (parent, _, shell)) in attachments.iter().enumerate() {
            for (other_parent, _, other) in &attachments[..index] {
                if parent == other_parent
                    && shells_intersect(topology, *shell, *other, tolerance, None)?
                {
                    return Err(geometry_error("cavity boundaries intersect"));
                }
            }
        }
        for (parent, body, _) in attachments {
            self.merge_shell_into_body(parent, body)?;
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
        let topology = &self.working;
        let tolerance = self.kernel.tolerance;
        let changed_faces = topology
            .faces()
            .filter_map(|(id, face)| {
                self.history
                    .construction
                    .get(&face.name())
                    .is_some_and(|sources| {
                        sources.iter().any(|source| affected.contains(&source.name))
                    })
                    .then_some(id)
            })
            .collect::<BTreeSet<_>>();
        let source_bodies = rebuilt
            .keys()
            .map(|shell| source.shell(*shell).map(|value| value.body()))
            .collect::<KernelResult<BTreeSet<_>>>()?;
        let mut merges = Vec::new();
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
                    if shells_intersect(topology, cavity, *other, tolerance, Some(&changed_faces))? {
                        return Err(geometry_error(
                            "an edited cavity boundary intersects another shell",
                        ));
                    }
                }
                let changed = [outer, cavity].into_iter().try_fold(false, |changed, shell| {
                    Ok::<_, super::KernelError>(
                        changed
                            || topology
                                .shell(shell)?
                                .faces()
                                .iter()
                                .any(|face| changed_faces.contains(face)),
                    )
                })?;
                if changed
                    && !super::brep_classification::BrepClassifier::new(topology, outer, tolerance)?
                        .contains(shell_witness(topology, cavity)?)?
                {
                    return Err(geometry_error(
                        "an edited cavity lies outside its original outer shell",
                    ));
                }
                shells.push(cavity);
                merges.push((outer_body, cavity_body));
            }
        }
        for (outer_body, cavity_body) in merges {
            self.merge_shell_into_body(outer_body, cavity_body)?;
        }
        Ok(())
    }
}

fn ordered_vertices(first: VertexId, second: VertexId) -> [VertexId; 2] {
    if first <= second {
        [first, second]
    } else {
        [second, first]
    }
}

fn shell_witness(topology: &Topology, shell: ShellId) -> KernelResult<Point3> {
    let face = topology.shell(shell)?.faces()[0];
    let edge = topology.edge(topology.face_edges(face)?[0])?;
    edge.curve()
        .evaluate(edge.range().start)
        .map(|value| value.point)
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

/// The planar face bounded by a profile placed on `plane`.
pub(crate) fn cap(profile: &super::Profile, plane: super::Plane3, reversed: bool) -> KernelResult<BrepPatch> {
    let mut loops = Vec::new();
    for (index, wire) in std::iter::once(profile.outer())
        .chain(profile.holes())
        .enumerate()
    {
        let segments = wire
            .segments()
            .iter()
            .map(|segment| {
                Ok(BrepPatchSegment {
                    provenance: Default::default(),
                    curve: plane.lift_trimmed_curve(segment, profile.tolerance())?,
                    pcurve: segment.curve().clone(),
                    range: segment.range(),
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        loops.push(BrepPatchLoop {
            kind: if index == 0 {
                LoopKind::Outer
            } else {
                LoopKind::Inner
            },
            segments,
        });
    }
    let mut patch = BrepPatch {
        preserved: Vec::new(),
        sources: Vec::new(),
        surface: plane.into(),
        orientation: FaceOrientation::Forward,
        loops,
    };
    if reversed {
        patch.reverse();
    }
    Ok(patch)
}
/// Publish a closed shell sewn from `patches` as a new compound.
pub(crate) fn publish(patches: &[BrepPatch], tolerance: ModelTolerance) -> KernelResult<super::Compound> {
    let mut kernel = super::Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    transaction.brep_shell(patches)?;
    let committed = transaction.finish()?;
    super::Compound::from_operation(kernel, committed, [])
}

/// Publish patches bounding several closed shells. Each connected shell is
/// sewn on its own and inward shells become cavities of the shells holding them.
fn publish_shells(patches: Vec<BrepPatch>, tolerance: ModelTolerance) -> KernelResult<super::Compound> {
    let components = super::boolean::patch_components(patches, tolerance)?;
    let mut kernel = super::Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    let bodies = components
        .iter()
        .map(|component| transaction.brep_shell(component))
        .collect::<KernelResult<Vec<_>>>()?;
    transaction.nest_brep_shells(&bodies)?;
    let committed = transaction.finish()?;
    super::Compound::from_operation(kernel, committed, [])
}

// Attach roles at construction, before welding/unification can merge patches.
// The tokens identify generators, not positions in the published face array.
fn role_roots(patches: &mut [BrepPatch], roles: Vec<super::CreationRole>)
    -> std::collections::BTreeMap<PersistentId, super::CreationRole> {
    let namespace = super::identity::fresh_uuid();
    let roots = roles.into_iter().enumerate().map(|(index, role)|
        (PersistentId { namespace, local: index as u64 }, role)).collect::<std::collections::BTreeMap<_, _>>();
    for (patch, (name, _)) in patches.iter_mut().zip(&roots) {
        patch.sources.push(*name);
    }
    roots
}

pub(crate) fn publish_with_roles(mut patches: Vec<BrepPatch>, roles: Vec<super::CreationRole>, tolerance: ModelTolerance) -> KernelResult<super::Compound> {
    let roots = role_roots(&mut patches, roles);
    Ok(publish(&patches, tolerance)?.with_patch_roles(&roots))
}

/// Like `publish_with_roles`, for bodies that can enclose a cavity, such as
/// a hollow profile swept or revolved all the way around.
pub(crate) fn publish_shells_with_roles(mut patches: Vec<BrepPatch>, roles: Vec<super::CreationRole>, tolerance: ModelTolerance) -> KernelResult<super::Compound> {
    let roots = role_roots(&mut patches, roles);
    Ok(publish_shells(patches, tolerance)?.with_patch_roles(&roots))
}
