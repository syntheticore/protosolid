//! The face patch each spine contributes: its simplest exact blend surface,
//! contact and end boundaries, and certification against the parent trims.

use super::*;

impl Transaction<'_> {
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
            RollingBallSurface3::with_section(
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
                true,
            )?
            .with_center_period(spine.center_domain().period)
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
            .with_center_period(spine.center_domain().period)
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

    pub(super) fn prepare_blend_patch(
        &self,
        mut spine: BlendSpine,
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
                    || matches!(junction.kind, BlendJunctionKind::TangentChain { .. })
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
                        let tangent_chain =
                            matches!(junction.kind, BlendJunctionKind::TangentChain { .. });
                        // Retain the incident branch before correcting each
                        // contact. A global rail projection can select another
                        // turn of the same closed support.
                        let mut parameter = if tangent_chain {
                            if spine.local_guide {
                                project_local_blend_endpoint(
                                    &spine.center.curve, junction.center, endpoint_index, tolerance,
                                )?
                            } else {
                                spine.center.curve.project(junction.center, tolerance)?.parameter
                            }
                        } else {
                            rail.project(point, tolerance)?.parameter
                        };
                        parameter = spine
                            .center_domain()
                            .nearest(parameter, spine.endpoint_seeds[endpoint_index]);
                        if tangent_chain || spine.shared_seam.is_some() {
                            parameter = correct_blend_contact_parameter(&rail, point, parameter)?;
                        }
                        if rail.evaluate(parameter)?.point.distance(point)
                            > tolerance.linear_at_scale(point.to_vec().magnitude()) * 16.0
                        {
                            return Err(KernelError::new(
                                DiagnosticKind::GeometryInvariant,
                                "blend corner misses its incident contact rail",
                            ));
                        }
                        contact_parameters[endpoint_index][parent] = parameter;
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
                    UNIT_PERIOD.nearest(projected, spine.endpoint_seeds[endpoint_index])
                } else {
                    blend_parameter_near(
                        &spine.center.curve,
                        projected,
                        spine.endpoint_seeds[endpoint_index],
                        tolerance,
                    )?
                };
                if spine.local_guide {
                    parameter = continue_local_blend_parameter(
                        &mut spine, junction.center, parameter,
                    )?;
                } else if spine.shared_seam.is_some() {
                    // Repeated guide turns need not have identical parameter
                    // speeds, so an integer shift alone is only a seed.
                    // Resolve the shared parameter to numerical accuracy: a
                    // model-tolerance error on the center curve can grow on
                    // its contact rails and separate them from their vertex.
                    parameter = correct_blend_contact_parameter(
                        &spine.center.curve, junction.center, parameter,
                    )?;
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
        // A periodic support belongs to the whole circle, independently of
        // temporary spine trims. Keep one chart for adjacent blend patches.
        let periodic_range = center_domain.period
            .map(|period| ParameterRange::new(0.0, period).unwrap())
            .filter(|range| {
                spine.center.first_pcurve.domain().contains_range(*range, tolerance.parameter)
                    && spine.center.second_pcurve.domain().contains_range(*range, tolerance.parameter)
            });
        let blend_support_range = match (periodic_range, center_domain.bounds) {
            (Some(range), _) => range,
            (None, _) if center_domain.period.is_some() => {
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
            (None, DomainBounds::Bounded(range)) => range,
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
                // Temporary arcs trim one shared rail. Do not bake each arc's
                // trim into a different bounded SurfaceCurve support.
                let support_range = if spine.shared_seam.is_some() || periodic_range.is_some() {
                    blend_support_range
                } else {
                    range
                };
                let blend_curve = blend_surface.boundary_curve(
                    blend_pcurve.clone(),
                    support_range,
                    tolerance,
                )?;
                let curve = if matches!(blend_curve, Curve3::Line(_) | Curve3::Circle(_)) {
                    blend_curve
                } else {
                    spine.parent_surfaces[parent_index].boundary_curve(
                        parent_pcurve.clone(),
                        support_range,
                        tolerance,
                    )?
                };
                Ok(BlendBoundary {
                    curve,
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
        let mut crossings = [None, None];
        let end_boundaries = (0..2)
            .map(|endpoint_index| {
                let blend_pcurve = blend_parameterization.end_pcurve(
                    contact_parameters[endpoint_index][0],
                    contact_parameters[endpoint_index][1],
                )?;
                match &ends[endpoint_index] {
                    BlendPatchEnd::Boundary(endpoint) if endpoint.crossing.is_some() => {
                        let (first, point, second) = self.prepare_crossing_end(
                            &blend_surface,
                            &blend_pcurve,
                            endpoint,
                            contact_points[endpoint_index],
                        )?;
                        crossings[endpoint_index] = Some((point, second));
                        Ok(first)
                    }
                    BlendPatchEnd::Boundary(endpoint) => {
                        let terminating_surface = match endpoint.cap {
                            Some((_, plane)) => Surface3::from(plane),
                            None => self
                                .working
                                .face(endpoint.terminating_face)?
                                .surface
                                .clone(),
                        };
                        let reference = if endpoint.cap.is_some() {
                            let uv = terminating_surface
                                .project(contact_points[endpoint_index][0], tolerance)?
                                .parameters;
                            Point2::new(uv.0, uv.1)
                        } else {
                            let adjacent_edge = match endpoint.tangent_termination {
                                Some((seam, 0)) => seam,
                                _ => self.working.coedge(endpoint.adjacent_uses[0])?.edge,
                            };
                            self.terminating_chart_reference(
                                adjacent_edge,
                                endpoint.terminating_face,
                                endpoint.old_vertex,
                            )?
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
                        let curve = blend_surface.boundary_curve(
                            blend_pcurve.clone(),
                            unit,
                            tolerance,
                        )?;
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
                                uv.x = junction_surface.domains()[0].nearest(uv.x, previous.x);
                            }
                            parameters.push(uv);
                        }
                        let guide: Curve2 =
                            super::super::intersection::smooth_nurbs_guide(&parameters, unit)?.into();
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
            crossings,
            blend_surface,
            blend_parameterization,
        })
    }

    /// Certify that each new contact curve remains in the trimmed domain of
    /// its parent face. Crossing another loop would make the local
    /// Euler rewrite publish a self-overlapping shell instead of a blend.
    pub(super) fn certify_blend_contact_trims(&self, patches: &[BlendPatch]) -> KernelResult<()> {
        let tolerance = self.kernel.tolerance;
        for patch in patches {
            for parent_index in 0..2 {
                let face_id = patch.spine.faces[parent_index];
                let surface = &self.working.face(face_id)?.surface;
                let face_boundary = face_boundary_curves(&self.working, face_id, tolerance)?;
                let boundary = &patch.contact_boundaries[parent_index];
                let pcurve = align_pcurve_to_face_chart(
                    &boundary.parent_pcurve, Some(boundary.range), surface, &face_boundary, tolerance,
                )?;
                let contacts = intersection_chart_copies(
                    DomainBounds::Bounded(boundary.range), &pcurve, surface,
                    face_parameter_bounds(&face_boundary, tolerance)?, tolerance,
                )?;
                let contact = TrimmedCurve2::new(pcurve, boundary.range)?;
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
                        .working
                        .loop_coedges(loop_id)?
                        .into_iter()
                        .filter(|coedge_id| {
                            self.working
                                .coedge(*coedge_id)
                                .is_ok_and(|coedge| coedge.range.span() != 0.0)
                        })
                        .map(|coedge_id| {
                            let coedge = self.working.coedge(coedge_id)?;
                            let chart_curve = face_boundary.iter()
                                .find(|curve| curve.provenance == coedge_id)
                                .expect("face boundary contains every loop use");
                            Ok((
                                coedge_id,
                                TrimmedCurve2::new(chart_curve.pcurve.clone(), coedge.range)?,
                            ))
                        })
                        .collect::<KernelResult<Vec<_>>>()?;
                    for (coedge_id, segment) in &identified_segments {
                        // A periodic chart cut is not a material boundary.
                        if self.working.edge_faces(self.working.coedge(*coedge_id)?.edge)?.len() == 1 {
                            continue;
                        }
                        for pcurve in &contacts {
                            let contact = TrimmedCurve2::new(pcurve.clone(), boundary.range)?;
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
                    }
                    if closed {
                        let segments = identified_segments
                            .into_iter()
                            .map(|(_, segment)| segment)
                            .collect::<Vec<_>>();
                        let contains = curve_loop_contains_point(&segments, middle, tolerance)?;
                        match self.working.loop_(loop_id)?.kind {
                            super::super::LoopKind::Outer => outer_contains_middle |= contains,
                            super::super::LoopKind::Inner if contains => inside_existing_hole = true,
                            super::super::LoopKind::Inner => {}
                        }
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
}

fn signed_periodic_delta(start: f64, end: f64) -> f64 {
    (end - start + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

// Continue a local intersection only as far as its solved junction requires.
// A fixed fraction of the source edge can end before the corner on short arcs.
// Extend both exact contact rails together with the implicit center curve.
fn continue_local_blend_parameter(
    spine: &mut BlendSpine,
    point: Point3,
    mut parameter: f64,
) -> KernelResult<f64> {
    let target = f64::EPSILON * point.to_vec().magnitude().max(1.0) * 2048.0;
    for _ in 0..16 {
        let value = spine.center.curve.evaluate(parameter)?;
        let residual = value.point - point;
        if residual.magnitude() <= target {
            break;
        }
        let next = parameter
            - residual.dot(value.first_derivative) / value.first_derivative.magnitude2();
        if let Curve3::SurfaceIntersection(curve) = &spine.center.curve {
            let range = curve.range();
            if next < range.start || next > range.end {
                let range = ParameterRange::new(range.start.min(next), range.end.max(next))?;
                let extended = curve.extended(range)?;
                let [first, second] = extended.exact_pcurves();
                spine.center.curve = extended.into();
                spine.center.first_pcurve = first;
                spine.center.second_pcurve = second;
                spine.provisional_range = range;
            }
        }
        parameter = next;
    }
    Ok(parameter)
}

// Refine within the already selected branch. Callers certify the remaining
// geometric residual against their junction or contact tolerance.
fn correct_blend_contact_parameter(
    curve: &Curve3,
    point: Point3,
    mut parameter: f64,
) -> KernelResult<f64> {
    let target = f64::EPSILON * point.to_vec().magnitude().max(1.0) * 2048.0;
    for _ in 0..16 {
        let value = curve.evaluate(parameter)?;
        let residual = value.point - point;
        if residual.magnitude() <= target {
            break;
        }
        parameter -= residual.dot(value.first_derivative) / value.first_derivative.magnitude2();
    }
    Ok(parameter)
}
