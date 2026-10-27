use super::*;

impl Profile {
    pub fn revolve(&self, plane: Plane3, axis: Axis3, angle: f64) -> KernelResult<Compound> {
        if !angle.is_finite() || angle.abs() <= self.tolerance().parameter {
            return Err(invalid("revolution angle must be finite and nonzero"));
        }
        if angle.abs() > std::f64::consts::TAU + self.tolerance().parameter {
            return Err(invalid("revolution angle may not exceed one full turn"));
        }
        let full = (angle.abs() - std::f64::consts::TAU).abs() <= self.tolerance().parameter;
        if full {
            if let Some(exact) = exact_full_revolution(self, plane, axis)? {
                return Ok(exact);
            }
        }
        crate::revolve_loft::revolve(self, plane, axis, angle)
    }

    pub fn loft<'a>(
        &'a self,
        plane: Plane3,
        sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
    ) -> KernelResult<Compound> {
        self.loft_with_alignment(plane, sections, true)
    }

    pub fn loft_with_alignment<'a>(
        &'a self,
        plane: Plane3,
        sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
        align_sections: bool,
    ) -> KernelResult<Compound> {
        self.loft_with_guides(plane, sections, align_sections, &[])
    }

    /// Loft whose sides follow guide wires. Every guide crosses each section
    /// on its boundary, the sections in loft order and all guides in the same
    /// order around the sections; the sides between neighbouring guides blend
    /// between them. Guides fix the section correspondence, so alignment
    /// applies only without them.
    pub fn loft_with_guides<'a>(
        &'a self,
        plane: Plane3,
        sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
        align_sections: bool,
        guides: &[crate::Wire3],
    ) -> KernelResult<Compound> {
        let sections = std::iter::once((plane, self))
            .chain(sections)
            .collect::<Vec<_>>();
        crate::revolve_loft::loft(&sections, align_sections, guides)
    }
}

pub(crate) fn draft(
    source: &Compound,
    faces: &[FaceHandle],
    neutral: Plane3,
    angle: f64,
) -> KernelResult<Compound> {
    if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 {
        return Err(invalid(
            "draft angle must be finite and smaller than 90 degrees",
        ));
    }
    if angle.abs() <= source.tolerance().angular {
        return source.unchanged();
    }
    let tolerance = source.tolerance();
    let selected = faces
        .iter()
        .map(FaceHandle::persistent_id)
        .collect::<BTreeSet<_>>();
    let direction = neutral.normal();
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    for handle in source.faces() {
        let face = source.topology().face(handle.topology_id())?;
        let mut surface = face.surface().clone();
        let orientation = face.orientation().sign();
        if selected.contains(&face.name()) {
            surface = match surface {
                Surface3::Plane(plane) => {
                    let normal = plane.normal() * orientation;
                    let horizontal = normal - direction * normal.dot(direction);
                    if horizontal.magnitude() <= tolerance.angular {
                        return Err(invalid("draft face must intersect the neutral plane"));
                    }
                    let horizontal = horizontal.normalize();
                    let point = neutral.origin()
                        + horizontal * (plane.origin() - neutral.origin()).dot(normal)
                            / normal.dot(horizontal);
                    let normal = horizontal * angle.cos() + direction * angle.sin();
                    Plane3::from_origin_normal(point, normal * orientation)?.into()
                }
                Surface3::Cylinder(cylinder) => {
                    if cylinder.axis().normalize().dot(direction).abs() < 1.0 - tolerance.angular {
                        return Err(not_implemented(
                            "draft of a cylinder oblique to the pull direction",
                        ));
                    }
                    // Retain the cylindrical UV frame so its seam and cap edges
                    // can be rebuilt by the same support-intersection machinery.
                    let height = (cylinder.origin() - neutral.origin()).dot(direction);
                    let radial_sign = Surface3::Cylinder(cylinder)
                        .normal(0.0, 0.0)?
                        .dot(cylinder.x_radius())
                        .signum()
                        * orientation;
                    let slope = -angle.tan() * radial_sign;
                    let origin_radius = cylinder.radius() + slope * height;
                    let axial_slope = slope * cylinder.axis().dot(direction);
                    let apex_parameter = -origin_radius / axial_slope;
                    crate::Cone3::new(
                        cylinder.origin() + cylinder.axis() * apex_parameter,
                        -cylinder.axis() * apex_parameter,
                        cylinder.x_radius() * (origin_radius / cylinder.radius()),
                        cylinder.y_radius() * (origin_radius / cylinder.radius()),
                        tolerance,
                    )?
                    .into()
                }
                Surface3::Cone(cone) => {
                    if cone.axis().normalize().dot(direction).abs() < 1.0 - tolerance.angular {
                        return Err(not_implemented("draft of a cone oblique to the pull direction"));
                    }
                    let neutral_parameter = (neutral.origin() - cone.apex()).dot(direction)
                        / cone.axis().dot(direction);
                    let neutral_radius = cone.reference_radius() * neutral_parameter;
                    let reference_center = cone.apex() + cone.axis();
                    let radial_sign = Surface3::Cone(cone)
                        .normal(0.0, 1.0)?
                        .dot(cone.x_radius())
                        .signum()
                        * orientation;
                    let slope = -angle.tan() * radial_sign;
                    let reference_radius = neutral_radius
                        + slope * (reference_center - neutral.origin()).dot(direction);
                    if reference_radius <= tolerance.linear {
                        return Err(invalid("draft collapses the conical reference section"));
                    }
                    let axis = direction * (reference_radius / slope);
                    crate::Cone3::new(
                        reference_center - axis,
                        axis,
                        cone.x_radius() * (reference_radius / cone.reference_radius()),
                        cone.y_radius() * (reference_radius / cone.reference_radius()),
                        tolerance,
                    )?
                    .into()
                }
                Surface3::Nurbs(spline) if spline.degrees().1 == 1
                    && spline.control_points().len() == 2
                    && spline.weights()[0] == spline.weights()[1] => {
                    let controls = spline.control_points();
                    let heights = [0, 1].map(|row| (controls[row][0] - neutral.origin()).dot(direction));
                    if (heights[1] - heights[0]).abs() <= tolerance.linear
                        || controls.iter().enumerate().any(|(row, points)| points.iter().any(|p|
                            ((p - neutral.origin()).dot(direction) - heights[row]).abs() > tolerance.linear)) {
                        return Err(not_implemented("draft requires sections parallel to the neutral plane"));
                    }
                    let fraction = -heights[0] / (heights[1] - heights[0]);
                    let section = controls[0].iter().zip(&controls[1])
                        .map(|(a, b)| *a + (*b - *a) * fraction).collect::<Vec<_>>();
                    let base = crate::NurbsSurface3::new(spline.degrees(),
                        heights.iter().map(|h| section.iter().map(|p| *p + direction * *h).collect()).collect(),
                        spline.weights().to_vec(), (spline.knots().0.to_vec(), spline.knots().1.to_vec()))?;
                    let crate::DomainBounds::Bounded(v) = base.domains()[1].bounds else { unreachable!() };
                    let slope = -orientation * angle.tan() * (heights[1] - heights[0]) / v.span();
                    let distance = -orientation * angle.tan() * heights[0] - slope * v.start;
                    let bounds = face_parameter_bounds(source.topology(), handle.topology_id(), tolerance)?;
                    let padding = offset_parameter_padding(&base.clone().into(), bounds,
                        heights[0].abs().max(heights[1].abs()) * angle.tan(), tolerance)?;
                    crate::OffsetSurface3::with_linear_distance(base.extended_bezier(padding)?.into(), distance, slope, tolerance)?.into()
                }
                _ => return Err(not_implemented("draft of this face support")),
            };
        }
        bounds.insert(
            handle.topology_id(),
            face_parameter_bounds(source.topology(), handle.topology_id(), tolerance)?,
        );
        supports.insert(handle.topology_id(), surface);
    }
    let changed_supports = faces.iter().map(FaceHandle::topology_id).collect();
    rebuild_supports(
        source,
        SupportEdit {
            supports,
            bounds,
            changed: changed_supports,
            ..Default::default()
        },
        crate::DerivationRole::Draft,
    )
}
