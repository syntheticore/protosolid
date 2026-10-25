//! Rolling-ball and ruled edge-blend surfaces.
use super::*;

/// Exact circular or ruled cross sections between two retained contact rails.
///
/// The center curve is the intersection of the two inward offsets. Its two
/// p-curves retain the corresponding parameters on the un-offset supports.
/// Consequently this is an analytical/implicit surface definition rather than
/// a fitted patch: every evaluation is constructed from the exact center and
/// contact points on the parent surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeBlendSurface3 {
    pub(super) ruled: bool,
    pub(super) shared_intersection: bool,
    pub(super) center_curve: Box<Curve3>,
    pub(super) first_surface: Box<Surface3>,
    pub(super) second_surface: Box<Surface3>,
    pub(super) first_pcurve: Curve2,
    pub(super) second_pcurve: Curve2,
    pub(super) center_range: ParameterRange,
    pub(super) center_period: Option<f64>,
    pub(super) radius: f64,
    pub(super) tolerance: ModelTolerance,
    pub(super) sections: ParameterEvaluationCache<BlendSection>,
}

#[derive(Clone, Copy)]
pub(super) struct BlendSection {
    pub(super) first: (Point3, Vec3),
    pub(super) second: (Point3, Vec3),
    pub(super) circular: Option<CircularBlendSection>,
}

/// Prepared differential data shared by every transverse sample of a section.
/// Geometry validation and the circular frame depend on the rail parameter,
/// not on the position across the fillet.
#[derive(Clone, Copy)]
pub(super) struct CircularBlendSection {
    pub(super) center_point: Point3,
    pub(super) center_tangent: Vec3,
    pub(super) first_unit: Vec3,
    pub(super) second_unit: Vec3,
    pub(super) first_derivative: Vec3,
    pub(super) second_derivative: Vec3,
    pub(super) angle: f64,
    pub(super) sine: f64,
    pub(super) angle_cosine: f64,
    pub(super) angle_derivative: f64,
}

/// Circular-section compatibility name for the shared edge blend support.
pub type RollingBallSurface3 = EdgeBlendSurface3;

impl EdgeBlendSurface3 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        center_curve: Curve3,
        first_surface: Surface3,
        second_surface: Surface3,
        first_pcurve: Curve2,
        second_pcurve: Curve2,
        center_range: ParameterRange,
        radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        Self::with_section(
            center_curve,
            first_surface,
            second_surface,
            first_pcurve,
            second_pcurve,
            center_range,
            radius,
            tolerance,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn with_section(
        center_curve: Curve3,
        first_surface: Surface3,
        second_surface: Surface3,
        first_pcurve: Curve2,
        second_pcurve: Curve2,
        center_range: ParameterRange,
        radius: f64,
        tolerance: ModelTolerance,
        ruled: bool,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !radius.is_finite() || radius <= tolerance.linear {
            return Err(invalid(
                "rolling-ball radius must be finite and exceed model tolerance",
            ));
        }
        if !center_curve
            .domain()
            .contains_range(center_range, tolerance.parameter)
            || !first_pcurve
                .domain()
                .contains_range(center_range, tolerance.parameter)
            || !second_pcurve
                .domain()
                .contains_range(center_range, tolerance.parameter)
        {
            return Err(invalid(
                "rolling-ball center and contact guides must cover the trim range",
            ));
        }
        // The center and both contact p-curves can retain the same exact
        // intersection. Recognize it once so evaluations solve it only once.
        let shared_intersection = matches!(
            (&center_curve, &first_pcurve, &second_pcurve),
            (Curve3::SurfaceIntersection(center), Curve2::SurfaceIntersection(first), Curve2::SurfaceIntersection(second))
                if first.first && !second.first
                    && first.curve.as_ref() == center && second.curve.as_ref() == center
        );
        let surface = Self {
            ruled,
            center_period: center_curve.domain().period,
            shared_intersection,
            center_curve: Box::new(center_curve),
            first_surface: Box::new(first_surface),
            second_surface: Box::new(second_surface),
            first_pcurve,
            second_pcurve,
            center_range,
            radius,
            tolerance,
            sections: ParameterEvaluationCache::default(),
        };
        for parameter in [
            center_range.start,
            (center_range.start + center_range.end) * 0.5,
            center_range.end,
        ] {
            for blend in [0.0, 0.5, 1.0] {
                surface.point(parameter, blend)?;
            }
        }
        Ok(surface)
    }

    /// Whether the section is a straight chamfer line rather than a circular arc.
    pub(crate) fn is_ruled(&self) -> bool {
        self.ruled
    }

    /// Continue the blend along its spine by `padding` at both ends, without
    /// changing it inside its current range. Closed spines are returned as is.
    pub(crate) fn extended(&self, padding: f64) -> KernelResult<Self> {
        if self.center_period.is_some() || padding <= 0.0 {
            return Ok(self.clone());
        }
        let low = self.center_range.start.min(self.center_range.end) - padding;
        let high = self.center_range.start.max(self.center_range.end) + padding;
        let range = if self.center_range.span() >= 0.0 {
            ParameterRange::new(low, high)?
        } else {
            ParameterRange::new(high, low)?
        };
        let center: Curve3 = match self.center_curve.as_ref() {
            Curve3::SurfaceIntersection(curve) => curve.extended(range)?.into(),
            curve if curve.domain().contains_range(range, 0.0) => curve.clone(),
            _ => return Err(crate::unsupported("blend spine cannot be continued")),
        };
        let [first_pcurve, second_pcurve] = match &center {
            Curve3::SurfaceIntersection(curve) if self.shared_intersection => curve.exact_pcurves(),
            _ => [self.first_pcurve.extended(range)?, self.second_pcurve.extended(range)?],
        };
        let extended = Self::with_section(
            center,
            (*self.first_surface).clone(),
            (*self.second_surface).clone(),
            first_pcurve,
            second_pcurve,
            range,
            self.radius,
            self.tolerance,
            self.ruled,
        )?;
        // Construction certifies only the ends and middle. A continued spine
        // can leave its supports' intersection in between.
        let (start, end) = (self.center_range.start.min(self.center_range.end),
            self.center_range.start.max(self.center_range.end));
        for index in 1..8 {
            let fraction = index as f64 / 8.0;
            for parameter in [low + (start - low) * fraction, end + (high - end) * fraction] {
                extended.point(parameter, 0.5)?;
            }
        }
        Ok(extended)
    }

    pub fn center_curve(&self) -> &Curve3 {
        &self.center_curve
    }

    pub(crate) fn center_period(&self) -> Option<f64> {
        self.center_period
    }

    pub(crate) fn with_center_period(mut self, period: Option<f64>) -> Self {
        self.center_period = period;
        self
    }

    pub fn first_surface(&self) -> &Surface3 {
        &self.first_surface
    }

    pub fn second_surface(&self) -> &Surface3 {
        &self.second_surface
    }

    pub fn first_pcurve(&self) -> &Curve2 {
        &self.first_pcurve
    }

    pub fn second_pcurve(&self) -> &Curve2 {
        &self.second_pcurve
    }

    pub fn center_range(&self) -> ParameterRange {
        self.center_range
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    pub(super) fn contact(
        surface: &Surface3,
        pcurve: &Curve2,
        parameter: f64,
    ) -> KernelResult<(Point3, Vec3)> {
        let (parameters, parameter_tangent) = pcurve.first_order(parameter)?;
        Self::contact_at(surface, parameters, parameter_tangent)
    }

    pub(super) fn contact_at(
        surface: &Surface3,
        parameters: Point2,
        tangent: Vec2,
    ) -> KernelResult<(Point3, Vec3)> {
        let (point, du, dv) = surface.first_order(parameters.x, parameters.y)?;
        Ok((point, du * tangent.x + dv * tangent.y))
    }

    // Every point across a blend section uses the same contact rails and spine.
    // Offset derivatives and quadrature revisit these exact parameters many
    // times. Cache that section independently of the transverse coordinate.
    pub(super) fn section(&self, parameter: f64) -> KernelResult<BlendSection> {
        if let Some(section) = self.sections.get(parameter) {
            return Ok(section);
        }
        let mut center = None;
        let (first, second) = if self.shared_intersection {
            let Curve3::SurfaceIntersection(intersection) = self.center_curve.as_ref() else {
                unreachable!("shared intersection is checked at construction")
            };
            let uv = intersection.corrected_parameters(parameter)?;
            let tangent = intersection.parameter_derivative(parameter, uv)?;
            if !self.ruled {
                center = Some(intersection.first_order_at(uv, tangent)?);
            }
            (
                Self::contact_at(
                    &self.first_surface,
                    Point2::new(uv[0], uv[1]),
                    Vec2::new(tangent[0], tangent[1]),
                )?,
                Self::contact_at(
                    &self.second_surface,
                    Point2::new(uv[2], uv[3]),
                    Vec2::new(tangent[2], tangent[3]),
                )?,
            )
        } else {
            (
                Self::contact(&self.first_surface, &self.first_pcurve, parameter)?,
                Self::contact(&self.second_surface, &self.second_pcurve, parameter)?,
            )
        };
        if !self.ruled && center.is_none() {
            center = Some(self.center_curve.first_order(parameter)?);
        }
        let section = BlendSection {
            first,
            second,
            circular: center
                .map(|center| self.circular_section(first, second, center))
                .transpose()?,
        };
        self.sections.insert(parameter, section);
        Ok(section)
    }

    pub(super) fn circular_section(
        &self,
        first: (Point3, Vec3),
        second: (Point3, Vec3),
        (center_point, center_tangent): (Point3, Vec3),
    ) -> KernelResult<CircularBlendSection> {
        let first_radial = first.0 - center_point;
        let second_radial = second.0 - center_point;
        let scale = center_point
            .to_vec()
            .magnitude()
            .max(first.0.to_vec().magnitude())
            .max(second.0.to_vec().magnitude())
            .max(self.radius)
            .max(1.0);
        let allowed = self.tolerance.linear_at_scale(scale) * 32.0;
        if (first_radial.magnitude() - self.radius).abs() > allowed
            || (second_radial.magnitude() - self.radius).abs() > allowed
        {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                format!(
                    "rolling-ball contact does not lie at the requested radius (requested {}, first {}, second {}, allowed {})",
                    self.radius,
                    first_radial.magnitude(),
                    second_radial.magnitude(),
                    allowed,
                ),
            ));
        }
        let first_unit = first_radial.normalize();
        let second_unit = second_radial.normalize();
        let cosine = first_unit.dot(second_unit).clamp(-1.0, 1.0);
        let angle = first_unit.cross(second_unit).magnitude().atan2(cosine);
        if std::f64::consts::PI - angle <= self.tolerance.angular
        {
            return Err(geometry_error("rolling-ball contact normals do not define a regular circular span"));
        }
        let sine = angle.sin();
        let first_derivative = (first.1 - center_tangent) / self.radius;
        let second_derivative = (second.1 - center_tangent) / self.radius;
        let cosine_derivative =
            first_derivative.dot(second_unit) + first_unit.dot(second_derivative);
        let angle_derivative = if angle <= self.tolerance.angular { 0.0 } else { -cosine_derivative / sine };
        Ok(CircularBlendSection {
            center_point,
            center_tangent,
            first_unit,
            second_unit,
            first_derivative,
            second_derivative,
            angle,
            sine,
            angle_cosine: angle.cos(),
            angle_derivative,
        })
    }

    pub(super) fn center_parameter(&self, parameter: f64) -> f64 {
        self.center_period.map_or(parameter, |period| {
            self.center_range.start.min(self.center_range.end)
                + (parameter - self.center_range.start.min(self.center_range.end))
                    .rem_euclid(period)
        })
    }

    pub(super) fn blend_period(&self, parameter: f64) -> KernelResult<Option<f64>> {
        if self.ruled {
            return Ok(None);
        }
        let section = self
            .section(self.center_parameter(parameter))?
            .circular
            .unwrap();
        Ok((section.angle > self.tolerance.angular).then(|| std::f64::consts::TAU / section.angle))
    }

    pub(super) fn first_order(&self, parameter: f64, blend: f64) -> KernelResult<(Point3, Vec3, Vec3)> {
        let parameter = self.center_parameter(parameter);
        if !self.center_range.contains(parameter, self.tolerance) || !blend.is_finite() {
            return Err(invalid("rolling-ball parameter lies outside its domain"));
        }
        let BlendSection {
            first,
            second,
            circular,
        } = self.section(parameter)?;
        if self.ruled {
            return Ok((
                first.0 + (second.0 - first.0) * blend,
                first.1 * (1.0 - blend) + second.1 * blend,
                second.0 - first.0,
            ));
        }
        let CircularBlendSection {
            center_point,
            center_tangent,
            first_unit,
            second_unit,
            first_derivative,
            second_derivative,
            angle,
            sine,
            angle_cosine,
            angle_derivative,
        } = circular.expect("circular sections include their prepared frame");
        let weight = |fraction: f64, fraction_derivative: f64| {
            // A concave crease can fade tangentially into a filleted rim.
            // Its contact rails coincide at the end; slerp has a finite limit
            // there even though the usual sine quotient is 0/0.
            if angle * fraction.abs().max(1.0) < 1.0e-4 {
                let squared = fraction * fraction;
                let angle2 = angle * angle;
                return (
                    fraction * (1.0 + (1.0 - squared) * angle2 / 6.0
                        + (7.0 - 10.0 * squared + 3.0 * squared * squared) * angle2 * angle2 / 360.0),
                    fraction * ((1.0 - squared) * angle / 3.0
                        + (7.0 - 10.0 * squared + 3.0 * squared * squared) * angle * angle2 / 90.0),
                    fraction_derivative * (1.0 + (1.0 - 3.0 * squared) * angle2 / 6.0
                        + (7.0 - 30.0 * squared + 15.0 * squared * squared) * angle2 * angle2 / 360.0),
                );
            }
            let numerator = (fraction * angle).sin();
            let numerator_theta = fraction * (fraction * angle).cos();
            let value = numerator / sine;
            let theta_derivative =
                (numerator_theta * sine - numerator * angle_cosine) / sine.powi(2);
            let blend_derivative = fraction_derivative * angle * (fraction * angle).cos() / sine;
            (value, theta_derivative, blend_derivative)
        };
        let first_weight = weight(1.0 - blend, -1.0);
        let second_weight = weight(blend, 1.0);
        let radial = first_unit * first_weight.0 + second_unit * second_weight.0;
        let radial_u = first_unit * (first_weight.1 * angle_derivative)
            + first_derivative * first_weight.0
            + second_unit * (second_weight.1 * angle_derivative)
            + second_derivative * second_weight.0;
        let radial_v = first_unit * first_weight.2 + second_unit * second_weight.2;
        Ok((
            center_point + radial * self.radius,
            center_tangent + radial_u * self.radius,
            radial_v * self.radius,
        ))
    }

    pub(super) fn point(&self, parameter: f64, blend: f64) -> KernelResult<Point3> {
        self.first_order(parameter, blend).map(|value| value.0)
    }

    pub(crate) fn radial_normal(&self, parameter: f64, blend: f64) -> KernelResult<Vec3> {
        let parameter = self.center_parameter(parameter);
        let section = self.section(parameter)?.circular.unwrap();
        let radial = (self.point(parameter, blend)? - section.center_point).normalize();
        let axis = if section.angle > self.tolerance.angular * 8.0 {
            section.first_unit.cross(section.second_unit)
        } else {
            (section.first_derivative.cross(section.second_unit)
                + section.first_unit.cross(section.second_derivative))
                * (self.center_range.middle() - parameter).signum()
        };
        Ok(radial * (-section.center_tangent.dot(axis)).signum())
    }

    pub(super) fn normal(&self, parameter: f64, blend: f64) -> KernelResult<Vec3> {
        let parameter = self.center_parameter(parameter);
        let (point, du, dv) = self.first_order(parameter, blend)?;
        let section = self.section(parameter)?.circular.unwrap();
        let radial = (point - section.center_point).normalize();
        let mut sign = if section.angle > self.tolerance.angular * 8.0 {
            du.cross(dv).dot(radial)
        } else {
            // At a collapsed end section, the two contact normals agree.
            // Their derivative gives the one-sided circular frame without
            // amplifying the residual difference between those normals.
            let direction = (self.center_range.middle() - parameter).signum();
            let axis = (section.first_derivative.cross(section.second_unit)
                + section.first_unit.cross(section.second_derivative)) * direction;
            let center = self.center_curve.evaluate(parameter)?;
            let longitudinal = center.first_derivative.magnitude2()
                - (point - center.point).dot(center.second_derivative);
            -center.first_derivative.dot(axis) * longitudinal
        };
        if sign == 0.0 {
            let direction = (self.center_range.middle() - parameter).signum();
            let nearby = self.shifted(parameter, direction * self.center_range.span().abs() * 1.0e-4);
            let (point, du, dv) = self.first_order(nearby, blend)?;
            let center = self.center_curve.first_order(nearby)?.0;
            sign = du.cross(dv).dot(point - center);
        }
        if sign == 0.0 { return Err(geometry_error("rolling-ball normal has no regular one-sided limit")); }
        Ok(radial * sign.signum())
    }

    pub(super) fn shifted(&self, parameter: f64, delta: f64) -> f64 {
        (parameter + delta).clamp(
            self.center_range.start.min(self.center_range.end),
            self.center_range.start.max(self.center_range.end),
        )
    }

    pub(super) fn evaluate(&self, parameter: f64, blend: f64) -> KernelResult<SurfaceEvaluation> {
        let parameter = self.center_parameter(parameter);
        let (point, du, dv) = self.first_order(parameter, blend)?;
        let parameter_step = (self.center_range.span().abs()
            * self.tolerance.parameter.sqrt().max(f64::EPSILON.sqrt()))
        .max(self.tolerance.parameter * 32.0);
        let blend_step = self
            .tolerance
            .parameter
            .sqrt()
            .max(f64::EPSILON.sqrt())
            .max(1.0e-6);
        let parameter_minimum = self.center_range.start.min(self.center_range.end);
        let parameter_maximum = self.center_range.start.max(self.center_range.end);
        let duu = if parameter - parameter_step >= parameter_minimum
            && parameter + parameter_step <= parameter_maximum
        {
            let first = self.first_order(parameter - parameter_step, blend)?.1;
            let last = self.first_order(parameter + parameter_step, blend)?.1;
            (last - first) / (2.0 * parameter_step)
        } else if parameter + parameter_step * 2.0 <= parameter_maximum {
            let first = self.first_order(parameter + parameter_step, blend)?.1;
            let second = self.first_order(parameter + parameter_step * 2.0, blend)?.1;
            (du * -3.0 + first * 4.0 - second) / (2.0 * parameter_step)
        } else {
            let first = self.first_order(parameter - parameter_step, blend)?.1;
            let second = self.first_order(parameter - parameter_step * 2.0, blend)?.1;
            (du * 3.0 - first * 4.0 + second) / (2.0 * parameter_step)
        };
        let blend_before = self.first_order(parameter, blend - blend_step)?;
        let blend_after = self.first_order(parameter, blend + blend_step)?;
        let dvv = (blend_after.2 - blend_before.2) / (2.0 * blend_step);
        let parameter_minus = self.shifted(parameter, -parameter_step);
        let parameter_plus = self.shifted(parameter, parameter_step);
        let blend_minus = blend - blend_step;
        let blend_plus = blend + blend_step;
        let parameter_span = parameter_plus - parameter_minus;
        let blend_span = blend_plus - blend_minus;
        let parameter_before = self.first_order(parameter_minus, blend)?;
        let parameter_after = self.first_order(parameter_plus, blend)?;
        let duv = ((parameter_after.2 - parameter_before.2) / parameter_span
            + (blend_after.1 - blend_before.1) / blend_span)
            * 0.5;
        Ok(SurfaceEvaluation {
            point,
            du,
            dv,
            duu,
            duv,
            dvv,
        })
    }

    pub(super) fn project(&self, point: Point3) -> KernelResult<Projection2> {
        let mut parameters = (0..=16)
            .flat_map(|first| {
                (0..=24).map(move |second| {
                    (
                        self.center_range.start + self.center_range.span() * first as f64 / 16.0,
                        -1.0 + second as f64 / 8.0,
                    )
                })
            })
            // The extended spine can leave the regular intersection branch
            // outside the retained face. Seed from its evaluable portion.
            .filter_map(|parameters| {
                self.point(parameters.0, parameters.1)
                    .ok()
                    .map(|value| (parameters, value.distance2(point)))
            })
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|candidate| candidate.0)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "rolling-ball projection has no seed",
                )
            })?;
        for _ in 0..48 {
            let value = self.evaluate(parameters.0, parameters.1)?;
            let residual = value.point - point;
            let matrix = [
                [
                    value.du.magnitude2() + residual.dot(value.duu),
                    value.du.dot(value.dv) + residual.dot(value.duv),
                ],
                [
                    value.du.dot(value.dv) + residual.dot(value.duv),
                    value.dv.magnitude2() + residual.dot(value.dvv),
                ],
            ];
            let determinant = matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0];
            if determinant.abs() <= f64::EPSILON * 256.0 {
                break;
            }
            let first = residual.dot(value.du);
            let second = residual.dot(value.dv);
            let step = (
                (-first * matrix[1][1] + second * matrix[0][1]) / determinant,
                (-second * matrix[0][0] + first * matrix[1][0]) / determinant,
            );
            let mut next = parameters;
            let mut damping = 1.0;
            for _ in 0..24 {
                let trial = (
                    (parameters.0 + step.0 * damping).clamp(
                        self.center_range.start.min(self.center_range.end),
                        self.center_range.start.max(self.center_range.end),
                    ),
                    parameters.1 + step.1 * damping,
                );
                if let Ok(projected) = self.point(trial.0, trial.1) {
                    if projected.distance2(point) < residual.magnitude2() {
                        next = trial;
                        break;
                    }
                }
                damping *= 0.5;
            }
            if (next.0 - parameters.0)
                .abs()
                .max((next.1 - parameters.1).abs())
                <= self.tolerance.parameter
            {
                parameters = next;
                break;
            }
            parameters = next;
        }
        if let Some(period) = self.blend_period(parameters.0)? {
            parameters.1 += ((0.5 - parameters.1) / period).round() * period;
        }
        let projected = self.point(parameters.0, parameters.1)?;
        Ok(Projection2 {
            parameters,
            point: projected,
            distance: projected.distance(point),
        })
    }
}
