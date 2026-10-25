//! Corner transitions where blend spines meet: miters, trihedral corners,
//! inner-rim corners, and smooth and tangent-chain continuations.

use super::*;
use crate::{geometry_error, invalid, topology_error, unsupported};

impl Transaction<'_> {
    pub(super) fn prepare_miter_junction(
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
                return Err(topology_error("miter contact needs two incident curves"));
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

    pub(super) fn prepare_chamfer_junction(
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
                return Err(topology_error("a chamfer corner needs two contact rails per parent face"));
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

    pub(super) fn prepare_fillet_junction(
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
            return Err(unsupported("a rolling-ball vertex transition requires a regular valence-three BREP vertex"));
        }
        let third_face = vertex_faces
            .iter()
            .copied()
            .find(|face| !spine.faces.contains(face))
            .ok_or_else(|| {
                topology_error("blend junction has no third incident face")
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
                return Err(geometry_error("trihedral blend contact is not at the requested rolling-ball radius"));
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

    pub(super) fn prepare_rim_corner_junction(
        &self,
        vertex: VertexId,
        kind: BlendJunctionKind,
    ) -> KernelResult<BlendJunction> {
        let BlendJunctionKind::RimCorner {
            carrier_face,
            seam_edge,
        } = kind
        else {
            return Err(invalid("rim-corner preparation requires a rim-corner junction"));
        };
        let faces = self.working.vertex_faces(vertex)?;
        if faces.len() != 3 || self.working.vertex_edges(vertex)?.len() != 3 {
            return Err(unsupported("an inner-rim transition requires a regular valence-three BREP vertex"));
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

    pub(super) fn rim_corner_surface(
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
            return Err(unsupported("inner-rim corner sweeps currently require a planar carrier face"));
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
            return Err(topology_error("inner-rim seam is not incident to its corner vertex"));
        };
        let seam_tangent = seam.curve.evaluate(seam_parameter)?.first_derivative;
        if seam_tangent.normalize().cross(outward).magnitude() > tolerance.angular * 32.0 {
            return Err(unsupported("inner-rim seam must be normal to its planar carrier face"));
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
    pub(super) fn split_smooth_blend_transition(
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
                self.working
                    .face(*face)?
                    .oriented_normal_near(point, self.kernel.tolerance)
            })
            .collect::<KernelResult<Vec<_>>>()?;
        if normals[0].dot(normals[1])
            < 1.0 - self.kernel.tolerance.angular.max(f64::EPSILON.sqrt()) * 8.0
        {
            return Err(original_error);
        }
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
            let attempt = self.atomic(|transaction| {
                let bridge = transaction.insert_blend_transition_bridge(vertex, cross, selected)?;
                let mut selected = selected.clone();
                selected.insert(bridge);
                match transaction.blend_network(selected, radius, follow, EdgeBlendKind::Fillet, None)? {
                    BlendOutcome::Built => Ok(()),
                    BlendOutcome::Staged(_) => Err(unsupported("a bridged blend transition needs a staged corner")),
                }
            });
            if attempt.is_ok() {
                return attempt;
            }
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
                topology_error("transition bridge has no source boundary")
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
                topology_error("transition source has no support coedge")
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
            super::super::intersection::interpolating_nurbs_guide(&guide, &parameters)?.into(),
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
        let name = self.names.next_derived(DerivationRole::Fillet(BlendRole::TransitionVertex))?;
        let new_vertex = self.make_vertex(name, point);
        self.history.derived(
            name,
            [self.working.vertex(vertex)?.name],
            DerivationRole::Fillet(BlendRole::TransitionVertex),
        );
        // Move the source and the adjacent seam on the first support to the
        // second vertex, leaving a gap in each cross-support loop.
        let source_faces = self.working.edge_faces(source_id)?;
        let moved_face = *source_faces
            .iter()
            .find(|face| **face != cross[1])
            .ok_or_else(|| {
                topology_error("transition source has no opposite support")
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
        let name = self.names.next_derived(DerivationRole::Fillet(BlendRole::TransitionEdge))?;
        let edge = self.make_edge(name, [vertex, new_vertex], curve.into(), range)?;
        self.history.derived(
            name,
            [self.working.vertex(vertex)?.name],
            DerivationRole::Fillet(BlendRole::TransitionEdge),
        );
        for side in 0..2 {
            let mut gap = None;
            for loop_id in self.working.face(cross[side])?.loops.clone() {
                for previous in self.working.loop_coedges(loop_id)? {
                    let next = self.working.coedge(previous)?.next;
                    let a = self.working.coedge_vertices(previous)?[1];
                    let b = self.working.coedge_vertices(next)?[0];
                    if (a == vertex && b == new_vertex) || (a == new_vertex && b == vertex) {
                        gap = Some((previous, next, loop_id, a == vertex));
                    }
                }
            }
            let (previous, next, loop_id, forward) = gap.ok_or_else(|| {
                topology_error("transition bridge has no loop gap")
            })?;
            let name = self.names.next_derived(DerivationRole::Fillet(BlendRole::TransitionUse))?;
            let coedge =
                self.make_coedge(name, edge, loop_id, forward, pcurves[side].clone(), range)?;
            self.history.derived(
                name,
                [self.working.edge(edge)?.name],
                DerivationRole::Fillet(BlendRole::TransitionUse),
            );
            self.insert_loop_coedge(previous, next, coedge)?;
        }
        Ok(edge)
    }

    pub(super) fn prepare_smooth_fillet_junction(
        &self,
        vertex: VertexId,
        spine: &BlendSpine,
        seam_edge: EdgeId,
        tangent_faces: [FaceId; 2],
    ) -> KernelResult<BlendJunction> {
        let tolerance = self.kernel.tolerance;
        let concave = spine.concave;
        let tangent_index = spine
            .faces
            .iter()
            .position(|face| tangent_faces.contains(face))
            .unwrap();
        let tangent_radius = spine.offsets[tangent_index];
        let common_radius = spine.offsets[1 - tangent_index];
        let radius = spine.offset;
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
                        unsupported("smooth blend transition has no opposite support seam")
                    })?;
                Some((edge, pair))
            } else if vertex_faces.len() == 3 && incident.len() == 3 && other_faces.len() == 1 {
                None
            } else {
                return Err(unsupported("a smooth blend transition requires three faces or two pairs of tangent supports"));
            };
        let common_face = other_faces[0];
        let seam = self.working.edge(seam_edge)?.clone();
        let seam_parameter = if seam.vertices[0] == vertex {
            seam.range.start
        } else if seam.vertices[1] == vertex {
            seam.range.end
        } else {
            return Err(topology_error("smooth blend seam is not incident to its junction vertex"));
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
                topology_error("smooth blend seam has no use on its tangent support")
            })?;
        let seam_pcurve = self.working.coedge(tangent_use)?.pcurve.clone();
        let tangent_outward = match self.working.face(tangent_face)?.orientation {
            FaceOrientation::Forward => tangent_radius,
            FaceOrientation::Reversed => -tangent_radius,
        };
        let tangent_distance = if concave {
            tangent_outward
        } else {
            -tangent_outward
        };
        let common_surface = self.working.face(common_face)?.surface.clone();
        let common_outward = match self.working.face(common_face)?.orientation {
            FaceOrientation::Forward => common_radius,
            FaceOrientation::Reversed => -common_radius,
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
                return Err(geometry_error("smooth blend seam reached a singular tangent support"));
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
            // The adjoining edges share this vertex. Converge their common
            // center beyond model tolerance before projecting its contacts.
            let allowed = f64::EPSILON * 2048.0
                * center
                    .to_vec()
                    .magnitude()
                    .max(common.point.to_vec().magnitude())
                    .max(1.0);
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
            let Some(delta) = super::super::linalg::solve_linear(
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
        for (contact, offset) in [
            (seam_contact, tangent_radius),
            (common_contact, common_radius),
        ] {
            if (contact.distance(center) - offset.abs()).abs() > allowed {
                return Err(geometry_error("smooth blend junction contact is not at the requested rolling-ball radius"));
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
                return Err(unsupported("smooth blend contacts do not meet the opposite support seam"));
            }
            let mut normals = [Vec3::new(0.0, 0.0, 0.0); 2];
            for side in 0..2 {
                let face = self.working.face(pair[side])?;
                let uv = face.surface.project(common_contact, tolerance)?;
                if uv.distance > allowed {
                    return Err(geometry_error("smooth blend contact misses an opposite support"));
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
                return Err(unsupported("opposite blend supports are not tangent at their seam"));
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

    pub(super) fn prepare_tangent_chain_junction(
        &self,
        spine: &BlendSpine,
        endpoint_index: usize,
        tangent_faces: [FaceId; 2],
    ) -> KernelResult<BlendJunction> {
        let tolerance = self.kernel.tolerance;
        let old_vertex = spine.edge.vertices[endpoint_index];
        let old_point = self.working.vertex(old_vertex)?.point;
        let projected = spine.center.curve.project(old_point, tolerance)?.parameter;
        let parameter = spine
            .center_domain()
            .nearest(projected, spine.endpoint_seeds[endpoint_index]);
        // Each support seam meets its own contact rail. On general surfaces
        // those intersections need not share a center-curve parameter; the
        // new blend seam connects the two distinct contact stations.
        let mut contact_parameters = [parameter; 2];
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
            contact_parameters[parent] = contact
                .intersect_curve_near(&seam.curve, parameter, seam_parameter, tolerance)?
                .first_parameter;
        }
        let center = spine.center.curve.evaluate(parameter)?.point;
        let mut contact_points = BTreeMap::new();
        for parent_index in 0..2 {
            let uv = if parent_index == 0 {
                spine.center.first_pcurve.evaluate(contact_parameters[parent_index])?.point
            } else {
                spine.center.second_pcurve.evaluate(contact_parameters[parent_index])?.point
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
}
