//! Free ends of a blend network: contacts with the terminating face, capped
//! partial chamfers, and the exact boundary closing each end.

use super::*;
use crate::{invalid, topology_error, unsupported};

impl Transaction<'_> {
    // A selected circular segment can stop before its tangent continuation.
    // Its inset rail misses the unselected tangent edge; retain that endpoint
    // and close the bevel with a transverse triangular face instead.
    pub(super) fn prepare_capped_endpoint(
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
                topology_error("partial blend has no terminating face")
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
            return Err(unsupported("partial blend cannot close on a transverse end plane"));
        }
        Ok(BlendEndpoint {
            old_vertex: vertex,
            terminating_face,
            tangent_termination: None,
            crossing: None,
            adjacent_uses: uses,
            contact_parameters: [parameter; 2],
            contact_points: points,
            allow_adjacent_extension: [false; 2],
            cap: Some((free[0], plane)),
        })
    }

    pub(super) fn prepare_blend_endpoint(
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
            return Err(unsupported("a regular blend endpoint requires one face or two tangent terminating faces"));
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
            let adjacent_seed = if adjacent_edge.vertices[0] == old_vertex {
                adjacent_edge.range.start
            } else {
                adjacent_edge.range.end
            };
            // Termination can grow a retained intersection or contact rail
            // past its old domain. Continue its exact supports before solving
            // the contact, so projection is not clamped to the old endpoint.
            let adjacent_range = match &adjacent_edge.curve {
                Curve3::SurfaceIntersection(curve) => Some(curve.range()),
                Curve3::SurfaceCurve(curve)
                    if contact_curve
                        .intersect_curve_near(
                            &adjacent_edge.curve,
                            spine.endpoint_seeds[endpoint_index],
                            adjacent_seed,
                            tolerance,
                        )
                        .is_err() =>
                {
                    Some(curve.range())
                }
                _ => None,
            };
            if let Some(range) = adjacent_range {
                let endpoint = if adjacent_edge.vertices[0] == old_vertex {
                    adjacent_edge.range.start
                } else {
                    adjacent_edge.range.end
                };
                let value = adjacent_edge.curve.evaluate(endpoint)?;
                let target = contact_curve.evaluate(spine.endpoint_seeds[endpoint_index])?.point;
                let delta = (target - value.point).dot(value.first_derivative)
                    / value.first_derivative.magnitude2();
                let outside_start =
                    (endpoint - range.start).abs() <= tolerance.parameter && delta < 0.0;
                let outside_end = (endpoint - range.end).abs() <= tolerance.parameter && delta > 0.0;
                if outside_start || outside_end {
                    let padding =
                        4.0 * target.distance(value.point) / value.first_derivative.magnitude();
                    let extended_range = ParameterRange::new(
                        range.start - if outside_start { padding } else { 0.0 },
                        range.end + if outside_end { padding } else { 0.0 },
                    )?;
                    let extended: Curve3 = match &adjacent_edge.curve {
                        Curve3::SurfaceIntersection(curve) => {
                            curve.extended(extended_range)?.into()
                        }
                        Curve3::SurfaceCurve(curve) => SurfaceCurve3::new(
                            curve.surface().clone(),
                            curve.pcurve().extended(extended_range)?,
                            extended_range,
                            tolerance,
                        )?
                        .into(),
                        _ => unreachable!(),
                    };
                    let uses = adjacent_edge
                        .coedges
                        .iter()
                        .map(|id| {
                            let coedge = self.working.coedge(*id)?;
                            let start = coedge.use_parameter(adjacent_edge, extended_range.start);
                            let end = coedge.use_parameter(adjacent_edge, extended_range.end);
                            let range = ParameterRange::new(start.min(end), start.max(end))?;
                            Ok((*id, coedge.pcurve.extended(range)?))
                        })
                        .collect::<KernelResult<Vec<_>>>()?;
                    for (id, pcurve) in uses {
                        self.working.coedges.get_mut(id)?.pcurve = pcurve;
                    }
                    self.working.edges.get_mut(adjacent_edge_id)?.curve = extended;
                }
            }
            let adjacent_edge = self.working.edge(adjacent_edge_id)?;
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
            // Local correction can select a different intersection branch on
            // an extended adjacent rail. Prefer a hit on the retained edge
            // and contact span before allowing either rail to extend.
            let retained_contact = ParameterRange {
                start: spine.endpoint_seeds[0],
                end: spine.endpoint_seeds[1],
            };
            let on_retained_edges = |hit: &super::super::CurveCurveHit| {
                retained_contact.contains(hit.first_parameter, tolerance)
                    && adjacent_edge.range.contains(hit.second_parameter, tolerance)
            };
            let search_retained = match &local_hit {
                Ok(hit) => {
                    matches!(adjacent_edge.curve, Curve3::SurfaceIntersection(_))
                        && !retained_contact.contains(hit.first_parameter, tolerance)
                        && !adjacent_edge.range.contains(hit.second_parameter, tolerance)
                }
                Err(_) => matches!(adjacent_edge.curve, Curve3::Circle(_)),
            };
            let local_hit = if !search_retained {
                local_hit
            } else {
                let first_seeds = [
                    seed,
                    spine.endpoint_seeds[endpoint_index],
                    (retained_contact.start + retained_contact.end) * 0.5,
                ];
                let second_seeds = [
                    rail_seed.parameter,
                    adjacent_edge.range.start,
                    (adjacent_edge.range.start + adjacent_edge.range.end) * 0.5,
                    adjacent_edge.range.end,
                ];
                let mut retained_hits = Vec::new();
                for first in first_seeds {
                    for second in second_seeds {
                        if let Ok(hit) = contact_curve.intersect_curve_near(
                            &adjacent_edge.curve,
                            first,
                            second,
                            tolerance,
                        ) {
                            if on_retained_edges(&hit) {
                                retained_hits.push(hit);
                            }
                        }
                    }
                }
                retained_hits
                    .into_iter()
                    .min_by(|first, second| {
                        (first.first_parameter - spine.endpoint_seeds[endpoint_index])
                            .abs()
                            .total_cmp(
                                &(second.first_parameter - spine.endpoint_seeds[endpoint_index])
                                    .abs(),
                            )
                    })
                    .map(Ok)
                    .unwrap_or(local_hit)
            };
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
        // with its other contact extending their shared seam. Supports that
        // meet at an angle each carry the part of the boundary from the
        // contact on their own adjacent edge to where it crosses the seam.
        let mut tangent_termination = None;
        let mut crossing = None;
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
                    unsupported("blend terminating supports have no shared seam")
                })?;
            let point = self.working.vertex(old_vertex)?.point;
            let mut normals = Vec::new();
            for face in &terminating_faces {
                normals.push(self.working.face(*face)?.oriented_normal_near(point, tolerance)?);
            }
            if normals[0].dot(normals[1]) < 1.0 - tolerance.angular * 8.0 {
                let mut faces = [terminating_faces[0]; 2];
                for parent in 0..2 {
                    let adjacent = self.working.coedge(adjacent_uses[parent])?.edge;
                    let adjacent_faces = self.working.edge_faces(adjacent)?;
                    faces[parent] = terminating_faces
                        .iter()
                        .copied()
                        .find(|face| adjacent_faces.contains(face))
                        .ok_or_else(|| {
                            topology_error("blend rail does not bound a terminating support")
                        })?;
                }
                if faces[0] == faces[1] {
                    return Err(unsupported("blend rails end on the same non-tangent terminating support"));
                }
                terminating_face = faces[0];
                crossing = Some((seam, faces[1]));
            } else {
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
                    return Err(unsupported("blend endpoint crosses both terminating supports"));
                }
            }
        }
        if spine.concave && allow_adjacent_extension.iter().all(|extension| *extension) {
            return Err(invalid("blend radius reaches beyond an adjacent boundary edge"));
        }
        Ok(BlendEndpoint {
            old_vertex,
            terminating_face,
            tangent_termination,
            crossing,
            adjacent_uses,
            contact_parameters,
            contact_points,
            allow_adjacent_extension,
            cap: None,
        })
    }

    /// The chart point of a terminating face at a blended vertex, from that
    /// face's use of an edge through the vertex.
    pub(super) fn terminating_chart_reference(
        &self,
        edge: EdgeId,
        face: FaceId,
        vertex: VertexId,
    ) -> KernelResult<Point2> {
        let (id, coedge) = self
            .working
            .edge(edge)?
            .coedges
            .iter()
            .filter_map(|id| self.working.coedge(*id).ok().map(|coedge| (*id, coedge)))
            .find(|(_, coedge)| {
                self.working
                    .loop_(coedge.loop_id)
                    .is_ok_and(|loop_| loop_.face == face)
            })
            .ok_or_else(|| {
                topology_error("blend endpoint lost its terminating chart")
            })?;
        let traversal = self.working.coedge_vertices(id)?;
        Ok(coedge
            .pcurve
            .evaluate(if traversal[0] == vertex {
                coedge.range.start
            } else {
                coedge.range.end
            })?
            .point)
    }

    /// Split the end boundary `blend_pcurve` where it crosses the seam between
    /// two non-tangent terminating supports. Returns its part on the first
    /// support, the crossing point and its part on the second support.
    pub(super) fn prepare_crossing_end(
        &self,
        blend_surface: &Surface3,
        blend_pcurve: &Curve2,
        endpoint: &BlendEndpoint,
        contact_points: [Point3; 2],
    ) -> KernelResult<(BlendBoundary, Point3, BlendBoundary)> {
        let tolerance = self.kernel.tolerance;
        let (seam, second_face) = endpoint.crossing.ok_or_else(|| {
            topology_error("blend end boundary has no terminating seam to cross")
        })?;
        let seam_edge = self.working.edge(seam)?;
        let [start, middle, end] = [0.0, 0.5, 1.0]
            .map(|parameter| blend_pcurve.evaluate(parameter).map(|value| value.point));
        let (start, middle, end) = (start?, middle?, end?);
        let seed = seam_edge
            .curve
            .project(blend_surface.evaluate(middle.x, middle.y)?.point, tolerance)?
            .parameter;
        let hit = seam_edge
            .curve
            .intersect_surface_near(blend_surface, seed, (middle.x, middle.y), tolerance)?;
        let uv = nearest_uv(
            blend_surface.domains(),
            Point2::new(hit.surface_parameters.0, hit.surface_parameters.1),
            middle,
        );
        let fraction = (uv - start).dot(end - start) / (end - start).magnitude2();
        if !seam_edge.range.contains(hit.curve_parameter, tolerance)
            || fraction <= 0.0
            || fraction >= 1.0
        {
            return Err(unsupported("blend end boundary does not cross its terminating seam between its contacts"));
        }
        let first_edge = self.working.coedge(endpoint.adjacent_uses[0])?.edge;
        let second_edge = self.working.coedge(endpoint.adjacent_uses[1])?.edge;
        let first = self.prepare_end_boundary(
            blend_surface,
            self.working.face(endpoint.terminating_face)?.surface.clone(),
            Line2::new(start, uv - start)?.into(),
            [contact_points[0], hit.point],
            self.terminating_chart_reference(
                first_edge,
                endpoint.terminating_face,
                endpoint.old_vertex,
            )?,
        )?;
        let second = self.prepare_end_boundary(
            blend_surface,
            self.working.face(second_face)?.surface.clone(),
            Line2::new(uv, end - uv)?.into(),
            [hit.point, contact_points[1]],
            self.terminating_chart_reference(second_edge, second_face, endpoint.old_vertex)?,
        )?;
        Ok((first, hit.point, second))
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
        let domain = candidate.curve.domain();
        let middle_parameter = domain.nearest(middle_projection.parameter, first.parameter);
        second.parameter = domain.nearest(second.parameter, middle_parameter * 2.0 - first.parameter);
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

    pub(super) fn prepare_end_boundary(
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
            let corrected = match super::super::intersection::correct_surface_pair_near(
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
            blend_parameters.push(nearest_uv(
                blend_surface.domains(),
                Point2::new(corrected[0], corrected[1]),
                blend_parameters.last().copied().unwrap_or(blend_uv),
            ));
            parent_parameters.push(nearest_uv(
                terminating_surface.domains(),
                Point2::new(corrected[2], corrected[3]),
                parent_parameters.last().copied().unwrap_or(reference),
            ));
        }
        let blend_guide: Curve2 =
            super::super::intersection::interpolating_nurbs_guide(&blend_parameters, &guide_parameters)?
                .into();
        let parent_guide: Curve2 =
            super::super::intersection::interpolating_nurbs_guide(&parent_parameters, &guide_parameters)?
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
}
