use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3};

use super::euler::EulerTransaction;
use super::history::ChangeKind;
use super::topology::Edge;
use super::validation::{validate, ValidationMode};
use super::{
    Circle2, CoedgeId, Curve2, Curve3, Cylinder3, DiagnosticKind, DomainBounds, EdgeId, FaceId,
    FaceOrientation, Frame3, IntersectionSet, KernelError, KernelResult, Line2, ModelTolerance,
    ParameterRange, RevolvedSurface3, RollingBallSurface3, Sphere3, Surface3, SurfaceCurve3,
    SurfaceIntersectionCurve, SurfaceProjectionPcurve2, SurfaceSurfaceCurve, Torus3, VertexId,
};

fn signed_periodic_delta(start: f64, end: f64) -> f64 {
    (end - start + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

#[derive(Debug, Clone)]
struct FilletEndpoint {
    old_vertex: VertexId,
    terminating_face: FaceId,
    adjacent_uses: [CoedgeId; 2],
    contact_parameters: [f64; 2],
    contact_points: [Point3; 2],
}

#[derive(Debug, Clone)]
struct FilletBoundary {
    curve: Curve3,
    parent_pcurve: Curve2,
    blend_pcurve: Curve2,
    range: ParameterRange,
}

#[derive(Debug, Clone)]
struct FilletSpine {
    edge_id: EdgeId,
    edge: Edge,
    uses: [CoedgeId; 2],
    faces: [FaceId; 2],
    parent_surfaces: [Surface3; 2],
    center: SurfaceSurfaceCurve,
    endpoint_seeds: [f64; 2],
    provisional_range: ParameterRange,
}

#[derive(Debug, Clone)]
struct FilletJunction {
    faces: Vec<FaceId>,
    center: Point3,
    contact_points: BTreeMap<FaceId, Point3>,
    kind: FilletJunctionKind,
}

#[derive(Debug, Clone)]
enum FilletJunctionKind {
    Smooth {
        seam_edge: EdgeId,
        tangent_faces: [FaceId; 2],
    },
    TangentChain {
        tangent_faces: [FaceId; 2],
    },
    Trihedral,
}

#[derive(Debug, Clone)]
enum FilletPatchEnd {
    Boundary(FilletEndpoint),
    Junction(VertexId),
}

#[derive(Debug, Clone)]
struct FilletPatch {
    spine: FilletSpine,
    ends: [FilletPatchEnd; 2],
    contact_parameters: [[f64; 2]; 2],
    contact_points: [[Point3; 2]; 2],
    contact_boundaries: [FilletBoundary; 2],
    end_boundaries: [FilletBoundary; 2],
    blend_surface: Surface3,
    blend_parameterization: FilletSurfaceParameterization,
}

/// Affine map from rolling coordinates `(center_parameter, blend_fraction)`
/// into a selected support surface's native parameters.
#[derive(Debug, Clone, Copy)]
struct FilletSurfaceParameterization {
    origin: Point2,
    center_direction: Vec2,
    blend_direction: Vec2,
}

impl FilletSurfaceParameterization {
    const ROLLING_BALL: Self = Self {
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
    pub(crate) fn fillet_edges(
        &mut self,
        edges: &[EdgeId],
        radius: f64,
        follow_tangent_edges: bool,
    ) -> KernelResult<()> {
        let snapshot = self.working.clone();
        let history_snapshot = self.history.clone();
        let names_snapshot = self.names.clone();
        let touched_snapshot = self.touched_bodies.clone();
        let result = self.fillet_network_unchecked(
            edges.iter().copied().collect(),
            radius,
            follow_tangent_edges,
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

    fn fillet_network_unchecked(
        &mut self,
        mut selected: BTreeSet<EdgeId>,
        radius: f64,
        follow_tangent_edges: bool,
    ) -> KernelResult<()> {
        let mut junction_kinds = BTreeMap::<VertexId, FilletJunctionKind>::new();
        let mut junction_sources = BTreeMap::<VertexId, EdgeId>::new();
        let mut spines = BTreeMap::<EdgeId, FilletSpine>::new();
        let mut regular_endpoints = BTreeMap::<(EdgeId, usize), Option<FilletEndpoint>>::new();
        loop {
            let missing = selected
                .iter()
                .copied()
                .filter(|edge| !spines.contains_key(edge))
                .collect::<Vec<_>>();
            for edge in missing {
                spines.insert(edge, self.prepare_fillet_spine(edge, radius)?);
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
                    let endpoint_requires_junction = if selected_degree > 1 {
                        true
                    } else {
                        regular_endpoints
                            .entry((spine.edge_id, endpoint_index))
                            .or_insert_with(|| {
                                self.prepare_fillet_endpoint(spine, endpoint_index).ok()
                            })
                            .is_none()
                    };
                    if follow_tangent_edges || endpoint_requires_junction {
                        if let Some((continuation, tangent_faces)) =
                            self.tangent_chain_continuation(spine, endpoint_index, &selected)?
                        {
                            junction_kinds
                                .insert(vertex, FilletJunctionKind::TangentChain { tangent_faces });
                            junction_sources.insert(vertex, spine.edge_id);
                            additions.insert(continuation);
                        } else if let Some((seam_edge, continuation, tangent_faces)) =
                            self.smooth_fillet_continuation(vertex, &selected)?
                        {
                            junction_kinds.insert(
                                vertex,
                                FilletJunctionKind::Smooth {
                                    seam_edge,
                                    tangent_faces,
                                },
                            );
                            junction_sources.insert(vertex, spine.edge_id);
                            additions.insert(continuation);
                        } else if endpoint_requires_junction {
                            junction_kinds.insert(vertex, FilletJunctionKind::Trihedral);
                            junction_sources.insert(vertex, spine.edge_id);
                            additions.extend(self.working.vertex_edges(vertex)?);
                        }
                    }
                }
            }
            let previous = selected.len();
            selected.extend(additions);
            if selected.len() == previous {
                let mut junctions = BTreeMap::new();
                for (vertex, kind) in &junction_kinds {
                    if let FilletJunctionKind::Smooth {
                        seam_edge,
                        tangent_faces,
                    } = kind
                    {
                        junctions.insert(
                            *vertex,
                            self.prepare_smooth_fillet_junction(
                                *vertex,
                                radius,
                                *seam_edge,
                                *tangent_faces,
                            )?,
                        );
                        continue;
                    }
                    let spine = spines.get(&junction_sources[vertex]).ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::TopologyInvariant,
                            "fillet junction source edge was not retained",
                        )
                    })?;
                    let endpoint_index = spine
                        .edge
                        .vertices
                        .iter()
                        .position(|candidate| candidate == vertex)
                        .ok_or_else(|| {
                            KernelError::new(
                                DiagnosticKind::TopologyInvariant,
                                "fillet network junction has no incident rolling patch",
                            )
                        })?;
                    if let FilletJunctionKind::TangentChain { tangent_faces } = kind {
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
                    junctions.insert(
                        *vertex,
                        self.prepare_fillet_junction(spine, endpoint_index, radius, kind.clone())?,
                    );
                }
                let sphere_surfaces = junctions
                    .iter()
                    .filter(|(_, junction)| matches!(junction.kind, FilletJunctionKind::Trihedral))
                    .map(|(vertex, junction)| {
                        let radials = junction
                            .faces
                            .iter()
                            .map(|face| {
                                (junction.contact_points[face] - junction.center).normalize()
                            })
                            .collect::<Vec<_>>();
                        let polar = (radials[0]
                            + radials[1] * std::f64::consts::SQRT_2
                            + radials[2] * std::f64::consts::PI)
                            .normalize();
                        let frame = Frame3::from_z(junction.center, polar)?;
                        Sphere3::new(
                            junction.center,
                            frame.x() * radius,
                            frame.y() * radius,
                            frame.z() * radius,
                            self.kernel.tolerance,
                        )
                        .map(|sphere| (*vertex, Surface3::from(sphere)))
                    })
                    .collect::<KernelResult<BTreeMap<_, _>>>()?;
                let patches = spines
                    .into_values()
                    .map(|spine| {
                        self.prepare_fillet_patch(
                            spine,
                            &junctions,
                            &sphere_surfaces,
                            &regular_endpoints,
                            radius,
                        )
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                return self.rewrite_fillet_network(&patches, &junctions, &sphere_surfaces);
            }
            if selected.len() > 256 {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "fillet transition propagation exceeded the connected regular edge network",
                ));
            }
        }
    }

    fn prepare_fillet_spine(&self, edge_id: EdgeId, radius: f64) -> KernelResult<FilletSpine> {
        let tolerance = self.kernel.tolerance;
        let edge = self.working.edge(edge_id)?.clone();
        if edge.coedges.len() != 2 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "fillet requires a manifold edge with two face uses",
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
                "fillet requires two distinct incident faces",
            ));
        }
        let parent_surfaces = [
            self.working.face(faces[0])?.surface.clone(),
            self.working.face(faces[1])?.surface.clone(),
        ];
        let inward = |face: FaceId| -> KernelResult<f64> {
            Ok(match self.working.face(face)?.orientation {
                FaceOrientation::Forward => -radius,
                FaceOrientation::Reversed => radius,
            })
        };
        let offsets = [
            parent_surfaces[0].offset(inward(faces[0])?, tolerance)?,
            parent_surfaces[1].offset(inward(faces[1])?, tolerance)?,
        ];
        let midpoint = edge
            .curve
            .evaluate((edge.range.start + edge.range.end) * 0.5)?
            .point;
        let candidates = match offsets[0].intersect_surface(&offsets[1], tolerance) {
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
                    "incident face offsets are coincident and do not define one fillet spine",
                ))
            }
            Err(_) => {
                vec![self.local_offset_intersection(uses, &offsets, tolerance)?]
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
                    "fillet could not select the local offset intersection branch",
                )
            })?;

        let endpoint_seeds = edge
            .vertices
            .iter()
            .map(|vertex| {
                center
                    .curve
                    .project(self.working.vertex(*vertex)?.point, tolerance)
                    .map(|projection| projection.parameter)
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
        Ok(FilletSpine {
            edge_id,
            edge,
            uses,
            faces,
            parent_surfaces,
            center,
            endpoint_seeds,
            provisional_range,
        })
    }

    fn prepare_fillet_endpoint(
        &self,
        spine: &FilletSpine,
        endpoint_index: usize,
    ) -> KernelResult<FilletEndpoint> {
        let tolerance = self.kernel.tolerance;
        let old_vertex = spine.edge.vertices[endpoint_index];
        let terminating_faces = self
            .working
            .vertex_faces(old_vertex)?
            .into_iter()
            .filter(|face| !spine.faces.contains(face))
            .collect::<Vec<_>>();
        if terminating_faces.len() != 1 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "a regular fillet endpoint must have one terminating face",
            ));
        }
        let terminating_face = terminating_faces[0];
        let mut adjacent_uses = [spine.uses[0]; 2];
        let mut contact_parameters = [0.0; 2];
        let mut contact_points = [Point3::new(0.0, 0.0, 0.0); 2];
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
            let adjacent_edge = self
                .working
                .edge(self.working.coedge(adjacent_uses[parent_index])?.edge)?;
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
                        "fillet endpoint contact search found no local seed",
                    )
                })?;
            let seed_point = contact_curve.evaluate(seed)?.point;
            let seed_allowed =
                tolerance.linear_at_scale(seed_point.to_vec().magnitude().max(1.0)) * 8.0;
            let (contact_parameter, contact_point, rail_parameter) = if rail_seed.distance
                <= seed_allowed
            {
                (
                    seed,
                    Point3::from_vec((seed_point.to_vec() + rail_seed.point.to_vec()) * 0.5),
                    rail_seed.parameter,
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
                                "fillet endpoint {endpoint_index} contact {parent_index} cannot terminate on its adjacent exact edge (nearest distance {}): {error}",
                                rail_seed.distance,
                            ),
                        )
                    })?;
                (hit.first_parameter, hit.point, hit.second_parameter)
            };
            if !adjacent_edge.range.contains(rail_parameter, tolerance) {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "fillet endpoint contact falls beyond its adjacent trimmed edge",
                ));
            }
            contact_parameters[parent_index] = contact_parameter;
            contact_points[parent_index] = contact_point;
        }
        Ok(FilletEndpoint {
            old_vertex,
            terminating_face,
            adjacent_uses,
            contact_parameters,
            contact_points,
        })
    }

    fn prepare_fillet_junction(
        &self,
        spine: &FilletSpine,
        endpoint_index: usize,
        radius: f64,
        kind: FilletJunctionKind,
    ) -> KernelResult<FilletJunction> {
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
                    "fillet junction has no third incident face",
                )
            })?;
        let third_surface = self.working.face(third_face)?.surface.clone();
        let third_distance = match self.working.face(third_face)?.orientation {
            FaceOrientation::Forward => -radius,
            FaceOrientation::Reversed => radius,
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
                    "trihedral fillet contact is not at the requested rolling-ball radius",
                ));
            }
        }
        Ok(FilletJunction {
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

    fn prepare_smooth_fillet_junction(
        &self,
        vertex: VertexId,
        radius: f64,
        seam_edge: EdgeId,
        tangent_faces: [FaceId; 2],
    ) -> KernelResult<FilletJunction> {
        let tolerance = self.kernel.tolerance;
        let vertex_faces = self.working.vertex_faces(vertex)?;
        if vertex_faces.len() != 3 || self.working.vertex_edges(vertex)?.len() != 3 {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "a smooth fillet transition requires a regular valence-three BREP vertex",
            ));
        }
        let common_face = vertex_faces
            .iter()
            .copied()
            .find(|face| !tangent_faces.contains(face))
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "smooth fillet transition has no common incident face",
                )
            })?;
        let seam = self.working.edge(seam_edge)?.clone();
        let seam_parameter = if seam.vertices[0] == vertex {
            seam.range.start
        } else if seam.vertices[1] == vertex {
            seam.range.end
        } else {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "smooth fillet seam is not incident to its junction vertex",
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
                    "smooth fillet seam has no use on its tangent support",
                )
            })?;
        let seam_pcurve = self.working.coedge(tangent_use)?.pcurve.clone();
        let tangent_distance = match self.working.face(tangent_face)?.orientation {
            FaceOrientation::Forward => -radius,
            FaceOrientation::Reversed => radius,
        };
        let common_surface = self.working.face(common_face)?.surface.clone();
        let common_distance = match self.working.face(common_face)?.orientation {
            FaceOrientation::Forward => -radius,
            FaceOrientation::Reversed => radius,
        };
        let common_offset = common_surface.offset(common_distance, tolerance)?;
        let old_point = self.working.vertex(vertex)?.point;
        let common_seed = match &common_offset {
            Surface3::Offset(offset) => offset.base().project(old_point, tolerance)?.parameters,
            _ => common_offset.project(old_point, tolerance)?.parameters,
        };
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
                    "smooth fillet seam reached a singular tangent support",
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
                "exact smooth fillet junction correction did not converge",
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
                    "smooth fillet junction contact is not at the requested rolling-ball radius",
                ));
            }
        }
        Ok(FilletJunction {
            faces: vec![tangent_faces[0], tangent_faces[1], common_face],
            center,
            contact_points: BTreeMap::from([
                (tangent_faces[0], seam_contact),
                (tangent_faces[1], seam_contact),
                (common_face, common_contact),
            ]),
            kind: FilletJunctionKind::Smooth {
                seam_edge,
                tangent_faces,
            },
        })
    }

    fn prepare_tangent_chain_junction(
        &self,
        spine: &FilletSpine,
        endpoint_index: usize,
        tangent_faces: [FaceId; 2],
    ) -> KernelResult<FilletJunction> {
        let tolerance = self.kernel.tolerance;
        let old_vertex = spine.edge.vertices[endpoint_index];
        let old_point = self.working.vertex(old_vertex)?.point;
        let projected = spine.center.curve.project(old_point, tolerance)?.parameter;
        let parameter = if let Some(period) = spine.center.curve.domain().period {
            projected
                + ((spine.endpoint_seeds[endpoint_index] - projected) / period).round() * period
        } else {
            projected
        };
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
        Ok(FilletJunction {
            faces: tangent_faces.to_vec(),
            center,
            contact_points,
            kind: FilletJunctionKind::TangentChain { tangent_faces },
        })
    }

    fn tangent_chain_continuation(
        &self,
        spine: &FilletSpine,
        endpoint_index: usize,
        selected: &BTreeSet<EdgeId>,
    ) -> KernelResult<Option<(EdgeId, [FaceId; 2])>> {
        let vertex = spine.edge.vertices[endpoint_index];
        let source_parameter = if endpoint_index == 0 {
            spine.edge.range.start
        } else {
            spine.edge.range.end
        };
        let source = spine.edge.curve.evaluate(source_parameter)?;
        let source_tangent = if endpoint_index == 0 {
            source.first_derivative
        } else {
            -source.first_derivative
        }
        .normalize();
        let source_faces = spine.faces.into_iter().collect::<BTreeSet<_>>();
        let mut continuation = None;
        for candidate_id in self
            .working
            .vertex_edges(vertex)?
            .into_iter()
            .filter(|edge| !selected.contains(edge))
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
            let (parameter, sign) = if candidate.vertices[0] == vertex {
                (candidate.range.start, 1.0)
            } else if candidate.vertices[1] == vertex {
                (candidate.range.end, -1.0)
            } else {
                continue;
            };
            let tangent = candidate.curve.evaluate(parameter)?.first_derivative * sign;
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
                    "a fillet tangent chain branches at one topological vertex",
                ));
            }
        }
        Ok(continuation.map(|edge| (edge, spine.faces)))
    }

    fn smooth_fillet_continuation(
        &self,
        vertex: VertexId,
        selected: &BTreeSet<EdgeId>,
    ) -> KernelResult<Option<(EdgeId, EdgeId, [FaceId; 2])>> {
        let point = self.working.vertex(vertex)?.point;
        let incident = self.working.vertex_edges(vertex)?;
        for seam in incident
            .iter()
            .copied()
            .filter(|edge| !selected.contains(edge))
        {
            let faces = self.working.edge_faces(seam)?;
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
            let continuation = incident
                .iter()
                .copied()
                .find(|edge| *edge != seam && !selected.contains(edge));
            if let Some(continuation) = continuation {
                return Ok(Some((seam, continuation, [faces[0], faces[1]])));
            }
        }
        Ok(None)
    }

    fn most_implicit_fillet_surface(
        &self,
        spine: &FilletSpine,
        center_range: ParameterRange,
        radius: f64,
    ) -> KernelResult<(Surface3, FilletSurfaceParameterization)> {
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
                        let parameterization = FilletSurfaceParameterization {
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
                    let parameterization = FilletSurfaceParameterization {
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
                center_range,
                radius,
                tolerance,
            )?
            .into(),
            FilletSurfaceParameterization::ROLLING_BALL,
        ))
    }

    fn fillet_surface_matches(
        &self,
        spine: &FilletSpine,
        center_range: ParameterRange,
        surface: &Surface3,
        parameterization: FilletSurfaceParameterization,
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
    ) -> KernelResult<Option<FilletBoundary>> {
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
        Ok(Some(FilletBoundary {
            curve: candidate.curve,
            parent_pcurve: candidate.second_pcurve,
            blend_pcurve: candidate.first_pcurve,
            range,
        }))
    }

    fn prepare_fillet_patch(
        &self,
        spine: FilletSpine,
        junctions: &BTreeMap<VertexId, FilletJunction>,
        sphere_surfaces: &BTreeMap<VertexId, Surface3>,
        regular_endpoints: &BTreeMap<(EdgeId, usize), Option<FilletEndpoint>>,
        radius: f64,
    ) -> KernelResult<FilletPatch> {
        let tolerance = self.kernel.tolerance;
        let mut ends = Vec::with_capacity(2);
        let mut contact_parameters = [[0.0; 2]; 2];
        let mut contact_points = [[Point3::new(0.0, 0.0, 0.0); 2]; 2];
        for endpoint_index in 0..2 {
            let vertex = spine.edge.vertices[endpoint_index];
            if let Some(junction) = junctions.get(&vertex) {
                let projected = spine
                    .center
                    .curve
                    .project(junction.center, tolerance)?
                    .parameter;
                let parameter = if let Some(period) = spine.center.curve.domain().period {
                    projected
                        + ((spine.endpoint_seeds[endpoint_index] - projected) / period).round()
                            * period
                } else {
                    projected
                };
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
                        "fillet junction center does not lie on every incident offset intersection",
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
                                "fillet junction lost an incident face contact",
                            )
                        })?;
                }
                ends.push(FilletPatchEnd::Junction(vertex));
            } else {
                let endpoint = regular_endpoints
                    .get(&(spine.edge_id, endpoint_index))
                    .and_then(Clone::clone)
                    .ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::TopologyInvariant,
                            "regular fillet endpoint was not retained during network discovery",
                        )
                    })?;
                contact_parameters[endpoint_index] = endpoint.contact_parameters;
                contact_points[endpoint_index] = endpoint.contact_points;
                ends.push(FilletPatchEnd::Boundary(endpoint));
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
            _ if center_domain.period.is_some() => ParameterRange::new(minimum, maximum).map_err(
                |_| {
                    KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        format!(
                            "fillet rolling support collapsed to zero span ({minimum} to {maximum})",
                        ),
                    )
                },
            )?,
            DomainBounds::Bounded(range) => range,
            _ => {
                let span = (maximum - minimum).abs().max(1.0);
                ParameterRange::new(minimum - span, maximum + span)?
            }
        };
        let (blend_surface, blend_parameterization) =
            self.most_implicit_fillet_surface(&spine, blend_support_range, radius)?;
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
                            "fillet contact boundary {parent_index} collapsed to zero span ({} to {})",
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
                Ok(FilletBoundary {
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
                    FilletPatchEnd::Boundary(endpoint) => {
                        let terminating_surface = self
                            .working
                            .face(endpoint.terminating_face)?
                            .surface
                            .clone();
                        if let Some(boundary) = self.implicit_end_boundary(
                            &blend_surface,
                            &terminating_surface,
                            &blend_pcurve,
                            contact_points[endpoint_index],
                        )? {
                            return Ok(boundary);
                        }
                        let mut guide_parameters = Vec::with_capacity(65);
                        let mut blend_parameters: Vec<Point2> = Vec::with_capacity(65);
                        let mut parent_parameters: Vec<Point2> = Vec::with_capacity(65);
                        for index in 0..=64 {
                            let parameter = 0.5
                                - 0.5
                                    * (std::f64::consts::PI * index as f64 / 64.0).cos();
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
                                &blend_surface,
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
                                            "fillet end-boundary guide correction failed at sample {index}: {error}"
                                        ),
                                    ))
                                }
                            };
                            blend_parameters.push(Point2::new(corrected[0], corrected[1]));
                            parent_parameters.push(Point2::new(corrected[2], corrected[3]));
                        }
                        let blend_guide: Curve2 =
                            super::intersection::interpolating_nurbs_guide(
                                &blend_parameters,
                                &guide_parameters,
                            )?
                            .into();
                        let parent_guide: Curve2 =
                            super::intersection::interpolating_nurbs_guide(
                                &parent_parameters,
                                &guide_parameters,
                            )?
                            .into();
                        let intersection = SurfaceIntersectionCurve::new(
                            blend_surface.clone(),
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
                                format!(
                                    "fillet end boundary {endpoint_index} exact intersection failed: {error}"
                                ),
                            )
                        })?;
                        let exact = intersection.exact_pcurves();
                        Ok(FilletBoundary {
                            curve: intersection.into(),
                            parent_pcurve: exact[1].clone(),
                            blend_pcurve: exact[0].clone(),
                            range: unit,
                        })
                    }
                    FilletPatchEnd::Junction(vertex) => {
                        let curve: Curve3 = SurfaceCurve3::new(
                            blend_surface.clone(),
                            blend_pcurve.clone(),
                            unit,
                            tolerance,
                        )?
                        .into();
                        let Some(sphere) = sphere_surfaces.get(vertex) else {
                            return Ok(FilletBoundary {
                                curve,
                                parent_pcurve: blend_pcurve.clone(),
                                blend_pcurve,
                                range: unit,
                            });
                        };
                        let mut parameters: Vec<Point2> = Vec::with_capacity(17);
                        for index in 0..=16 {
                            let parameter = index as f64 / 16.0;
                            let projection =
                                sphere.project(curve.evaluate(parameter)?.point, tolerance)?;
                            let mut uv =
                                Point2::new(projection.parameters.0, projection.parameters.1);
                            if let Some(previous) = parameters.last().copied() {
                                if let Some(period) = sphere.domains()[0].period {
                                    uv.x += ((previous.x - uv.x) / period).round() * period;
                                }
                            }
                            parameters.push(uv);
                        }
                        let guide: Curve2 =
                            super::intersection::smooth_nurbs_guide(&parameters, unit)?.into();
                        let parent_pcurve: Curve2 = SurfaceProjectionPcurve2::new(
                            curve.clone(),
                            sphere.clone(),
                            guide,
                            unit,
                            tolerance,
                        )?
                        .into();
                        Ok(FilletBoundary {
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
        Ok(FilletPatch {
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

    fn rewrite_fillet_network(
        &mut self,
        patches: &[FilletPatch],
        junctions: &BTreeMap<VertexId, FilletJunction>,
        sphere_surfaces: &BTreeMap<VertexId, Surface3>,
    ) -> KernelResult<()> {
        let tolerance = self.kernel.tolerance;
        let first_patch = patches.first().ok_or_else(|| {
            KernelError::new(DiagnosticKind::InvalidInput, "fillet network is empty")
        })?;
        let shell = self.working.face(first_patch.spine.faces[0])?.shell;
        for patch in patches {
            for face in patch.spine.faces {
                if self.working.face(face)?.shell != shell {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "fillet network crosses shell boundaries",
                    ));
                }
            }
        }

        // Capture every external loop gap before any endpoint changes.
        let mut termination_gaps = BTreeMap::new();
        for patch in patches {
            for endpoint_index in 0..2 {
                let FilletPatchEnd::Boundary(endpoint) = &patch.ends[endpoint_index] else {
                    continue;
                };
                let mut uses = Vec::with_capacity(2);
                for adjacent in endpoint.adjacent_uses {
                    let adjacent_edge = self.working.coedge(adjacent)?.edge;
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
                                "fillet boundary rail has no terminating-face use",
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
                        "fillet terminating-face uses do not form one vertex gap",
                    ));
                };
                if self.working.coedge(gap.0)?.next != gap.1 {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "fillet terminating-face gap is not consecutive",
                    ));
                }
                termination_gaps.insert((patch.spine.edge.name, endpoint.old_vertex), gap);
            }
        }

        let mut contact_vertices = BTreeMap::<(VertexId, FaceId), VertexId>::new();
        for patch in patches {
            for endpoint_index in 0..2 {
                let old_vertex = patch.spine.edge.vertices[endpoint_index];
                for parent_index in 0..2 {
                    let face = patch.spine.faces[parent_index];
                    let key = (old_vertex, face);
                    if contact_vertices.contains_key(&key) {
                        continue;
                    }
                    let canonical_key = junctions
                        .get(&old_vertex)
                        .and_then(|junction| match junction.kind {
                            FilletJunctionKind::Smooth { tangent_faces, .. }
                                if tangent_faces.contains(&face) =>
                            {
                                Some((old_vertex, tangent_faces[0]))
                            }
                            _ => None,
                        })
                        .unwrap_or(key);
                    if let Some(vertex) = contact_vertices.get(&canonical_key).copied() {
                        contact_vertices.insert(key, vertex);
                        continue;
                    }
                    let point = patch.contact_points[endpoint_index][parent_index];
                    let name = self.names.next_for("fillet.contact.vertex")?;
                    let vertex = self.make_vertex(name, point);
                    self.history.derived(
                        name,
                        [
                            self.working.vertex(old_vertex)?.name,
                            self.working.face(face)?.name,
                        ],
                        "fillet.contact.vertex",
                    );
                    contact_vertices.insert(canonical_key, vertex);
                    contact_vertices.insert(key, vertex);
                }
            }
        }

        let mut trimmed = BTreeSet::new();
        for patch in patches {
            for endpoint_index in 0..2 {
                let FilletPatchEnd::Boundary(endpoint) = &patch.ends[endpoint_index] else {
                    continue;
                };
                for parent_index in 0..2 {
                    let adjacent_edge = self
                        .working
                        .coedge(endpoint.adjacent_uses[parent_index])?
                        .edge;
                    if !trimmed.insert((adjacent_edge, endpoint.old_vertex)) {
                        continue;
                    }
                    self.retarget_edge_endpoint(
                        adjacent_edge,
                        endpoint.old_vertex,
                        contact_vertices[&(endpoint.old_vertex, patch.spine.faces[parent_index])],
                        endpoint.contact_points[parent_index],
                    )?;
                }
            }
        }

        for (old_vertex, junction) in junctions {
            let FilletJunctionKind::Smooth {
                seam_edge,
                tangent_faces,
            } = junction.kind
            else {
                continue;
            };
            let new_vertex = contact_vertices[&(*old_vertex, tangent_faces[0])];
            self.retarget_edge_endpoint(
                seam_edge,
                *old_vertex,
                new_vertex,
                junction.contact_points[&tangent_faces[0]],
            )?;
        }
        let selected_edges = patches
            .iter()
            .map(|patch| patch.spine.edge_id)
            .collect::<BTreeSet<_>>();
        for (old_vertex, junction) in junctions {
            let FilletJunctionKind::TangentChain { tangent_faces } = junction.kind else {
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
                    contact_vertices[&(*old_vertex, seam_face)],
                    junction.contact_points[&seam_face],
                )?;
            }
        }

        let mut sphere_faces = BTreeMap::new();
        let mut sphere_loops = BTreeMap::new();
        let mut sphere_uses = BTreeMap::<VertexId, Vec<CoedgeId>>::new();
        for (vertex, junction) in junctions {
            if !matches!(junction.kind, FilletJunctionKind::Trihedral) {
                continue;
            }
            let surface = sphere_surfaces[vertex].clone();
            let face_name = self.names.next_for("fillet.vertex.face")?;
            let loop_name = self.names.next_for("fillet.vertex.loop")?;
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
                "fillet.vertex.face",
            );
            self.history
                .derived(loop_name, [face_name], "fillet.vertex.loop");
            sphere_faces.insert(*vertex, face);
            sphere_loops.insert(*vertex, loop_id);
            sphere_uses.insert(*vertex, Vec::with_capacity(3));
        }

        let mut blend_cycles = Vec::with_capacity(patches.len());
        let mut smooth_edges = BTreeMap::<VertexId, (EdgeId, Curve3)>::new();
        for patch in patches {
            let selected_name = patch.spine.edge.name;
            let blend_face_name = self.names.next_for("fillet.blend.face")?;
            let blend_loop_name = self.names.next_for("fillet.blend.loop")?;
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
                "fillet.blend.face",
            );
            self.history
                .derived(blend_loop_name, [blend_face_name], "fillet.blend.loop");

            let mut blend_uses = Vec::with_capacity(4);
            for parent_index in 0..2 {
                let boundary = &patch.contact_boundaries[parent_index];
                let first_vertex = contact_vertices[&(
                    patch.spine.edge.vertices[0],
                    patch.spine.faces[parent_index],
                )];
                let second_vertex = contact_vertices[&(
                    patch.spine.edge.vertices[1],
                    patch.spine.faces[parent_index],
                )];
                let edge_name = self.names.next_for("fillet.contact.edge")?;
                let parent_use_name = self.names.next_for("fillet.contact.parent-use")?;
                let blend_use_name = self.names.next_for("fillet.contact.blend-use")?;
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
                blend_uses.push(blend_use);
                self.history.derived(
                    edge_name,
                    [
                        selected_name,
                        self.working.face(patch.spine.faces[parent_index])?.name,
                    ],
                    "fillet.contact.edge",
                );
                self.history
                    .derived(parent_use_name, [edge_name], "fillet.contact.parent-use");
                self.history
                    .derived(blend_use_name, [edge_name], "fillet.contact.blend-use");
            }

            for endpoint_index in 0..2 {
                let boundary = &patch.end_boundaries[endpoint_index];
                if let FilletPatchEnd::Junction(vertex) = &patch.ends[endpoint_index] {
                    if matches!(
                        junctions[vertex].kind,
                        FilletJunctionKind::Smooth { .. } | FilletJunctionKind::TangentChain { .. }
                    ) {
                        let vertices = [
                            contact_vertices[&(
                                patch.spine.edge.vertices[endpoint_index],
                                patch.spine.faces[0],
                            )],
                            contact_vertices[&(
                                patch.spine.edge.vertices[endpoint_index],
                                patch.spine.faces[1],
                            )],
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
                                        "smooth fillet transition patches do not share contact vertices",
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
                            let edge_name = self.names.next_for("fillet.transition.edge")?;
                            let edge =
                                self.make_edge(edge_name, vertices, boundary.curve.clone(), unit)?;
                            self.history.derived(
                                edge_name,
                                [selected_name],
                                "fillet.transition.edge",
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
                        let use_name = self.names.next_for("fillet.transition.blend-use")?;
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
                            "fillet.transition.blend-use",
                        );
                        continue;
                    }
                }
                let vertices = [
                    contact_vertices[&(
                        patch.spine.edge.vertices[endpoint_index],
                        patch.spine.faces[0],
                    )],
                    contact_vertices[&(
                        patch.spine.edge.vertices[endpoint_index],
                        patch.spine.faces[1],
                    )],
                ];
                let edge_name = self.names.next_for("fillet.end.edge")?;
                let other_use_name = self.names.next_for("fillet.end.other-use")?;
                let blend_use_name = self.names.next_for("fillet.end.blend-use")?;
                let edge =
                    self.make_edge(edge_name, vertices, boundary.curve.clone(), boundary.range)?;
                let (other_loop, other_forward) = match &patch.ends[endpoint_index] {
                    FilletPatchEnd::Boundary(endpoint) => {
                        let gap = termination_gaps[&(selected_name, endpoint.old_vertex)];
                        (self.working.coedge(gap.0)?.loop_id, gap.2)
                    }
                    FilletPatchEnd::Junction(vertex) => (sphere_loops[vertex], true),
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
                    FilletPatchEnd::Boundary(endpoint) => {
                        let gap = termination_gaps[&(selected_name, endpoint.old_vertex)];
                        self.insert_loop_coedge(gap.0, gap.1, other_use)?;
                    }
                    FilletPatchEnd::Junction(vertex) => {
                        sphere_uses.get_mut(vertex).unwrap().push(other_use);
                    }
                }
                blend_uses.push(blend_use);
                self.history
                    .derived(edge_name, [selected_name], "fillet.end.edge");
                self.history
                    .derived(other_use_name, [edge_name], "fillet.end.other-use");
                self.history
                    .derived(blend_use_name, [edge_name], "fillet.end.blend-use");
            }
            blend_cycles.push((blend_loop, blend_uses));
        }

        for (vertex, (edge, _)) in &smooth_edges {
            if self.working.edge(*edge)?.coedges.len() != 2 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    format!(
                        "smooth fillet junction {:?} is not shared by exactly two rolling patches",
                        vertex
                    ),
                ));
            }
        }
        for uses in sphere_uses.values() {
            for sphere_use in uses {
                let edge = self.working.coedge(*sphere_use)?.edge;
                let blend_use = self
                    .working
                    .edge(edge)?
                    .coedges
                    .iter()
                    .copied()
                    .find(|candidate| candidate != sphere_use)
                    .ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::TopologyInvariant,
                            "trihedral boundary has no paired rolling-patch use",
                        )
                    })?;
                let blend_loop = self.working.coedge(blend_use)?.loop_id;
                let cycle = blend_cycles
                    .iter()
                    .find_map(|(loop_id, uses)| (*loop_id == blend_loop).then_some(uses))
                    .ok_or_else(|| {
                        KernelError::new(
                            DiagnosticKind::TopologyInvariant,
                            "trihedral boundary lost its rolling-patch cycle",
                        )
                    })?;
                self.orient_closing_coedge(cycle, blend_use)?;
                let blend_forward = self.working.coedge(blend_use)?.forward;
                self.orient_coedge(*sphere_use, !blend_forward)?;
            }
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
        for (vertex, uses) in &sphere_uses {
            if uses.len() != 3 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "a regular trihedral fillet junction must have three rolling patches",
                ));
            }
            self.close_loop_from_coedges(sphere_loops[vertex], uses)
                .map_err(|error| {
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        format!("trihedral sphere boundary cycle failed: {error}"),
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
            self.orient_face(
                sphere_faces[vertex],
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
                if let FilletPatchEnd::Boundary(endpoint) = end {
                    modified_faces.insert(endpoint.terminating_face);
                }
            }
            self.delete_edge_and_uses(patch.spine.edge_id, &patch.spine.uses)?;
        }
        for old_vertex in old_vertices {
            let outputs = contact_vertices
                .iter()
                .filter_map(|((vertex, _), replacement)| {
                    (*vertex == old_vertex).then_some(*replacement)
                })
                .map(|vertex| self.working.vertex(vertex).map(|value| value.name))
                .collect::<KernelResult<Vec<_>>>()?;
            self.delete_split_vertex(old_vertex, outputs)?;
        }
        for vertex in contact_vertices.values().copied() {
            self.repair_outgoing(vertex)?;
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
                "selected coedge is not incident to its fillet endpoint",
            ))
        }
    }

    fn local_offset_intersection(
        &self,
        uses: [CoedgeId; 2],
        offsets: &[Surface3; 2],
        tolerance: ModelTolerance,
    ) -> KernelResult<SurfaceSurfaceCurve> {
        let support_range = ParameterRange::new(-1.0, 2.0)?;
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
                let pcurve_parameter =
                    coedge.range.start + coedge.range.span() * traversal_fraction;
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
        let extrapolated = |side: usize, index: i32| {
            if index < 0 {
                boundary_parameters[side][0]
                    + (boundary_parameters[side][1] - boundary_parameters[side][0]) * index as f64
            } else if index > 16 {
                boundary_parameters[side][16]
                    + (boundary_parameters[side][16] - boundary_parameters[side][15])
                        * (index - 16) as f64
            } else {
                boundary_parameters[side][index as usize]
            }
        };
        let mut parameters: [Vec<Point2>; 2] = [Vec::with_capacity(49), Vec::with_capacity(49)];
        for index in -16..=32 {
            let first = extrapolated(0, index);
            let second = extrapolated(1, index);
            let corrected = super::intersection::correct_surface_pair_near(
                &offsets[0],
                &offsets[1],
                [first.x, first.y, second.x, second.y],
                tolerance,
            )?;
            parameters[0].push(Point2::new(corrected[0], corrected[1]));
            parameters[1].push(Point2::new(corrected[2], corrected[3]));
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
                format!("local fillet offset intersection failed: {error}"),
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
