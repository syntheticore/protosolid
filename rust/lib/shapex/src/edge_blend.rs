//! Local edge treatments: shared discovery, trimming, junction topology and
//! transactional publication for circular blends and ruled chamfers.
//!
//! [`Transaction::blend_network`] drives one blend through its phases:
//! spines along the selected edges (`spine`), their free ends (`endpoints`)
//! and corner transitions (`junctions`), one face patch per spine (`patch`),
//! and finally the topology rewrite (`rewrite`). Periodic seams that would
//! interfere with the network are moved out of the way first (`seams`).

mod endpoints;
mod junctions;
mod patch;
mod rewrite;
mod seams;
mod spine;

use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3};

use super::transaction::Transaction;
use super::history::{BlendRole, ChangeKind, DerivationRole};
use super::modeling::{ChamferMode, ChamferSettings};
use super::topology::{Edge, Vertex};
use super::trim::{
    align_pcurve_to_face_chart, face_boundary_curves, face_parameter_bounds,
    intersection_chart_copies,
};
use super::wire::{curve_loop_contains_point, curve_pair_intersection};
use super::{
    nearest_uv, Circle2, Circle3, CoedgeId, Cone3, Curve2, Curve3, Cylinder3, DiagnosticKind,
    DomainBounds, EdgeId, FaceId, FaceOrientation, Frame3, IntersectionSet, KernelError,
    KernelResult, Line2, LoopId, ModelTolerance, ParameterDomain, ParameterRange, PersistentId,
    Plane3, RevolvedSurface3, ShellId,
    RollingBallSurface3, Sphere3, Surface3, SurfaceCurve3, SurfaceIntersectionCurve,
    SurfaceProjectionPcurve2, SurfaceSurfaceCurve, Torus3, TrimmedCurve2, VertexId,
};
use crate::{topology_error, unsupported};

/// Whether a local blend was built or needs another construction from the source.
pub(crate) enum BlendOutcome {
    Built,
    Staged(BlendStaging),
    Overflow(BlendTrimViolation),
}

/// Where the contact rail prevents a local rewrite of its support.
pub(crate) enum BlendTrimViolation {
    CrossesBoundary,
    InsideHole,
    OutsideFace,
    LeavesHoleOutsideTrim,
}

impl BlendTrimViolation {
    pub(crate) fn into_error(self) -> KernelError {
        let message = match self {
            Self::CrossesBoundary => "blend contact boundary crosses an existing face boundary",
            Self::InsideHole => "blend contact boundary enters an existing face hole",
            Self::OutsideFace => "blend contact boundary leaves its incident face trim",
            Self::LeavesHoleOutsideTrim => "blend contact boundary leaves an existing hole outside the new face trim",
        };
        crate::geometry_error(message)
    }
}

/// A corner that one network cannot round, and the sequence that can.
pub(crate) enum BlendStaging {
    /// Concave rails set back the corner before the convex network enters it.
    MixedTrihedral { concave_edges: Vec<PersistentId> },
    /// Opposite blends of a four-face corner, each ending on the other.
    CrossingCorner { pair: [PersistentId; 2] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeBlendKind {
    Fillet,
    WidthFillet,
    CurvatureFillet,
    CurvatureWidthFillet,
    Chamfer,
}

impl EdgeBlendKind {
    pub(crate) fn fillet(width: bool, curvature: bool) -> Self {
        match (width, curvature) {
            (false, false) => Self::Fillet,
            (true, false) => Self::WidthFillet,
            (false, true) => Self::CurvatureFillet,
            (true, true) => Self::CurvatureWidthFillet,
        }
    }

    fn constant_width(self) -> bool {
        matches!(self, Self::WidthFillet | Self::CurvatureWidthFillet)
    }

    fn curvature_continuous(self) -> bool {
        matches!(self, Self::CurvatureFillet | Self::CurvatureWidthFillet)
    }

    fn role(self, role: BlendRole) -> DerivationRole {
        match self {
            Self::Chamfer => DerivationRole::Chamfer(role),
            _ => DerivationRole::Fillet(role),
        }
    }
}

#[derive(Debug, Clone)]
struct BlendEndpoint {
    old_vertex: VertexId,
    terminating_face: FaceId,
    // Shared terminating seam and the parent contact that moves its endpoint.
    tangent_termination: Option<(EdgeId, usize)>,
    // Seam to a second, non-tangent terminating face that the end boundary
    // crosses on its way to the second contact.
    crossing: Option<(EdgeId, FaceId)>,
    adjacent_uses: [CoedgeId; 2],
    contact_parameters: [f64; 2],
    contact_points: [Point3; 2],
    allow_adjacent_extension: [bool; 2],
    cap: Option<(usize, Plane3)>,
}

#[derive(Debug, Clone)]
struct BlendBoundary {
    curve: Curve3,
    parent_pcurve: Curve2,
    blend_pcurve: Curve2,
    range: ParameterRange,
}

#[derive(Debug, Clone)]
struct BlendSpine {
    edge_id: EdgeId,
    edge: Edge,
    uses: [CoedgeId; 2],
    faces: [FaceId; 2],
    parent_surfaces: [Surface3; 2],
    concave: bool,
    offset: f64,
    offsets: [f64; 2],
    center: SurfaceSurfaceCurve,
    endpoint_seeds: [f64; 2],
    provisional_range: ParameterRange,
    shared_seam: Option<VertexId>,
    local_guide: bool,
}

impl BlendSpine {
    /// The domain of the spine's center parameter. Along a shared seam the
    /// local guide uses a normalized traversal with unit period.
    fn center_domain(&self) -> ParameterDomain {
        if self.shared_seam.is_some() {
            UNIT_PERIOD
        } else {
            self.center.curve.domain()
        }
    }
}

/// The normalized traversal of a closed guide.
const UNIT_PERIOD: ParameterDomain = ParameterDomain {
    bounds: DomainBounds::Unbounded,
    period: Some(1.0),
};

#[derive(Debug, Clone)]
struct BlendJunction {
    faces: Vec<FaceId>,
    center: Point3,
    contact_points: BTreeMap<FaceId, Point3>,
    kind: BlendJunctionKind,
}

#[derive(Debug, Clone)]
enum BlendJunctionKind {
    Miter {
        seam_edge: EdgeId,
        wall_faces: [FaceId; 2],
    },
    RimCorner {
        carrier_face: FaceId,
        seam_edge: EdgeId,
    },
    Smooth {
        seam_edge: EdgeId,
        tangent_faces: [FaceId; 2],
        second_seam: Option<(EdgeId, [FaceId; 2])>,
    },
    TangentChain {
        tangent_faces: [FaceId; 2],
    },
    Trihedral,
}

/// How a selected spine ends at one of its vertices.
enum SpineEnd {
    /// The spine meets other blends. A `continuation` edge, tangent across
    /// the given faces, joins the network with the spine's offsets.
    Junction {
        kind: BlendJunctionKind,
        continuation: Option<(EdgeId, [FaceId; 2])>,
    },
    /// The network ends here.
    Endpoint(BlendEndpoint),
    /// The corner needs a staged construction.
    Staged(BlendStaging),
}

/// For each blended edge end on a boundary, its consecutive terminating-face
/// uses around the blended vertex and whether the first is the rail's mate.
type TerminationGaps = BTreeMap<(PersistentId, VertexId), (CoedgeId, CoedgeId, bool)>;

/// Topology created while rewriting a blend network, shared by its phases.
#[derive(Default)]
struct BlendRewrite {
    termination_gaps: TerminationGaps,
    /// The gaps on the second support of end boundaries crossing a seam.
    crossing_gaps: TerminationGaps,
    /// Contact vertex of each blended vertex on a face, per selected edge
    /// where the vertex does not join a junction. An end boundary crossing a
    /// terminating seam has its crossing keyed by the second support and seam.
    contact_vertices: BTreeMap<(VertexId, FaceId, Option<EdgeId>), VertexId>,
    /// Boundary edges consumed entirely by a contact, with their kept vertex.
    collapsed_edges: BTreeMap<EdgeId, VertexId>,
    junction_faces: BTreeMap<VertexId, FaceId>,
    junction_loops: BTreeMap<VertexId, LoopId>,
    junction_uses: BTreeMap<VertexId, Vec<CoedgeId>>,
    cap_loops: BTreeMap<(EdgeId, usize), LoopId>,
    cap_uses: BTreeMap<(EdgeId, usize), Vec<CoedgeId>>,
    /// The unordered boundary uses and construction winding of each rolling face.
    blend_cycles: Vec<(LoopId, Vec<CoedgeId>, FaceOrientation)>,
    /// The transition edge and the mate's p-curve when supplied by its construction.
    smooth_edges: BTreeMap<VertexId, (EdgeId, Option<Curve2>)>,
    contact_parent_uses: BTreeMap<(EdgeId, usize), CoedgeId>,
}

#[derive(Debug, Clone)]
enum BlendPatchEnd {
    Boundary(BlendEndpoint),
    Junction(VertexId),
}

#[derive(Debug, Clone)]
struct BlendPatch {
    spine: BlendSpine,
    ends: [BlendPatchEnd; 2],
    contact_parameters: [[f64; 2]; 2],
    contact_points: [[Point3; 2]; 2],
    contact_boundaries: [BlendBoundary; 2],
    end_boundaries: [BlendBoundary; 2],
    /// Where an end boundary crosses its terminating seam, and its part from
    /// there to the second contact. `end_boundaries` then ends at the crossing.
    crossings: [Option<(Point3, BlendBoundary)>; 2],
    blend_surface: Surface3,
    blend_parameterization: BlendSurfaceParameterization,
}

/// Affine map from rolling coordinates `(center_parameter, blend_fraction)`
/// into a selected support surface's native parameters.
#[derive(Debug, Clone, Copy)]
struct BlendSurfaceParameterization {
    origin: Point2,
    center_direction: Vec2,
    blend_direction: Vec2,
}

impl BlendSurfaceParameterization {
    const CONTACT_RAILS: Self = Self {
        origin: Point2::new(0.0, 0.0),
        center_direction: Vec2::new(1.0, 0.0),
        blend_direction: Vec2::new(0.0, 1.0),
    };

    fn contact_pcurve(self, parent_index: usize) -> KernelResult<Curve2> {
        Line2::new(
            self.origin + self.blend_direction * parent_index as f64,
            self.center_direction,
        )
        .map(Curve2::from)
    }

    fn end_pcurve(self, first: f64, second: f64) -> KernelResult<Curve2> {
        Line2::new(
            self.origin + self.center_direction * first,
            self.center_direction * (second - first) + self.blend_direction,
        )
        .map(Curve2::from)
    }

    fn oriented_end_pcurve(self, first: f64, second: f64, forward: bool) -> KernelResult<Curve2> {
        if forward {
            return self.end_pcurve(first, second);
        }
        Line2::new(
            self.origin + self.center_direction * second + self.blend_direction,
            self.center_direction * (first - second) - self.blend_direction,
        )
        .map(Curve2::from)
    }
}

impl Transaction<'_> {
    pub(crate) fn blend_edges(
        &mut self,
        edges: &[EdgeId],
        radius: f64,
        follow_tangent_edges: bool,
        kind: EdgeBlendKind,
        chamfer: Option<&ChamferSettings>,
    ) -> KernelResult<BlendOutcome> {
        self.blend_network(
            edges.iter().copied().collect(),
            radius,
            follow_tangent_edges,
            kind,
            chamfer,
        )
    }

    fn blend_network(
        &mut self,
        selected: BTreeSet<EdgeId>,
        radius: f64,
        follow_tangent_edges: bool,
        kind: EdgeBlendKind,
        chamfer: Option<&ChamferSettings>,
    ) -> KernelResult<BlendOutcome> {
        if kind == EdgeBlendKind::Chamfer {
            return self.blend_network_with_guide(
                selected, radius, follow_tangent_edges, kind, chamfer, false,
            );
        }
        let checkpoint = self.checkpoint();
        let result = self.blend_network_with_guide(
            selected.clone(), radius, follow_tangent_edges, kind, chamfer, false,
        );
        match result {
            Err(error) if error.diagnostics().iter()
                .any(|diagnostic| diagnostic.kind == DiagnosticKind::NumericalFailure) => {
                // A long approximating guide can move off its corrected branch
                // between samples. Retry the network once with an interpolating guide.
                self.restore(checkpoint);
                self.blend_network_with_guide(
                    selected, radius, follow_tangent_edges, kind, chamfer, true,
                )
            }
            result => result,
        }
    }

    fn blend_network_with_guide(
        &mut self,
        mut selected: BTreeSet<EdgeId>,
        radius: f64,
        follow_tangent_edges: bool,
        kind: EdgeBlendKind,
        chamfer: Option<&ChamferSettings>,
        interpolate_guides: bool,
    ) -> KernelResult<BlendOutcome> {
        let input_names = selected
            .iter()
            .map(|edge| self.working.edge(*edge).map(|edge| edge.name))
            .collect::<KernelResult<Vec<_>>>()?;
        self.relocate_blend_endpoint_seams(&selected, kind)?;
        let (rebound, settings) = self.rebind_blend_selection(&input_names, chamfer)?;
        selected = rebound;
        let chamfer = settings.as_ref();
        let input_chamfer = chamfer;
        let input_state = self.checkpoint();
        // General rolling supports must share one chart before their temporary
        // arcs are built, otherwise structurally different guides prevent KEF.
        let mut closed_spines = Vec::new();
        for edge in &selected {
            let value = self.working.edge(*edge)?;
            if value.vertices[0] == value.vertices[1] {
                if let Ok(mut spine) =
                    self.prepare_blend_spine(*edge, radius, kind, None, None, chamfer, interpolate_guides)
                {
                    if matches!(spine.center.curve, Curve3::SurfaceIntersection(_)) {
                        if !spine.local_guide {
                            spine.center = closed_blend_chart(&spine.center, value, self.kernel.tolerance)?;
                            spine.local_guide = true;
                        }
                        closed_spines.push(spine);
                    }
                }
            }
        }
        let sources = selected
            .iter()
            .map(|edge| self.working.edge(*edge).cloned())
            .collect::<KernelResult<Vec<_>>>()?;
        let closed_seams = sources
            .iter()
            .filter(|edge| edge.vertices[0] == edge.vertices[1])
            .map(|edge| edge.vertices[0])
            .collect::<BTreeSet<_>>();
        selected = self.subdivide_closed_blend_edges(&selected, kind)?;
        let mut chamfer = chamfer.cloned();
        if let Some(settings) = &mut chamfer {
            for edge in &selected {
                let value = self.working.edge(*edge)?;
                if !settings.reference_faces.contains_key(&value.name) {
                    let source = sources
                        .iter()
                        .find(|source| source.curve == value.curve)
                        .expect("subdivided chamfer edge retains its source curve");
                    settings
                        .reference_faces
                        .insert(value.name, settings.reference_faces[&source.name]);
                }
            }
        }
        let mut junction_kinds = BTreeMap::<VertexId, BlendJunctionKind>::new();
        let mut junction_sources = BTreeMap::<VertexId, EdgeId>::new();
        let mut spines = BTreeMap::<EdgeId, BlendSpine>::new();
        let mut propagated_offsets = BTreeMap::<EdgeId, (BlendSpine, [FaceId; 2])>::new();
        let mut offset_components = BTreeSet::new();
        let mut regular_endpoints = BTreeMap::<(EdgeId, usize), Option<BlendEndpoint>>::new();
        loop {
            let missing = selected
                .iter()
                .copied()
                .filter(|edge| !spines.contains_key(edge))
                .collect::<Vec<_>>();
            for edge in missing {
                let value = self.working.edge(edge)?;
                let faces = self.working.edge_faces(edge)?;
                let shared = closed_spines.iter().find(|spine| {
                    spine.edge.curve == value.curve
                        && spine.faces.iter().all(|face| faces.contains(face))
                });
                let spine = self.prepare_blend_spine(
                    edge,
                    radius,
                    kind,
                    shared,
                    propagated_offsets.get(&edge),
                    chamfer.as_ref(),
                    interpolate_guides,
                )?;
                // Splitting a rim between the same supports must not change
                // its generating offsets at each fragment's midpoint.
                let mut pending = if offset_components.contains(&edge) {
                    Vec::new()
                } else {
                    vec![edge]
                };
                while let Some(current) = pending.pop() {
                    if !offset_components.insert(current) {
                        continue;
                    }
                    for vertex in self.working.edge(current)?.vertices {
                        for adjacent in self.working.vertex_edges(vertex)? {
                            if selected.contains(&adjacent)
                                && self
                                    .working
                                    .edge_faces(adjacent)?
                                    .iter()
                                    .all(|face| spine.faces.contains(face))
                            {
                                propagated_offsets
                                    .entry(adjacent)
                                    .or_insert_with(|| (spine.clone(), spine.faces));
                                pending.push(adjacent);
                            }
                        }
                    }
                }
                spines.insert(edge, spine);
            }
            let mut additions = BTreeSet::new();
            for spine in spines.values() {
                for endpoint_index in 0..2 {
                    let vertex = spine.edge.vertices[endpoint_index];
                    if junction_kinds.contains_key(&vertex) {
                        continue;
                    }
                    match self.classify_spine_end(
                        spine,
                        endpoint_index,
                        &selected,
                        &spines,
                        kind,
                        follow_tangent_edges,
                    )? {
                        SpineEnd::Junction {
                            kind: junction_kind,
                            continuation,
                        } => {
                            junction_kinds.insert(vertex, junction_kind);
                            junction_sources.insert(vertex, spine.edge_id);
                            if let Some((continuation, tangent_faces)) = continuation {
                                additions.insert(continuation);
                                propagated_offsets
                                    .entry(continuation)
                                    .or_insert_with(|| (spine.clone(), tangent_faces));
                            }
                        }
                        SpineEnd::Endpoint(endpoint) => {
                            regular_endpoints
                                .insert((spine.edge_id, endpoint_index), Some(endpoint));
                        }
                        SpineEnd::Staged(staging) => return Ok(BlendOutcome::Staged(staging)),
                    }
                }
            }
            let previous = selected.len();
            selected.extend(additions);
            if selected.len() == previous {
                if (kind.curvature_continuous() || kind.constant_width()) && junction_kinds.values().any(|junction| {
                    matches!(junction, BlendJunctionKind::Trihedral | BlendJunctionKind::RimCorner { .. })
                }) {
                    return Err(unsupported("constant-width and G2 fillets do not yet support three-edge corners or inner-rim corner patches"));
                }
                let mut junctions = BTreeMap::new();
                for (vertex, junction_kind) in &junction_kinds {
                    if matches!(junction_kind, BlendJunctionKind::Miter { .. }) {
                        junctions.insert(
                            *vertex,
                            self.prepare_miter_junction(*vertex, junction_kind.clone(), &spines)?,
                        );
                        continue;
                    }
                    let spine = spines.get(&junction_sources[vertex]).ok_or_else(|| {
                        topology_error("blend junction source edge was not retained")
                    })?;
                    if let BlendJunctionKind::Smooth {
                        seam_edge,
                        tangent_faces,
                        ..
                    } = junction_kind
                    {
                        let junction = self.prepare_smooth_fillet_junction(
                            *vertex,
                            spine,
                            *seam_edge,
                            *tangent_faces,
                        );
                        match junction {
                            Ok(mut junction) => {
                                // Adjacent walls may share a tangent at the
                                // source vertex and form a crease farther up
                                // the seam where the new rails meet them.
                                if junction.faces.len() == 3 {
                                    let point = junction.contact_points[&tangent_faces[0]];
                                    let tolerance = self.kernel.tolerance;
                                    let first = self.working.face(tangent_faces[0])?.oriented_normal_near(point, tolerance)?;
                                    let second = self.working.face(tangent_faces[1])?.oriented_normal_near(point, tolerance)?;
                                    if first.dot(second) < 1.0 - tolerance.angular.max(f64::EPSILON.sqrt()) * 8.0 {
                                        junction = self.prepare_miter_junction(
                                            *vertex,
                                            BlendJunctionKind::Miter { seam_edge: *seam_edge, wall_faces: *tangent_faces },
                                            &spines,
                                        )?;
                                    }
                                }
                                junctions.insert(*vertex, junction);
                            }
                            Err(error)
                                if kind == EdgeBlendKind::Fillet
                                    && self.working.vertex_faces(*vertex)?.len() == 4 =>
                            {
                                return self.split_smooth_blend_transition(
                                    *vertex,
                                    *tangent_faces,
                                    &selected,
                                    radius,
                                    follow_tangent_edges,
                                    error,
                                ).map(|()| BlendOutcome::Built);
                            }
                            Err(error) => return Err(error),
                        }
                        continue;
                    }
                    let endpoint_index = spine
                        .edge
                        .vertices
                        .iter()
                        .position(|candidate| candidate == vertex)
                        .ok_or_else(|| {
                            topology_error("blend network junction has no incident rolling patch")
                        })?;
                    if let BlendJunctionKind::TangentChain { tangent_faces } = junction_kind {
                        junctions.insert(
                            *vertex,
                            self.prepare_tangent_chain_junction(
                                spine,
                                endpoint_index,
                                *tangent_faces,
                            )?,
                        );
                        continue;
                    }
                    if kind == EdgeBlendKind::Chamfer
                        && matches!(junction_kind, BlendJunctionKind::Trihedral)
                    {
                        junctions.insert(*vertex, self.prepare_chamfer_junction(*vertex, &spines)?);
                        continue;
                    }
                    junctions.insert(
                        *vertex,
                        match junction_kind {
                            BlendJunctionKind::RimCorner { .. } => {
                                self.prepare_rim_corner_junction(*vertex, junction_kind.clone())?
                            }
                            _ => self.prepare_fillet_junction(
                                spine,
                                endpoint_index,
                                spine.offset,
                                junction_kind.clone(),
                            )?,
                        },
                    );
                }
                let junction_surfaces = junctions
                    .iter()
                    .filter_map(|(vertex, junction)| match junction.kind {
                        BlendJunctionKind::Trihedral => Some((|| {
                            let radials = junction
                                .faces
                                .iter()
                                .map(|face| {
                                    (junction.contact_points[face] - junction.center).normalize()
                                })
                                .collect::<Vec<_>>();
                            if kind == EdgeBlendKind::Chamfer {
                                let points = junction
                                    .faces
                                    .iter()
                                    .map(|face| junction.contact_points[face])
                                    .collect::<Vec<_>>();
                                return Plane3::new(
                                    points[0],
                                    points[1] - points[0],
                                    points[2] - points[0],
                                    self.kernel.tolerance,
                                )
                                .map(|plane| (*vertex, Surface3::from(plane)));
                            }
                            // Keep the spherical corner near the equator and
                            // away from the longitude seam. A pole inside the
                            // patch makes its single UV loop wind around a
                            // chart singularity instead of bounding the corner.
                            let equator = (radials[0] + radials[1] + radials[2]).normalize();
                            let across = radials[1] - radials[0];
                            let across = (across - equator * across.dot(equator)).normalize();
                            // Center the patch at longitude PI, not at the
                            // projection seam where longitude wraps to zero.
                            let frame = Frame3::new(
                                junction.center,
                                -equator,
                                across,
                                self.kernel.tolerance,
                            )?;
                            Sphere3::new(
                                junction.center,
                                frame.x() * radius,
                                frame.y() * radius,
                                frame.z() * radius,
                                self.kernel.tolerance,
                            )
                            .map(|sphere| (*vertex, Surface3::from(sphere)))
                        })()),
                        BlendJunctionKind::RimCorner {
                            carrier_face,
                            seam_edge,
                        } => Some(
                            self.rim_corner_surface(*vertex, carrier_face, seam_edge, radius, kind)
                                .map(|surface| (*vertex, surface)),
                        ),
                        _ => None,
                    })
                    .collect::<KernelResult<BTreeMap<_, _>>>()?;
                let mut patches = spines
                    .into_values()
                    .map(|spine| {
                        self.prepare_blend_patch(
                            spine,
                            &junctions,
                            &junction_surfaces,
                            &regular_endpoints,
                            kind,
                        )
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                // At a non-tangent two-edge corner the rolling patches meet
                // along their exact intersection, not a constant-angle arc.
                for (vertex, junction) in &junctions {
                    if !matches!(junction.kind, BlendJunctionKind::Miter { .. }) {
                        continue;
                    }
                    let incident = patches
                        .iter()
                        .enumerate()
                        .filter_map(|(index, patch)| {
                            patch
                                .ends
                                .iter()
                                .position(
                                    |end| matches!(end, BlendPatchEnd::Junction(v) if v == vertex),
                                )
                                .map(|end| (index, end))
                        })
                        .collect::<Vec<_>>();
                    if incident.len() != 2 {
                        return Err(topology_error("a miter needs two rolling patches"));
                    }
                    for side in 0..2 {
                        let (index, end) = incident[side];
                        let other = patches[incident[1 - side].0].blend_surface.clone();
                        let patch = &mut patches[index];
                        let uv = other
                            .project(patch.contact_points[end][0], self.kernel.tolerance)?
                            .parameters;
                        patch.end_boundaries[end] = self.prepare_end_boundary(
                            &patch.blend_surface,
                            other,
                            patch.end_boundaries[end].blend_pcurve.clone(),
                            patch.contact_points[end],
                            Point2::new(uv.0, uv.1),
                        )?;
                    }
                }
                let consumed = self.consumed_blend_boundaries(&patches)?;
                if !consumed.is_empty() {
                    let prepared = self.checkpoint();
                    for (boundary, vertex) in consumed {
                        // Normalize the input, before temporary periodic splits.
                        // Replaying a partially split network would lose pieces
                        // of another selected closed rim when its seam moves.
                        self.restore(input_state.clone());
                        if self.normalize_blend_boundary(boundary, vertex, kind)? {
                            let (selected, settings) =
                                self.rebind_blend_selection(&input_names, input_chamfer)?;
                            return self.blend_network_with_guide(
                                selected,
                                radius,
                                follow_tangent_edges,
                                kind,
                                settings.as_ref(),
                                interpolate_guides,
                            );
                        }
                    }
                    self.restore(prepared);
                }
                if let Some(violation) = self.blend_contact_trim_violation(&patches)? {
                    return Ok(BlendOutcome::Overflow(violation));
                }
                if let Some(violation) = self.rewrite_blend_network(
                    &patches,
                    &junctions,
                    &junction_surfaces,
                    &closed_seams,
                    kind,
                )? {
                    return Ok(BlendOutcome::Overflow(violation));
                }
                if junctions.values().any(|junction| {
                    matches!(junction.kind, BlendJunctionKind::Miter { wall_faces, .. }
                        if junction.contact_points[&wall_faces[0]].distance(junction.contact_points[&wall_faces[1]]) > self.kernel.tolerance.linear)
                }) {
                    // Distinct wall contacts form a four-face corner. Reuse
                    // support reconstruction to split it into two vertices
                    // and their exact intersection edge.
                    let tolerance = self.kernel.tolerance;
                    let mut supports = BTreeMap::new();
                    let mut bounds = BTreeMap::new();
                    for (face, value) in self.working.faces() {
                        let trim = super::features::face_parameter_bounds(
                            &self.working, face, tolerance,
                        )?;
                        let surface = super::features::extended_support(
                            &value.surface, trim, radius, tolerance,
                        )?;
                        bounds.insert(face, super::features::expanded_offset_bounds(
                            &surface, trim, radius, tolerance,
                        )?);
                        supports.insert(face, surface);
                    }
                    let patches = super::features::support_patches(
                        &self.working,
                        tolerance,
                        super::features::SupportEdit {
                            supports,
                            bounds,
                            preserve_orientation: true,
                            reconstruct_edges: true,
                            ..Default::default()
                        },
                    )?;
                    let source = std::mem::take(&mut self.working);
                    self.touched_bodies.clear();
                    let bodies = patches.into_iter()
                        .map(|(shell, patches)| Ok((shell, self.brep_shell(&patches)?)))
                        .collect::<KernelResult<BTreeMap<_, _>>>()?;
                    self.restore_brep_shells(&source, &bodies, &BTreeSet::new())?;
                    // Reconstruction changes the boundary graph, not the
                    // identity of its supports. Retain each authored face and
                    // its blend history instead of replacing it with a BREP
                    // face whose sequential name can change on regeneration.
                    let identities = self.working.faces().map(|(id, face)| {
                        (id, face.name(), self.history.primary_source(face.name()))
                    }).collect::<Vec<_>>();
                    for (id, generated, authored) in identities {
                        self.working.faces.get_mut(id)?.name = authored;
                        self.history.forget_output(generated);
                    }
                }
                return Ok(BlendOutcome::Built);
            }
            if selected.len() > 256 {
                return Err(unsupported("blend transition propagation exceeded the connected regular edge network"));
            }
        }
    }

    fn adjacent_use_at_vertex(
        &self,
        selected_use: CoedgeId,
        vertex: VertexId,
    ) -> KernelResult<CoedgeId> {
        let vertices = self.working.coedge_vertices(selected_use)?;
        if vertices[0] == vertex {
            Ok(self.working.coedge(selected_use)?.previous)
        } else if vertices[1] == vertex {
            Ok(self.working.coedge(selected_use)?.next)
        } else {
            Err(topology_error("selected coedge is not incident to its blend endpoint"))
        }
    }
}

// Shared closed blends use one source traversal per unit parameter, with a
// continuation on either side of the seam. Analytical intersection charts
// (such as a tilted plane/cylinder ellipse) need the same chart as local guides
// before the edge is divided into temporary arcs.
fn closed_blend_chart(
    center: &SurfaceSurfaceCurve,
    edge: &Edge,
    tolerance: ModelTolerance,
) -> KernelResult<SurfaceSurfaceCurve> {
    let source = edge.curve.evaluate(edge.range.start)?;
    let seam = center.curve.project(source.point, tolerance)?.parameter;
    let tangent = center.curve.evaluate(seam)?.first_derivative;
    let direction = if tangent.dot(source.first_derivative) * edge.range.span() >= 0.0 {
        1.0
    } else {
        -1.0
    };
    turn_blend_chart(center, seam, direction, ParameterRange::new(-1.0, 2.0)?, tolerance)
}

/// The center of an open edge whose closed, bounded section chart is cut
/// inside the edge, recharted over one turn that starts in the edge's gap.
/// Otherwise the edge's seeds would select the complementary arc.
fn open_blend_chart(
    center: &SurfaceSurfaceCurve,
    edge: &Edge,
    middle: f64,
    tolerance: ModelTolerance,
) -> KernelResult<Option<SurfaceSurfaceCurve>> {
    let Curve3::SurfaceIntersection(curve) = &center.curve else { return Ok(None) };
    let range = curve.range();
    let first = center.curve.evaluate(range.start)?;
    let last = center.curve.evaluate(range.end)?;
    if first.point.distance(last.point)
        > tolerance.linear_at_scale(first.point.to_vec().magnitude().max(1.0))
    {
        return Ok(None);
    }
    let project = |parameter: f64| -> KernelResult<f64> {
        let point = edge.curve.evaluate(parameter)?.point;
        Ok(center.curve.project(point, tolerance)?.parameter)
    };
    let start = project(edge.range.start)?;
    let end = project(edge.range.end)?;
    if (middle - start) * (middle - end) <= 0.0 {
        return Ok(None);
    }
    // The arc runs through the chart cut, so the gap lies between the endpoints.
    turn_blend_chart(center, (start + end) * 0.5, 1.0, ParameterRange::new(0.0, 1.0)?, tolerance)
        .map(Some)
}

/// Rebuild a closed section's guides with one unit of parameter per turn,
/// starting at `seam` and repeating the turn over `support_range`.
fn turn_blend_chart(
    center: &SurfaceSurfaceCurve,
    seam: f64,
    direction: f64,
    support_range: ParameterRange,
    tolerance: ModelTolerance,
) -> KernelResult<SurfaceSurfaceCurve> {
    let Curve3::SurfaceIntersection(curve) = &center.curve else { unreachable!() };
    let range = curve.range();
    let exact = curve.exact_pcurves();
    let mut points = [Vec::new(), Vec::new()];
    // One sample beyond either end supports the uniform cubic guide.
    let samples = (support_range.start * 32.0).round() as i32 - 1
        ..=(support_range.end * 32.0).round() as i32 + 1;
    for index in samples {
        let fraction = (seam - range.start) / range.span() + direction * index as f64 / 32.0;
        let parameter = range.start + range.span() * fraction.rem_euclid(1.0);
        for side in 0..2 {
            points[side].push(exact[side].evaluate(parameter)?.point);
        }
    }
    let surfaces = [curve.first_surface(), curve.second_surface()];
    for side in 0..2 {
        super::intersection::unwrap_periodic_points(&mut points[side], surfaces[side].domains());
    }
    let rebuilt = SurfaceIntersectionCurve::new_with_width(
        surfaces[0].clone(),
        surfaces[1].clone(),
        super::intersection::periodic_nurbs_guide(&points[0], support_range)?.into(),
        super::intersection::periodic_nurbs_guide(&points[1], support_range)?.into(),
        support_range,
        tolerance,
        center.achieved_error,
        curve.constant_width,
    )?;
    // Surface intersections can be returned in reversed support order. Their
    // 3D curve retains its internal order, while the outer p-curves are swapped.
    // Preserve that correspondence when moving the chart cut.
    let reversed = matches!(&center.first_pcurve, Curve2::SurfaceIntersection(pcurve)
        if pcurve.surface() == curve.second_surface());
    let [first_pcurve, second_pcurve] = if reversed {
        let [first, second] = rebuilt.exact_pcurves();
        [second, first]
    } else { rebuilt.exact_pcurves() };
    Ok(SurfaceSurfaceCurve {
        curve: rebuilt.into(),
        first_pcurve,
        second_pcurve,
        achieved_error: center.achieved_error,
    })
}

fn project_local_blend_endpoint(
    curve: &Curve3,
    point: Point3,
    endpoint: usize,
    tolerance: ModelTolerance,
) -> KernelResult<f64> {
    // Local guides follow the source edge from zero to one. Their continuations
    // can overlap on nearly closed edges, so retain the endpoint's own branch.
    let DomainBounds::Bounded(range) = curve.domain().bounds else {
        return curve.project(point, tolerance).map(|p| p.parameter);
    };
    let range = if endpoint == 0 {
        ParameterRange::new(range.start, 0.5)?
    } else {
        ParameterRange::new(0.5, range.end)?
    };
    let projected = curve.project(point, tolerance)?;
    if projected.parameter >= range.start && projected.parameter <= range.end {
        return Ok(projected.parameter);
    }
    let local = curve.project_on_trim(point, range, tolerance)?;
    let allowed = tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0));
    // A short edge can legitimately move past its midpoint as it is trimmed.
    // Prefer the local chart only when it supplies an equivalent projection.
    Ok(if local.distance <= projected.distance + allowed {
        local.parameter
    } else {
        projected.parameter
    })
}

// Bounded implicit intersections can describe a complete closed section without
// declaring a period. Their two endpoints are equivalent, but a blend must use
// the endpoint in its local chart instead of traversing the rest of the section.
fn blend_parameter_near(
    curve: &Curve3,
    parameter: f64,
    reference: f64,
    tolerance: ModelTolerance,
) -> KernelResult<f64> {
    let domain = curve.domain();
    if domain.period.is_some() {
        return Ok(domain.nearest(parameter, reference));
    }
    if let DomainBounds::Bounded(range) = domain.bounds {
        if (parameter - range.start)
            .abs()
            .min((parameter - range.end).abs())
            <= tolerance.parameter * 64.0
        {
            let first = curve.evaluate(range.start)?;
            let last = curve.evaluate(range.end)?;
            let allowed = tolerance.linear_at_scale(
                first
                    .point
                    .to_vec()
                    .magnitude()
                    .max(last.point.to_vec().magnitude())
                    .max(1.0),
            );
            if first.point.distance(last.point) <= allowed
                && first
                    .first_derivative
                    .normalize()
                    .dot(last.first_derivative.normalize())
                    >= 1.0 - tolerance.angular * 8.0
            {
                return Ok(
                    if (reference - range.start).abs() <= (reference - range.end).abs() {
                        range.start
                    } else {
                        range.end
                    },
                );
            }
        }
    }
    Ok(parameter)
}
