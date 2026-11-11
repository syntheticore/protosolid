//! Blend spines: the offset-surface intersection carrying each selected edge's
//! rolling center, and how each spine ends or continues at its vertices.

use std::collections::VecDeque;

use super::*;
use crate::{geometry_error, invalid, topology_error, unsupported, ConstructionBranch, ConstructionTag, CurveEvaluation};

struct BlendSide {
    concave: bool,
    cusp_inward: Option<Vec3>,
    support_normals: [Vec3; 2],
}

impl Transaction<'_> {
    /// Decide how `spine` ends at its endpoint `endpoint_index`. A spine whose
    /// end meets no other selected edge either continues tangentially (when
    /// `follow_tangent_edges`) or terminates; otherwise the local corner must
    /// be one of the supported junction kinds.
    pub(super) fn classify_spine_end(
        &mut self,
        spine: &BlendSpine,
        endpoint_index: usize,
        selected: &BTreeSet<EdgeId>,
        spines: &BTreeMap<EdgeId, BlendSpine>,
        kind: EdgeBlendKind,
        follow_tangent_edges: bool,
    ) -> KernelResult<SpineEnd> {
        let vertex = spine.edge.vertices[endpoint_index];
        let incident = self.working.vertex_edges(vertex)?;
        let selected_degree = incident.iter().filter(|edge| selected.contains(edge)).count();
        let junction = |kind, continuation| SpineEnd::Junction { kind, continuation };
        if selected_degree == 1 {
            if follow_tangent_edges {
                if let Some((continuation, tangent_faces)) =
                    self.tangent_chain_continuation(spine, endpoint_index)?
                {
                    return Ok(junction(
                        BlendJunctionKind::TangentChain { tangent_faces },
                        Some((continuation, tangent_faces)),
                    ));
                }
                if let Some((seam_edge, continuation, tangent_faces)) =
                    self.smooth_blend_continuation(vertex, spine, selected, kind)?
                {
                    return Ok(junction(
                        BlendJunctionKind::Smooth {
                            seam_edge,
                            tangent_faces,
                            second_seam: None,
                        },
                        Some((continuation, tangent_faces)),
                    ));
                }
            }
            let endpoint = self
                .prepare_blend_endpoint(spine, endpoint_index)
                .or_else(|error| {
                    if kind == EdgeBlendKind::Chamfer {
                        self.prepare_capped_endpoint(spine, endpoint_index)
                            .map_err(|_| error)
                    } else {
                        Err(error)
                    }
                })?;
            return Ok(SpineEnd::Endpoint(endpoint));
        }

        if let Some((continuation, tangent_faces)) =
            self.tangent_chain_continuation(spine, endpoint_index)?
        {
            if selected.contains(&continuation) {
                return Ok(junction(BlendJunctionKind::TangentChain { tangent_faces }, None));
            }
        }
        if let Some((seam_edge, continuation, tangent_faces)) =
            self.smooth_blend_continuation(vertex, spine, selected, kind)?
        {
            if selected.contains(&continuation) {
                return Ok(junction(
                    BlendJunctionKind::Smooth {
                        seam_edge,
                        tangent_faces,
                        second_seam: None,
                    },
                    None,
                ));
            }
        }
        if let Some((continuation, carrier_face, seam_edge)) = self.inner_rim_corner(spine, vertex)? {
            if selected.contains(&continuation) {
                return Ok(junction(
                    BlendJunctionKind::RimCorner {
                        carrier_face,
                        seam_edge,
                    },
                    None,
                ));
            }
        }
        if selected_degree == 2 && incident.len() == 3 {
            let seam_edge = *incident
                .iter()
                .find(|edge| !selected.contains(edge))
                .expect("one unselected seam");
            let walls = self.working.edge_faces(seam_edge)?;
            if walls.len() == 2 {
                return Ok(junction(
                    BlendJunctionKind::Miter {
                        seam_edge,
                        wall_faces: [walls[0], walls[1]],
                    },
                    None,
                ));
            }
        }
        if selected_degree == 3 && incident.len() == 3 {
            let first_concavity = spines[&incident[0]].concave;
            if incident
                .iter()
                .any(|edge| spines[edge].concave != first_concavity)
            {
                return Ok(SpineEnd::Staged(BlendStaging::MixedTrihedral {
                    concave_edges: spines
                        .values()
                        .filter(|spine| spine.concave)
                        .map(|spine| spine.edge.name)
                        .collect(),
                }));
            }
            return Ok(junction(BlendJunctionKind::Trihedral, None));
        }
        if selected_degree == 2 && incident.len() == 4 {
            let pair = incident
                .iter()
                .copied()
                .filter(|edge| selected.contains(edge))
                .collect::<Vec<_>>();
            let faces = [self.working.edge_faces(pair[0])?, self.working.edge_faces(pair[1])?];
            if !faces[0].iter().any(|face| faces[1].contains(face)) {
                // Opposite edges of a four-face corner do not blend each other's
                // supports. Each blend can end on the other one in sequence.
                return Ok(SpineEnd::Staged(BlendStaging::CrossingCorner {
                    pair: [self.working.edge(pair[0])?.name, self.working.edge(pair[1])?.name],
                }));
            }
        }
        Err(unsupported("selected blend edges do not define a supported local corner transition"))
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
                    (self.working.loop_(coedge.loop_id)?.kind == super::super::LoopKind::Inner)
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
                topology_error("an inner-rim corner has no wall seam edge")
            })?;
        let seam = self.working.edge(seam_edge)?;
        let walls = self.working.edge_faces(seam_edge)?;
        if walls.len() == 2 {
            let surfaces = [self.working.face(walls[0])?.surface.clone(), self.working.face(walls[1])?.surface.clone()];
            let midpoint = seam.curve.evaluate(seam.range.middle())?;
            if !self.blend_side(seam, [seam.coedges[0], seam.coedges[1]], [walls[0], walls[1]], &surfaces, midpoint)?.concave {
                return Ok(None);
            }
        }
        Ok(Some((continuation, carrier_loop.face, seam_edge)))
    }

    /// The blend network represents rolling-patch joins at edge endpoints.
    /// Published periodic edges correctly have one artificial seam vertex, so
    /// subdivide them only inside this transaction. The temporary arcs are all
    /// selected and are consumed by the subsequent blend rewrite.
    pub(super) fn subdivide_closed_blend_edges(
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
                return Err(topology_error("a closed blend edge must have exactly two face uses"));
            }

            let source_name = edge.name;
            let seam = edge.vertices[0];
            // Label the intervals in the source curve's increasing chart,
            // even when the stored edge traverses that chart in reverse.
            let forward = edge.range.span() > 0.0;
            let seam_name = self.working.vertex(seam)?.name;
            self.history.tag_construction(seam_name, ConstructionTag {
                source: source_name, branch: ConstructionBranch::BlendJunction(0),
            });
            let mut vertices = vec![seam];
            for part in 1..4 {
                let parameter = edge.range.start + edge.range.span() * part as f64 / 4.0;
                let point = edge.curve.evaluate(parameter)?.point;
                let name = self.names.next_derived(kind.role(BlendRole::PeriodicSplitVertex))?;
                let vertex = self.working.vertices.insert(Vertex::new(
                    name,
                    point,
                    self.kernel.tolerance.linear,
                ));
                self.history
                    .derived(name, [source_name], kind.role(BlendRole::PeriodicSplitVertex));
                let quarter = if forward { part } else { 4 - part };
                self.history.tag_construction(name, ConstructionTag {
                    source: source_name, branch: ConstructionBranch::BlendJunction(quarter),
                });
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
            self.history.tag_construction(source_name, ConstructionTag {
                source: source_name, branch: ConstructionBranch::BlendSpan(if forward { 0 } else { 3 }),
            });
            for part in 1..4 {
                let name = self.names.next_derived(kind.role(BlendRole::PeriodicSplitEdge))?;
                let arc = self.make_edge(
                    name,
                    [vertices[part], vertices[part + 1]],
                    edge.curve.clone(),
                    ranges[part],
                )?;
                self.history
                    .derived(name, [source_name], kind.role(BlendRole::PeriodicSplitEdge));
                let quarter = if forward { part } else { 3 - part };
                self.history.tag_construction(name, ConstructionTag {
                    source: source_name, branch: ConstructionBranch::BlendSpan(quarter as u8),
                });
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
                    let name = self.names.next_derived(kind.role(BlendRole::PeriodicSplitUse))?;
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
                        .derived(name, [source_name], kind.role(BlendRole::PeriodicSplitUse));
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
            self.history
                .changed(source_name, ChangeKind::Modified, [source_name]);
            result.extend(arcs);
        }
        Ok(result)
    }

    pub(super) fn prepare_blend_spine(
        &self,
        edge_id: EdgeId,
        size: f64,
        kind: EdgeBlendKind,
        shared: Option<&BlendSpine>,
        propagated: Option<&(BlendSpine, [FaceId; 2])>,
        chamfer: Option<&ChamferSettings>,
        interpolate_guides: bool,
    ) -> KernelResult<BlendSpine> {
        let tolerance = self.kernel.tolerance;
        let edge = self.working.edge(edge_id)?.clone();
        if edge.coedges.len() != 2 {
            return Err(unsupported("blend requires a manifold edge with two face uses"));
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
            return Err(unsupported("blend requires two distinct incident faces"));
        }
        let mut parent_surfaces = [
            self.working.face(faces[0])?.surface.clone(),
            self.working.face(faces[1])?.surface.clone(),
        ];
        // A local guide continues an open curved edge by whole sixteenths of
        // its length. Past an obtuse corner, the contact can meet its
        // terminating edge beyond the source end, so the supports must cover
        // that collar and not only the blend size.
        let collar = if edge.vertices[0] == edge.vertices[1] || matches!(edge.curve, Curve3::Line(_)) {
            0.0
        } else {
            let mut step: f64 = 0.0;
            let mut previous = edge.curve.evaluate(edge.range.start)?.point;
            for index in 1..=16 {
                let point = edge
                    .curve
                    .evaluate(edge.range.start + edge.range.span() * index as f64 / 16.0)?
                    .point;
                step = step.max(point.distance(previous));
                previous = point;
            }
            step
        };
        for (index, surface) in parent_surfaces.iter_mut().enumerate() {
            let bounds = super::super::features::face_parameter_bounds(&self.working, faces[index], tolerance)?;
            *surface = super::super::features::extended_support(surface, bounds, size.abs() + collar, tolerance)?;
        }
        let sample = edge.curve.evaluate(edge.range.middle())?;
        let midpoint = sample.point;
        let BlendSide { concave, cusp_inward, support_normals } =
            self.blend_side(&edge, uses, faces, &parent_surfaces, sample)?;
        if let (true, [Surface3::Cylinder(first), Surface3::Cylinder(second)],
            Some(settings), Curve3::SurfaceIntersection(intersection)) =
            (edge.vertices[0] == edge.vertices[1], &parent_surfaces, chamfer, &edge.curve)
        {
            let same_order = intersection.first_surface() == &parent_surfaces[0]
                && intersection.second_surface() == &parent_surfaces[1];
            let opposite_order = intersection.first_surface() == &parent_surfaces[1]
                && intersection.second_surface() == &parent_surfaces[0];
            if let ChamferMode::TwoDistances(second_distance) = settings.mode {
                if same_order || opposite_order {
                    let reference = settings.reference_faces.get(&edge.name)
                        .ok_or_else(|| invalid("chamfer edge has no reference face"))?;
                    let distances = if self.working.face(faces[0])?.name == *reference {
                        [size, second_distance]
                    } else if self.working.face(faces[1])?.name == *reference {
                        [second_distance, size]
                    } else {
                        return Err(invalid("chamfer reference face must border its edge"));
                    };
                    let mut pcurves = intersection.exact_pcurves();
                    if opposite_order { pcurves.swap(0, 1); }
                    let [first_pcurve, second_pcurve] = pcurves;
                    let source = SurfaceSurfaceCurve {
                        curve: edge.curve.clone(),
                        first_pcurve,
                        second_pcurve,
                        achieved_error: intersection.achieved_error(),
                    };
                    let mut center = closed_blend_chart(&source, &edge, tolerance)?;
                    let rail = |side: usize, base: Curve2, cylinder: &crate::Cylinder3| {
                        let coedge = self.working.coedge(uses[side])?;
                        let sign = if coedge.forward { 1.0 } else { -1.0 };
                        let scale = Vec2::new(cylinder.radius(), cylinder.axis().magnitude());
                        let distance = distances[side] * sign * self.working.face(faces[side])?.orientation.sign();
                        crate::OffsetCurve2::with_scale(base, distance, scale).map(Into::into)
                    };
                    // Constant setbacks on cylindrical faces are parallel
                    // curves on their exact developed charts.
                    center.first_pcurve = rail(0, center.first_pcurve, first)?;
                    center.second_pcurve = rail(1, center.second_pcurve, second)?;
                    return Ok(BlendSpine {
                        edge_id, edge, uses, faces, parent_surfaces, concave,
                        offset: size.max(second_distance), offsets: distances, center,
                        endpoint_seeds: [0.0, 1.0],
                        provisional_range: ParameterRange::new(-1.0, 2.0)?,
                        shared_seam: None, local_guide: true,
                    });
                }
            }
        }
        // Equal setbacks on planar supports correspond to r = d cot(theta/2),
        // where theta is the angle between outward normals. Perpendicular rims
        // retain r = d, including plane/cylinder tangent chains.
        let offsets = if let Some(source) = shared {
            // Subdivision changes the trim, not the generating offset surfaces.
            // In particular, a chamfer's varying dihedral angle must not cause
            // each temporary arc to redefine an already shared spine.
            faces.map(|face| {
                source.offsets[source.faces.iter().position(|id| *id == face).unwrap()]
            })
        } else if let Some((source, tangent_faces)) = propagated {
            // A propagated rolling chamfer must retain its source offset across
            // smooth support seams. Recomputing it at each edge midpoint makes
            // the contact rails disagree at their shared endpoint.
            faces.map(|face| {
                let index = source
                    .faces
                    .iter()
                    .position(|candidate| *candidate == face)
                    .unwrap_or_else(|| {
                        source
                            .faces
                            .iter()
                            .position(|candidate| {
                                tangent_faces.contains(candidate) == tangent_faces.contains(&face)
                            })
                            .unwrap()
                    });
                source.offsets[index]
            })
        } else if kind == EdgeBlendKind::Chamfer {
            let cosine = support_normals[0].dot(support_normals[1]).clamp(-1.0, 1.0);
            let dimensions = chamfer.map(|settings| {
                let reference = settings.reference_faces.get(&edge.name).ok_or_else(|| {
                    invalid("chamfer edge has no reference face")
                })?;
                let first = faces
                    .iter()
                    .position(|face| {
                        self.working
                            .face(*face)
                            .is_ok_and(|face| face.name == *reference)
                    })
                    .ok_or_else(|| {
                        invalid("chamfer reference face must border its edge")
                    })?;
                Ok::<_, KernelError>((first, settings.mode))
            }).transpose()?;
            if let Some(inward) = cusp_inward {
                self.cusp_chamfer_offsets(&parent_surfaces, faces, midpoint, inward, concave, size, dimensions)?
            } else if let Some((first, mode)) = dimensions {
                let sine = (1.0 - cosine * cosine).sqrt();
                let second = match mode {
                    ChamferMode::TwoDistances(distance) => distance,
                    ChamferMode::DistanceAngle(angle) => {
                        let denominator = sine * angle.cos() - cosine * angle.sin();
                        if denominator <= tolerance.angular {
                            return Err(invalid("chamfer angle does not intersect the opposite face"));
                        }
                        size * angle.sin() / denominator
                    }
                };
                // The spine's projections onto the supports are the two
                // setback rails. Solve their two normal offsets separately.
                let mut offsets = [0.0; 2];
                offsets[first] = (second + cosine * size) / sine;
                offsets[1 - first] = (size + cosine * second) / sine;
                offsets
            } else {
                [size * ((1.0 + cosine) / (1.0 - cosine)).sqrt(); 2]
            }
        } else if kind.constant_width() {
            let chord_factor = (support_normals[0] - support_normals[1]).magnitude();
            if chord_factor <= tolerance.angular {
                return Err(unsupported("constant-width fillet requires distinct support normals"));
            }
            [size / chord_factor; 2]
        } else {
            [size; 2]
        };
        let radius = offsets[0].abs().max(offsets[1].abs());
        let offset_distance = |index: usize| -> KernelResult<f64> {
            let outward = match self.working.face(faces[index])?.orientation {
                FaceOrientation::Forward => offsets[index],
                FaceOrientation::Reversed => -offsets[index],
            };
            Ok(if concave { outward } else { -outward })
        };
        let offset_values = offsets;
        let offsets = [
            parent_surfaces[0].offset(offset_distance(0)?, tolerance)?,
            parent_surfaces[1].offset(offset_distance(1)?, tolerance)?,
        ];
        let mut local_guide = shared.is_some_and(|spine| spine.local_guide);
        let candidates = if let Some(shared) = shared {
            vec![shared.center.clone()]
        } else {
            let intersection = if kind != EdgeBlendKind::Chamfer {
                match (&offsets[0], &offsets[1]) {
                    (Surface3::Plane(plane), Surface3::Torus(torus)) => {
                        super::super::intersection::parallel_plane_torus_section(*plane, *torus, tolerance)
                    }
                    (Surface3::Torus(torus), Surface3::Plane(plane)) => {
                        super::super::intersection::parallel_plane_torus_section(*plane, *torus, tolerance)
                            .map(super::super::intersection::swap_surface_curves)
                    }
                    _ => offsets[0].intersect_support(&offsets[1], tolerance),
                }
            } else {
                offsets[0].intersect_support(&offsets[1], tolerance)
            };
            match intersection {
                Ok(IntersectionSet::Isolated(curves)) => curves,
                Ok(IntersectionSet::Empty) => {
                    return Err(geometry_error("incident face offsets do not intersect at the requested radius"))
                }
                Ok(IntersectionSet::Coincident) => {
                    return Err(unsupported("incident face offsets are coincident and do not define one blend spine"))
                }
                Err(_) => {
                    local_guide = true;
                    // Chamfers need the same angular continuation as fillets:
                    // an unconstrained guide can fold across a rounded corner.
                    vec![self.local_offset_intersection(
                        uses,
                        &offsets,
                        radius,
                        tolerance,
                        true,
                        interpolate_guides,
                    )?]
                }
            }
        };
        let (mut center, projection) = candidates
            .into_iter()
            .filter_map(|candidate| {
                let projection = candidate.curve.project(midpoint, tolerance).ok()?;
                if cusp_inward.is_some_and(|inward| (projection.point - midpoint).dot(inward) <= tolerance.linear) {
                    return None;
                }
                Some((candidate, projection))
            })
            .min_by(|first, second| first.1.distance.total_cmp(&second.1.distance))
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "blend could not select the local offset intersection branch",
                )
            })?;
        let mut middle_parameter = projection.parameter;
        if shared.is_none() && !local_guide && edge.vertices[0] != edge.vertices[1] {
            if let Some(chart) = open_blend_chart(&center, &edge, middle_parameter, tolerance)? {
                middle_parameter = chart.curve.project(midpoint, tolerance)?.parameter;
                center = chart;
            }
        }

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
        let domain = center.curve.domain();
        let endpoint_seeds = if domain.period.is_some() {
            let middle = domain.nearest(middle_parameter, endpoint_seeds[0]);
            [
                endpoint_seeds[0],
                domain.nearest(endpoint_seeds[1], middle * 2.0 - endpoint_seeds[0]),
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
                UNIT_PERIOD.nearest(endpoint_seeds[i], reference)
            })
        } else {
            endpoint_seeds
        };
        let seed_span = (endpoint_seeds[1] - endpoint_seeds[0]).abs().max(1.0);
        let center_domain = center.curve.domain();
        let mut provisional_range = match center_domain.bounds {
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
        // A periodic 3D spine may cross the cut of either bounded UV guide.
        // Continue the guides on their unwrapped charts over the support range.
        center.first_pcurve = center.first_pcurve.extended(provisional_range)?;
        center.second_pcurve = center.second_pcurve.extended(provisional_range)?;
        if kind.constant_width() && shared.is_none() {
            // The implicit width guide is bounded, unlike an analytical
            // circle. Keep the unwrapped interval needed by an open rim's
            // contact rails; only a closed source needs a complete turn.
            let range = if edge.vertices[0] == edge.vertices[1] {
                center_domain.period.map(|period| ParameterRange::new(0.0, period))
                    .transpose()?.unwrap_or(provisional_range)
            } else { provisional_range };
            let signs = faces.map(|face| {
                let outward = if self.working.face(face).unwrap().orientation == FaceOrientation::Forward { 1.0 } else { -1.0 };
                if concave { outward } else { -outward }
            });
            let guide = SurfaceIntersectionCurve::new_with_width(
                parent_surfaces[0].clone(), parent_surfaces[1].clone(),
                center.first_pcurve.extended(range)?, center.second_pcurve.extended(range)?, range, tolerance,
                tolerance.linear, Some(crate::geometry::ConstantWidthConstraint { width: size, signs }),
            )?;
            let [first_pcurve, second_pcurve] = guide.exact_pcurves();
            center = SurfaceSurfaceCurve { curve: guide.into(), first_pcurve, second_pcurve,
                achieved_error: tolerance.linear };
            provisional_range = range;
        }
        Ok(BlendSpine {
            edge_id,
            edge,
            uses,
            faces,
            parent_surfaces,
            concave,
            offset: radius,
            offsets: offset_values,
            center,
            endpoint_seeds,
            provisional_range,
            shared_seam: shared.map(|spine| spine.edge.vertices[0]),
            local_guide,
        })
    }

    fn blend_side(
        &self,
        edge: &Edge,
        uses: [CoedgeId; 2],
        faces: [FaceId; 2],
        parent_surfaces: &[Surface3; 2],
        midpoint: CurveEvaluation,
    ) -> KernelResult<BlendSide> {
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
        let tangent = midpoint.first_derivative.normalize() * traversal;
        let mut support_normals = [Vec3::new(0.0, 0.0, 0.0); 2];
        let mut parameters = [(0.0, 0.0); 2];
        for index in 0..2 {
            let uv = parent_surfaces[index]
                .project(midpoint.point, self.kernel.tolerance)?
                .parameters;
            parameters[index] = uv;
            let normal = parent_surfaces[index].normal(uv.0, uv.1)?;
            support_normals[index] = normal * self.working.face(faces[index])?.orientation.sign();
        }
        let normals = support_normals.map(|normal| normal.normalize());
        let signed_turn = tangent.dot(normals[0].cross(normals[1]));
        if signed_turn.abs() > self.kernel.tolerance.angular {
            return Ok(BlendSide { concave: signed_turn < 0.0, cusp_inward: None, support_normals });
        }
        if normals[0].dot(normals[1]) < 0.0 {
            // At a cusp both face interiors extend along the same tangent.
            // Their transverse curvature orders the two sheets across it.
            let inward = normals[0].cross(tangent).normalize();
            let bending = |index: usize| {
                let (u, v) = parameters[index];
                let sample = parent_surfaces[index].evaluate(u, v)?;
                let normal = sample.du.cross(sample.dv);
                let area = normal.magnitude2();
                let du = inward.cross(sample.dv).dot(normal) / area;
                let dv = sample.du.cross(inward).dot(normal) / area;
                Ok::<_, KernelError>(normals[0].dot(
                    sample.duu * du.powi(2) + sample.duv * (2.0 * du * dv) + sample.dvv * dv.powi(2),
                ))
            };
            let [first, second] = [bending(0)?, bending(1)?];
            if (first - second).abs() > f64::EPSILON * 64.0 * first.abs().max(second.abs()) {
                return Ok(BlendSide { concave: first < second, cusp_inward: Some(inward), support_normals });
            }
        }
        Err(unsupported("blend edge has no well-defined convex or concave dihedral"))
    }

    /// Use the true transverse section where the tangent-plane setback
    /// formula degenerates at a plane/cylinder cusp.
    fn cusp_chamfer_offsets(
        &self,
        surfaces: &[Surface3; 2],
        faces: [FaceId; 2],
        tip: Point3,
        inward: Vec3,
        concave: bool,
        size: f64,
        dimensions: Option<(usize, ChamferMode)>,
    ) -> KernelResult<[f64; 2]> {
        let (cylindrical, cylinder) = match surfaces {
            [Surface3::Cylinder(cylinder), Surface3::Plane(_)] => (0, cylinder),
            [Surface3::Plane(_), Surface3::Cylinder(cylinder)] => (1, cylinder),
            _ => return Err(unsupported("a cusp chamfer requires a planar and a cylindrical support")),
        };
        let tolerance = self.kernel.tolerance;
        let (u, v) = surfaces[cylindrical].project(tip, tolerance)?.parameters;
        let angular_direction = cylinder.evaluate(u, v)?.du.dot(inward).signum();
        let setback = |side, distance| {
            if side == cylindrical {
                let angle = distance / cylinder.radius();
                if angle >= std::f64::consts::PI {
                    return Err(invalid("chamfer setback exceeds the local cylindrical section"));
                }
                let value = cylinder.evaluate(u + angular_direction * angle, v)?;
                Ok((value.point, value.du.normalize() * angular_direction))
            } else {
                Ok((tip + inward * distance, inward))
            }
        };
        let normal_at = |side: usize, point| -> KernelResult<Vec3> {
            let (u, v) = surfaces[side].project(point, tolerance)?.parameters;
            Ok(surfaces[side].normal(u, v)? * self.working.face(faces[side])?.orientation.sign())
        };
        let (first, mode) = dimensions.unwrap_or((0, ChamferMode::TwoDistances(size)));
        let second = 1 - first;
        let (first_contact, first_tangent) = setback(first, size)?;
        let first_normal = normal_at(first, first_contact)?;
        let sign = if concave { 1.0 } else { -1.0 };
        let second_contact = match mode {
            ChamferMode::TwoDistances(distance) => setback(second, distance)?.0,
            ChamferMode::DistanceAngle(angle) => {
                let direction = -first_tangent * angle.cos() + first_normal * (sign * angle.sin());
                let ray = Curve3::from(crate::Line3::new(first_contact, direction)?);
                let IntersectionSet::Isolated(hits) = ray.intersect_surface(&surfaces[second], tolerance)? else {
                    return Err(invalid("chamfer angle does not intersect the opposite face"));
                };
                hits.into_iter()
                    .filter(|hit| hit.curve_parameter > tolerance.linear
                        && (hit.point - tip).dot(inward) > tolerance.linear)
                    .min_by(|a, b| a.curve_parameter.total_cmp(&b.curve_parameter))
                    .ok_or_else(|| invalid("chamfer angle does not intersect the opposite face"))?
                    .point
            }
        };
        let second_normal = normal_at(second, second_contact)?;
        // The spine is the intersection of the two contact normals. Express
        // it as offsets so the usual trimming and junction code can use it.
        let cross = first_normal.cross(second_normal);
        let denominator = cross.magnitude2();
        if denominator <= tolerance.angular.powi(2) {
            return Err(invalid("cusp chamfer contacts have parallel normals"));
        }
        let delta = second_contact - first_contact;
        let mut offsets = [0.0; 2];
        offsets[first] = sign * delta.cross(second_normal).dot(cross) / denominator;
        offsets[second] = sign * delta.cross(first_normal).dot(cross) / denominator;
        Ok(offsets)
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
                return Err(unsupported("a blend tangent chain branches at one topological vertex"));
            }
        }
        Ok(continuation.map(|edge| (edge, spine.faces)))
    }

    fn smooth_blend_continuation(
        &mut self,
        vertex: VertexId,
        spine: &BlendSpine,
        selected: &BTreeSet<EdgeId>,
        kind: EdgeBlendKind,
    ) -> KernelResult<Option<(EdgeId, EdgeId, [FaceId; 2])>> {
        let current_edge = spine.edge_id;
        let current = self.working.edge(current_edge)?;
        let current_tangent = current
            .endpoint_tangent(usize::from(current.vertices[1] == vertex))?
            .normalize();
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
                normals.push(
                    self.working
                        .face(*face)?
                        .oriented_normal_near(point, self.kernel.tolerance)?,
                );
            }
            if normals[0].dot(normals[1])
                < 1.0 - self.kernel.tolerance.angular.max(f64::EPSILON.sqrt()) * 8.0
            {
                continue;
            }
            // A chamfer over a rounded corner has two smooth seams at this
            // vertex. Continue along the sharp rim, never along the other seam.
            // Tangent supports can also meet at a cusp where a convex rim
            // becomes a concave cavity edge. That is a termination, not a
            // continuation of the same rolling blend.
            let continuation = incident.iter().copied().find(|edge| {
                if *edge == seam || *edge == current_edge {
                    return false;
                }
                let Ok(value) = self.working.edge(*edge) else {
                    return false;
                };
                let Ok(tangent) = value.endpoint_tangent(usize::from(value.vertices[1] == vertex))
                else {
                    return false;
                };
                let continues_direction = current_tangent.dot(tangent.normalize())
                    <= -1.0 + self.kernel.tolerance.angular.max(f64::EPSILON.sqrt()) * 8.0;
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
                    .evaluate(value.range.middle())
                else {
                    return false;
                };
                self.blend_side(
                    value,
                    [value.coedges[0], value.coedges[1]],
                    [faces[0], faces[1]],
                    &supports,
                    midpoint,
                )
                .is_ok_and(|side| continues_direction || side.concave == spine.concave)
            });
            if let Some(continuation) = continuation {
                if kind == EdgeBlendKind::Chamfer && !selected.contains(&continuation) {
                    let candidate = self.prepare_blend_spine(
                        continuation,
                        spine.offset,
                        kind,
                        None,
                        Some(&(spine.clone(), [faces[0], faces[1]])),
                        None,
                        false,
                    );
                    if let Ok(candidate) = candidate {
                        let far_end = usize::from(candidate.edge.vertices[0] == vertex);
                        let endpoint = self.speculate(|transaction| {
                            transaction.prepare_blend_endpoint(&candidate, far_end)
                        });
                        if let Ok(endpoint) = endpoint {
                            // A chamfer cannot terminate after extending both open
                            // neighboring rails past their shared end. Periodic
                            // rails may only need a chart turn, so keep those.
                            if endpoint.allow_adjacent_extension == [true; 2]
                                && self.tangent_chain_continuation(&candidate, far_end)?.is_none()
                                && endpoint.adjacent_uses.iter().all(|use_id| {
                                    self.working
                                        .coedge(*use_id)
                                        .and_then(|use_| self.working.edge(use_.edge))
                                        .is_ok_and(|edge| edge.curve.domain().period.is_none())
                                })
                            {
                                continue;
                            }
                        }
                    }
                }
                return Ok(Some((seam, continuation, [faces[0], faces[1]])));
            }
        }
        Ok(None)
    }

    fn local_offset_intersection(
        &self,
        uses: [CoedgeId; 2],
        offsets: &[Surface3; 2],
        size: f64,
        tolerance: ModelTolerance,
        cylindrical_chart: bool,
        interpolate_guides: bool,
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
                    uv = nearest_uv(offsets[side].domains(), uv, previous);
                }
                boundary_parameters[side].push(uv);
            }
        }
        let source = self.working.edge(self.working.coedge(uses[0])?.edge)?;
        let closed = source.vertices[0] == source.vertices[1];
        // Retain an angular chart through rounded supports. A least-norm
        // correction can slide across the section and choose another inverse
        // branch, especially where a cylindrical corner meets a torus.
        // Between cylinders, use the smaller cylinder's angle to follow its
        // tighter turn instead of extrapolating across the other cylinder.
        // A bounded circular revolution needs its profile angle retained at
        // the ends, where it meets the neighboring blend supports.
        let angular_coordinate = offsets.iter().enumerate().find_map(|(side, surface)| {
            let axis = match surface {
                Surface3::Cylinder(cylinder)
                    if cylindrical_chart
                        && !closed
                        && (boundary_parameters[side][16].x - boundary_parameters[side][0].x)
                            .abs()
                            < std::f64::consts::PI
                        && (matches!(offsets[1 - side], Surface3::Torus(_))
                            || matches!(&offsets[1 - side], Surface3::Cylinder(other) if cylinder.radius() < other.radius())) =>
                {
                    0
                }
                Surface3::Offset(offset)
                    if matches!(offset.base(), Surface3::RollingBall(blend)
                    if matches!(blend.center_curve(), Curve3::Line(_)))
                        || matches!(offset.base(), Surface3::Revolved(surface)
                            if matches!(surface.profile(), Curve2::Circle(_))) =>
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
        // Continue open curved edges by a size-dependent collar. A fixed
        // fraction of the source edge can end before a larger blend's contact
        // reaches its terminating support. Use each endpoint's spatial step
        // to keep the continuation local to the requested treatment.
        let extension = if closed || matches!(source.curve, Curve3::Line(_)) {
            [16; 2]
        } else {
            let parent_loop = self.working.coedge(uses[0])?.loop_id;
            let support = &self.working.face(self.working.loop_(parent_loop)?.face)?.surface;
            // Transition connectors have zero topological length; their
            // retained p-curves still describe the supporting traversal.
            let padding = |index: usize, neighbor: usize| -> KernelResult<i32> {
                let first = boundary_parameters[0][index];
                let second = boundary_parameters[0][neighbor];
                let step = support.evaluate(first.x, first.y)?.point
                    .distance(support.evaluate(second.x, second.y)?.point);
                Ok((2.0 * size / step).ceil().max(2.0) as i32)
            };
            [padding(0, 1)?, padding(16, 15)?]
        };
        let continue_implicit_offsets = !closed
            && offsets.iter().all(|surface| matches!(surface, Surface3::Offset(_)));
        let interpolate_guides = interpolate_guides || continue_implicit_offsets;
        let offset_domains = offsets.each_ref().map(Surface3::domains);
        let ranges_near = |side: usize, seed: [f64; 4]| {
            std::array::from_fn(|axis| {
                let domain = offset_domains[side][axis];
                if let Some(period) = domain.period {
                    let value = seed[side * 2 + axis];
                    return ParameterRange { start: value - period, end: value + period };
                }
                match domain.bounds {
                    DomainBounds::Bounded(range) => range,
                    _ => ParameterRange { start: -f64::MAX / 4.0, end: f64::MAX / 4.0 },
                }
            })
        };
        let extrapolated = |side: usize, index: i32| -> Point2 {
            if closed {
                // A closed spatial edge can have a bounded, nonperiodic UV
                // curve. Continue its lifted chart from one certified turn,
                // rather than evaluating that curve outside its domain.
                let turn = index.div_euclid(16);
                let sample = index.rem_euclid(16) as usize;
                let winding = boundary_parameters[side][16] - boundary_parameters[side][0];
                return boundary_parameters[side][sample] + winding * turn as f64;
            }
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
        // Do not extend a guide past a bounded support. The local corrector
        // clamps those seeds to its natural domain, producing repeated points
        // and a stationary (or folded) spine outside the original edge.
        let in_domains = |index| -> bool {
            for side in 0..2 {
                let uv = extrapolated(side, index);
                for axis in 0..2 {
                    if !offset_domains[side][axis].contains(uv[axis], tolerance.parameter) {
                        return false;
                    }
                }
            }
            true
        };
        let mut first_index = 0;
        let mut last_index = 16;
        while first_index > -extension[0] && in_domains(first_index - 1) {
            first_index -= 1;
        }
        while last_index < 16 + extension[1] && in_domains(last_index + 1) {
            last_index += 1;
        }
        let correct = |index| -> KernelResult<Option<[f64; 4]>> {
            let first = extrapolated(0, index);
            let second = extrapolated(1, index);
            if let Some((side, axis)) = angular_coordinate {
                let guide = [first.x, first.y, second.x, second.y];
                let mut gauge = [0.0; 4];
                gauge[side * 2 + axis] = 1.0;
                super::super::intersection::correct_surface_pair_with_gauge(
                    &offsets[0],
                    &offsets[1],
                    guide,
                    gauge,
                    ranges_near(0, guide),
                    ranges_near(1, guide),
                    tolerance,
                )
            } else {
                super::super::intersection::correct_surface_pair_near(
                    &offsets[0],
                    &offsets[1],
                    [first.x, first.y, second.x, second.y],
                    tolerance,
                ).map(Some)
            }
        };
        // A periodic guide needs its entire turn and padding. Only open-edge
        // continuation may stop when a corrected branch ends.
        let mut first_sample = if closed { first_index - 1 } else { 0 };
        let mut last_sample = if closed { last_index + 1 } else { 16 };
        let mut samples = VecDeque::with_capacity((last_sample - first_sample + 1) as usize);
        for index in first_sample..=last_sample {
            let corrected = correct(index)?;
            if corrected.is_none() && matches!(angular_coordinate, Some((_, 0))) {
                return self.local_offset_intersection(uses, offsets, size, tolerance, false, interpolate_guides);
            }
            samples.push_back(corrected.ok_or_else(|| KernelError::new(
                DiagnosticKind::NumericalFailure, "fixed angular offset correction failed",
            ))?);
        }
        // Continue from the retained sector, stopping when this branch ends.
        // A failed distant sample must not select another chart for the spine.
        if !closed {
            let continue_branch = |previous: [f64; 4], before: [f64; 4]| -> KernelResult<Option<[f64; 4]>> {
                // Extrapolating the source p-curves past their trims can
                // seed unrelated roots on folded offset supports. Predict
                // from the last two corrected samples of the offset spine.
                let step: [f64; 4] = std::array::from_fn(|i| {
                    previous[i] - offset_domains[i / 2][i % 2].nearest(before[i], previous[i])
                });
                let length = step.iter().map(|value| value * value).sum::<f64>().sqrt();
                if length <= tolerance.parameter { return Ok(None); }
                let gauge = step.map(|value| value / length);
                for scale in [1.0, 0.5, 0.25] {
                    let seed = std::array::from_fn(|i| previous[i] + scale * step[i]);
                    let candidate = super::super::intersection::correct_surface_pair_with_gauge(
                        &offsets[0], &offsets[1], seed, gauge,
                        ranges_near(0, previous), ranges_near(1, previous), tolerance,
                    )?;
                    if let Some(candidate) = candidate {
                        let moved = (0..4).map(|i| (candidate[i] - previous[i]).powi(2)).sum::<f64>();
                        if moved <= (scale * length * 2.0).powi(2) { return Ok(Some(candidate)); }
                    }
                }
                Ok(None)
            };
            for index in (first_index..0).rev() {
                let sample = if continue_implicit_offsets {
                    continue_branch(samples[0], samples[1])
                } else { correct(index) };
                let Ok(Some(sample)) = sample else { break; };
                samples.push_front(sample);
                first_sample = index;
            }
            for index in 17..=last_index {
                let sample = if continue_implicit_offsets {
                    continue_branch(samples[samples.len() - 1], samples[samples.len() - 2])
                } else { correct(index) };
                let Ok(Some(sample)) = sample else { break; };
                samples.push_back(sample);
                last_sample = index;
            }
        }
        first_index = first_sample + i32::from(closed);
        last_index = last_sample - i32::from(closed);
        let support_range =
            ParameterRange::new(first_index as f64 / 16.0, last_index as f64 / 16.0)?;
        let mut parameters: [Vec<Point2>; 2] = [Vec::new(), Vec::new()];
        for (index, corrected) in (first_sample..=last_sample).zip(samples) {
            // Correction normalizes periodic coordinates. Lift them back onto
            // the sampled branch before interpolating a continuous guide.
            for side in 0..2 {
                let seed = extrapolated(side, index);
                parameters[side].push(nearest_uv(
                    offset_domains[side],
                    Point2::new(corrected[side * 2], corrected[side * 2 + 1]),
                    seed,
                ));
            }
        }
        let guide = |points: &[Point2]| -> KernelResult<Curve2> {
            if closed {
                return super::super::intersection::periodic_nurbs_guide(points, support_range).map(Into::into);
            }
            if !interpolate_guides {
                return super::super::intersection::smooth_nurbs_guide(points, support_range).map(Into::into);
            }
            let parameters = (first_index..=last_index).map(|index| index as f64 / 16.0).collect::<Vec<_>>();
            let guide = super::super::intersection::interpolating_nurbs_guide(points, &parameters)?;
            super::super::intersection::smooth_interpolating_guide(&guide)
        };
        let first_pcurve = guide(&parameters[0])?;
        let second_pcurve = guide(&parameters[1])?;
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
