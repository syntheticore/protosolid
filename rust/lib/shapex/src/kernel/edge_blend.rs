//! Local edge treatments: shared discovery, trimming, junction topology and
//! transactional publication for circular blends and ruled chamfers.

use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3};

use super::euler::EulerTransaction;
use super::history::ChangeKind;
use super::topology::{Edge, Vertex};
use super::validation::{validate, ValidationMode};
use super::wire::{curve_loop_contains_point, curve_pair_intersection};
use super::{
    Circle2, Circle3, CoedgeId, Cone3, Curve2, Curve3, Cylinder3, Diagnostic, DiagnosticKind,
    DomainBounds, EdgeId, FaceId, FaceOrientation, Frame3, IntersectionSet, KernelError,
    KernelResult, Line2, ModelTolerance, ParameterRange, Plane3, RevolvedSurface3,
    RollingBallSurface3, Sphere3, Surface3, SurfaceCurve3, SurfaceIntersectionCurve,
    SurfaceProjectionPcurve2, SurfaceSurfaceCurve, Torus3, TrimmedCurve2, VertexId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeBlendKind {
    Fillet,
    Chamfer,
}

impl EdgeBlendKind {
    fn role(self, suffix: &str) -> String {
        let name = match self {
            Self::Fillet => "fillet",
            Self::Chamfer => "chamfer",
        };
        format!("{name}.{suffix}")
    }
}

fn signed_periodic_delta(start: f64, end: f64) -> f64 {
    (end - start + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

#[derive(Debug, Clone)]
struct BlendEndpoint {
    old_vertex: VertexId,
    terminating_face: FaceId,
    // Shared terminating seam and the parent contact that moves its endpoint.
    tangent_termination: Option<(EdgeId, usize)>,
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
    center: SurfaceSurfaceCurve,
    endpoint_seeds: [f64; 2],
    provisional_range: ParameterRange,
    shared_seam: Option<VertexId>,
    local_guide: bool,
}

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

impl EulerTransaction<'_> {
    pub(crate) fn blend_edges(
        &mut self,
        edges: &[EdgeId],
        radius: f64,
        follow_tangent_edges: bool,
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self.blend_network_unchecked(
            edges.iter().copied().collect(),
            radius,
            follow_tangent_edges,
            kind,
        );
        let result = result.and_then(|_| {
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
            self.names = names_snapshot;
            self.touched_bodies = touched_snapshot;
        }
        result
    }

    fn blend_network_unchecked(
        &mut self,
        mut selected: BTreeSet<EdgeId>,
        radius: f64,
        follow_tangent_edges: bool,
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        // General rolling supports must share one chart before their temporary
        // arcs are built, otherwise structurally different guides prevent KEF.
        let mut closed_spines = Vec::new();
        for edge in &selected {
            let value = self.working.edge(*edge)?;
            if value.vertices[0] == value.vertices[1] {
                if let Ok(spine) = self.prepare_blend_spine(*edge, radius, kind, None, None) {
                    if matches!(spine.center.curve, Curve3::SurfaceIntersection(_)) {
                        closed_spines.push(spine);
                    }
                }
            }
        }
        selected = self.subdivide_closed_blend_edges(&selected, kind)?;
        let mut junction_kinds = BTreeMap::<VertexId, BlendJunctionKind>::new();
        let mut junction_sources = BTreeMap::<VertexId, EdgeId>::new();
        let mut spines = BTreeMap::<EdgeId, BlendSpine>::new();
        let mut propagated_offsets = BTreeMap::<EdgeId, f64>::new();
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
                spines.insert(
                    edge,
                    self.prepare_blend_spine(
                        edge,
                        radius,
                        kind,
                        shared,
                        propagated_offsets.get(&edge).copied(),
                    )?,
                );
            }
            let mut additions = BTreeSet::new();
            for spine in spines.values() {
                for endpoint_index in 0..2 {
                    let vertex = spine.edge.vertices[endpoint_index];
                    if junction_kinds.contains_key(&vertex) {
                        continue;
                    }
                    let selected_degree = self
                        .working
                        .vertex_edges(vertex)?
                        .into_iter()
                        .filter(|edge| selected.contains(edge))
                        .count();
                    if selected_degree == 1 && follow_tangent_edges {
                        if let Some((continuation, tangent_faces)) =
                            self.tangent_chain_continuation(spine, endpoint_index)?
                        {
                            junction_kinds
                                .insert(vertex, BlendJunctionKind::TangentChain { tangent_faces });
                            junction_sources.insert(vertex, spine.edge_id);
                            additions.insert(continuation);
                            propagated_offsets
                                .entry(continuation)
                                .or_insert(spine.offset);
                            continue;
                        } else if let Some((seam_edge, continuation, tangent_faces)) =
                            self.smooth_blend_continuation(vertex, spine.edge_id, &selected)?
                        {
                            junction_kinds.insert(
                                vertex,
                                BlendJunctionKind::Smooth {
                                    seam_edge,
                                    tangent_faces,
                                    second_seam: None,
                                },
                            );
                            junction_sources.insert(vertex, spine.edge_id);
                            additions.insert(continuation);
                            propagated_offsets
                                .entry(continuation)
                                .or_insert(spine.offset);
                            continue;
                        }
                    }
                    if selected_degree == 1 {
                        let endpoint = self.prepare_blend_endpoint(spine, endpoint_index).or_else(
                            |error| {
                                if kind == EdgeBlendKind::Chamfer {
                                    self.prepare_capped_endpoint(spine, endpoint_index)
                                        .map_err(|_| error)
                                } else {
                                    Err(error)
                                }
                            },
                        )?;
                        regular_endpoints.insert((spine.edge_id, endpoint_index), Some(endpoint));
                        continue;
                    }

                    if let Some((continuation, tangent_faces)) =
                        self.tangent_chain_continuation(spine, endpoint_index)?
                    {
                        if selected.contains(&continuation) {
                            junction_kinds
                                .insert(vertex, BlendJunctionKind::TangentChain { tangent_faces });
                            junction_sources.insert(vertex, spine.edge_id);
                            continue;
                        }
                    }
                    if let Some((seam_edge, continuation, tangent_faces)) =
                        self.smooth_blend_continuation(vertex, spine.edge_id, &selected)?
                    {
                        if selected.contains(&continuation) {
                            junction_kinds.insert(
                                vertex,
                                BlendJunctionKind::Smooth {
                                    seam_edge,
                                    tangent_faces,
                                    second_seam: None,
                                },
                            );
                            junction_sources.insert(vertex, spine.edge_id);
                            continue;
                        }
                    }
                    if let Some((continuation, carrier_face, seam_edge)) =
                        self.inner_rim_corner(spine, vertex)?
                    {
                        if selected.contains(&continuation) {
                            junction_kinds.insert(
                                vertex,
                                BlendJunctionKind::RimCorner {
                                    carrier_face,
                                    seam_edge,
                                },
                            );
                            junction_sources.insert(vertex, spine.edge_id);
                            continue;
                        }
                    }
                    let incident_degree = self.working.vertex_edges(vertex)?.len();
                    if kind == EdgeBlendKind::Fillet && selected_degree == 2 && incident_degree == 3
                    {
                        let seam_edge = self
                            .working
                            .vertex_edges(vertex)?
                            .into_iter()
                            .find(|edge| !selected.contains(edge))
                            .expect("one unselected seam");
                        let walls = self.working.edge_faces(seam_edge)?;
                        if walls.len() == 2 {
                            junction_kinds.insert(
                                vertex,
                                BlendJunctionKind::Miter {
                                    seam_edge,
                                    wall_faces: [walls[0], walls[1]],
                                },
                            );
                            junction_sources.insert(vertex, spine.edge_id);
                            continue;
                        }
                    }
                    if selected_degree == 3 && incident_degree == 3 {
                        let incident = self.working.vertex_edges(vertex)?;
                        let first_concavity = spines[&incident[0]].concave;
                        if incident
                            .iter()
                            .any(|edge| spines[edge].concave != first_concavity)
                        {
                            let diagnostic = incident
                                .iter()
                                .filter(|edge| spines[edge].concave)
                                .fold(
                                    Diagnostic::new(
                                        DiagnosticKind::UnsupportedGeometry,
                                        "mixed convex-concave three-edge blends require a setback corner patch",
                                    )
                                    .with_operation("blend.mixed-trihedral-setback"),
                                    |diagnostic, edge| {
                                        diagnostic.with_entity(spines[edge].edge.name)
                                    },
                                );
                            return Err(KernelError::from_diagnostic(diagnostic));
                        }
                        junction_kinds.insert(vertex, BlendJunctionKind::Trihedral);
                        junction_sources.insert(vertex, spine.edge_id);
                        continue;
                    }
                    return Err(KernelError::new(
                        DiagnosticKind::UnsupportedGeometry,
                        "selected blend edges do not define a supported local corner transition",
                    ));
                }
            }
            let previous = selected.len();
            selected.extend(additions);
            if selected.len() == previous {
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
                        KernelError::new(
                            DiagnosticKind::TopologyInvariant,
                            "blend junction source edge was not retained",
                        )
                    })?;
                    if let BlendJunctionKind::Smooth {
                        seam_edge,
                        tangent_faces,
                        ..
                    } = junction_kind
                    {
                        let junction = self.prepare_smooth_fillet_junction(
                            *vertex,
                            spine.offset,
                            *seam_edge,
                            *tangent_faces,
                            spine.concave,
                        );
                        match junction {
                            Ok(junction) => {
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
                                );
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
                            KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "blend network junction has no incident rolling patch",
                            )
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
                            let frame = Frame3::new(
                                junction.center,
                                equator,
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
                        return Err(KernelError::new(
                            DiagnosticKind::TopologyInvariant,
                            "a miter needs two rolling patches",
                        ));
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
                self.certify_blend_contact_trims(&patches)?;
                return self.rewrite_blend_network(&patches, &junctions, &junction_surfaces, kind);
            }
            if selected.len() > 256 {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "blend transition propagation exceeded the connected regular edge network",
                ));
            }
        }
    }

    /// Identify the continuation and wall seam of a concave inner-rim corner.
    /// The transition stays on the carrier loop instead of escaping through
    /// the seam and continuing onto the opposite rim.
    fn inner_rim_corner(
        &self,
        spine: &BlendSpine,
        vertex: VertexId,
    ) -> KernelResult<Option<(EdgeId, FaceId, EdgeId)>> {
        let inner_uses = spine
            .uses
            .iter()
            .copied()
            .map(|use_id| {
                let coedge = self.working.coedge(use_id)?;
                Ok(
                    (self.working.loop_(coedge.loop_id)?.kind == super::LoopKind::Inner)
                        .then_some(use_id),
                )
            })
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if inner_uses.len() != 1 {
            return Ok(None);
        }
        let carrier_use = inner_uses[0];
        let carrier_loop = self
            .working
            .loop_(self.working.coedge(carrier_use)?.loop_id)?;
        let continuation_use = self.adjacent_use_at_vertex(carrier_use, vertex)?;
        let continuation = self.working.coedge(continuation_use)?.edge;
        let seam_edge = self
            .working
            .vertex_edges(vertex)?
            .into_iter()
            .find(|edge| *edge != spine.edge_id && *edge != continuation)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "an inner-rim corner has no wall seam edge",
                )
            })?;
        Ok(Some((continuation, carrier_loop.face, seam_edge)))
    }

    /// The blend network represents rolling-patch joins at edge endpoints.
    /// Published periodic edges correctly have one artificial seam vertex, so
    /// subdivide them only inside this transaction. The temporary arcs are all
    /// selected and are consumed by the subsequent blend rewrite.
    fn subdivide_closed_blend_edges(
        &mut self,
        selected: &BTreeSet<EdgeId>,
        kind: EdgeBlendKind,
    ) -> KernelResult<BTreeSet<EdgeId>> {
        let mut result = BTreeSet::new();
        for edge_id in selected {
            let edge = self.working.edge(*edge_id)?.clone();
            if edge.vertices[0] != edge.vertices[1] {
                result.insert(*edge_id);
                continue;
            }
            if edge.coedges.len() != 2 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "a closed blend edge must have exactly two face uses",
                ));
            }

            let source_name = edge.name;
            let seam = edge.vertices[0];
            let mut vertices = vec![seam];
            for part in 1..4 {
                let parameter = edge.range.start + edge.range.span() * part as f64 / 4.0;
                let point = edge.curve.evaluate(parameter)?.point;
                let name = self.names.next_for(&kind.role("periodic-split.vertex"))?;
                let vertex = self.working.vertices.insert(Vertex::new(
                    name,
                    point,
                    self.kernel.tolerance.linear,
                ));
                self.history
                    .derived(name, [source_name], &kind.role("periodic-split.vertex"));
                vertices.push(vertex);
            }
            vertices.push(seam);

            let ranges = (0..4)
                .map(|part| {
                    ParameterRange::new(
                        edge.range.start + edge.range.span() * part as f64 / 4.0,
                        edge.range.start + edge.range.span() * (part + 1) as f64 / 4.0,
                    )
                })
                .collect::<KernelResult<Vec<_>>>()?;
            {
                let survivor = self.working.edges.get_mut(*edge_id)?;
                survivor.vertices = [vertices[0], vertices[1]];
                survivor.range = ranges[0];
                survivor.coedges.clear();
            }
            let mut arcs = vec![*edge_id];
            for part in 1..4 {
                let name = self.names.next_for(&kind.role("periodic-split.edge"))?;
                let arc = self.make_edge(
                    name,
                    [vertices[part], vertices[part + 1]],
                    edge.curve.clone(),
                    ranges[part],
                )?;
                self.history
                    .derived(name, [source_name], &kind.role("periodic-split.edge"));
                arcs.push(arc);
            }

            for old_use_id in edge.coedges {
                let old_use = self.working.coedge(old_use_id)?.clone();
                let traversal_arcs = if old_use.forward {
                    arcs.clone()
                } else {
                    arcs.iter().rev().copied().collect()
                };
                let mut uses = Vec::with_capacity(4);
                for (part, arc) in traversal_arcs.into_iter().enumerate() {
                    let name = self.names.next_for(&kind.role("periodic-split.use"))?;
                    let range = ParameterRange::new(
                        old_use.range.start + old_use.range.span() * part as f64 / 4.0,
                        old_use.range.start + old_use.range.span() * (part + 1) as f64 / 4.0,
                    )?;
                    let use_id = self.make_coedge(
                        name,
                        arc,
                        old_use.loop_id,
                        old_use.forward,
                        old_use.pcurve.clone(),
                        range,
                    )?;
                    self.history
                        .derived(name, [source_name], &kind.role("periodic-split.use"));
                    uses.push(use_id);
                }
                for index in 0..uses.len() {
                    let previous = if index == 0 {
                        if old_use.previous == old_use_id {
                            *uses.last().unwrap()
                        } else {
                            old_use.previous
                        }
                    } else {
                        uses[index - 1]
                    };
                    let next = if index + 1 == uses.len() {
                        if old_use.next == old_use_id {
                            uses[0]
                        } else {
                            old_use.next
                        }
                    } else {
                        uses[index + 1]
                    };
                    let value = self.working.coedges.get_mut(uses[index])?;
                    value.previous = previous;
                    value.next = next;
                }
                if old_use.previous != old_use_id {
                    self.working.coedges.get_mut(old_use.previous)?.next = uses[0];
                }
                if old_use.next != old_use_id {
                    self.working.coedges.get_mut(old_use.next)?.previous = *uses.last().unwrap();
                }
                if self.working.loop_(old_use.loop_id)?.first == Some(old_use_id) {
                    self.working.loops.get_mut(old_use.loop_id)?.first = Some(uses[0]);
                }
                self.working.coedges.remove(old_use_id)?;
                self.history
                    .changed(old_use.name, ChangeKind::Deleted, std::iter::empty());
            }
            for vertex in vertices.into_iter().take(4) {
                self.repair_outgoing(vertex)?;
            }
            self.history
                .changed(source_name, ChangeKind::Modified, [source_name]);
            result.extend(arcs);
        }
        Ok(result)
    }

    fn prepare_blend_spine(
        &self,
        edge_id: EdgeId,
        size: f64,
        kind: EdgeBlendKind,
        shared: Option<&BlendSpine>,
        propagated_offset: Option<f64>,
    ) -> KernelResult<BlendSpine> {
        let tolerance = self.kernel.tolerance;
        let edge = self.working.edge(edge_id)?.clone();
        if edge.coedges.len() != 2 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "blend requires a manifold edge with two face uses",
            ));
        }
        let uses = [edge.coedges[0], edge.coedges[1]];
        let faces = [
            self.working
                .loop_(self.working.coedge(uses[0])?.loop_id)?
                .face,
            self.working
                .loop_(self.working.coedge(uses[1])?.loop_id)?
                .face,
        ];
        if faces[0] == faces[1] {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "blend requires two distinct incident faces",
            ));
        }
        let parent_surfaces = [
            self.working.face(faces[0])?.surface.clone(),
            self.working.face(faces[1])?.surface.clone(),
        ];
        let midpoint = edge
            .curve
            .evaluate((edge.range.start + edge.range.end) * 0.5)?
            .point;
        let concave = self.blend_edge_is_concave(&edge, uses, faces, &parent_surfaces, midpoint)?;
        // Equal setbacks on planar supports correspond to r = d cot(theta/2),
        // where theta is the angle between outward normals. Perpendicular rims
        // retain r = d, including plane/cylinder tangent chains.
        let radius = if let Some(offset) = propagated_offset {
            // A propagated rolling chamfer must retain its source offset across
            // smooth support seams. Recomputing it at each edge midpoint makes
            // the contact rails disagree at their shared endpoint.
            offset
        } else if kind == EdgeBlendKind::Chamfer {
            let normals = (0..2)
                .map(|index| {
                    let uv = parent_surfaces[index]
                        .project(midpoint, tolerance)?
                        .parameters;
                    let sign = if self.working.face(faces[index])?.orientation
                        == FaceOrientation::Forward
                    {
                        1.0
                    } else {
                        -1.0
                    };
                    Ok(parent_surfaces[index].normal(uv.0, uv.1)? * sign)
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let cosine = normals[0].dot(normals[1]).clamp(-1.0, 1.0);
            size * ((1.0 + cosine) / (1.0 - cosine)).sqrt()
        } else {
            size
        };
        let offset_distance = |face: FaceId| -> KernelResult<f64> {
            let outward = match self.working.face(face)?.orientation {
                FaceOrientation::Forward => radius,
                FaceOrientation::Reversed => -radius,
            };
            Ok(if concave { outward } else { -outward })
        };
        let offsets = [
            parent_surfaces[0].offset(offset_distance(faces[0])?, tolerance)?,
            parent_surfaces[1].offset(offset_distance(faces[1])?, tolerance)?,
        ];
        let mut local_guide = false;
        let candidates = if let Some(shared) = shared {
            vec![shared.center.clone()]
        } else {
            let intersection = if kind == EdgeBlendKind::Fillet {
                match (&offsets[0], &offsets[1]) {
                    (Surface3::Plane(plane), Surface3::Torus(torus)) => {
                        super::intersection::parallel_plane_torus_section(*plane, *torus, tolerance)
                    }
                    (Surface3::Torus(torus), Surface3::Plane(plane)) => {
                        super::intersection::parallel_plane_torus_section(*plane, *torus, tolerance)
                            .map(super::intersection::swap_surface_curves)
                    }
                    _ => offsets[0].intersect_surface(&offsets[1], tolerance),
                }
            } else {
                offsets[0].intersect_surface(&offsets[1], tolerance)
            };
            match intersection {
                Ok(IntersectionSet::Isolated(curves)) => curves,
                Ok(IntersectionSet::Empty) => {
                    return Err(KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "incident face offsets do not intersect at the requested radius",
                    ))
                }
                Ok(IntersectionSet::Coincident) => {
                    return Err(KernelError::new(
                        DiagnosticKind::UnsupportedGeometry,
                        "incident face offsets are coincident and do not define one blend spine",
                    ))
                }
                Err(_) => {
                    local_guide = true;
                    vec![self.local_offset_intersection(
                        uses,
                        &offsets,
                        tolerance,
                        kind == EdgeBlendKind::Fillet,
                    )?]
                }
            }
        };
        let center = candidates
            .into_iter()
            .filter_map(|candidate| {
                candidate
                    .curve
                    .project(midpoint, tolerance)
                    .ok()
                    .map(|projection| (candidate, projection.distance))
            })
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|candidate| candidate.0)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "blend could not select the local offset intersection branch",
                )
            })?;

        let middle_parameter = center.curve.project(midpoint, tolerance)?.parameter;
        let endpoint_seeds = edge
            .vertices
            .iter()
            .enumerate()
            .map(|(index, vertex)| {
                let point = self.working.vertex(*vertex)?.point;
                let projection = if local_guide {
                    project_local_blend_endpoint(&center.curve, point, index, tolerance)
                } else {
                    center.curve.project(point, tolerance).map(|p| p.parameter)
                };
                projection.and_then(|projection| {
                    blend_parameter_near(&center.curve, projection, middle_parameter, tolerance)
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let endpoint_seeds = [endpoint_seeds[0], endpoint_seeds[1]];
        let endpoint_seeds = if let Some(period) = center.curve.domain().period {
            let middle_parameter = center.curve.project(midpoint, tolerance)?.parameter;
            let unwrap_near = |value: f64, reference: f64| {
                value + ((reference - value) / period).round() * period
            };
            let middle = unwrap_near(middle_parameter, endpoint_seeds[0]);
            [
                endpoint_seeds[0],
                unwrap_near(endpoint_seeds[1], middle * 2.0 - endpoint_seeds[0]),
            ]
        } else {
            endpoint_seeds
        };
        // The local guide uses a normalized source traversal. These are branch
        // hints; the final junction projection is corrected on the exact curve.
        let endpoint_seeds = if let Some(shared) = shared {
            std::array::from_fn(|i| {
                let source_parameter = if i == 0 {
                    edge.range.start
                } else {
                    edge.range.end
                };
                let reference =
                    (source_parameter - shared.edge.range.start) / shared.edge.range.span();
                endpoint_seeds[i] + (reference - endpoint_seeds[i]).round()
            })
        } else {
            endpoint_seeds
        };
        let seed_span = (endpoint_seeds[1] - endpoint_seeds[0]).abs().max(1.0);
        let center_domain = center.curve.domain();
        let provisional_range = match center_domain.bounds {
            _ if center_domain.period.is_some() => ParameterRange::new(
                endpoint_seeds[0].min(endpoint_seeds[1]) - seed_span,
                endpoint_seeds[0].max(endpoint_seeds[1]) + seed_span,
            )?,
            DomainBounds::Bounded(range) => range,
            _ => ParameterRange::new(
                endpoint_seeds[0].min(endpoint_seeds[1]) - seed_span,
                endpoint_seeds[0].max(endpoint_seeds[1]) + seed_span,
            )?,
        };
        Ok(BlendSpine {
            edge_id,
            edge,
            uses,
            faces,
            parent_surfaces,
            concave,
            offset: radius,
            center,
            endpoint_seeds,
            provisional_range,
            shared_seam: shared.map(|spine| spine.edge.vertices[0]),
            local_guide,
        })
    }

    fn blend_edge_is_concave(
        &self,
        edge: &Edge,
        uses: [CoedgeId; 2],
        faces: [FaceId; 2],
        parent_surfaces: &[Surface3; 2],
        midpoint: Point3,
    ) -> KernelResult<bool> {
        let parameter = (edge.range.start + edge.range.end) * 0.5;
        let use_value = self.working.coedge(uses[0])?;
        let traversal = if edge.range.span() == 0.0 {
            if use_value.forward {
                1.0
            } else {
                -1.0
            }
        } else {
            use_value.range.span().signum()
        };
        let tangent = edge.curve.evaluate(parameter)?.first_derivative * traversal;
        let mut normals = [Vec3::new(0.0, 0.0, 0.0); 2];
        for index in 0..2 {
            let uv = parent_surfaces[index]
                .project(midpoint, self.kernel.tolerance)?
                .parameters;
            normals[index] = parent_surfaces[index].normal(uv.0, uv.1)?;
            if self.working.face(faces[index])?.orientation == FaceOrientation::Reversed {
                normals[index] = -normals[index];
            }
            normals[index] = normals[index].normalize();
        }
        let signed_turn = tangent.normalize().dot(normals[0].cross(normals[1]));
        if signed_turn.abs() <= self.kernel.tolerance.angular {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "blend edge has no well-defined convex or concave dihedral",
            ));
        }
        Ok(signed_turn < 0.0)
    }

    // A selected circular segment can stop before its tangent continuation.
    // Its inset rail misses the unselected tangent edge; retain that endpoint
    // and close the bevel with a transverse triangular face instead.
    fn prepare_capped_endpoint(
        &self,
        spine: &BlendSpine,
        index: usize,
    ) -> KernelResult<BlendEndpoint> {
        let tolerance = self.kernel.tolerance;
        let vertex = spine.edge.vertices[index];
        let old_point = self.working.vertex(vertex)?.point;
        let plane = Plane3::from_origin_normal(old_point, spine.edge.endpoint_tangent(index)?)?;
        let terminating_face = self
            .working
            .vertex_faces(vertex)?
            .into_iter()
            .find(|face| !spine.faces.contains(face))
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "partial blend has no terminating face",
                )
            })?;
        let parameter = spine.endpoint_seeds[index];
        let mut points = [old_point; 2];
        let mut uses = spine.uses;
        let mut free = Vec::new();
        for parent in 0..2 {
            uses[parent] = self.adjacent_use_at_vertex(spine.uses[parent], vertex)?;
            let uv = if parent == 0 {
                &spine.center.first_pcurve
            } else {
                &spine.center.second_pcurve
            }
            .evaluate(parameter)?
            .point;
            points[parent] = spine.parent_surfaces[parent].evaluate(uv.x, uv.y)?.point;
            let edge = self.working.edge(self.working.coedge(uses[parent])?.edge)?;
            let projection = edge.curve.project(points[parent], tolerance)?;
            if projection.distance > tolerance.linear_at_scale(old_point.to_vec().magnitude()) * 8.0
            {
                free.push(parent);
            }
        }
        if free.len() != 1
            || points.iter().any(|point| {
                plane.project(*point).map_or(true, |projection| {
                    projection.distance > tolerance.linear * 32.0
                })
            })
        {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "partial blend cannot close on a transverse end plane",
            ));
        }
        Ok(BlendEndpoint {
            old_vertex: vertex,
            terminating_face,
            tangent_termination: None,
            adjacent_uses: uses,
            contact_parameters: [parameter; 2],
            contact_points: points,
            allow_adjacent_extension: [false; 2],
            cap: Some((free[0], plane)),
        })
    }

    fn prepare_blend_endpoint(
        &mut self,
        spine: &BlendSpine,
        endpoint_index: usize,
    ) -> KernelResult<BlendEndpoint> {
        let tolerance = self.kernel.tolerance;
        let old_vertex = spine.edge.vertices[endpoint_index];
        let terminating_faces = self
            .working
            .vertex_faces(old_vertex)?
            .into_iter()
            .filter(|face| !spine.faces.contains(face))
            .collect::<Vec<_>>();
        if terminating_faces.is_empty() || terminating_faces.len() > 2 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "a regular blend endpoint requires one face or two tangent terminating faces",
            ));
        }
        let mut terminating_face = terminating_faces[0];
        let mut adjacent_uses = [spine.uses[0]; 2];
        let mut contact_parameters = [0.0; 2];
        let mut contact_points = [Point3::new(0.0, 0.0, 0.0); 2];
        let mut allow_adjacent_extension = [false; 2];
        for parent_index in 0..2 {
            adjacent_uses[parent_index] =
                self.adjacent_use_at_vertex(spine.uses[parent_index], old_vertex)?;
            let contact_curve: Curve3 = SurfaceCurve3::new(
                spine.parent_surfaces[parent_index].clone(),
                if parent_index == 0 {
                    spine.center.first_pcurve.clone()
                } else {
                    spine.center.second_pcurve.clone()
                },
                spine.provisional_range,
                tolerance,
            )?
            .into();
            let adjacent_edge_id = self.working.coedge(adjacent_uses[parent_index])?.edge;
            let adjacent_edge = self.working.edge(adjacent_edge_id)?;
            // A concave termination can grow a Boolean intersection edge past
            // its traced domain. Continue both exact support p-curves before
            // solving the contact; changing only the topological trim leaves
            // the curve/curve solver clamped to the old endpoint.
            if let Curve3::SurfaceIntersection(curve) = &adjacent_edge.curve {
                let endpoint = if adjacent_edge.vertices[0] == old_vertex {
                    adjacent_edge.range.start
                } else {
                    adjacent_edge.range.end
                };
                let value = adjacent_edge.curve.evaluate(endpoint)?;
                let target = contact_curve.evaluate(spine.endpoint_seeds[endpoint_index])?.point;
                let delta = (target - value.point).dot(value.first_derivative)
                    / value.first_derivative.magnitude2();
                let range = curve.range();
                let outside_start =
                    (endpoint - range.start).abs() <= tolerance.parameter && delta < 0.0;
                let outside_end = (endpoint - range.end).abs() <= tolerance.parameter && delta > 0.0;
                if outside_start || outside_end {
                    let padding =
                        4.0 * target.distance(value.point) / value.first_derivative.magnitude();
                    let extended = curve.extended(ParameterRange::new(
                        range.start - if outside_start { padding } else { 0.0 },
                        range.end + if outside_end { padding } else { 0.0 },
                    )?)?;
                    let uses = adjacent_edge
                        .coedges
                        .iter()
                        .map(|id| {
                            let coedge = self.working.coedge(*id)?;
                            let map = |parameter| {
                                let fraction = (parameter - adjacent_edge.range.start)
                                    / adjacent_edge.range.span();
                                let fraction = if coedge.forward {
                                    fraction
                                } else {
                                    1.0 - fraction
                                };
                                coedge.range.start + coedge.range.span() * fraction
                            };
                            let start = map(extended.range().start);
                            let end = map(extended.range().end);
                            let range = ParameterRange::new(start.min(end), start.max(end))?;
                            Ok((*id, coedge.pcurve.extended(range)?))
                        })
                        .collect::<KernelResult<Vec<_>>>()?;
                    for (id, pcurve) in uses {
                        self.working.coedges.get_mut(id)?.pcurve = pcurve;
                    }
                    self.working.edges.get_mut(adjacent_edge_id)?.curve = extended.into();
                }
            }
            let adjacent_edge = self.working.edge(adjacent_edge_id)?;
            let adjacent_seed = if adjacent_edge.vertices[0] == old_vertex {
                adjacent_edge.range.start
            } else {
                adjacent_edge.range.end
            };
            let (seed, rail_seed) = (0..=32)
                .map(|index| {
                    let parameter = spine.provisional_range.start
                        + spine.provisional_range.span() * index as f64 / 32.0;
                    let point = contact_curve.evaluate(parameter)?.point;
                    let projection = adjacent_edge.curve.project(point, tolerance)?;
                    Ok((parameter, projection))
                })
                .collect::<KernelResult<Vec<_>>>()?
                .into_iter()
                .min_by(|first, second| {
                    let first_score = first.1.distance
                        + (first.0 - spine.endpoint_seeds[endpoint_index]).abs() * tolerance.linear;
                    let second_score = second.1.distance
                        + (second.0 - spine.endpoint_seeds[endpoint_index]).abs()
                            * tolerance.linear;
                    first_score.total_cmp(&second_score)
                })
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::NumericalFailure,
                        "blend endpoint contact search found no local seed",
                    )
                })?;
            let seed_point = contact_curve.evaluate(seed)?.point;
            let seed_allowed =
                tolerance.linear_at_scale(seed_point.to_vec().magnitude().max(1.0)) * 8.0;
            let local_hit = contact_curve.intersect_curve_near(
                &adjacent_edge.curve,
                spine.endpoint_seeds[endpoint_index],
                adjacent_seed,
                tolerance,
            );
            let (contact_parameter, rail_parameter, contact_point) = if let Ok(hit) = local_hit {
                (hit.first_parameter, hit.second_parameter, hit.point)
            } else if rail_seed.distance <= seed_allowed {
                (
                    seed,
                    rail_seed.parameter,
                    Point3::from_vec((seed_point.to_vec() + rail_seed.point.to_vec()) * 0.5),
                )
            } else {
                let hit = contact_curve
                    .intersect_curve_near(
                        &adjacent_edge.curve,
                        seed,
                        if rail_seed.distance.is_finite() {
                            rail_seed.parameter
                        } else {
                            adjacent_seed
                        },
                        tolerance,
                    )
                    .map_err(|error| {
                        KernelError::new(
                            DiagnosticKind::NumericalFailure,
                            format!(
                                "blend endpoint {endpoint_index} contact {parent_index} cannot terminate on its adjacent exact edge (nearest distance {}): {error}",
                                rail_seed.distance,
                            ),
                        )
                    })?;
                (hit.first_parameter, hit.second_parameter, hit.point)
            };
            let needs_extension = !adjacent_edge.range.contains(rail_parameter, tolerance);
            allow_adjacent_extension[parent_index] = needs_extension;
            // The intersection solver may return an equivalent point one turn
            // away. Keep both rails on the same local branch as their spine.
            contact_parameters[parent_index] = blend_parameter_near(
                &spine.center.curve,
                contact_parameter,
                spine.endpoint_seeds[endpoint_index],
                tolerance,
            )?;
            contact_points[parent_index] = contact_point;
        }
        // At a partial chamfer end, the two terminating supports meet
        // tangentially. The new boundary can lie entirely on one support,
        // with its other contact extending their shared seam.
        let mut tangent_termination = None;
        if terminating_faces.len() == 2 {
            let seam = self
                .working
                .vertex_edges(old_vertex)?
                .into_iter()
                .find(|edge| {
                    self.working.edge_faces(*edge).is_ok_and(|faces| {
                        faces.len() == 2
                            && faces.iter().all(|face| terminating_faces.contains(face))
                    })
                })
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::UnsupportedGeometry,
                        "blend terminating supports have no shared seam",
                    )
                })?;
            let point = self.working.vertex(old_vertex)?.point;
            let mut normals = Vec::new();
            for face in &terminating_faces {
                let face = self.working.face(*face)?;
                let uv = face.surface.project(point, tolerance)?.parameters;
                let normal = face.surface.normal(uv.0, uv.1)?;
                normals.push(if face.orientation == FaceOrientation::Reversed {
                    -normal
                } else {
                    normal
                });
            }
            if normals[0].normalize().dot(normals[1].normalize()) < 1.0 - tolerance.angular * 8.0 {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "blend endpoint requires tangent terminating supports",
                ));
            }
            let allowed = tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0)) * 8.0;
            for face in &terminating_faces {
                let surface = &self.working.face(*face)?.surface;
                if contact_points.iter().any(|point| {
                    surface
                        .project(*point, tolerance)
                        .map_or(true, |projection| projection.distance > allowed)
                }) {
                    continue;
                }
                for parent in 0..2 {
                    let adjacent = self.working.coedge(adjacent_uses[parent])?.edge;
                    if self.working.edge_faces(adjacent)?.contains(face) {
                        continue;
                    }
                    if self
                        .working
                        .edge(seam)?
                        .curve
                        .project(contact_points[parent], tolerance)?
                        .distance
                        > allowed
                    {
                        continue;
                    }
                    terminating_face = *face;
                    tangent_termination = Some((seam, parent));
                }
            }
            if tangent_termination.is_none() {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "blend endpoint crosses both terminating supports",
                ));
            }
        }
        if spine.concave && allow_adjacent_extension.iter().all(|extension| *extension) {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "blend radius reaches beyond an adjacent boundary edge",
            ));
        }
        Ok(BlendEndpoint {
            old_vertex,
            terminating_face,
            tangent_termination,
            adjacent_uses,
            contact_parameters,
            contact_points,
            allow_adjacent_extension,
            cap: None,
        })
    }

    fn prepare_miter_junction(
        &self,
        vertex: VertexId,
        kind: BlendJunctionKind,
        spines: &BTreeMap<EdgeId, BlendSpine>,
    ) -> KernelResult<BlendJunction> {
        let BlendJunctionKind::Miter { seam_edge, .. } = kind else {
            unreachable!()
        };
        let tolerance = self.kernel.tolerance;
        let seam = self.working.edge(seam_edge)?;
        let seam_seed = if seam.vertices[0] == vertex {
            seam.range.start
        } else {
            seam.range.end
        };
        let faces = self.working.vertex_faces(vertex)?;
        let mut contact_points = BTreeMap::new();
        for face in &faces {
            let mut rails = spines
                .values()
                .filter(|spine| spine.edge.vertices.contains(&vertex) && spine.faces.contains(face))
                .map(|spine| {
                    let parent = usize::from(spine.faces[0] != *face);
                    let endpoint = usize::from(spine.edge.vertices[0] != vertex);
                    let pcurve = if parent == 0 {
                        &spine.center.first_pcurve
                    } else {
                        &spine.center.second_pcurve
                    };
                    Ok((
                        Curve3::from(SurfaceCurve3::new(
                            spine.parent_surfaces[parent].clone(),
                            pcurve.clone(),
                            spine.provisional_range,
                            tolerance,
                        )?),
                        spine.endpoint_seeds[endpoint],
                    ))
                })
                .collect::<KernelResult<Vec<_>>>()?;
            if rails.len() == 1 {
                rails.push((seam.curve.clone(), seam_seed));
            }
            if rails.len() != 2 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "miter contact needs two incident curves",
                ));
            }
            let hit =
                rails[0]
                    .0
                    .intersect_curve_near(&rails[1].0, rails[0].1, rails[1].1, tolerance)?;
            contact_points.insert(*face, hit.point);
        }
        Ok(BlendJunction {
            faces,
            center: self.working.vertex(vertex)?.point,
            contact_points,
            kind,
        })
    }

    fn prepare_chamfer_junction(
        &self,
        vertex: VertexId,
        spines: &BTreeMap<EdgeId, BlendSpine>,
    ) -> KernelResult<BlendJunction> {
        let tolerance = self.kernel.tolerance;
        let faces = self.working.vertex_faces(vertex)?;
        let mut contact_points = BTreeMap::new();
        for face in &faces {
            let rails = spines
                .values()
                .filter(|spine| spine.edge.vertices.contains(&vertex) && spine.faces.contains(face))
                .map(|spine| {
                    let parent = usize::from(spine.faces[0] != *face);
                    let endpoint = usize::from(spine.edge.vertices[0] != vertex);
                    let pcurve = if parent == 0 {
                        &spine.center.first_pcurve
                    } else {
                        &spine.center.second_pcurve
                    };
                    Ok((
                        Curve3::from(SurfaceCurve3::new(
                            spine.parent_surfaces[parent].clone(),
                            pcurve.clone(),
                            spine.provisional_range,
                            tolerance,
                        )?),
                        spine.endpoint_seeds[endpoint],
                    ))
                })
                .collect::<KernelResult<Vec<_>>>()?;
            if rails.len() != 2 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "a chamfer corner needs two contact rails per parent face",
                ));
            }
            let hit =
                rails[0]
                    .0
                    .intersect_curve_near(&rails[1].0, rails[0].1, rails[1].1, tolerance)?;
            contact_points.insert(*face, hit.point);
        }
        let center = Point3::from_vec(
            contact_points
                .values()
                .map(|point| point.to_vec())
                .fold(Vec3::new(0.0, 0.0, 0.0), |sum, point| sum + point)
                / contact_points.len() as f64,
        );
        Ok(BlendJunction {
            faces,
            center,
            contact_points,
            kind: BlendJunctionKind::Trihedral,
        })
    }

    fn prepare_fillet_junction(
        &self,
        spine: &BlendSpine,
        endpoint_index: usize,
        radius: f64,
        kind: BlendJunctionKind,
    ) -> KernelResult<BlendJunction> {
        let tolerance = self.kernel.tolerance;
        let old_vertex = spine.edge.vertices[endpoint_index];
        let vertex_faces = self.working.vertex_faces(old_vertex)?;
        if vertex_faces.len() != 3 || self.working.vertex_edges(old_vertex)?.len() != 3 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "a rolling-ball vertex transition requires a regular valence-three BREP vertex",
            ));
        }
        let third_face = vertex_faces
            .iter()
            .copied()
            .find(|face| !spine.faces.contains(face))
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "blend junction has no third incident face",
                )
            })?;
        let third_surface = self.working.face(third_face)?.surface.clone();
        let third_outward = match self.working.face(third_face)?.orientation {
            FaceOrientation::Forward => radius,
            FaceOrientation::Reversed => -radius,
        };
        let third_distance = if spine.concave {
            third_outward
        } else {
            -third_outward
        };
        let third_offset = third_surface.offset(third_distance, tolerance)?;
        let old_point = self.working.vertex(old_vertex)?.point;
        let third_seed = match &third_offset {
            Surface3::Offset(offset) => offset.base().project(old_point, tolerance)?.parameters,
            _ => third_offset.project(old_point, tolerance)?.parameters,
        };
        let hit = spine.center.curve.intersect_surface_near(
            &third_offset,
            spine.endpoint_seeds[endpoint_index],
            third_seed,
            tolerance,
        )?;
        let first_uv = spine
            .center
            .first_pcurve
            .evaluate(hit.curve_parameter)?
            .point;
        let second_uv = spine
            .center
            .second_pcurve
            .evaluate(hit.curve_parameter)?
            .point;
        let first = spine.parent_surfaces[0]
            .evaluate(first_uv.x, first_uv.y)?
            .point;
        let second = spine.parent_surfaces[1]
            .evaluate(second_uv.x, second_uv.y)?
            .point;
        let third = third_surface
            .evaluate(hit.surface_parameters.0, hit.surface_parameters.1)?
            .point;
        let allowed =
            tolerance.linear_at_scale(hit.point.to_vec().magnitude().max(radius).max(1.0)) * 16.0;
        for point in [first, second, third] {
            if (point.distance(hit.point) - radius).abs() > allowed {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "trihedral blend contact is not at the requested rolling-ball radius",
                ));
            }
        }
        Ok(BlendJunction {
            faces: vec![spine.faces[0], spine.faces[1], third_face],
            center: hit.point,
            contact_points: BTreeMap::from([
                (spine.faces[0], first),
                (spine.faces[1], second),
                (third_face, third),
            ]),
            kind,
        })
    }

    fn prepare_rim_corner_junction(
        &self,
        vertex: VertexId,
        kind: BlendJunctionKind,
    ) -> KernelResult<BlendJunction> {
        let BlendJunctionKind::RimCorner {
            carrier_face,
            seam_edge,
        } = kind
        else {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "rim-corner preparation requires a rim-corner junction",
            ));
        };
        let faces = self.working.vertex_faces(vertex)?;
        if faces.len() != 3 || self.working.vertex_edges(vertex)?.len() != 3 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "an inner-rim transition requires a regular valence-three BREP vertex",
            ));
        }
        self.working.face(carrier_face)?;
        self.working.edge(seam_edge)?;
        Ok(BlendJunction {
            faces,
            center: self.working.vertex(vertex)?.point,
            contact_points: BTreeMap::new(),
            kind: BlendJunctionKind::RimCorner {
                carrier_face,
                seam_edge,
            },
        })
    }

    fn rim_corner_surface(
        &self,
        vertex: VertexId,
        carrier_face: FaceId,
        seam_edge: EdgeId,
        radius: f64,
        kind: EdgeBlendKind,
    ) -> KernelResult<Surface3> {
        let tolerance = self.kernel.tolerance;
        let point = self.working.vertex(vertex)?.point;
        let face = self.working.face(carrier_face)?;
        if !matches!(face.surface, Surface3::Plane(_)) {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "inner-rim corner sweeps currently require a planar carrier face",
            ));
        }
        let projection = face.surface.project(point, tolerance)?;
        let mut outward = face
            .surface
            .normal(projection.parameters.0, projection.parameters.1)?
            .normalize();
        if face.orientation == FaceOrientation::Reversed {
            outward = -outward;
        }
        let seam = self.working.edge(seam_edge)?;
        let seam_parameter = if seam.vertices[0] == vertex {
            seam.range.start
        } else if seam.vertices[1] == vertex {
            seam.range.end
        } else {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "inner-rim seam is not incident to its corner vertex",
            ));
        };
        let seam_tangent = seam.curve.evaluate(seam_parameter)?.first_derivative;
        if seam_tangent.normalize().cross(outward).magnitude() > tolerance.angular * 32.0 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "inner-rim seam must be normal to its planar carrier face",
            ));
        }
        let singular = point - outward * radius;
        let frame = Frame3::from_z(singular, outward)?;
        let profile: Curve2 = if kind == EdgeBlendKind::Chamfer {
            // Keep the shared rim parameter interval and its singular endpoint.
            let span = std::f64::consts::FRAC_PI_2;
            Line2::new(
                Point2::new(2.0 * radius, 2.0 * radius),
                Vec2::new(-radius / span, -radius / span),
            )?
            .into()
        } else {
            Circle2::new(Point2::new(radius, 0.0), radius)?.into()
        };
        RevolvedSurface3::new(
            frame,
            profile,
            ParameterRange::new(std::f64::consts::FRAC_PI_2, std::f64::consts::PI)?,
            tolerance,
        )
        .map(Surface3::from)
    }

    // A four-face tangent vertex can separate two transitions that the
    // rolling ball reaches at different positions. Split its zero-length
    // cross-support edge internally; the finished blend replaces it completely.
    fn split_smooth_blend_transition(
        &mut self,
        vertex: VertexId,
        tangent: [FaceId; 2],
        selected: &BTreeSet<EdgeId>,
        radius: f64,
        follow: bool,
        original_error: KernelError,
    ) -> KernelResult<()> {
        let other = self
            .working
            .vertex_faces(vertex)?
            .into_iter()
            .filter(|face| !tangent.contains(face))
            .collect::<Vec<_>>();
        let incident = self.working.vertex_edges(vertex)?;
        if other.len() != 2
            || incident.len() != 4
            || incident
                .iter()
                .filter(|edge| selected.contains(edge))
                .count()
                != 2
        {
            return Err(original_error);
        }
        let point = self.working.vertex(vertex)?.point;
        let normals = other
            .iter()
            .map(|face| {
                let face = self.working.face(*face)?;
                let uv = face
                    .surface
                    .project(point, self.kernel.tolerance)?
                    .parameters;
                let normal = face.surface.normal(uv.0, uv.1)?.normalize();
                Ok(if face.orientation == FaceOrientation::Reversed {
                    -normal
                } else {
                    normal
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        if normals[0].dot(normals[1])
            < 1.0 - self.kernel.tolerance.angular.max(f64::EPSILON.sqrt()) * 8.0
        {
            return Err(original_error);
        }
        let snapshot = (
            self.working.clone(),
            self.history.clone(),
            self.names.clone(),
            self.touched_bodies.clone(),
            self.composes_imported_topology,
        );
        for cross in tangent
            .into_iter()
            .flat_map(|face| other.iter().map(move |other| [face, *other]))
        {
            if incident.iter().any(|edge| {
                self.working
                    .edge_faces(*edge)
                    .is_ok_and(|faces| cross.iter().all(|face| faces.contains(face)))
            }) {
                continue;
            }
            let attempt = (|| {
                let bridge = self.insert_blend_transition_bridge(vertex, cross, selected)?;
                let mut selected = selected.clone();
                selected.insert(bridge);
                self.blend_network_unchecked(selected, radius, follow, EdgeBlendKind::Fillet)
            })();
            if attempt.is_ok() {
                return attempt;
            }
            self.working = snapshot.0.clone();
            self.history = snapshot.1.clone();
            self.names = snapshot.2.clone();
            self.touched_bodies = snapshot.3.clone();
            self.composes_imported_topology = snapshot.4;
        }
        Err(original_error)
    }

    fn insert_blend_transition_bridge(
        &mut self,
        vertex: VertexId,
        cross: [FaceId; 2],
        selected: &BTreeSet<EdgeId>,
    ) -> KernelResult<EdgeId> {
        let tolerance = self.kernel.tolerance;
        let incident = self.working.vertex_edges(vertex)?;
        // Seed the new support intersection from the existing sharp boundary
        // on the second support. Its projected guide is corrected exactly.
        let source_id = incident
            .iter()
            .copied()
            .find(|edge| {
                selected.contains(edge)
                    && self
                        .working
                        .edge_faces(*edge)
                        .is_ok_and(|faces| faces.contains(&cross[1]))
            })
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "transition bridge has no source boundary",
                )
            })?;
        let source = self.working.edge(source_id)?.clone();
        let source_use = source
            .coedges
            .iter()
            .copied()
            .find(|id| {
                self.working
                    .coedge(*id)
                    .and_then(|coedge| self.working.loop_(coedge.loop_id))
                    .is_ok_and(|loop_| loop_.face == cross[1])
            })
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "transition source has no support coedge",
                )
            })?;
        let first = self.working.face(cross[0])?.surface.clone();
        let second = self.working.face(cross[1])?.surface.clone();
        let mut guide = Vec::new();
        let mut parameters = Vec::new();
        for index in 0..=16 {
            let t = source.range.start + source.range.span() * index as f64 / 16.0;
            let uv = first
                .project(source.curve.evaluate(t)?.point, tolerance)?
                .parameters;
            guide.push(Point2::new(uv.0, uv.1));
            parameters.push(t);
        }
        let curve = SurfaceIntersectionCurve::new(
            first,
            second,
            super::intersection::interpolating_nurbs_guide(&guide, &parameters)?.into(),
            self.working.coedge(source_use)?.pcurve.clone(),
            source.range,
            tolerance,
            tolerance.linear,
        )?;
        let pcurves = curve.exact_pcurves();
        let parameter = if source.vertices[0] == vertex {
            source.range.start
        } else {
            source.range.end
        };
        let point = self.working.vertex(vertex)?.point;
        let name = self.names.next_for("fillet.transition.vertex")?;
        let new_vertex = self.make_vertex(name, point);
        self.history.derived(
            name,
            [self.working.vertex(vertex)?.name],
            "fillet.transition.vertex",
        );
        // Move the source and the adjacent seam on the first support to the
        // second vertex, leaving a gap in each cross-support loop.
        let source_faces = self.working.edge_faces(source_id)?;
        let moved_face = *source_faces
            .iter()
            .find(|face| **face != cross[1])
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "transition source has no opposite support",
                )
            })?;
        for edge in &incident {
            if self.working.edge_faces(*edge)?.contains(&moved_face) {
                for endpoint in &mut self.working.edges.get_mut(*edge)?.vertices {
                    if *endpoint == vertex {
                        *endpoint = new_vertex;
                    }
                }
            }
        }
        let range = ParameterRange {
            start: parameter,
            end: parameter,
        };
        let name = self.names.next_for("fillet.transition.edge")?;
        let edge = self.make_edge(name, [vertex, new_vertex], curve.into(), range)?;
        self.history.derived(
            name,
            [self.working.vertex(vertex)?.name],
            "fillet.transition.edge",
        );
        for side in 0..2 {
            let mut gap = None;
            for loop_id in self.working.face(cross[side])?.loops.clone() {
                for previous in self.collect_loop(loop_id)? {
                    let next = self.working.coedge(previous)?.next;
                    let a = self.working.coedge_vertices(previous)?[1];
                    let b = self.working.coedge_vertices(next)?[0];
                    if (a == vertex && b == new_vertex) || (a == new_vertex && b == vertex) {
                        gap = Some((previous, next, loop_id, a == vertex));
                    }
                }
            }
            let (previous, next, loop_id, forward) = gap.ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "transition bridge has no loop gap",
                )
            })?;
            let name = self.names.next_for("fillet.transition.use")?;
            let coedge =
                self.make_coedge(name, edge, loop_id, forward, pcurves[side].clone(), range)?;
            self.history.derived(
                name,
                [self.working.edge(edge)?.name],
                "fillet.transition.use",
            );
            self.insert_loop_coedge(previous, next, coedge)?;
        }
        self.repair_outgoing(vertex)?;
        self.repair_outgoing(new_vertex)?;
        Ok(edge)
    }

    fn prepare_smooth_fillet_junction(
        &self,
        vertex: VertexId,
        radius: f64,
        seam_edge: EdgeId,
        tangent_faces: [FaceId; 2],
        concave: bool,
    ) -> KernelResult<BlendJunction> {
        let tolerance = self.kernel.tolerance;
        let vertex_faces = self.working.vertex_faces(vertex)?;
        let other_faces = vertex_faces
            .iter()
            .copied()
            .filter(|face| !tangent_faces.contains(face))
            .collect::<Vec<_>>();
        let incident = self.working.vertex_edges(vertex)?;
        // Both incident supports can change across a tangent transition.
        // Retain a shared contact vertex for each pair of tangent faces.
        let second_seam =
            if vertex_faces.len() == 4 && incident.len() == 4 && other_faces.len() == 2 {
                let pair = [other_faces[0], other_faces[1]];
                let edge = incident
                    .iter()
                    .copied()
                    .find(|edge| {
                        self.working.edge_faces(*edge).is_ok_and(|faces| {
                            faces.len() == 2 && faces.iter().all(|face| pair.contains(face))
                        })
                    })
                    .ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::UnsupportedGeometry,
                            "smooth blend transition has no opposite support seam",
                        )
                    })?;
                Some((edge, pair))
            } else if vertex_faces.len() == 3 && incident.len() == 3 && other_faces.len() == 1 {
                None
            } else {
                return Err(KernelError::new(DiagnosticKind::UnsupportedGeometry,
                "a smooth blend transition requires three faces or two pairs of tangent supports"));
            };
        let common_face = other_faces[0];
        let seam = self.working.edge(seam_edge)?.clone();
        let seam_parameter = if seam.vertices[0] == vertex {
            seam.range.start
        } else if seam.vertices[1] == vertex {
            seam.range.end
        } else {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "smooth blend seam is not incident to its junction vertex",
            ));
        };
        let tangent_face = tangent_faces[0];
        let tangent_surface = self.working.face(tangent_face)?.surface.clone();
        let tangent_use = seam
            .coedges
            .iter()
            .copied()
            .find(|coedge| {
                self.working
                    .coedge(*coedge)
                    .and_then(|value| self.working.loop_(value.loop_id))
                    .map(|loop_value| loop_value.face == tangent_face)
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "smooth blend seam has no use on its tangent support",
                )
            })?;
        let seam_pcurve = self.working.coedge(tangent_use)?.pcurve.clone();
        let tangent_outward = match self.working.face(tangent_face)?.orientation {
            FaceOrientation::Forward => radius,
            FaceOrientation::Reversed => -radius,
        };
        let tangent_distance = if concave {
            tangent_outward
        } else {
            -tangent_outward
        };
        let common_surface = self.working.face(common_face)?.surface.clone();
        let common_outward = match self.working.face(common_face)?.orientation {
            FaceOrientation::Forward => radius,
            FaceOrientation::Reversed => -radius,
        };
        let common_distance = if concave {
            common_outward
        } else {
            -common_outward
        };
        let common_offset = common_surface.offset(common_distance, tolerance)?;
        let offset_seam = |parameter: f64| -> KernelResult<(Point3, Vec3, Point3)> {
            let uv = seam_pcurve.evaluate(parameter)?;
            let surface = tangent_surface.evaluate(uv.point.x, uv.point.y)?;
            let tangent = surface.du * uv.first_derivative.x + surface.dv * uv.first_derivative.y;
            let cross = surface.du.cross(surface.dv);
            let magnitude = cross.magnitude();
            if magnitude
                <= tolerance
                    .linear_at_scale(surface.du.magnitude().max(surface.dv.magnitude()).max(1.0))
            {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "smooth blend seam reached a singular tangent support",
                ));
            }
            let normal = cross / magnitude;
            let cross_u = surface.duu.cross(surface.dv) + surface.du.cross(surface.duv);
            let cross_v = surface.duv.cross(surface.dv) + surface.du.cross(surface.dvv);
            let project_tangent =
                |derivative: Vec3| (derivative - normal * normal.dot(derivative)) / magnitude;
            let normal_derivative = project_tangent(cross_u) * uv.first_derivative.x
                + project_tangent(cross_v) * uv.first_derivative.y;
            Ok((
                surface.point + normal * tangent_distance,
                tangent + normal_derivative * tangent_distance,
                surface.point,
            ))
        };

        // Seed from the offset seam, not the old vertex: at a tangential
        // termination the old vertex can put Newton on a singular boundary.
        let seed_center = offset_seam(seam_parameter)?.0;
        let common_seed = match &common_offset {
            Surface3::Offset(offset) => offset.base().project(seed_center, tolerance)?.parameters,
            _ => common_offset.project(seed_center, tolerance)?.parameters,
        };
        let mut parameters = [seam_parameter, common_seed.0, common_seed.1];
        let mut solution = None;
        for _ in 0..48 {
            parameters[0] = parameters[0].clamp(
                seam.range.start.min(seam.range.end),
                seam.range.start.max(seam.range.end),
            );
            let domains = common_offset.domains();
            parameters[1] = domains[0].normalize(parameters[1]);
            parameters[2] = domains[1].normalize(parameters[2]);
            let (center, derivative, contact) = offset_seam(parameters[0])?;
            let common = common_offset.evaluate(parameters[1], parameters[2])?;
            let residual = center - common.point;
            let allowed = tolerance.linear_at_scale(
                center
                    .to_vec()
                    .magnitude()
                    .max(common.point.to_vec().magnitude())
                    .max(1.0),
            );
            if residual.magnitude() <= allowed {
                solution = Some((
                    Point3::from_vec((center.to_vec() + common.point.to_vec()) * 0.5),
                    contact,
                    (parameters[1], parameters[2]),
                ));
                break;
            }
            let matrix = [
                [derivative.x, -common.du.x, -common.dv.x],
                [derivative.y, -common.du.y, -common.dv.y],
                [derivative.z, -common.du.z, -common.dv.z],
            ];
            let Some(delta) = super::intersection::solve_linear_3(
                matrix,
                [-residual.x, -residual.y, -residual.z],
            ) else {
                break;
            };
            let previous = residual.magnitude2();
            let mut damping = 1.0;
            let mut accepted = false;
            for _ in 0..12 {
                let candidate = [
                    (parameters[0] + delta[0] * damping).clamp(
                        seam.range.start.min(seam.range.end),
                        seam.range.start.max(seam.range.end),
                    ),
                    domains[0].normalize(parameters[1] + delta[1] * damping),
                    domains[1].normalize(parameters[2] + delta[2] * damping),
                ];
                let center = offset_seam(candidate[0])?.0;
                let common = common_offset.evaluate(candidate[1], candidate[2])?;
                if (center - common.point).magnitude2() < previous {
                    parameters = candidate;
                    accepted = true;
                    break;
                }
                damping *= 0.5;
            }
            if !accepted {
                break;
            }
        }
        let (center, seam_contact, common_parameters) = solution.ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::NumericalFailure,
                "exact smooth blend junction correction did not converge",
            )
        })?;
        let common_contact = common_surface
            .evaluate(common_parameters.0, common_parameters.1)?
            .point;
        let allowed =
            tolerance.linear_at_scale(center.to_vec().magnitude().max(radius).max(1.0)) * 16.0;
        for contact in [seam_contact, common_contact] {
            if (contact.distance(center) - radius).abs() > allowed {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "smooth blend junction contact is not at the requested rolling-ball radius",
                ));
            }
        }
        let mut contact_points = BTreeMap::from([
            (tangent_faces[0], seam_contact),
            (tangent_faces[1], seam_contact),
            (common_face, common_contact),
        ]);
        if let Some((opposite, pair)) = second_seam {
            let edge = self.working.edge(opposite)?;
            let projection = edge.curve.project(common_contact, tolerance)?;
            if projection.distance > allowed {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "smooth blend contacts do not meet the opposite support seam",
                ));
            }
            let mut normals = [Vec3::new(0.0, 0.0, 0.0); 2];
            for side in 0..2 {
                let face = self.working.face(pair[side])?;
                let uv = face.surface.project(common_contact, tolerance)?;
                if uv.distance > allowed {
                    return Err(KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "smooth blend contact misses an opposite support",
                    ));
                }
                normals[side] = face
                    .surface
                    .normal(uv.parameters.0, uv.parameters.1)?
                    .normalize();
                if face.orientation == FaceOrientation::Reversed {
                    normals[side] = -normals[side];
                }
            }
            if normals[0].dot(normals[1]) < 1.0 - tolerance.angular.max(f64::EPSILON.sqrt()) * 8.0 {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "opposite blend supports are not tangent at their seam",
                ));
            }
            contact_points.insert(pair[1], common_contact);
        }
        Ok(BlendJunction {
            faces: vertex_faces,
            center,
            contact_points,
            kind: BlendJunctionKind::Smooth {
                seam_edge,
                tangent_faces,
                second_seam,
            },
        })
    }

    fn prepare_tangent_chain_junction(
        &self,
        spine: &BlendSpine,
        endpoint_index: usize,
        tangent_faces: [FaceId; 2],
    ) -> KernelResult<BlendJunction> {
        let tolerance = self.kernel.tolerance;
        let old_vertex = spine.edge.vertices[endpoint_index];
        let old_point = self.working.vertex(old_vertex)?.point;
        let projected = spine.center.curve.project(old_point, tolerance)?.parameter;
        let mut parameter = if let Some(period) = spine.shared_seam.map(|_| 1.0).or(spine
            .center
            .curve
            .domain()
            .period)
        {
            projected
                + ((spine.endpoint_seeds[endpoint_index] - projected) / period).round() * period
        } else {
            projected
        };
        // A periodic support seam must meet the exact contact curve. The
        // nearest center to the old vertex need not lie on that seam.
        for seam_id in self.working.vertex_edges(old_vertex)? {
            let faces = self.working.edge_faces(seam_id)?;
            if faces.len() != 1 {
                continue;
            }
            let Some(parent) = spine.faces.iter().position(|face| faces.contains(face)) else {
                continue;
            };
            let seam = self.working.edge(seam_id)?;
            let contact: Curve3 = SurfaceCurve3::new(
                spine.parent_surfaces[parent].clone(),
                if parent == 0 {
                    spine.center.first_pcurve.clone()
                } else {
                    spine.center.second_pcurve.clone()
                },
                spine.provisional_range,
                tolerance,
            )?
            .into();
            let seam_parameter = if seam.vertices[0] == old_vertex {
                seam.range.start
            } else {
                seam.range.end
            };
            parameter = contact
                .intersect_curve_near(&seam.curve, parameter, seam_parameter, tolerance)?
                .first_parameter;
        }
        let center = spine.center.curve.evaluate(parameter)?.point;
        let mut contact_points = BTreeMap::new();
        for parent_index in 0..2 {
            let uv = if parent_index == 0 {
                spine.center.first_pcurve.evaluate(parameter)?.point
            } else {
                spine.center.second_pcurve.evaluate(parameter)?.point
            };
            let point = spine.parent_surfaces[parent_index]
                .evaluate(uv.x, uv.y)?
                .point;
            contact_points.insert(spine.faces[parent_index], point);
        }
        Ok(BlendJunction {
            faces: tangent_faces.to_vec(),
            center,
            contact_points,
            kind: BlendJunctionKind::TangentChain { tangent_faces },
        })
    }

    fn tangent_chain_continuation(
        &self,
        spine: &BlendSpine,
        endpoint_index: usize,
    ) -> KernelResult<Option<(EdgeId, [FaceId; 2])>> {
        let vertex = spine.edge.vertices[endpoint_index];
        let source_tangent = spine.edge.endpoint_tangent(endpoint_index)?.normalize();
        let source_faces = spine.faces.into_iter().collect::<BTreeSet<_>>();
        let mut continuation = None;
        for candidate_id in self
            .working
            .vertex_edges(vertex)?
            .into_iter()
            .filter(|edge| *edge != spine.edge_id)
        {
            let candidate = self.working.edge(candidate_id)?;
            let same_support = match (&spine.edge.curve, &candidate.curve) {
                (Curve3::Circle(first), Curve3::Circle(second)) => {
                    let scale = first
                        .center()
                        .to_vec()
                        .magnitude()
                        .max(first.radius())
                        .max(1.0);
                    first.center().distance(second.center())
                        <= self.kernel.tolerance.linear_at_scale(scale) * 8.0
                        && (first.radius() - second.radius()).abs()
                            <= self.kernel.tolerance.linear_at_scale(scale) * 8.0
                        && first.normal().dot(second.normal()).abs()
                            >= 1.0 - self.kernel.tolerance.angular * 8.0
                }
                (Curve3::Line(first), Curve3::Line(second)) => {
                    let first_direction = first.direction().normalize();
                    let second_direction = second.direction().normalize();
                    first_direction.cross(second_direction).magnitude()
                        <= self.kernel.tolerance.angular * 8.0
                        && (second.origin() - first.origin())
                            .cross(first_direction)
                            .magnitude()
                            <= self.kernel.tolerance.linear_at_scale(
                                first
                                    .origin()
                                    .to_vec()
                                    .magnitude()
                                    .max(second.origin().to_vec().magnitude())
                                    .max(1.0),
                            ) * 8.0
                }
                // Boolean intersections can retain separate parameter guides
                // on either side of a periodic seam. Their charts need not be
                // identical: the incident faces and endpoint tangents below
                // identify the continuation geometrically.
                (Curve3::SurfaceIntersection(_), Curve3::SurfaceIntersection(_)) => true,
                _ => spine.edge.curve == candidate.curve,
            };
            if candidate.coedges.len() != 2
                || !same_support
                || self
                    .working
                    .edge_faces(candidate_id)?
                    .into_iter()
                    .collect::<BTreeSet<_>>()
                    != source_faces
            {
                continue;
            }
            let endpoint = if candidate.vertices[0] == vertex {
                0
            } else if candidate.vertices[1] == vertex {
                1
            } else {
                continue;
            };
            let tangent = candidate.endpoint_tangent(endpoint)?;
            if tangent.magnitude2() <= self.kernel.tolerance.linear.powi(2) {
                continue;
            }
            if source_tangent.dot(tangent.normalize()).abs()
                < 1.0 - self.kernel.tolerance.angular.max(f64::EPSILON.sqrt()) * 8.0
            {
                continue;
            }
            if continuation.replace(candidate_id).is_some() {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "a blend tangent chain branches at one topological vertex",
                ));
            }
        }
        Ok(continuation.map(|edge| (edge, spine.faces)))
    }

    fn smooth_blend_continuation(
        &self,
        vertex: VertexId,
        current_edge: EdgeId,
        selected: &BTreeSet<EdgeId>,
    ) -> KernelResult<Option<(EdgeId, EdgeId, [FaceId; 2])>> {
        let point = self.working.vertex(vertex)?.point;
        let current_faces = self.working.edge_faces(current_edge)?;
        let incident = self.working.vertex_edges(vertex)?;
        for seam in incident
            .iter()
            .copied()
            .filter(|edge| !selected.contains(edge))
        {
            let faces = self.working.edge_faces(seam)?;
            // A nearby tangent seam is only a continuation when it belongs
            // to a support of the selected edge. Four-face cap corners also
            // contain an unrelated seam between their terminating supports.
            if !faces.iter().any(|face| current_faces.contains(face)) {
                continue;
            }
            if faces.len() != 2 {
                continue;
            }
            let mut normals = Vec::with_capacity(2);
            for face in &faces {
                let value = self.working.face(*face)?;
                let projection = value.surface.project(point, self.kernel.tolerance)?;
                let mut normal = value
                    .surface
                    .normal(projection.parameters.0, projection.parameters.1)?;
                if value.orientation == FaceOrientation::Reversed {
                    normal = -normal;
                }
                normals.push(normal.normalize());
            }
            if normals[0].dot(normals[1])
                < 1.0 - self.kernel.tolerance.angular.max(f64::EPSILON.sqrt()) * 8.0
            {
                continue;
            }
            // A chamfer over a rounded corner has two smooth seams at this
            // vertex. Continue along the sharp rim, never along the other seam.
            let continuation = incident.iter().copied().find(|edge| {
                if *edge == seam || *edge == current_edge {
                    return false;
                }
                let Ok(value) = self.working.edge(*edge) else {
                    return false;
                };
                let Ok(faces) = self.working.edge_faces(*edge) else {
                    return false;
                };
                if faces.len() != 2 || value.coedges.len() != 2 {
                    return false;
                }
                let (Ok(first_face), Ok(second_face)) =
                    (self.working.face(faces[0]), self.working.face(faces[1]))
                else {
                    return false;
                };
                let supports = [first_face.surface.clone(), second_face.surface.clone()];
                let Ok(midpoint) = value
                    .curve
                    .evaluate((value.range.start + value.range.end) * 0.5)
                else {
                    return false;
                };
                self.blend_edge_is_concave(
                    value,
                    [value.coedges[0], value.coedges[1]],
                    [faces[0], faces[1]],
                    &supports,
                    midpoint.point,
                )
                .is_ok()
            });
            if let Some(continuation) = continuation {
                return Ok(Some((seam, continuation, [faces[0], faces[1]])));
            }
        }
        Ok(None)
    }

    fn chamfer_surface(
        &self,
        spine: &BlendSpine,
        range: ParameterRange,
    ) -> KernelResult<(Surface3, BlendSurfaceParameterization)> {
        let tolerance = self.kernel.tolerance;
        let contact = |index: usize, parameter: f64| -> KernelResult<Point3> {
            let uv = if index == 0 {
                &spine.center.first_pcurve
            } else {
                &spine.center.second_pcurve
            }
            .evaluate(parameter)?
            .point;
            Ok(spine.parent_surfaces[index].evaluate(uv.x, uv.y)?.point)
        };
        let t = (range.start + range.end) * 0.5;
        let first = contact(0, t)?;
        let second = contact(1, t)?;
        if spine
            .parent_surfaces
            .iter()
            .all(|surface| matches!(surface, Surface3::Plane(_)))
        {
            if let Curve3::Line(line) = &spine.center.curve {
                let plane = Plane3::new(first, line.direction(), second - first, tolerance)?;
                let origin = plane.project(contact(0, t - 1.0)?)?.parameters;
                let middle = plane.project(first)?.parameters;
                let end = plane.project(second)?.parameters;
                let direction = Vec2::new(middle.0 - origin.0, middle.1 - origin.1);
                return Ok((
                    plane.into(),
                    BlendSurfaceParameterization {
                        origin: Point2::new(middle.0, middle.1) - direction * t,
                        center_direction: direction,
                        blend_direction: Vec2::new(end.0 - middle.0, end.1 - middle.1),
                    },
                ));
            }
        }
        if matches!(
            spine.parent_surfaces,
            [Surface3::Plane(_), Surface3::Cylinder(_)]
                | [Surface3::Cylinder(_), Surface3::Plane(_)]
        ) {
            if let Curve3::Circle(circle) = &spine.center.curve {
                let radial = (circle.evaluate(t)?.point - circle.center()).normalize();
                let normal = circle.normal();
                let r0 = (first - circle.center()).dot(radial);
                let r1 = (second - circle.center()).dot(radial);
                let z0 = (first - circle.center()).dot(normal);
                let z1 = (second - circle.center()).dot(normal);
                let dz_dr = (z1 - z0) / (r1 - r0);
                let apex = circle.center() + normal * (z0 - r0 * dz_dr);
                let cone = Cone3::new(
                    apex,
                    normal * dz_dr,
                    circle.x_radius().normalize(),
                    circle.y_radius().normalize(),
                    tolerance,
                )?;
                return Ok((
                    cone.into(),
                    BlendSurfaceParameterization {
                        origin: Point2::new(0.0, r0),
                        center_direction: Vec2::new(1.0, 0.0),
                        blend_direction: Vec2::new(0.0, r1 - r0),
                    },
                ));
            }
        }
        Ok((
            RollingBallSurface3::new(
                spine.center.curve.clone(),
                spine.parent_surfaces[0].clone(),
                spine.parent_surfaces[1].clone(),
                spine.center.first_pcurve.clone(),
                spine.center.second_pcurve.clone(),
                if spine.shared_seam.is_some() {
                    spine.provisional_range
                } else {
                    range
                },
                spine.offset,
                tolerance,
            )?
            .into_ruled()
            .into(),
            BlendSurfaceParameterization::CONTACT_RAILS,
        ))
    }

    fn most_implicit_fillet_surface(
        &self,
        spine: &BlendSpine,
        center_range: ParameterRange,
        radius: f64,
    ) -> KernelResult<(Surface3, BlendSurfaceParameterization)> {
        let tolerance = self.kernel.tolerance;
        if matches!(
            spine.parent_surfaces,
            [Surface3::Plane(_), Surface3::Plane(_)]
        ) {
            if let Curve3::Line(line) = &spine.center.curve {
                let reference = (center_range.start + center_range.end) * 0.5;
                let center = line.evaluate(reference)?.point;
                let mut radials = [Vec3::new(0.0, 0.0, 0.0); 2];
                for (index, radial) in radials.iter_mut().enumerate() {
                    let pcurve = if index == 0 {
                        &spine.center.first_pcurve
                    } else {
                        &spine.center.second_pcurve
                    };
                    let uv = pcurve.evaluate(reference)?.point;
                    *radial = spine.parent_surfaces[index].evaluate(uv.x, uv.y)?.point - center;
                }
                let axis = line.direction().normalize();
                let scale = center.to_vec().magnitude().max(radius).max(1.0);
                let allowed = tolerance.linear_at_scale(scale) * 32.0;
                if radials.iter().all(|radial| {
                    (radial.magnitude() - radius).abs() <= allowed
                        && radial.dot(axis).abs() <= allowed
                }) {
                    let x = radials[0].normalize();
                    let y = axis.cross(x).normalize();
                    let angle = radials[1].dot(y).atan2(radials[1].dot(x));
                    if angle.abs() > tolerance.angular
                        && std::f64::consts::PI - angle.abs() > tolerance.angular
                    {
                        let surface: Surface3 = Cylinder3::new(
                            line.origin(),
                            line.direction(),
                            x * radius,
                            y * radius,
                            tolerance,
                        )?
                        .into();
                        let parameterization = BlendSurfaceParameterization {
                            origin: Point2::new(0.0, 0.0),
                            center_direction: Vec2::new(0.0, 1.0),
                            blend_direction: Vec2::new(angle, 0.0),
                        };
                        if self.fillet_surface_matches(
                            spine,
                            center_range,
                            &surface,
                            parameterization,
                            radius,
                        )? {
                            return Ok((surface, parameterization));
                        }
                    }
                }
            }
        }

        let rotational_parents = matches!(
            &spine.parent_surfaces,
            [Surface3::Plane(_), Surface3::Cylinder(_)]
                | [Surface3::Cylinder(_), Surface3::Plane(_)]
        );
        if rotational_parents {
            if let Curve3::Circle(circle) = &spine.center.curve {
                let reference = (center_range.start + center_range.end) * 0.5;
                let center = circle.evaluate(reference)?.point;
                let major_radial = (center - circle.center()).normalize();
                let normal = circle.normal();
                let mut angles = [0.0; 2];
                let mut valid = true;
                for (index, angle) in angles.iter_mut().enumerate() {
                    let pcurve = if index == 0 {
                        &spine.center.first_pcurve
                    } else {
                        &spine.center.second_pcurve
                    };
                    let uv = pcurve.evaluate(reference)?.point;
                    let radial = spine.parent_surfaces[index].evaluate(uv.x, uv.y)?.point - center;
                    let scale = center.to_vec().magnitude().max(radius).max(1.0);
                    let allowed = tolerance.linear_at_scale(scale) * 32.0;
                    valid &= (radial.magnitude() - radius).abs() <= allowed
                        && radial.dot(major_radial.cross(normal)).abs() <= allowed;
                    *angle = radial.dot(normal).atan2(radial.dot(major_radial));
                }
                let delta = signed_periodic_delta(angles[0], angles[1]);
                if valid
                    && delta.abs() > tolerance.angular
                    && std::f64::consts::PI - delta.abs() > tolerance.angular
                {
                    let parameterization = BlendSurfaceParameterization {
                        origin: Point2::new(0.0, angles[0]),
                        center_direction: Vec2::new(1.0, 0.0),
                        blend_direction: Vec2::new(0.0, delta),
                    };
                    let canonical = Torus3::new(
                        circle.center(),
                        circle.x_radius(),
                        circle.y_radius(),
                        radius,
                        tolerance,
                    )
                    .map(Surface3::from)
                    .or_else(|_| {
                        let frame = Frame3::new(
                            circle.center(),
                            circle.x_radius(),
                            circle.y_radius(),
                            tolerance,
                        )?;
                        let profile: Curve2 =
                            Circle2::new(Point2::new(circle.radius(), 0.0), radius)?.into();
                        RevolvedSurface3::new(
                            frame,
                            profile,
                            ParameterRange::new(angles[0], angles[0] + delta)?,
                            tolerance,
                        )
                        .map(Surface3::from)
                    });
                    if let Ok(surface) = canonical {
                        if self.fillet_surface_matches(
                            spine,
                            center_range,
                            &surface,
                            parameterization,
                            radius,
                        )? {
                            return Ok((surface, parameterization));
                        }
                    }
                }
            }
        }

        Ok((
            RollingBallSurface3::new(
                spine.center.curve.clone(),
                spine.parent_surfaces[0].clone(),
                spine.parent_surfaces[1].clone(),
                spine.center.first_pcurve.clone(),
                spine.center.second_pcurve.clone(),
                if spine.shared_seam.is_some() {
                    spine.provisional_range
                } else {
                    center_range
                },
                radius,
                tolerance,
            )?
            .into(),
            BlendSurfaceParameterization::CONTACT_RAILS,
        ))
    }

    fn fillet_surface_matches(
        &self,
        spine: &BlendSpine,
        center_range: ParameterRange,
        surface: &Surface3,
        parameterization: BlendSurfaceParameterization,
        radius: f64,
    ) -> KernelResult<bool> {
        for sample in 0..=2 {
            let parameter = center_range.start + center_range.span() * sample as f64 / 2.0;
            let center = spine.center.curve.evaluate(parameter)?.point;
            for parent_index in 0..2 {
                let parent_pcurve = if parent_index == 0 {
                    &spine.center.first_pcurve
                } else {
                    &spine.center.second_pcurve
                };
                let parent_uv = parent_pcurve.evaluate(parameter)?.point;
                let expected = spine.parent_surfaces[parent_index]
                    .evaluate(parent_uv.x, parent_uv.y)?
                    .point;
                let native = parameterization.origin
                    + parameterization.center_direction * parameter
                    + parameterization.blend_direction * parent_index as f64;
                let achieved = surface.evaluate(native.x, native.y)?.point;
                let scale = expected
                    .to_vec()
                    .magnitude()
                    .max(center.to_vec().magnitude())
                    .max(radius)
                    .max(1.0);
                if expected.distance(achieved) > self.kernel.tolerance.linear_at_scale(scale) * 32.0
                    || (expected.distance(center) - radius).abs()
                        > self.kernel.tolerance.linear_at_scale(scale) * 32.0
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn implicit_end_boundary(
        &self,
        blend_surface: &Surface3,
        terminating_surface: &Surface3,
        blend_guide: &Curve2,
        contact_points: [Point3; 2],
    ) -> KernelResult<Option<BlendBoundary>> {
        let tolerance = self.kernel.tolerance;
        let IntersectionSet::Isolated(candidates) =
            (match blend_surface.intersect_surface(terminating_surface, tolerance) {
                Ok(intersection) => intersection,
                Err(_) => return Ok(None),
            })
        else {
            return Ok(None);
        };
        let desired_middle_uv = blend_guide.evaluate(0.5)?.point;
        let desired_middle = blend_surface
            .evaluate(desired_middle_uv.x, desired_middle_uv.y)?
            .point;
        let Some((candidate, middle_projection)) = candidates
            .into_iter()
            .filter_map(|candidate| {
                candidate
                    .curve
                    .project(desired_middle, tolerance)
                    .ok()
                    .map(|projection| (candidate, projection))
            })
            .min_by(|first, second| first.1.distance.total_cmp(&second.1.distance))
        else {
            return Ok(None);
        };
        let first = candidate.curve.project(contact_points[0], tolerance)?;
        let mut second = candidate.curve.project(contact_points[1], tolerance)?;
        let mut middle_parameter = middle_projection.parameter;
        if let Some(period) = candidate.curve.domain().period {
            middle_parameter += ((first.parameter - middle_parameter) / period).round() * period;
            second.parameter +=
                ((middle_parameter * 2.0 - first.parameter - second.parameter) / period).round()
                    * period;
        }
        let scale = contact_points
            .iter()
            .chain(std::iter::once(&desired_middle))
            .map(|point| point.to_vec().magnitude())
            .fold(1.0_f64, f64::max);
        let allowed = tolerance.linear_at_scale(scale) * 32.0;
        if first.distance > allowed
            || second.distance > allowed
            || middle_projection.distance > allowed
        {
            return Ok(None);
        }
        let Ok(range) = ParameterRange::new(first.parameter, second.parameter) else {
            return Ok(None);
        };
        Ok(Some(BlendBoundary {
            curve: candidate.curve,
            parent_pcurve: candidate.second_pcurve,
            blend_pcurve: candidate.first_pcurve,
            range,
        }))
    }

    fn prepare_end_boundary(
        &self,
        blend_surface: &Surface3,
        terminating_surface: Surface3,
        blend_pcurve: Curve2,
        contact_points: [Point3; 2],
        reference: Point2,
    ) -> KernelResult<BlendBoundary> {
        let tolerance = self.kernel.tolerance;
        let unit = ParameterRange::new(0.0, 1.0)?;
        if let Some(boundary) = self.implicit_end_boundary(
            blend_surface,
            &terminating_surface,
            &blend_pcurve,
            contact_points,
        )? {
            return Ok(boundary);
        }
        let mut guide_parameters = Vec::with_capacity(65);
        let mut blend_parameters: Vec<Point2> = Vec::with_capacity(65);
        let mut parent_parameters: Vec<Point2> = Vec::with_capacity(65);
        for index in 0..=64 {
            let parameter = 0.5 - 0.5 * (std::f64::consts::PI * index as f64 / 64.0).cos();
            guide_parameters.push(parameter);
            let blend_uv = blend_pcurve.evaluate(parameter)?.point;
            let point = blend_surface.evaluate(blend_uv.x, blend_uv.y)?.point;
            let projected = terminating_surface.project(point, tolerance)?;
            let seed = [
                blend_uv.x,
                blend_uv.y,
                projected.parameters.0,
                projected.parameters.1,
            ];
            let corrected = match super::intersection::correct_surface_pair_near(
                blend_surface,
                &terminating_surface,
                seed,
                tolerance,
            ) {
                Ok(corrected) => corrected,
                Err(_) if index == 0 || index == 64 => seed,
                Err(error) => {
                    return Err(KernelError::new(
                        DiagnosticKind::NumericalFailure,
                        format!(
                            "blend end-boundary guide correction failed at sample {index}: {error}"
                        ),
                    ))
                }
            };
            let lift = |mut uv: Point2, near: Point2, surface: &Surface3| {
                let domains = surface.domains();
                if let Some(period) = domains[0].period {
                    uv.x += ((near.x - uv.x) / period).round() * period;
                }
                if let Some(period) = domains[1].period {
                    uv.y += ((near.y - uv.y) / period).round() * period;
                }
                uv
            };
            blend_parameters.push(lift(
                Point2::new(corrected[0], corrected[1]),
                blend_parameters.last().copied().unwrap_or(blend_uv),
                blend_surface,
            ));
            parent_parameters.push(lift(
                Point2::new(corrected[2], corrected[3]),
                parent_parameters.last().copied().unwrap_or(reference),
                &terminating_surface,
            ));
        }
        let blend_guide: Curve2 =
            super::intersection::interpolating_nurbs_guide(&blend_parameters, &guide_parameters)?
                .into();
        let parent_guide: Curve2 =
            super::intersection::interpolating_nurbs_guide(&parent_parameters, &guide_parameters)?
                .into();
        let intersection = SurfaceIntersectionCurve::new(
            (*blend_surface).clone(),
            terminating_surface,
            blend_guide,
            parent_guide,
            unit,
            tolerance,
            tolerance.linear,
        )
        .map_err(|error| {
            KernelError::new(
                DiagnosticKind::NumericalFailure,
                format!("blend end boundary exact intersection failed: {error}"),
            )
        })?;
        let exact = intersection.exact_pcurves();
        Ok(BlendBoundary {
            curve: intersection.into(),
            parent_pcurve: exact[1].clone(),
            blend_pcurve: exact[0].clone(),
            range: unit,
        })
    }

    fn prepare_blend_patch(
        &self,
        spine: BlendSpine,
        junctions: &BTreeMap<VertexId, BlendJunction>,
        junction_surfaces: &BTreeMap<VertexId, Surface3>,
        regular_endpoints: &BTreeMap<(EdgeId, usize), Option<BlendEndpoint>>,
        kind: EdgeBlendKind,
    ) -> KernelResult<BlendPatch> {
        let tolerance = self.kernel.tolerance;
        let mut ends = Vec::with_capacity(2);
        let mut contact_parameters = [[0.0; 2]; 2];
        let mut contact_points = [[Point3::new(0.0, 0.0, 0.0); 2]; 2];
        for endpoint_index in 0..2 {
            let vertex = spine.edge.vertices[endpoint_index];
            if let Some(junction) = junctions.get(&vertex) {
                if matches!(junction.kind, BlendJunctionKind::RimCorner { .. }) {
                    let parameter = spine.endpoint_seeds[endpoint_index];
                    for parent_index in 0..2 {
                        let uv = if parent_index == 0 {
                            spine.center.first_pcurve.evaluate(parameter)?.point
                        } else {
                            spine.center.second_pcurve.evaluate(parameter)?.point
                        };
                        contact_parameters[endpoint_index][parent_index] = parameter;
                        contact_points[endpoint_index][parent_index] = spine.parent_surfaces
                            [parent_index]
                            .evaluate(uv.x, uv.y)?
                            .point;
                    }
                    ends.push(BlendPatchEnd::Junction(vertex));
                    continue;
                }
                if (kind == EdgeBlendKind::Chamfer
                    && matches!(junction.kind, BlendJunctionKind::Trihedral))
                    || matches!(junction.kind, BlendJunctionKind::Miter { .. })
                {
                    for parent in 0..2 {
                        let point = junction.contact_points[&spine.faces[parent]];
                        let pcurve = if parent == 0 {
                            &spine.center.first_pcurve
                        } else {
                            &spine.center.second_pcurve
                        };
                        let rail = Curve3::from(SurfaceCurve3::new(
                            spine.parent_surfaces[parent].clone(),
                            pcurve.clone(),
                            spine.provisional_range,
                            tolerance,
                        )?);
                        let projection = rail.project(point, tolerance)?;
                        if projection.distance
                            > tolerance.linear_at_scale(point.to_vec().magnitude()) * 16.0
                        {
                            return Err(KernelError::new(
                                DiagnosticKind::GeometryInvariant,
                                "blend corner misses its incident contact rail",
                            ));
                        }
                        contact_parameters[endpoint_index][parent] = projection.parameter;
                        contact_points[endpoint_index][parent] = point;
                    }
                    ends.push(BlendPatchEnd::Junction(vertex));
                    continue;
                }
                let projected = if spine.local_guide {
                    project_local_blend_endpoint(
                        &spine.center.curve,
                        junction.center,
                        endpoint_index,
                        tolerance,
                    )?
                } else {
                    spine
                        .center
                        .curve
                        .project(junction.center, tolerance)?
                        .parameter
                };
                let mut parameter = if spine.shared_seam.is_some() {
                    projected + (spine.endpoint_seeds[endpoint_index] - projected).round()
                } else {
                    blend_parameter_near(
                        &spine.center.curve,
                        projected,
                        spine.endpoint_seeds[endpoint_index],
                        tolerance,
                    )?
                };
                if spine.shared_seam.is_some() {
                    // Repeated guide turns need not have identical parameter
                    // speeds, so an integer shift alone is only a seed.
                    for _ in 0..16 {
                        let value = spine.center.curve.evaluate(parameter)?;
                        let residual = value.point - junction.center;
                        if residual.magnitude() <= tolerance.linear {
                            break;
                        }
                        parameter -= residual.dot(value.first_derivative)
                            / value.first_derivative.magnitude2();
                    }
                }
                let achieved = spine
                    .center
                    .curve
                    .evaluate(parameter)?
                    .point
                    .distance(junction.center);
                let allowed =
                    tolerance.linear_at_scale(junction.center.to_vec().magnitude().max(1.0)) * 8.0;
                if achieved > allowed {
                    return Err(KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "blend junction center does not lie on every incident offset intersection",
                    ));
                }
                for parent_index in 0..2 {
                    contact_parameters[endpoint_index][parent_index] = parameter;
                    contact_points[endpoint_index][parent_index] = *junction
                        .contact_points
                        .get(&spine.faces[parent_index])
                        .ok_or_else(|| {
                            KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "blend junction lost an incident face contact",
                            )
                        })?;
                }
                ends.push(BlendPatchEnd::Junction(vertex));
            } else {
                let endpoint = regular_endpoints
                    .get(&(spine.edge_id, endpoint_index))
                    .and_then(Clone::clone)
                    .ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::TopologyInvariant,
                            "regular blend endpoint was not retained during network discovery",
                        )
                    })?;
                contact_parameters[endpoint_index] = endpoint.contact_parameters;
                contact_points[endpoint_index] = endpoint.contact_points;
                ends.push(BlendPatchEnd::Boundary(endpoint));
            }
        }
        let minimum = contact_parameters
            .into_iter()
            .flatten()
            .fold(f64::INFINITY, f64::min);
        let maximum = contact_parameters
            .into_iter()
            .flatten()
            .fold(f64::NEG_INFINITY, f64::max);
        let center_domain = spine.center.curve.domain();
        let blend_support_range = match center_domain.bounds {
            _ if center_domain.period.is_some() => {
                let range = ParameterRange::new(minimum, maximum).map_err(|_| {
                    KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        format!(
                            "blend rolling support collapsed to zero span ({minimum} to {maximum})",
                        ),
                    )
                })?;
                // A curved end boundary can bow past both contact endpoints.
                // Leave a radius-sized collar on the rolling support; the
                // contact and end p-curves still define the actual face trim.
                let speed = spine
                    .center
                    .curve
                    .evaluate(minimum)?
                    .first_derivative
                    .magnitude()
                    .min(spine.center.curve.evaluate(maximum)?.first_derivative.magnitude());
                let padding = spine.offset / speed;
                ParameterRange::new(range.start - padding, range.end + padding)?
            }
            DomainBounds::Bounded(range) => range,
            _ => {
                let span = (maximum - minimum).abs().max(1.0);
                ParameterRange::new(minimum - span, maximum + span)?
            }
        };
        let (blend_surface, blend_parameterization) = if kind == EdgeBlendKind::Chamfer {
            self.chamfer_surface(&spine, blend_support_range)?
        } else {
            self.most_implicit_fillet_surface(&spine, blend_support_range, spine.offset)?
        };
        let contact_boundaries = (0..2)
            .map(|parent_index| {
                let range = ParameterRange::new(
                    contact_parameters[0][parent_index],
                    contact_parameters[1][parent_index],
                )
                .map_err(|_| {
                    KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        format!(
                            "blend contact boundary {parent_index} collapsed to zero span ({} to {})",
                            contact_parameters[0][parent_index],
                            contact_parameters[1][parent_index],
                        ),
                    )
                })?;
                let parent_pcurve = if parent_index == 0 {
                    spine.center.first_pcurve.clone()
                } else {
                    spine.center.second_pcurve.clone()
                };
                let blend_pcurve = blend_parameterization.contact_pcurve(parent_index)?;
                Ok(BlendBoundary {
                    curve: SurfaceCurve3::new(
                        spine.parent_surfaces[parent_index].clone(),
                        parent_pcurve.clone(),
                        range,
                        tolerance,
                    )?
                    .into(),
                    parent_pcurve,
                    blend_pcurve,
                    range,
                })
            })
            .collect::<KernelResult<Vec<_>>>()?
            .try_into()
            .map_err(|_| {
                KernelError::new(DiagnosticKind::TopologyInvariant, "two contacts expected")
            })?;

        let unit = ParameterRange::new(0.0, 1.0)?;
        let end_boundaries = (0..2)
            .map(|endpoint_index| {
                let blend_pcurve = blend_parameterization.end_pcurve(
                    contact_parameters[endpoint_index][0],
                    contact_parameters[endpoint_index][1],
                )?;
                match &ends[endpoint_index] {
                    BlendPatchEnd::Boundary(endpoint) => {
                        let terminating_surface = match endpoint.cap {
                            Some((_, plane)) => Surface3::from(plane),
                            None => self
                                .working
                                .face(endpoint.terminating_face)?
                                .surface
                                .clone(),
                        };
                        let adjacent_edge = match endpoint.tangent_termination {
                            Some((seam, 0)) => seam,
                            _ => self.working.coedge(endpoint.adjacent_uses[0])?.edge,
                        };
                        let (terminating_id, terminating_use) = self
                            .working
                            .edge(adjacent_edge)?
                            .coedges
                            .iter()
                            .filter_map(|id| {
                                self.working.coedge(*id).ok().map(|coedge| (*id, coedge))
                            })
                            .find(|(_, coedge)| {
                                self.working
                                    .loop_(coedge.loop_id)
                                    .is_ok_and(|loop_| loop_.face == endpoint.terminating_face)
                            })
                            .ok_or_else(|| {
                                KernelError::new(
                                    DiagnosticKind::TopologyInvariant,
                                    "blend endpoint lost its terminating chart",
                                )
                            })?;
                        let traversal = self.working.coedge_vertices(terminating_id)?;
                        let reference = if endpoint.cap.is_some() {
                            let uv = terminating_surface
                                .project(contact_points[endpoint_index][0], tolerance)?
                                .parameters;
                            Point2::new(uv.0, uv.1)
                        } else {
                            terminating_use
                                .pcurve
                                .evaluate(if traversal[0] == endpoint.old_vertex {
                                    terminating_use.range.start
                                } else {
                                    terminating_use.range.end
                                })?
                                .point
                        };
                        self.prepare_end_boundary(
                            &blend_surface,
                            terminating_surface,
                            blend_pcurve,
                            contact_points[endpoint_index],
                            reference,
                        )
                    }
                    BlendPatchEnd::Junction(vertex) => {
                        if kind == EdgeBlendKind::Chamfer
                            && matches!(junctions[vertex].kind, BlendJunctionKind::Trihedral)
                        {
                            let surface = junction_surfaces[vertex].clone();
                            let uv = surface
                                .project(contact_points[endpoint_index][0], tolerance)?
                                .parameters;
                            return self.prepare_end_boundary(
                                &blend_surface,
                                surface,
                                blend_pcurve,
                                contact_points[endpoint_index],
                                Point2::new(uv.0, uv.1),
                            );
                        }
                        let curve: Curve3 = SurfaceCurve3::new(
                            blend_surface.clone(),
                            blend_pcurve.clone(),
                            unit,
                            tolerance,
                        )?
                        .into();
                        let Some(junction_surface) = junction_surfaces.get(vertex) else {
                            return Ok(BlendBoundary {
                                curve,
                                parent_pcurve: blend_pcurve.clone(),
                                blend_pcurve,
                                range: unit,
                            });
                        };
                        if let BlendJunctionKind::RimCorner { carrier_face, .. } =
                            junctions[vertex].kind
                        {
                            let carrier_index = spine
                                .faces
                                .iter()
                                .position(|face| *face == carrier_face)
                                .ok_or_else(|| {
                                    KernelError::new(
                                        DiagnosticKind::TopologyInvariant,
                                        "inner-rim patch lost its carrier face",
                                    )
                                })?;
                            let top = contact_points[endpoint_index][carrier_index];
                            let angle = junction_surface.project(top, tolerance)?.parameters.0;
                            let start_v = if carrier_index == 0 {
                                std::f64::consts::FRAC_PI_2
                            } else {
                                std::f64::consts::PI
                            };
                            let end_v = if carrier_index == 0 {
                                std::f64::consts::PI
                            } else {
                                std::f64::consts::FRAC_PI_2
                            };
                            let parent_pcurve: Curve2 = Line2::new(
                                Point2::new(angle, start_v),
                                Vec2::new(0.0, end_v - start_v),
                            )?
                            .into();
                            return Ok(BlendBoundary {
                                curve,
                                parent_pcurve,
                                blend_pcurve,
                                range: unit,
                            });
                        }
                        let mut parameters: Vec<Point2> = Vec::with_capacity(17);
                        for index in 0..=16 {
                            let parameter = index as f64 / 16.0;
                            let projection = junction_surface
                                .project(curve.evaluate(parameter)?.point, tolerance)?;
                            let mut uv =
                                Point2::new(projection.parameters.0, projection.parameters.1);
                            if let Some(previous) = parameters.last().copied() {
                                if let Some(period) = junction_surface.domains()[0].period {
                                    uv.x += ((previous.x - uv.x) / period).round() * period;
                                }
                            }
                            parameters.push(uv);
                        }
                        let guide: Curve2 =
                            super::intersection::smooth_nurbs_guide(&parameters, unit)?.into();
                        let parent_pcurve: Curve2 = SurfaceProjectionPcurve2::new(
                            curve.clone(),
                            junction_surface.clone(),
                            guide,
                            unit,
                            tolerance,
                        )?
                        .into();
                        Ok(BlendBoundary {
                            curve,
                            parent_pcurve,
                            blend_pcurve,
                            range: unit,
                        })
                    }
                }
            })
            .collect::<KernelResult<Vec<_>>>()?
            .try_into()
            .map_err(|_| {
                KernelError::new(DiagnosticKind::TopologyInvariant, "two ends expected")
            })?;
        Ok(BlendPatch {
            spine,
            ends: ends.try_into().map_err(|_| {
                KernelError::new(DiagnosticKind::TopologyInvariant, "two patch ends expected")
            })?,
            contact_parameters,
            contact_points,
            contact_boundaries,
            end_boundaries,
            blend_surface,
            blend_parameterization,
        })
    }

    /// Certify that each new contact curve remains in the trimmed domain of
    /// its planar parent face. Crossing another loop would make the local
    /// Euler rewrite publish a self-overlapping shell instead of a blend.
    fn certify_blend_contact_trims(&self, patches: &[BlendPatch]) -> KernelResult<()> {
        let tolerance = self.kernel.tolerance;
        for patch in patches {
            for parent_index in 0..2 {
                let face_id = patch.spine.faces[parent_index];
                if !matches!(self.working.face(face_id)?.surface, Surface3::Plane(_)) {
                    continue;
                }
                let boundary = &patch.contact_boundaries[parent_index];
                let contact = TrimmedCurve2::new(boundary.parent_pcurve.clone(), boundary.range)?;
                let start = contact.curve().evaluate(contact.range().start)?.point;
                let end = contact.curve().evaluate(contact.range().end)?.point;
                let closed = start.distance(end) <= tolerance.linear;
                let parameter_scale = contact
                    .range()
                    .start
                    .abs()
                    .max(contact.range().end.abs())
                    .max(1.0);
                let endpoint_tolerance = tolerance.parameter * parameter_scale * 16.0;
                let at_contact_endpoint = |parameter: f64| {
                    (parameter - contact.range().start).abs() <= endpoint_tolerance
                        || (parameter - contact.range().end).abs() <= endpoint_tolerance
                };

                let mut outer_contains_middle = false;
                let mut inside_existing_hole = false;
                let mut crosses_locally_rewritten_boundary = false;
                let middle = contact
                    .curve()
                    .evaluate((contact.range().start + contact.range().end) * 0.5)?
                    .point;
                let locally_rewritten_uses = patch
                    .ends
                    .iter()
                    .map(|end| match end {
                        BlendPatchEnd::Boundary(endpoint) => {
                            Ok(endpoint.adjacent_uses[parent_index])
                        }
                        BlendPatchEnd::Junction(vertex) => {
                            self.adjacent_use_at_vertex(patch.spine.uses[parent_index], *vertex)
                        }
                    })
                    .collect::<KernelResult<BTreeSet<_>>>()?;
                for loop_id in self.working.face(face_id)?.loops.clone() {
                    let identified_segments = self
                        .collect_loop(loop_id)?
                        .into_iter()
                        .filter(|coedge_id| {
                            self.working
                                .coedge(*coedge_id)
                                .is_ok_and(|coedge| coedge.range.span() != 0.0)
                        })
                        .map(|coedge_id| {
                            let coedge = self.working.coedge(coedge_id)?;
                            Ok((
                                coedge_id,
                                TrimmedCurve2::new(coedge.pcurve.clone(), coedge.range)?,
                            ))
                        })
                        .collect::<KernelResult<Vec<_>>>()?;
                    for (coedge_id, segment) in &identified_segments {
                        let intersection = curve_pair_intersection(&contact, segment, tolerance)?;
                        if locally_rewritten_uses.contains(coedge_id)
                            && (intersection.coincident || !intersection.hits.is_empty())
                        {
                            crosses_locally_rewritten_boundary = true;
                        }
                        if !locally_rewritten_uses.contains(coedge_id)
                            && (intersection.coincident
                                || intersection
                                    .hits
                                    .iter()
                                    .any(|hit| closed || !at_contact_endpoint(hit.first_parameter)))
                        {
                            return Err(KernelError::new(
                                DiagnosticKind::GeometryInvariant,
                                "blend contact boundary crosses an existing face boundary",
                            ));
                        }
                    }
                    let segments = identified_segments
                        .into_iter()
                        .map(|(_, segment)| segment)
                        .collect::<Vec<_>>();
                    let contains = curve_loop_contains_point(&segments, middle, tolerance)?;
                    match self.working.loop_(loop_id)?.kind {
                        super::LoopKind::Outer => outer_contains_middle |= contains,
                        super::LoopKind::Inner if contains => inside_existing_hole = true,
                        super::LoopKind::Inner => {}
                    }
                }
                if closed && inside_existing_hole && !crosses_locally_rewritten_boundary {
                    return Err(KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "blend contact boundary enters an existing face hole",
                    ));
                }
                if closed && !outer_contains_middle && !crosses_locally_rewritten_boundary {
                    return Err(KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "blend contact boundary leaves its incident face trim",
                    ));
                }
            }
        }
        Ok(())
    }

    fn contact_vertex_key(
        patch: &BlendPatch,
        endpoint_index: usize,
        parent_index: usize,
        junctions: &BTreeMap<VertexId, BlendJunction>,
    ) -> (VertexId, FaceId, Option<EdgeId>) {
        let vertex = patch.spine.edge.vertices[endpoint_index];
        let face = patch.spine.faces[parent_index];
        match junctions.get(&vertex).map(|junction| &junction.kind) {
            Some(BlendJunctionKind::RimCorner { carrier_face, .. }) if face == *carrier_face => {
                (vertex, face, Some(patch.spine.edge_id))
            }
            Some(BlendJunctionKind::RimCorner { carrier_face, .. }) => {
                (vertex, *carrier_face, None)
            }
            Some(BlendJunctionKind::Smooth { tangent_faces, .. })
            | Some(BlendJunctionKind::Miter {
                wall_faces: tangent_faces,
                ..
            }) if tangent_faces.contains(&face) => (vertex, tangent_faces[0], None),
            Some(BlendJunctionKind::Smooth {
                second_seam: Some((_, pair)),
                ..
            }) if pair.contains(&face) => (vertex, pair[0], None),
            _ => (vertex, face, None),
        }
    }

    fn loop_parameter_area(&self, loop_id: super::LoopId) -> KernelResult<f64> {
        let uses = self.collect_loop(loop_id)?;
        let mut area = 0.0;
        for (index, use_id) in uses.iter().enumerate() {
            let use_value = self.working.coedge(*use_id)?;
            area += use_value
                .pcurve
                .signed_area_contribution(use_value.range, self.kernel.tolerance.parameter)?;
            let next = self.working.coedge(uses[(index + 1) % uses.len()])?;
            let end = use_value.pcurve.evaluate(use_value.range.end)?.point;
            let start = next.pcurve.evaluate(next.range.start)?.point;
            area += 0.5 * (end.x * start.y - start.x * end.y);
        }
        Ok(area)
    }

    fn rewrite_blend_network(
        &mut self,
        patches: &[BlendPatch],
        junctions: &BTreeMap<VertexId, BlendJunction>,
        junction_surfaces: &BTreeMap<VertexId, Surface3>,
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        let tolerance = self.kernel.tolerance;
        let first_patch = patches.first().ok_or_else(|| {
            KernelError::new(DiagnosticKind::InvalidInput, "blend network is empty")
        })?;
        let shell = self.working.face(first_patch.spine.faces[0])?.shell;
        for patch in patches {
            for face in patch.spine.faces {
                if self.working.face(face)?.shell != shell {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "blend network crosses shell boundaries",
                    ));
                }
            }
        }

        // Capture every external loop gap before any endpoint changes.
        let mut termination_gaps = BTreeMap::new();
        for patch in patches {
            for endpoint_index in 0..2 {
                let BlendPatchEnd::Boundary(endpoint) = &patch.ends[endpoint_index] else {
                    continue;
                };
                let mut uses = Vec::with_capacity(2);
                for (parent, adjacent) in endpoint.adjacent_uses.into_iter().enumerate() {
                    let adjacent_edge = match endpoint.tangent_termination {
                        Some((seam, index)) if index == parent => seam,
                        _ => self.working.coedge(adjacent)?.edge,
                    };
                    let terminating_use = self
                        .working
                        .edge(adjacent_edge)?
                        .coedges
                        .iter()
                        .copied()
                        .find(|coedge| {
                            self.working
                                .coedge(*coedge)
                                .and_then(|value| self.working.loop_(value.loop_id))
                                .map(|loop_value| loop_value.face == endpoint.terminating_face)
                                .unwrap_or(false)
                        })
                        .ok_or_else(|| {
                            KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "blend boundary rail has no terminating-face use",
                            )
                        })?;
                    uses.push(terminating_use);
                }
                let first = self.working.coedge_vertices(uses[0])?;
                let second = self.working.coedge_vertices(uses[1])?;
                let gap = if first[1] == endpoint.old_vertex && second[0] == endpoint.old_vertex {
                    (uses[0], uses[1], true)
                } else if second[1] == endpoint.old_vertex && first[0] == endpoint.old_vertex {
                    (uses[1], uses[0], false)
                } else {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "blend terminating-face uses do not form one vertex gap",
                    ));
                };
                if self.working.coedge(gap.0)?.next != gap.1 {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "blend terminating-face gap is not consecutive",
                    ));
                }
                termination_gaps.insert((patch.spine.edge.name, endpoint.old_vertex), gap);
            }
        }

        let mut contact_vertices = BTreeMap::<(VertexId, FaceId, Option<EdgeId>), VertexId>::new();
        for patch in patches {
            for endpoint_index in 0..2 {
                let old_vertex = patch.spine.edge.vertices[endpoint_index];
                for parent_index in 0..2 {
                    let face = patch.spine.faces[parent_index];
                    let key =
                        Self::contact_vertex_key(patch, endpoint_index, parent_index, junctions);
                    if contact_vertices.contains_key(&key) {
                        continue;
                    }
                    let point = patch.contact_points[endpoint_index][parent_index];
                    let name = self.names.next_for(&kind.role("contact.vertex"))?;
                    let vertex = self.make_vertex(name, point);
                    self.history.derived(
                        name,
                        [
                            self.working.vertex(old_vertex)?.name,
                            self.working.face(face)?.name,
                        ],
                        &kind.role("contact.vertex"),
                    );
                    contact_vertices.insert(key, vertex);
                }
            }
        }

        let mut trimmed = BTreeSet::new();
        for patch in patches {
            for endpoint_index in 0..2 {
                let BlendPatchEnd::Boundary(endpoint) = &patch.ends[endpoint_index] else {
                    continue;
                };
                if let Some((seam, parent)) = endpoint.tangent_termination {
                    let vertex = contact_vertices
                        [&Self::contact_vertex_key(patch, endpoint_index, parent, junctions)];
                    self.retarget_edge_endpoint_allow_extension(
                        seam,
                        endpoint.old_vertex,
                        vertex,
                        endpoint.contact_points[parent],
                    )?;
                }
                for parent_index in 0..2 {
                    if endpoint.cap.is_some_and(|(free, _)| free == parent_index) {
                        continue;
                    }
                    let adjacent_edge = self
                        .working
                        .coedge(endpoint.adjacent_uses[parent_index])?
                        .edge;
                    if !trimmed.insert((adjacent_edge, endpoint.old_vertex)) {
                        continue;
                    }
                    let contact_vertex = contact_vertices
                        [&Self::contact_vertex_key(patch, endpoint_index, parent_index, junctions)];
                    if endpoint.allow_adjacent_extension[parent_index] {
                        self.retarget_edge_endpoint_allow_extension(
                            adjacent_edge,
                            endpoint.old_vertex,
                            contact_vertex,
                            endpoint.contact_points[parent_index],
                        )?;
                    } else {
                        self.retarget_edge_endpoint(
                            adjacent_edge,
                            endpoint.old_vertex,
                            contact_vertex,
                            endpoint.contact_points[parent_index],
                        )?;
                    }
                }
            }
        }

        for (old_vertex, junction) in junctions {
            let (seam_edge, tangent_faces, second_seam) = match junction.kind {
                BlendJunctionKind::Smooth {
                    seam_edge,
                    tangent_faces,
                    second_seam,
                } => (seam_edge, tangent_faces, second_seam),
                BlendJunctionKind::Miter {
                    seam_edge,
                    wall_faces,
                } => (seam_edge, wall_faces, None),
                _ => continue,
            };
            let new_vertex = contact_vertices[&(*old_vertex, tangent_faces[0], None)];
            self.retarget_edge_endpoint(
                seam_edge,
                *old_vertex,
                new_vertex,
                junction.contact_points[&tangent_faces[0]],
            )?;
            if let Some((edge, pair)) = second_seam {
                self.retarget_edge_endpoint(
                    edge,
                    *old_vertex,
                    contact_vertices[&(*old_vertex, pair[0], None)],
                    junction.contact_points[&pair[0]],
                )?;
            }
        }
        let selected_edges = patches
            .iter()
            .map(|patch| patch.spine.edge_id)
            .collect::<BTreeSet<_>>();
        for (old_vertex, junction) in junctions {
            let BlendJunctionKind::TangentChain { tangent_faces } = junction.kind else {
                continue;
            };
            for seam_edge in self
                .working
                .vertex_edges(*old_vertex)?
                .into_iter()
                .filter(|edge| !selected_edges.contains(edge))
            {
                let seam_faces = self
                    .working
                    .edge_faces(seam_edge)?
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                if seam_faces.len() != 1 {
                    continue;
                }
                let seam_face = *seam_faces.first().ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "tangent-chain seam lost its support face",
                    )
                })?;
                if !tangent_faces.contains(&seam_face) {
                    continue;
                }
                self.retarget_edge_endpoint(
                    seam_edge,
                    *old_vertex,
                    contact_vertices[&(*old_vertex, seam_face, None)],
                    junction.contact_points[&seam_face],
                )?;
            }
        }
        for (old_vertex, junction) in junctions {
            let BlendJunctionKind::RimCorner {
                carrier_face,
                seam_edge,
            } = junction.kind
            else {
                continue;
            };
            let singular_key = (*old_vertex, carrier_face, None);
            let singular_vertex = contact_vertices[&singular_key];
            let singular_point = self.working.vertex(singular_vertex)?.point;
            self.retarget_edge_endpoint(seam_edge, *old_vertex, singular_vertex, singular_point)?;
        }

        let mut junction_faces = BTreeMap::new();
        let mut junction_loops = BTreeMap::new();
        let mut junction_uses = BTreeMap::<VertexId, Vec<CoedgeId>>::new();
        for (vertex, junction) in junctions {
            if !matches!(
                junction.kind,
                BlendJunctionKind::Trihedral | BlendJunctionKind::RimCorner { .. }
            ) {
                continue;
            }
            let surface = junction_surfaces[vertex].clone();
            let face_name = self.names.next_for(&kind.role("vertex.face"))?;
            let loop_name = self.names.next_for(&kind.role("vertex.loop"))?;
            let (face, loop_id) = self.make_face_loop(
                shell,
                face_name,
                loop_name,
                surface,
                FaceOrientation::Forward,
            )?;
            self.history.derived(
                face_name,
                junction
                    .faces
                    .iter()
                    .map(|face| self.working.face(*face).map(|value| value.name))
                    .collect::<KernelResult<Vec<_>>>()?,
                &kind.role("vertex.face"),
            );
            self.history
                .derived(loop_name, [face_name], &kind.role("vertex.loop"));
            junction_faces.insert(*vertex, face);
            junction_loops.insert(*vertex, loop_id);
            junction_uses.insert(*vertex, Vec::with_capacity(3));
        }

        let mut cap_loops = BTreeMap::new();
        let mut cap_uses = BTreeMap::<(EdgeId, usize), Vec<CoedgeId>>::new();
        for patch in patches {
            for index in 0..2 {
                if let BlendPatchEnd::Boundary(BlendEndpoint {
                    cap: Some((_, plane)),
                    ..
                }) = &patch.ends[index]
                {
                    let face_name = self.names.next_for(&kind.role("cap.face"))?;
                    let loop_name = self.names.next_for(&kind.role("cap.loop"))?;
                    let (_, loop_id) = self.make_face_loop(
                        shell,
                        face_name,
                        loop_name,
                        (*plane).into(),
                        FaceOrientation::Forward,
                    )?;
                    self.history.derived(
                        face_name,
                        [patch.spine.edge.name],
                        &kind.role("cap.face"),
                    );
                    self.history
                        .derived(loop_name, [face_name], &kind.role("cap.loop"));
                    cap_loops.insert((patch.spine.edge_id, index), loop_id);
                    cap_uses.insert((patch.spine.edge_id, index), Vec::new());
                }
            }
        }
        let mut blend_cycles = Vec::with_capacity(patches.len());
        let mut smooth_edges = BTreeMap::<VertexId, (EdgeId, Curve3)>::new();
        let mut contact_parent_uses = BTreeMap::<(EdgeId, usize), CoedgeId>::new();
        for patch in patches {
            let selected_name = patch.spine.edge.name;
            let blend_face_name = self.names.next_for(&kind.role("blend.face"))?;
            let blend_loop_name = self.names.next_for(&kind.role("blend.loop"))?;
            let (_blend_face, blend_loop) = self.make_face_loop(
                shell,
                blend_face_name,
                blend_loop_name,
                patch.blend_surface.clone(),
                FaceOrientation::Forward,
            )?;
            self.history.derived(
                blend_face_name,
                [
                    selected_name,
                    self.working.face(patch.spine.faces[0])?.name,
                    self.working.face(patch.spine.faces[1])?.name,
                ],
                &kind.role("blend.face"),
            );
            self.history
                .derived(blend_loop_name, [blend_face_name], &kind.role("blend.loop"));

            let mut blend_uses = Vec::with_capacity(4);
            for parent_index in 0..2 {
                let boundary = &patch.contact_boundaries[parent_index];
                let first_vertex =
                    contact_vertices[&Self::contact_vertex_key(patch, 0, parent_index, junctions)];
                let second_vertex =
                    contact_vertices[&Self::contact_vertex_key(patch, 1, parent_index, junctions)];
                let edge_name = self.names.next_for(&kind.role("contact.edge"))?;
                let parent_use_name = self.names.next_for(&kind.role("contact.parent-use"))?;
                let blend_use_name = self.names.next_for(&kind.role("contact.blend-use"))?;
                let edge = self.make_edge(
                    edge_name,
                    [first_vertex, second_vertex],
                    boundary.curve.clone(),
                    boundary.range,
                )?;
                let old_use = patch.spine.uses[parent_index];
                let parent_forward = self.working.coedge(old_use)?.forward;
                let parent_loop = self.working.coedge(old_use)?.loop_id;
                let parent_use = self.make_coedge(
                    parent_use_name,
                    edge,
                    parent_loop,
                    parent_forward,
                    boundary.parent_pcurve.clone(),
                    if parent_forward {
                        boundary.range
                    } else {
                        boundary.range.reversed()
                    },
                )?;
                let blend_forward = !parent_forward;
                let blend_use = self.make_coedge(
                    blend_use_name,
                    edge,
                    blend_loop,
                    blend_forward,
                    boundary.blend_pcurve.clone(),
                    if blend_forward {
                        boundary.range
                    } else {
                        boundary.range.reversed()
                    },
                )?;
                self.replace_loop_coedge(old_use, parent_use)?;
                contact_parent_uses.insert((patch.spine.edge_id, parent_index), parent_use);
                blend_uses.push(blend_use);
                self.history.derived(
                    edge_name,
                    [
                        selected_name,
                        self.working.face(patch.spine.faces[parent_index])?.name,
                    ],
                    &kind.role("contact.edge"),
                );
                self.history.derived(
                    parent_use_name,
                    [edge_name],
                    &kind.role("contact.parent-use"),
                );
                self.history
                    .derived(blend_use_name, [edge_name], &kind.role("contact.blend-use"));
            }

            for endpoint_index in 0..2 {
                let boundary = &patch.end_boundaries[endpoint_index];
                if let BlendPatchEnd::Junction(vertex) = &patch.ends[endpoint_index] {
                    if matches!(
                        junctions[vertex].kind,
                        BlendJunctionKind::Smooth { .. }
                            | BlendJunctionKind::TangentChain { .. }
                            | BlendJunctionKind::Miter { .. }
                    ) {
                        let vertices = [
                            contact_vertices
                                [&Self::contact_vertex_key(patch, endpoint_index, 0, junctions)],
                            contact_vertices
                                [&Self::contact_vertex_key(patch, endpoint_index, 1, junctions)],
                        ];
                        let contact_vertices_on_use =
                            self.working.coedge_vertices(blend_uses[0])?;
                        let desired_current_forward = contact_vertices_on_use[0] != vertices[0];
                        let unit = boundary.range;
                        let (edge, curve, same_direction, pcurve) = if let Some((edge, curve)) =
                            smooth_edges.get(vertex).cloned()
                        {
                            let edge_vertices = self.working.edge(edge)?.vertices;
                            let same_direction = if edge_vertices == vertices {
                                true
                            } else if edge_vertices == [vertices[1], vertices[0]] {
                                false
                            } else {
                                return Err(KernelError::new(
                                    DiagnosticKind::TopologyInvariant,
                                    "smooth blend transition patches do not share contact vertices",
                                ));
                            };
                            let first = patch.contact_parameters[endpoint_index][0];
                            let second = patch.contact_parameters[endpoint_index][1];
                            let guide = patch.blend_parameterization.oriented_end_pcurve(
                                first,
                                second,
                                same_direction,
                            )?;
                            let pcurve: Curve2 = SurfaceProjectionPcurve2::new(
                                curve.clone(),
                                patch.blend_surface.clone(),
                                guide,
                                unit,
                                tolerance,
                            )?
                            .into();
                            (edge, curve, same_direction, pcurve)
                        } else {
                            let edge_name = self.names.next_for(&kind.role("transition.edge"))?;
                            let edge =
                                self.make_edge(edge_name, vertices, boundary.curve.clone(), unit)?;
                            self.history.derived(
                                edge_name,
                                [selected_name],
                                &kind.role("transition.edge"),
                            );
                            smooth_edges.insert(*vertex, (edge, boundary.curve.clone()));
                            (
                                edge,
                                boundary.curve.clone(),
                                true,
                                boundary.blend_pcurve.clone(),
                            )
                        };
                        let _ = curve;
                        let blend_forward = if same_direction {
                            desired_current_forward
                        } else {
                            !desired_current_forward
                        };
                        let use_name = self.names.next_for(&kind.role("transition.blend-use"))?;
                        let blend_use = self.make_coedge(
                            use_name,
                            edge,
                            blend_loop,
                            blend_forward,
                            pcurve,
                            if blend_forward { unit } else { unit.reversed() },
                        )?;
                        blend_uses.push(blend_use);
                        self.history.derived(
                            use_name,
                            [self.working.edge(edge)?.name],
                            &kind.role("transition.blend-use"),
                        );
                        continue;
                    }
                }
                let vertices = [
                    contact_vertices
                        [&Self::contact_vertex_key(patch, endpoint_index, 0, junctions)],
                    contact_vertices
                        [&Self::contact_vertex_key(patch, endpoint_index, 1, junctions)],
                ];
                let edge_name = self.names.next_for(&kind.role("end.edge"))?;
                let other_use_name = self.names.next_for(&kind.role("end.other-use"))?;
                let blend_use_name = self.names.next_for(&kind.role("end.blend-use"))?;
                let edge =
                    self.make_edge(edge_name, vertices, boundary.curve.clone(), boundary.range)?;
                let (other_loop, other_forward) = match &patch.ends[endpoint_index] {
                    BlendPatchEnd::Boundary(endpoint) => {
                        let gap = termination_gaps[&(selected_name, endpoint.old_vertex)];
                        (
                            if endpoint.cap.is_some() {
                                cap_loops[&(patch.spine.edge_id, endpoint_index)]
                            } else {
                                self.working.coedge(gap.0)?.loop_id
                            },
                            gap.2,
                        )
                    }
                    BlendPatchEnd::Junction(vertex) => (junction_loops[vertex], true),
                };
                let other_use = self.make_coedge(
                    other_use_name,
                    edge,
                    other_loop,
                    other_forward,
                    boundary.parent_pcurve.clone(),
                    if other_forward {
                        boundary.range
                    } else {
                        boundary.range.reversed()
                    },
                )?;
                let blend_forward = !other_forward;
                let blend_use = self.make_coedge(
                    blend_use_name,
                    edge,
                    blend_loop,
                    blend_forward,
                    boundary.blend_pcurve.clone(),
                    if blend_forward {
                        boundary.range
                    } else {
                        boundary.range.reversed()
                    },
                )?;
                match &patch.ends[endpoint_index] {
                    BlendPatchEnd::Boundary(endpoint) => {
                        let gap = termination_gaps[&(selected_name, endpoint.old_vertex)];
                        if let Some((free, plane)) = endpoint.cap {
                            let cap_key = (patch.spine.edge_id, endpoint_index);
                            cap_uses.get_mut(&cap_key).unwrap().push(other_use);
                            for parent in 0..2 {
                                let contact_vertex = vertices[parent];
                                let original_vertex = endpoint.old_vertex;
                                let start = self.working.vertex(original_vertex)?.point;
                                let end = self.working.vertex(contact_vertex)?.point;
                                let target_loop = if parent == free {
                                    self.working
                                        .coedge(
                                            contact_parent_uses[&(patch.spine.edge_id, parent)],
                                        )?
                                        .loop_id
                                } else {
                                    self.working.coedge(gap.0)?.loop_id
                                };
                                let surface = self
                                    .working
                                    .face(self.working.loop_(target_loop)?.face)?
                                    .surface
                                    .clone();
                                let curve: Curve3 = super::Line3::new(start, end - start)?.into();
                                let range = ParameterRange::new(0.0, 1.0)?;
                                let pcurve = |support: &Surface3| -> KernelResult<Curve2> {
                                    let a = support.project(start, tolerance)?.parameters;
                                    let mut b = support.project(end, tolerance)?.parameters;
                                    if let Some(period) = support.domains()[0].period {
                                        b.0 += ((a.0 - b.0) / period).round() * period;
                                    }
                                    Line2::new(
                                        Point2::new(a.0, a.1),
                                        Vec2::new(b.0 - a.0, b.1 - a.1),
                                    )
                                    .map(Curve2::from)
                                };
                                let name = self.names.next_for(&kind.role("cap.edge"))?;
                                let cap_edge = self.make_edge(
                                    name,
                                    [original_vertex, contact_vertex],
                                    curve,
                                    range,
                                )?;
                                let (previous, next) = if parent == free {
                                    let rail = contact_parent_uses[&(patch.spine.edge_id, parent)];
                                    let rail_vertices = self.working.coedge_vertices(rail)?;
                                    if rail_vertices[0] == contact_vertex {
                                        (self.working.coedge(rail)?.previous, rail)
                                    } else {
                                        (rail, self.working.coedge(rail)?.next)
                                    }
                                } else {
                                    (gap.0, gap.1)
                                };
                                let forward =
                                    self.working.coedge_vertices(previous)?[1] == original_vertex;
                                let use_name = self.names.next_for(&kind.role("cap.parent-use"))?;
                                let use_id = self.make_coedge(
                                    use_name,
                                    cap_edge,
                                    target_loop,
                                    forward,
                                    pcurve(&surface)?,
                                    if forward { range } else { range.reversed() },
                                )?;
                                self.insert_loop_coedge(previous, next, use_id)?;
                                let cap_name = self.names.next_for(&kind.role("cap.use"))?;
                                let cap_use = self.make_coedge(
                                    cap_name,
                                    cap_edge,
                                    cap_loops[&cap_key],
                                    !forward,
                                    pcurve(&Surface3::from(plane))?,
                                    if forward { range.reversed() } else { range },
                                )?;
                                cap_uses.get_mut(&cap_key).unwrap().push(cap_use);
                                self.history
                                    .derived(name, [selected_name], &kind.role("cap.edge"));
                                self.history.derived(
                                    use_name,
                                    [name],
                                    &kind.role("cap.parent-use"),
                                );
                                self.history
                                    .derived(cap_name, [name], &kind.role("cap.use"));
                            }
                        } else {
                            self.insert_loop_coedge(gap.0, gap.1, other_use)?;
                        }
                    }
                    BlendPatchEnd::Junction(vertex) => {
                        junction_uses.get_mut(vertex).unwrap().push(other_use);
                    }
                }
                blend_uses.push(blend_use);
                self.history
                    .derived(edge_name, [selected_name], &kind.role("end.edge"));
                self.history
                    .derived(other_use_name, [edge_name], &kind.role("end.other-use"));
                self.history
                    .derived(blend_use_name, [edge_name], &kind.role("end.blend-use"));
            }
            blend_cycles.push((blend_loop, blend_uses));
        }

        for (vertex, (edge, _)) in &smooth_edges {
            if self.working.edge(*edge)?.coedges.len() != 2 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    format!(
                        "smooth blend junction {:?} is not shared by exactly two rolling patches",
                        vertex
                    ),
                ));
            }
        }
        for uses in junction_uses.values() {
            for junction_use in uses {
                let edge = self.working.coedge(*junction_use)?.edge;
                let blend_use = self
                    .working
                    .edge(edge)?
                    .coedges
                    .iter()
                    .copied()
                    .find(|candidate| candidate != junction_use)
                    .ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::TopologyInvariant,
                            "blend junction boundary has no paired rolling-patch use",
                        )
                    })?;
                let blend_loop = self.working.coedge(blend_use)?.loop_id;
                let cycle = blend_cycles
                    .iter()
                    .find_map(|(loop_id, uses)| (*loop_id == blend_loop).then_some(uses))
                    .ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::TopologyInvariant,
                            "blend junction boundary lost its rolling-patch cycle",
                        )
                    })?;
                self.orient_closing_coedge(cycle, blend_use)?;
                let blend_forward = self.working.coedge(blend_use)?.forward;
                self.orient_coedge(*junction_use, !blend_forward)?;
            }
        }
        for (vertex, junction) in junctions {
            let BlendJunctionKind::RimCorner { carrier_face, .. } = junction.kind else {
                continue;
            };
            let incidents = patches
                .iter()
                .flat_map(|patch| {
                    (0..2).filter_map(move |endpoint_index| {
                        (patch.spine.edge.vertices[endpoint_index] == *vertex)
                            .then_some((patch, endpoint_index))
                    })
                })
                .collect::<Vec<_>>();
            if incidents.len() != 2 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "an inner-rim corner must join exactly two rolling patches",
                ));
            }
            let carrier_uses = incidents
                .iter()
                .map(|(patch, _)| {
                    let parent_index = patch
                        .spine
                        .faces
                        .iter()
                        .position(|face| *face == carrier_face)
                        .ok_or_else(|| {
                            KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "inner-rim patch lost its carrier face",
                            )
                        })?;
                    Ok(contact_parent_uses[&(patch.spine.edge_id, parent_index)])
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let (previous, next) = if self.working.coedge(carrier_uses[0])?.next == carrier_uses[1]
            {
                (carrier_uses[0], carrier_uses[1])
            } else if self.working.coedge(carrier_uses[1])?.next == carrier_uses[0] {
                (carrier_uses[1], carrier_uses[0])
            } else {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "inner-rim carrier uses are not adjacent at their corner",
                ));
            };
            let start_vertex = self.working.coedge_vertices(previous)?[1];
            let end_vertex = self.working.coedge_vertices(next)?[0];
            let start = self.working.vertex(start_vertex)?.point;
            let end = self.working.vertex(end_vertex)?.point;
            let center = self.working.vertex(*vertex)?.point;
            let axis = match &junction_surfaces[vertex] {
                Surface3::Revolved(surface) => surface.axis().direction().normalize(),
                _ => {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "inner-rim junction lost its revolved surface",
                    ))
                }
            };
            let x_radius = start - center;
            let radius = x_radius.magnitude();
            let y_radius = axis.cross(x_radius).normalize() * radius;
            let x_unit = x_radius / radius;
            let y_unit = y_radius / radius;
            let delta = (end - center).dot(y_unit).atan2((end - center).dot(x_unit));
            let range = ParameterRange::new(0.0, delta)?;
            let curve: Curve3 = Circle3::new(center, x_radius, y_radius, tolerance)?.into();
            let projection_guide = |surface: &Surface3| -> KernelResult<Curve2> {
                let first = surface.project(start, tolerance)?.parameters;
                let mut second = surface.project(end, tolerance)?.parameters;
                if let Some(period) = surface.domains()[0].period {
                    second.0 += ((first.0 - second.0) / period).round() * period;
                }
                Line2::new(
                    Point2::new(first.0, first.1),
                    Vec2::new(second.0 - first.0, second.1 - first.1) / range.span(),
                )
                .map(Curve2::from)
            };
            let carrier_surface = self.working.face(carrier_face)?.surface.clone();
            let carrier_pcurve: Curve2 = SurfaceProjectionPcurve2::new(
                curve.clone(),
                carrier_surface.clone(),
                projection_guide(&carrier_surface)?,
                range,
                tolerance,
            )?
            .into();
            let junction_pcurve: Curve2 = SurfaceProjectionPcurve2::new(
                curve.clone(),
                junction_surfaces[vertex].clone(),
                projection_guide(&junction_surfaces[vertex])?,
                range,
                tolerance,
            )?
            .into();
            let edge_name = self.names.next_for(&kind.role("rim-corner.contact.edge"))?;
            let carrier_use_name = self.names.next_for(&kind.role("rim-corner.carrier-use"))?;
            let junction_use_name = self.names.next_for(&kind.role("rim-corner.junction-use"))?;
            let edge = self.make_edge(edge_name, [start_vertex, end_vertex], curve, range)?;
            let carrier_use = self.make_coedge(
                carrier_use_name,
                edge,
                self.working.coedge(previous)?.loop_id,
                true,
                carrier_pcurve,
                range,
            )?;
            let junction_use = self.make_coedge(
                junction_use_name,
                edge,
                junction_loops[vertex],
                true,
                junction_pcurve,
                range,
            )?;
            self.insert_loop_coedge(previous, next, carrier_use)?;
            junction_uses.get_mut(vertex).unwrap().push(junction_use);
            self.orient_closing_coedge(&junction_uses[vertex], junction_use)?;
            self.history.derived(
                edge_name,
                [self.working.face(carrier_face)?.name],
                &kind.role("rim-corner.contact.edge"),
            );
            self.history.derived(
                carrier_use_name,
                [edge_name],
                &kind.role("rim-corner.carrier-use"),
            );
            self.history.derived(
                junction_use_name,
                [edge_name],
                &kind.role("rim-corner.junction-use"),
            );
        }
        for (key, uses) in cap_uses {
            blend_cycles.push((cap_loops[&key], uses));
        }
        for (loop_id, uses) in blend_cycles {
            self.close_loop_from_coedges(loop_id, &uses)
                .map_err(|error| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        format!("rolling-patch boundary cycle failed: {error}"),
                    )
                })?;
            let area = uses.iter().try_fold(0.0, |sum, use_id| {
                let use_value = self.working.coedge(*use_id)?;
                Ok::<_, KernelError>(
                    sum + use_value
                        .pcurve
                        .signed_area_contribution(use_value.range, tolerance.parameter)?,
                )
            })?;
            let face = self.working.loop_(loop_id)?.face;
            self.orient_face(
                face,
                if area >= 0.0 {
                    FaceOrientation::Forward
                } else {
                    FaceOrientation::Reversed
                },
            )?;
        }
        for (vertex, uses) in &junction_uses {
            if uses.len() != 3 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "a regular blend junction must have three boundary patches",
                ));
            }
            self.close_loop_from_coedges(junction_loops[vertex], uses)
                .map_err(|error| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        format!("blend junction boundary cycle failed: {error}"),
                    )
                })?;
            let area = self.loop_parameter_area(junction_loops[vertex])?;
            self.orient_face(
                junction_faces[vertex],
                if area >= 0.0 {
                    FaceOrientation::Forward
                } else {
                    FaceOrientation::Reversed
                },
            )?;
        }

        let mut old_vertices = BTreeSet::new();
        let mut modified_faces = BTreeSet::new();
        for patch in patches {
            old_vertices.extend(patch.spine.edge.vertices);
            modified_faces.extend(patch.spine.faces);
            for end in &patch.ends {
                if let BlendPatchEnd::Boundary(endpoint) = end {
                    modified_faces.insert(endpoint.terminating_face);
                    if let Some((seam, _)) = endpoint.tangent_termination {
                        modified_faces.extend(self.working.edge_faces(seam)?);
                    }
                }
            }
            self.delete_edge_and_uses(patch.spine.edge_id, &patch.spine.uses)?;
        }
        for old_vertex in old_vertices {
            if patches.iter().any(|patch| patch.ends.iter().any(|end|
                matches!(end, BlendPatchEnd::Boundary(endpoint) if endpoint.old_vertex == old_vertex && endpoint.cap.is_some()))) {
                self.repair_outgoing(old_vertex)?;
                continue;
            }
            let outputs = contact_vertices
                .iter()
                .filter_map(|((vertex, _, _), replacement)| {
                    (*vertex == old_vertex).then_some(*replacement)
                })
                .map(|vertex| self.working.vertex(vertex).map(|value| value.name))
                .collect::<KernelResult<Vec<_>>>()?;
            self.delete_split_vertex(old_vertex, outputs)?;
        }
        for vertex in contact_vertices.values().copied() {
            self.repair_outgoing(vertex)?;
        }

        // Closed periodic edges are split into arcs while the rolling network
        // is constructed so that its endpoint machinery always works on
        // ordinary vertices.  Those construction joins are not material
        // boundaries.  Collapse every join whose two patches use the exact
        // same support; the final join then remains as the single
        // same-face parameter seam required by the periodic surface.
        // A general shared chart is bounded, so retain its original cut.
        // Merging across that cut would join different turns in UV space.
        let transition_edges = smooth_edges
            .iter()
            .filter(|(vertex, _)| {
                !patches
                    .iter()
                    .any(|patch| patch.spine.shared_seam == Some(**vertex))
            })
            .map(|(_, (edge, _))| *edge)
            .collect::<Vec<_>>();
        for edge in transition_edges {
            let faces = self.working.edge_faces(edge)?;
            if faces.len() != 2 || faces[0] == faces[1] {
                continue;
            }
            let first_surface = self.working.face(faces[0])?.surface.clone();
            let second_surface = self.working.face(faces[1])?.surface.clone();
            if first_surface == second_surface {
                self.kef(edge, faces[1])?;
            } else if let (Surface3::Cone(first), Surface3::Cone(second)) =
                (&first_surface, &second_surface)
            {
                // Independently constructed chamfer patches can describe the
                // same cone chart with slightly different rounded coefficients.
                // Compare the chart, not just its cone angle: existing p-curves
                // must remain valid on the surviving support without remapping.
                let tolerance = self.kernel.tolerance;
                let same_vector = |a: Vec3, b: Vec3| {
                    (a - b).magnitude()
                        <= tolerance.relative.max(f64::EPSILON * 64.0)
                            * a.magnitude().max(b.magnitude()).max(1.0)
                };
                if first.apex().distance(second.apex()) <= tolerance.linear
                    && same_vector(first.axis(), second.axis())
                    && same_vector(first.x_radius(), second.x_radius())
                    && same_vector(first.y_radius(), second.y_radius())
                    && self.working.face(faces[0])?.orientation
                        == self.working.face(faces[1])?.orientation
                {
                    // KEF validates the combined boundary on the survivor and
                    // rolls back atomically if the charts are not interchangeable
                    // over this face's actual extent.
                    let _ = self.kef(edge, faces[1]);
                }
            }
        }
        for face in modified_faces {
            let name = self.working.face(face)?.name;
            self.history.changed(name, ChangeKind::Modified, [name]);
        }
        let body = self.working.shell(shell)?.body;
        if !self.touched_bodies.contains(&body) {
            self.touched_bodies.push(body);
        }
        self.composes_imported_topology = true;
        Ok(())
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
            Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "selected coedge is not incident to its blend endpoint",
            ))
        }
    }

    fn local_offset_intersection(
        &self,
        uses: [CoedgeId; 2],
        offsets: &[Surface3; 2],
        tolerance: ModelTolerance,
        cylindrical_chart: bool,
    ) -> KernelResult<SurfaceSurfaceCurve> {
        let mut boundary_parameters: [Vec<Point2>; 2] =
            [Vec::with_capacity(17), Vec::with_capacity(17)];
        for index in 0..=16 {
            let fraction = index as f64 / 16.0;
            for side in 0..2 {
                let coedge = self.working.coedge(uses[side])?;
                let traversal_fraction = if coedge.forward {
                    fraction
                } else {
                    1.0 - fraction
                };
                // Internal transition connectors have zero source length;
                // their exact supporting intersection still supplies a guide.
                let range = if coedge.range.span() == 0.0 {
                    match coedge.pcurve.domain().bounds {
                        DomainBounds::Bounded(range) => {
                            if coedge.forward {
                                range
                            } else {
                                range.reversed()
                            }
                        }
                        _ => coedge.range,
                    }
                } else {
                    coedge.range
                };
                let pcurve_parameter = range.start + range.span() * traversal_fraction;
                let mut uv = coedge.pcurve.evaluate(pcurve_parameter)?.point;
                if let Some(previous) = boundary_parameters[side].last().copied() {
                    let domains = offsets[side].domains();
                    if let Some(period) = domains[0].period {
                        uv.x += ((previous.x - uv.x) / period).round() * period;
                    }
                    if let Some(period) = domains[1].period {
                        uv.y += ((previous.y - uv.y) / period).round() * period;
                    }
                }
                boundary_parameters[side].push(uv);
            }
        }
        let source = self.working.edge(self.working.coedge(uses[0])?.edge)?;
        // Retain an angular chart through rounded supports. A least-norm
        // correction can slide across the section and choose another inverse
        // branch, especially where a cylindrical corner meets a torus.
        let angular_coordinate = offsets.iter().enumerate().find_map(|(side, surface)| {
            let axis = match surface {
                Surface3::Cylinder(_)
                    if cylindrical_chart
                        && source.vertices[0] != source.vertices[1]
                        && (boundary_parameters[side][16].x - boundary_parameters[side][0].x)
                            .abs()
                            < std::f64::consts::PI
                        && matches!(offsets[1 - side], Surface3::Torus(_)) =>
                {
                    0
                }
                Surface3::Offset(offset)
                    if matches!(offset.base(), Surface3::RollingBall(blend)
                    if matches!(blend.center_curve(), Curve3::Line(_))) =>
                {
                    1
                }
                _ => return None,
            };
            ((boundary_parameters[side][16][axis] - boundary_parameters[side][0][axis]).abs()
                > tolerance.parameter)
                .then_some((side, axis))
        });
        if let Some((side, 0)) = angular_coordinate {
            // The retained intersection's parameter speed can reverse near
            // its endpoint. Use the cylinder angle itself as the traversal
            // coordinate, rather than inheriting that reversal in a new
            // offset guide. The other coordinates are still solved exactly.
            let start = boundary_parameters[side][0].x;
            let end = boundary_parameters[side][16].x;
            for index in 0..=16 {
                boundary_parameters[side][index].x = start + (end - start) * index as f64 / 16.0;
            }
        }
        // Keep open curved-edge continuation local to the retained sector.
        // Extending an entire edge length can wrap around a curved
        // support and introduce another inverse branch into the same guide.
        let extension = if source.vertices[0] == source.vertices[1]
            || matches!(source.curve, Curve3::Line(_))
        {
            16
        } else {
            2
        };
        let closed = source.vertices[0] == source.vertices[1];
        let extrapolated = |side: usize, index: i32| -> KernelResult<Point2> {
            if closed {
                let coedge = self.working.coedge(uses[side])?;
                let fraction = index as f64 / 16.0;
                let fraction = if coedge.forward {
                    fraction
                } else {
                    1.0 - fraction
                };
                return Ok(coedge
                    .pcurve
                    .evaluate(coedge.range.start + coedge.range.span() * fraction)?
                    .point);
            }
            Ok(if index < 0 {
                boundary_parameters[side][0]
                    + (boundary_parameters[side][1] - boundary_parameters[side][0]) * index as f64
            } else if index > 16 {
                boundary_parameters[side][16]
                    + (boundary_parameters[side][16] - boundary_parameters[side][15])
                        * (index - 16) as f64
            } else {
                boundary_parameters[side][index as usize]
            })
        };
        // Do not extend a guide past a bounded support. The local corrector
        // clamps those seeds to its natural domain, producing repeated points
        // and a stationary (or folded) spine outside the original edge.
        let in_domains = |index| -> KernelResult<bool> {
            for side in 0..2 {
                let uv = extrapolated(side, index)?;
                for axis in 0..2 {
                    if !offsets[side].domains()[axis].contains(uv[axis], tolerance.parameter) {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        };
        let mut first_index = 0;
        let mut last_index = 16;
        while first_index > -extension && in_domains(first_index - 1)? {
            first_index -= 1;
        }
        while last_index < 16 + extension && in_domains(last_index + 1)? {
            last_index += 1;
        }
        let support_range = ParameterRange::new(first_index as f64 / 16.0, last_index as f64 / 16.0)?;
        let mut parameters: [Vec<Point2>; 2] = [Vec::with_capacity(49), Vec::with_capacity(49)];
        for index in first_index..=last_index {
            let first = extrapolated(0, index)?;
            let second = extrapolated(1, index)?;
            let corrected = if let Some((side, axis)) = angular_coordinate {
                let guide = [first.x, first.y, second.x, second.y];
                let mut gauge = [0.0; 4];
                gauge[side * 2 + axis] = 1.0;
                let domains = |side: usize| {
                    std::array::from_fn(|axis| {
                        let domain = offsets[side].domains()[axis];
                        if let Some(period) = domain.period {
                            return ParameterRange::new(
                                guide[side * 2 + axis] - period,
                                guide[side * 2 + axis] + period,
                            )
                            .unwrap();
                        }
                        match domain.bounds {
                            DomainBounds::Bounded(range) => range,
                            _ => ParameterRange::new(-f64::MAX / 4.0, f64::MAX / 4.0).unwrap(),
                        }
                    })
                };
                let corrected = super::intersection::correct_surface_pair_with_gauge(
                    &offsets[0],
                    &offsets[1],
                    guide,
                    gauge,
                    domains(0),
                    domains(1),
                    tolerance,
                )?;
                if corrected.is_none() && axis == 0 {
                    // Some torus/cylinder intersections turn in the angular
                    // coordinate. Rebuild the whole guide in the general
                    // chart; mixing charts sample by sample creates folds.
                    return self.local_offset_intersection(uses, offsets, tolerance, false);
                }
                corrected.ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::NumericalFailure,
                        "fixed angular offset correction failed",
                    )
                })?
            } else {
                super::intersection::correct_surface_pair_near(
                    &offsets[0],
                    &offsets[1],
                    [first.x, first.y, second.x, second.y],
                    tolerance,
                )?
            };
            // Correction normalizes periodic coordinates. Lift them back onto
            // the sampled branch before interpolating a continuous guide.
            for side in 0..2 {
                let seed = if side == 0 { first } else { second };
                let mut uv = Point2::new(corrected[side * 2], corrected[side * 2 + 1]);
                let domains = offsets[side].domains();
                if let Some(period) = domains[0].period {
                    uv.x += ((seed.x - uv.x) / period).round() * period;
                }
                if let Some(period) = domains[1].period {
                    uv.y += ((seed.y - uv.y) / period).round() * period;
                }
                parameters[side].push(uv);
            }
        }
        let first_pcurve: Curve2 =
            super::intersection::smooth_nurbs_guide(&parameters[0], support_range)?.into();
        let second_pcurve: Curve2 =
            super::intersection::smooth_nurbs_guide(&parameters[1], support_range)?.into();
        let implicit = SurfaceIntersectionCurve::new(
            offsets[0].clone(),
            offsets[1].clone(),
            first_pcurve.clone(),
            second_pcurve.clone(),
            support_range,
            tolerance,
            tolerance.linear,
        )
        .map_err(|error| {
            KernelError::new(
                DiagnosticKind::NumericalFailure,
                format!("local blend offset intersection failed: {error}"),
            )
        })?;
        let exact_pcurves = implicit.exact_pcurves();
        Ok(SurfaceSurfaceCurve {
            curve: implicit.into(),
            first_pcurve: exact_pcurves[0].clone(),
            second_pcurve: exact_pcurves[1].clone(),
            achieved_error: tolerance.linear,
        })
    }
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
    if let Some(period) = domain.period {
        return Ok(parameter + ((reference - parameter) / period).round() * period);
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
