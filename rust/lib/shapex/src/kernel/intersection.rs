use cgmath::prelude::*;

use crate::{Point2, Point3, Vec3, Vec4};

use super::{
    Circle3, Curve2, Curve3, DiagnosticKind, DomainBounds, KernelError, KernelResult, Line2, Line3,
    ModelTolerance, NurbsCurve2, Plane3, Surface3, SurfaceIntersectionCurve,
    SurfaceProjectionPcurve2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntersectionKind {
    Transverse,
    Tangent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveCurveHit {
    pub point: Point3,
    pub first_parameter: f64,
    pub second_parameter: f64,
    pub kind: IntersectionKind,
    pub multiplicity: u8,
    pub achieved_error: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveSurfaceHit {
    pub point: Point3,
    pub curve_parameter: f64,
    pub surface_parameters: (f64, f64),
    pub kind: IntersectionKind,
    pub multiplicity: u8,
    pub achieved_error: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceSurfaceCurve {
    pub curve: Curve3,
    pub first_pcurve: Curve2,
    pub second_pcurve: Curve2,
    pub achieved_error: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntersectionSet<T> {
    Empty,
    Isolated(Vec<T>),
    Coincident,
}

impl Curve3 {
    pub fn intersect_curve(
        &self,
        other: &Self,
        tolerance: ModelTolerance,
    ) -> KernelResult<IntersectionSet<CurveCurveHit>> {
        match (self, other) {
            (Self::Line(first), Self::Line(second)) => line_line(*first, *second, tolerance),
            (Self::Line(line), Self::Circle(circle)) => line_circle(*line, *circle, tolerance),
            (Self::Circle(circle), Self::Line(line)) => {
                Ok(swap_curve_hits(line_circle(*line, *circle, tolerance)?))
            }
            (Self::Circle(first), Self::Circle(second)) => {
                circle_circle(*first, *second, tolerance)
            }
            _ => Err(unsupported(
                "general NURBS curve/curve intersection is not yet certified",
            )),
        }
    }

    pub(crate) fn intersect_curve_near(
        &self,
        other: &Self,
        first_parameter: f64,
        second_parameter: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<CurveCurveHit> {
        let mut parameters = [first_parameter, second_parameter];
        for _ in 0..48 {
            parameters[0] = normalize_parameter(self.domain(), parameters[0]);
            parameters[1] = normalize_parameter(other.domain(), parameters[1]);
            let first = self.evaluate(parameters[0])?;
            let second = other.evaluate(parameters[1])?;
            let residual = first.point - second.point;
            let scale = first
                .point
                .to_vec()
                .magnitude()
                .max(second.point.to_vec().magnitude())
                .max(1.0);
            // Topological vertices are shared by every incident edge, so a
            // model-tolerance early exit here can leave two individually
            // acceptable curve hits whose averaged points disagree after an
            // Euler weld. Drive the local identity to floating-point accuracy;
            // the model tolerance still selects and validates the branch.
            let linear_target = f64::EPSILON * scale * 2048.0;
            if residual.magnitude() <= linear_target {
                let transverse = first
                    .first_derivative
                    .normalize()
                    .cross(second.first_derivative.normalize())
                    .magnitude()
                    > tolerance.angular * 8.0;
                return Ok(CurveCurveHit {
                    point: Point3::from_vec((first.point.to_vec() + second.point.to_vec()) * 0.5),
                    first_parameter: parameters[0],
                    second_parameter: parameters[1],
                    kind: if transverse {
                        IntersectionKind::Transverse
                    } else {
                        IntersectionKind::Tangent
                    },
                    multiplicity: if transverse { 1 } else { 2 },
                    achieved_error: residual.magnitude(),
                });
            }
            let first_tangent = first.first_derivative;
            let second_tangent = -second.first_derivative;
            let normal = [
                [
                    first_tangent.dot(first_tangent),
                    first_tangent.dot(second_tangent),
                ],
                [
                    second_tangent.dot(first_tangent),
                    second_tangent.dot(second_tangent),
                ],
            ];
            let right = [-first_tangent.dot(residual), -second_tangent.dot(residual)];
            let determinant = normal[0][0] * normal[1][1] - normal[0][1] * normal[1][0];
            if determinant.abs() <= f64::EPSILON * 256.0 {
                break;
            }
            let delta = [
                (right[0] * normal[1][1] - normal[0][1] * right[1]) / determinant,
                (normal[0][0] * right[1] - right[0] * normal[1][0]) / determinant,
            ];
            let mut damping = 1.0;
            let mut accepted = false;
            for _ in 0..16 {
                let candidate = [
                    normalize_parameter(self.domain(), parameters[0] + delta[0] * damping),
                    normalize_parameter(other.domain(), parameters[1] + delta[1] * damping),
                ];
                let candidate_residual =
                    self.evaluate(candidate[0])?.point - other.evaluate(candidate[1])?.point;
                if candidate_residual.magnitude2() < residual.magnitude2() {
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
        Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "local curve/curve intersection correction did not converge",
        ))
    }

    pub fn intersect_surface(
        &self,
        surface: &Surface3,
        tolerance: ModelTolerance,
    ) -> KernelResult<IntersectionSet<CurveSurfaceHit>> {
        match (self, surface) {
            (Self::Line(line), Surface3::Plane(plane)) => line_plane(*line, *plane, tolerance),
            (Self::Line(line), Surface3::Cylinder(cylinder)) => {
                line_cylinder(*line, *cylinder, tolerance)
            }
            (Self::Line(line), Surface3::Cone(cone)) => line_cone(*line, *cone, tolerance),
            (Self::Line(line), Surface3::Sphere(sphere)) => line_sphere(*line, *sphere, tolerance),
            (Self::Circle(circle), Surface3::Plane(plane)) => {
                circle_plane(*circle, *plane, tolerance)
            }
            (Self::Circle(circle), Surface3::Sphere(sphere)) => {
                circle_sphere(*circle, *sphere, tolerance)
            }
            _ => Err(unsupported(
                "this curve/surface intersection pair is not yet certified",
            )),
        }
    }

    /// Correct a topologically supplied curve/surface intersection seed.
    ///
    /// BREP operations already know which endpoint neighborhood is intended;
    /// retaining that seed avoids ambiguous global root selection while this
    /// Newton corrector keeps the resulting point on both exact supports.
    pub(crate) fn intersect_surface_near(
        &self,
        surface: &Surface3,
        curve_parameter: f64,
        surface_parameters: (f64, f64),
        tolerance: ModelTolerance,
    ) -> KernelResult<CurveSurfaceHit> {
        let topological_parameter = curve_parameter;
        let mut parameters = [curve_parameter, surface_parameters.0, surface_parameters.1];
        for _ in 0..48 {
            parameters[0] = normalize_parameter(self.domain(), parameters[0]);
            let domains = surface.domains();
            parameters[1] = normalize_parameter(domains[0], parameters[1]);
            parameters[2] = normalize_parameter(domains[1], parameters[2]);
            let curve = self.evaluate(parameters[0])?;
            let surface_value = surface.evaluate(parameters[1], parameters[2])?;
            let residual = curve.point - surface_value.point;
            let scale = curve
                .point
                .to_vec()
                .magnitude()
                .max(surface_value.point.to_vec().magnitude())
                .max(1.0);
            let allowed = tolerance.linear_at_scale(scale);
            if residual.magnitude() <= allowed {
                let tangent = curve.first_derivative.normalize();
                let normal = surface_value.normal()?;
                let transverse = tangent.dot(normal).abs() > tolerance.angular * 8.0;
                return Ok(CurveSurfaceHit {
                    point: Point3::from_vec(
                        (curve.point.to_vec() + surface_value.point.to_vec()) * 0.5,
                    ),
                    curve_parameter: parameters[0],
                    surface_parameters: (parameters[1], parameters[2]),
                    kind: if transverse {
                        IntersectionKind::Transverse
                    } else {
                        IntersectionKind::Tangent
                    },
                    multiplicity: if transverse { 1 } else { 2 },
                    achieved_error: residual.magnitude(),
                });
            }
            let matrix = [
                [
                    curve.first_derivative.x,
                    -surface_value.du.x,
                    -surface_value.dv.x,
                ],
                [
                    curve.first_derivative.y,
                    -surface_value.du.y,
                    -surface_value.dv.y,
                ],
                [
                    curve.first_derivative.z,
                    -surface_value.du.z,
                    -surface_value.dv.z,
                ],
            ];
            let Some(delta) = solve_linear_3(matrix, [-residual.x, -residual.y, -residual.z])
            else {
                break;
            };
            let previous = residual.magnitude2();
            let mut damping = 1.0;
            let mut accepted = false;
            for _ in 0..12 {
                let candidate = [
                    normalize_parameter(self.domain(), parameters[0] + delta[0] * damping),
                    normalize_parameter(domains[0], parameters[1] + delta[1] * damping),
                    normalize_parameter(domains[1], parameters[2] + delta[2] * damping),
                ];
                let curve = self.evaluate(candidate[0])?;
                let surface_value = surface.evaluate(candidate[1], candidate[2])?;
                if (curve.point - surface_value.point).magnitude2() < previous {
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
        if let Surface3::Plane(plane) = surface {
            let range = match self.domain().bounds {
                DomainBounds::Bounded(range) => range,
                _ => super::ParameterRange::new(
                    topological_parameter - 1.0,
                    topological_parameter + 1.0,
                )?,
            };
            let signed_distance = |parameter: f64| -> KernelResult<f64> {
                Ok(plane
                    .normal()
                    .dot(self.evaluate(parameter)?.point - plane.origin()))
            };
            let mut roots = Vec::new();
            let mut previous_parameter = range.start;
            let mut previous_value = signed_distance(previous_parameter)?;
            let mut previous_derivative = plane
                .normal()
                .dot(self.evaluate(previous_parameter)?.first_derivative);
            let mut closest = (previous_parameter, previous_value.abs());
            for index in 1..=128 {
                let parameter = range.start + range.span() * index as f64 / 128.0;
                let value = signed_distance(parameter)?;
                let derivative = plane
                    .normal()
                    .dot(self.evaluate(parameter)?.first_derivative);
                if value.abs() < closest.1 {
                    closest = (parameter, value.abs());
                }
                if previous_value == 0.0
                    || value == 0.0
                    || previous_value.signum() != value.signum()
                {
                    let mut first = previous_parameter;
                    let mut second = parameter;
                    let mut first_value = previous_value;
                    for _ in 0..64 {
                        let middle = (first + second) * 0.5;
                        let middle_value = signed_distance(middle)?;
                        if middle_value.abs()
                            <= tolerance.linear_at_scale(
                                self.evaluate(middle)?.point.to_vec().magnitude().max(1.0),
                            )
                        {
                            first = middle;
                            second = middle;
                            break;
                        }
                        if first_value.signum() == middle_value.signum() {
                            first = middle;
                            first_value = middle_value;
                        } else {
                            second = middle;
                        }
                    }
                    roots.push((first + second) * 0.5);
                }
                if previous_derivative == 0.0
                    || derivative == 0.0
                    || previous_derivative.signum() != derivative.signum()
                {
                    let mut first = previous_parameter;
                    let mut second = parameter;
                    let mut first_derivative = previous_derivative;
                    for _ in 0..64 {
                        let middle = (first + second) * 0.5;
                        let middle_derivative =
                            plane.normal().dot(self.evaluate(middle)?.first_derivative);
                        if middle_derivative.abs() <= tolerance.parameter * 32.0 {
                            first = middle;
                            second = middle;
                            break;
                        }
                        if first_derivative.signum() == middle_derivative.signum() {
                            first = middle;
                            first_derivative = middle_derivative;
                        } else {
                            second = middle;
                        }
                    }
                    let stationary = (first + second) * 0.5;
                    let stationary_distance = signed_distance(stationary)?.abs();
                    if stationary_distance < closest.1 {
                        closest = (stationary, stationary_distance);
                    }
                }
                previous_parameter = parameter;
                previous_value = value;
                previous_derivative = derivative;
            }
            if roots.is_empty() {
                let mut parameter = closest.0;
                for _ in 0..32 {
                    let value = self.evaluate(parameter)?;
                    let residual = plane.normal().dot(value.point - plane.origin());
                    let derivative = plane.normal().dot(value.first_derivative);
                    if derivative.abs() <= f64::EPSILON * 256.0 {
                        break;
                    }
                    let next = (parameter - residual / derivative)
                        .clamp(range.start.min(range.end), range.start.max(range.end));
                    if (next - parameter).abs() <= tolerance.parameter {
                        parameter = next;
                        break;
                    }
                    parameter = next;
                }
                roots.push(parameter);
            }
            if let Some(parameter) = roots.into_iter().min_by(|first, second| {
                (first - topological_parameter)
                    .abs()
                    .total_cmp(&(second - topological_parameter).abs())
            }) {
                let curve = self.evaluate(parameter)?;
                let projection = plane.project(curve.point)?;
                let error = curve.point.distance(projection.point);
                let scale = curve.point.to_vec().magnitude().max(1.0);
                if error <= tolerance.linear_at_scale(scale) * 8.0 {
                    let transverse = curve.first_derivative.normalize().dot(plane.normal()).abs()
                        > tolerance.angular * 8.0;
                    return Ok(CurveSurfaceHit {
                        point: Point3::from_vec(
                            (curve.point.to_vec() + projection.point.to_vec()) * 0.5,
                        ),
                        curve_parameter: parameter,
                        surface_parameters: projection.parameters,
                        kind: if transverse {
                            IntersectionKind::Transverse
                        } else {
                            IntersectionKind::Tangent
                        },
                        multiplicity: if transverse { 1 } else { 2 },
                        achieved_error: error,
                    });
                }
            }
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                format!(
                    "curve/plane intersection has no root in the retained support range; closest signed distance is {}",
                    closest.1
                ),
            ));
        }
        Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "curve/surface intersection corrector did not converge",
        ))
    }
}

fn normalize_parameter(domain: super::ParameterDomain, parameter: f64) -> f64 {
    if domain.period.is_some() {
        domain.normalize(parameter)
    } else if let DomainBounds::Bounded(range) = domain.bounds {
        parameter.clamp(range.start.min(range.end), range.start.max(range.end))
    } else {
        parameter
    }
}

/// Least-norm correction of a topologically supplied local surface/surface
/// seed.  Unlike the global tracer this accepts unbounded analytical supports;
/// the BREP edge neighborhood already identifies the intended branch.
pub(crate) fn correct_surface_pair_near(
    first: &Surface3,
    second: &Surface3,
    mut parameters: [f64; 4],
    tolerance: ModelTolerance,
) -> KernelResult<[f64; 4]> {
    let first_domains = first.domains();
    let second_domains = second.domains();
    let normalize = |values: [f64; 4]| {
        [
            normalize_parameter(first_domains[0], values[0]),
            normalize_parameter(first_domains[1], values[1]),
            normalize_parameter(second_domains[0], values[2]),
            normalize_parameter(second_domains[1], values[3]),
        ]
    };
    parameters = normalize(parameters);
    for _ in 0..48 {
        let first_value = first.evaluate(parameters[0], parameters[1])?;
        let second_value = second.evaluate(parameters[2], parameters[3])?;
        let residual = first_value.point - second_value.point;
        let scale = first_value
            .point
            .to_vec()
            .magnitude()
            .max(second_value.point.to_vec().magnitude())
            .max(1.0);
        if residual.magnitude() <= tolerance.linear_at_scale(scale) {
            if first_value
                .normal()?
                .cross(second_value.normal()?)
                .magnitude()
                <= tolerance.angular * 8.0
            {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "local surface intersection is tangential or singular",
                ));
            }
            return Ok(parameters);
        }
        let jacobian = [
            [
                first_value.du.x,
                first_value.dv.x,
                -second_value.du.x,
                -second_value.dv.x,
            ],
            [
                first_value.du.y,
                first_value.dv.y,
                -second_value.du.y,
                -second_value.dv.y,
            ],
            [
                first_value.du.z,
                first_value.dv.z,
                -second_value.du.z,
                -second_value.dv.z,
            ],
        ];
        let normal = multiply_3x4_by_transpose(jacobian);
        let lambda =
            solve_linear_3(normal, [-residual.x, -residual.y, -residual.z]).ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "local surface-intersection correction is singular",
                )
            })?;
        let delta: [f64; 4] = std::array::from_fn(|column| {
            (0..3)
                .map(|row| jacobian[row][column] * lambda[row])
                .sum::<f64>()
        });
        let mut damping = 1.0;
        let mut accepted = false;
        for _ in 0..16 {
            let candidate = normalize(std::array::from_fn(|index| {
                parameters[index] + delta[index] * damping
            }));
            let candidate_residual = first.evaluate(candidate[0], candidate[1])?.point
                - second.evaluate(candidate[2], candidate[3])?.point;
            if candidate_residual.magnitude2() < residual.magnitude2() {
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
    Err(KernelError::new(
        DiagnosticKind::NumericalFailure,
        "local surface-intersection correction did not converge",
    ))
}

fn circle_sphere(
    circle: Circle3,
    sphere: super::Sphere3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveSurfaceHit>> {
    let delta = circle.center() - sphere.center();
    let cosine = 2.0 * delta.dot(circle.x_radius());
    let sine = 2.0 * delta.dot(circle.y_radius());
    let constant = delta.magnitude2() + circle.radius().powi(2) - sphere.radius().powi(2);
    let amplitude = cosine.hypot(sine);
    let scale = delta
        .magnitude()
        .max(circle.radius())
        .max(sphere.radius())
        .max(1.0);
    let linear = tolerance.linear_at_scale(scale);
    let equation_tolerance = linear * 2.0 * scale;
    if amplitude <= equation_tolerance {
        return Ok(if constant.abs() <= equation_tolerance {
            IntersectionSet::Coincident
        } else {
            IntersectionSet::Empty
        });
    }
    let normalized = -constant / amplitude;
    let normalized_tolerance = equation_tolerance / amplitude;
    if normalized < -1.0 - normalized_tolerance || normalized > 1.0 + normalized_tolerance {
        return Ok(IntersectionSet::Empty);
    }
    let phase = sine.atan2(cosine);
    let offset = normalized.clamp(-1.0, 1.0).acos();
    let tangent = offset.abs() <= tolerance.parameter
        || (std::f64::consts::PI - offset).abs() <= tolerance.parameter
        || (1.0 - normalized.abs()) <= normalized_tolerance;
    let parameters = if tangent {
        vec![(phase + offset).rem_euclid(std::f64::consts::TAU)]
    } else {
        vec![
            (phase + offset).rem_euclid(std::f64::consts::TAU),
            (phase - offset).rem_euclid(std::f64::consts::TAU),
        ]
    };
    let mut hits = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        let point = circle.evaluate(parameter)?.point;
        let projection = Surface3::Sphere(sphere).project(point, tolerance)?;
        hits.push(CurveSurfaceHit {
            point,
            curve_parameter: parameter,
            surface_parameters: projection.parameters,
            kind: if tangent {
                IntersectionKind::Tangent
            } else {
                IntersectionKind::Transverse
            },
            multiplicity: if tangent { 2 } else { 1 },
            achieved_error: projection.distance,
        });
    }
    Ok(IntersectionSet::Isolated(hits))
}

fn line_cylinder(
    line: Line3,
    cylinder: super::Cylinder3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveSurfaceHit>> {
    let axis_unit = cylinder.axis().normalize();
    let delta = line.origin() - cylinder.origin();
    let radial_origin = delta - axis_unit * delta.dot(axis_unit);
    let radial_direction = line.direction() - axis_unit * line.direction().dot(axis_unit);
    let roots = solve_quadratic(
        radial_direction.magnitude2(),
        2.0 * radial_origin.dot(radial_direction),
        radial_origin.magnitude2() - cylinder.radius().powi(2),
        tolerance.linear_at_scale(cylinder.radius()),
    )?;
    curve_surface_roots(line, Surface3::Cylinder(cylinder), roots, tolerance)
}

fn line_cone(
    line: Line3,
    cone: super::Cone3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveSurfaceHit>> {
    let axis_unit = cone.axis().normalize();
    let slope = cone.reference_radius() / cone.axis().magnitude();
    let delta = line.origin() - cone.apex();
    let axial_origin = delta.dot(axis_unit);
    let axial_direction = line.direction().dot(axis_unit);
    let radial_origin = delta - axis_unit * axial_origin;
    let radial_direction = line.direction() - axis_unit * axial_direction;
    let slope_squared = slope * slope;
    let roots = solve_quadratic(
        radial_direction.magnitude2() - slope_squared * axial_direction * axial_direction,
        2.0 * (radial_origin.dot(radial_direction)
            - slope_squared * axial_origin * axial_direction),
        radial_origin.magnitude2() - slope_squared * axial_origin * axial_origin,
        tolerance.linear_at_scale(cone.reference_radius()),
    )?;
    curve_surface_roots(line, Surface3::Cone(cone), roots, tolerance)
}

fn line_sphere(
    line: Line3,
    sphere: super::Sphere3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveSurfaceHit>> {
    let delta = line.origin() - sphere.center();
    let roots = solve_quadratic(
        line.direction().magnitude2(),
        2.0 * delta.dot(line.direction()),
        delta.magnitude2() - sphere.radius().powi(2),
        tolerance.linear_at_scale(sphere.radius()),
    )?;
    curve_surface_roots(line, Surface3::Sphere(sphere), roots, tolerance)
}

fn solve_quadratic(
    a: f64,
    b: f64,
    c: f64,
    tolerance: f64,
) -> KernelResult<IntersectionSet<(f64, bool)>> {
    let coefficient_scale = a.abs().max(b.abs()).max(c.abs()).max(1.0);
    if a.abs() <= f64::EPSILON * coefficient_scale {
        if b.abs() <= f64::EPSILON * coefficient_scale {
            return if c.abs() <= tolerance {
                Ok(IntersectionSet::Coincident)
            } else {
                Ok(IntersectionSet::Empty)
            };
        }
        return Ok(IntersectionSet::Isolated(vec![(-c / b, false)]));
    }
    let discriminant = b * b - 4.0 * a * c;
    let threshold = tolerance * tolerance * coefficient_scale;
    if discriminant < -threshold {
        return Ok(IntersectionSet::Empty);
    }
    if discriminant.abs() <= threshold {
        return Ok(IntersectionSet::Isolated(vec![(-b / (2.0 * a), true)]));
    }
    let root = discriminant.sqrt();
    // This form avoids cancellation when |b| is large.
    let q = -0.5 * (b + b.signum() * root);
    let mut roots = if q == 0.0 {
        vec![
            ((-b - root) / (2.0 * a), false),
            ((-b + root) / (2.0 * a), false),
        ]
    } else {
        vec![(q / a, false), (c / q, false)]
    };
    roots.sort_by(|first, second| {
        first
            .0
            .partial_cmp(&second.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(IntersectionSet::Isolated(roots))
}

fn curve_surface_roots(
    line: Line3,
    surface: Surface3,
    roots: IntersectionSet<(f64, bool)>,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveSurfaceHit>> {
    let roots = match roots {
        IntersectionSet::Empty => return Ok(IntersectionSet::Empty),
        IntersectionSet::Coincident => return Ok(IntersectionSet::Coincident),
        IntersectionSet::Isolated(roots) => roots,
    };
    let mut hits = Vec::with_capacity(roots.len());
    for (parameter, tangent) in roots {
        let point = line.evaluate(parameter)?.point;
        let projection = surface.project(point, tolerance)?;
        hits.push(CurveSurfaceHit {
            point,
            curve_parameter: parameter,
            surface_parameters: projection.parameters,
            kind: if tangent {
                IntersectionKind::Tangent
            } else {
                IntersectionKind::Transverse
            },
            multiplicity: if tangent { 2 } else { 1 },
            achieved_error: projection.distance,
        });
    }
    Ok(IntersectionSet::Isolated(hits))
}

impl Surface3 {
    pub fn intersect_surface(
        &self,
        other: &Self,
        tolerance: ModelTolerance,
    ) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
        match self.intersect_support(other, tolerance) {
            Err(error) if matches!((self, other),
                (Self::Nurbs(_) | Self::Offset(_), Self::Nurbs(_) | Self::Offset(_)))
                && error.diagnostics().iter().all(|d| d.kind == DiagnosticKind::UnsupportedGeometry) =>
            {
                trace_regular_surface_intersections(self, other, tolerance)
            }
            result => result,
        }
    }

    /// Support intersections without global search. Reconstruction has a BREP
    /// branch and must use it instead of searching entire surface domains.
    pub(crate) fn intersect_support(
        &self,
        other: &Self,
        tolerance: ModelTolerance,
    ) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
        // A trimmed feature cap can retain the very same spline or procedural
        // support as its target. This is an overlap, not an isolated curve;
        // tracing the identical equations is singular and loses the cap loop.
        if self == other {
            return Ok(IntersectionSet::Coincident);
        }
        match (self, other) {
            (Self::Torus(first), Self::Torus(second)) => {
                let linear = tolerance.linear_at_scale(
                    first
                        .center()
                        .to_vec()
                        .magnitude()
                        .max(second.center().to_vec().magnitude()),
                );
                let first_axis = first.x_major().cross(first.y_major()).normalize();
                let second_axis = second.x_major().cross(second.y_major()).normalize();
                if first.center().distance(second.center()) <= linear
                    && (first.major_radius() - second.major_radius()).abs() <= linear
                    && (first.minor_radius() - second.minor_radius()).abs() <= linear
                    && first_axis.dot(second_axis).abs() >= 1.0 - tolerance.angular
                {
                    Ok(IntersectionSet::Coincident)
                } else {
                    Err(unsupported(
                        "this torus/torus intersection pair is not yet certified",
                    ))
                }
            }
            (Self::Plane(first), Self::Plane(second)) => plane_plane(*first, *second, tolerance),
            (Self::Plane(first), Self::Cylinder(second)) => {
                plane_cylinder(*first, *second, tolerance)
            }
            (Self::Cylinder(first), Self::Plane(second)) => Ok(swap_surface_curves(
                plane_cylinder(*second, *first, tolerance)?,
            )),
            (Self::Plane(plane), Self::Cone(cone)) => plane_cone(*plane, *cone, tolerance),
            (Self::Cone(cone), Self::Plane(plane)) => {
                Ok(swap_surface_curves(plane_cone(*plane, *cone, tolerance)?))
            }
            (Self::Cylinder(cylinder), Self::Cone(cone)) => {
                cylinder_cone(*cylinder, *cone, tolerance)
            }
            (Self::Cone(cone), Self::Cylinder(cylinder)) => Ok(swap_surface_curves(cylinder_cone(
                *cylinder, *cone, tolerance,
            )?)),
            (Self::Cylinder(first), Self::Cylinder(second)) => {
                cylinder_cylinder(*first, *second, tolerance)
            }
            (Self::Cylinder(cylinder), Self::Torus(torus)) => {
                cylinder_torus(*cylinder, *torus, tolerance)
            }
            (Self::Torus(torus), Self::Cylinder(cylinder)) => Ok(swap_surface_curves(
                cylinder_torus(*cylinder, *torus, tolerance)?,
            )),
            (Self::Plane(plane), Self::Torus(torus)) => plane_torus(*plane, *torus, tolerance),
            (Self::Torus(torus), Self::Plane(plane)) => {
                Ok(swap_surface_curves(plane_torus(*plane, *torus, tolerance)?))
            }
            (Self::Plane(plane), Self::RollingBall(_) | Self::Offset(_)) => {
                plane_blend_contact(*plane, other, tolerance)
            }
            (Self::RollingBall(_) | Self::Offset(_), Self::Plane(plane)) => {
                Ok(swap_surface_curves(plane_blend_contact(
                    *plane, self, tolerance,
                )?))
            }
            _ => Err(unsupported(
                "this surface/surface intersection pair is not yet certified",
            )),
        }
    }

    /// Intersect two supports over finite parameter boxes supplied by their
    /// trimmed BREP faces.  This is the general Boolean/feature entry point for
    /// primitives with globally unbounded parameters and for procedural blend
    /// surfaces.  The resulting curve remains the exact common zero set of the
    /// retained supports; subdivision and tracing only construct branch guides.
    pub(crate) fn intersect_surface_in_bounds(
        &self,
        other: &Self,
        first_domains: [super::ParameterRange; 2],
        second_domains: [super::ParameterRange; 2],
        tolerance: ModelTolerance,
    ) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
        match self.intersect_support(other, tolerance) {
            Ok(result) => Ok(result),
            Err(error)
                if error
                    .diagnostics()
                    .iter()
                    .all(|diagnostic| diagnostic.kind == DiagnosticKind::UnsupportedGeometry) =>
            {
                trace_surface_intersections_in_domains(
                    self,
                    other,
                    first_domains,
                    second_domains,
                    tolerance,
                )
            }
            Err(error) => Err(error),
        }
    }
}

fn plane_blend_contact(
    plane: Plane3,
    surface: &Surface3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let mut base = surface;
    while let Surface3::Offset(offset) = base {
        base = offset.base();
    }
    let Surface3::RollingBall(blend) = base else {
        return Err(unsupported("plane is not a rolling-ball contact support"));
    };
    let range = blend.center_range();
    for (support, guide, side) in [
        (blend.first_surface(), blend.first_pcurve(), 0.0),
        (blend.second_surface(), blend.second_pcurve(), 1.0),
    ] {
        let Surface3::Plane(contact_plane) = support else {
            continue;
        };
        // A normal offset retains the contact rail's parameters and tangency,
        // but shifts its supporting plane along the blend's oriented normal.
        let contact_plane = if surface == base {
            *contact_plane
        } else {
            Plane3::new(
                surface.evaluate((range.start + range.end) * 0.5, side)?.point,
                contact_plane.u(),
                contact_plane.v(),
                tolerance,
            )?
        };
        if !matches!(
            plane_plane(plane, contact_plane, tolerance)?,
            IntersectionSet::Coincident
        ) {
            continue;
        }
        // This plane is tangent to every rolling sphere. Its only intersection
        // is the retained contact rail; tracing near it creates spurious curves.
        let second_pcurve: Curve2 =
            Line2::new(Point2::new(0.0, side), crate::Vec2::unit_x())?.into();
        let curve: Curve3 = if surface == base {
            super::SurfaceCurve3::new(support.clone(), guide.clone(), range, tolerance)?.into()
        } else {
            super::SurfaceCurve3::new(surface.clone(), second_pcurve.clone(), range, tolerance)?.into()
        };
        let first_pcurve = SurfaceProjectionPcurve2::new(
            curve.clone(),
            plane.into(),
            guide.clone(),
            range,
            tolerance,
        )?
        .into();
        return Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
            curve,
            first_pcurve,
            second_pcurve,
            achieved_error: 0.0,
        }]));
    }
    Err(unsupported("plane is not a rolling-ball contact support"))
}

fn plane_cone(
    plane: Plane3,
    cone: super::Cone3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    // A plane through the apex cuts generators, including the single generator
    // of a tangent plane. A regular surface tracer cannot find that singular
    // contact reliably (drafted cylindrical fillets produce it routinely).
    let normal = plane.normal();
    let apex_distance = (cone.apex() - plane.origin()).dot(normal);
    let scale = cone
        .axis()
        .magnitude()
        .max(cone.reference_radius())
        .max(1.0);
    if apex_distance.abs() <= tolerance.linear_at_scale(scale) {
        let a = normal.dot(cone.x_radius());
        let b = normal.dot(cone.y_radius());
        let c = normal.dot(cone.axis());
        let amplitude = a.hypot(b);
        if amplitude <= tolerance.linear_at_scale(scale) {
            return Ok(IntersectionSet::Empty);
        }
        let ratio = -c / amplitude;
        let angular_error = tolerance.linear_at_scale(scale) / amplitude;
        if ratio.abs() > 1.0 + angular_error {
            return Ok(IntersectionSet::Empty);
        }
        let phase = b.atan2(a);
        let delta = if (ratio.abs() - 1.0).abs() <= angular_error {
            if ratio >= 0.0 {
                0.0
            } else {
                std::f64::consts::PI
            }
        } else {
            ratio.acos()
        };
        let mut angles = vec![phase + delta];
        if delta.sin().abs() > tolerance.angular {
            angles.push(phase - delta);
        }
        let origin = plane.project(cone.apex())?.parameters;
        let curves = angles
            .into_iter()
            .map(|u| {
                let direction = cone.axis() + cone.x_radius() * u.cos() + cone.y_radius() * u.sin();
                let end = plane.project(cone.apex() + direction)?.parameters;
                Ok(SurfaceSurfaceCurve {
                    curve: Line3::new(cone.apex(), direction)?.into(),
                    first_pcurve: Line2::new(
                        Point2::new(origin.0, origin.1),
                        crate::Vec2::new(end.0 - origin.0, end.1 - origin.1),
                    )?
                    .into(),
                    second_pcurve: Line2::new(Point2::new(u, 0.0), crate::Vec2::unit_y())?.into(),
                    achieved_error: apex_distance.abs(),
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        return Ok(IntersectionSet::Isolated(curves));
    }
    if plane.normal().dot(cone.axis().normalize()).abs() < 1.0 - tolerance.angular {
        return Err(unsupported(
            "oblique plane/cone intersection requires bounded tracing",
        ));
    }
    let v = (plane.origin() - cone.apex()).dot(plane.normal()) / cone.axis().dot(plane.normal());
    if v <= tolerance.parameter {
        return Ok(IntersectionSet::Empty);
    }
    let center = cone.apex() + cone.axis() * v;
    let curve: Curve3 =
        Circle3::new(center, cone.x_radius() * v, cone.y_radius() * v, tolerance)?.into();
    let range = super::ParameterRange::new(0.0, std::f64::consts::TAU)?;
    let center_uv = plane.project(center)?.parameters;
    let x_uv = plane.project(center + cone.x_radius() * v)?.parameters;
    let y_uv = plane.project(center + cone.y_radius() * v)?.parameters;
    let guide_radius = (x_uv.0 - center_uv.0)
        .hypot(x_uv.1 - center_uv.1)
        .max((y_uv.0 - center_uv.0).hypot(y_uv.1 - center_uv.1));
    // Plane charts may be scaled or skewed. The section is still an exact
    // spatial circle; only its parameter-space image becomes an ellipse.
    let pcurve = SurfaceProjectionPcurve2::new(
        curve.clone(),
        Surface3::Plane(plane),
        super::Circle2::new(Point2::new(center_uv.0, center_uv.1), guide_radius)?.into(),
        range,
        tolerance,
    )?
    .into();
    Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
        curve,
        first_pcurve: pcurve,
        second_pcurve: Line2::new(Point2::new(0.0, v), crate::Vec2::unit_x())?.into(),
        achieved_error: 0.0,
    }]))
}
fn cylinder_cone(
    cylinder: super::Cylinder3,
    cone: super::Cone3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let axis = cylinder.axis().normalize();
    if axis.dot(cone.axis().normalize()).abs() < 1.0 - tolerance.angular
        || (cone.apex() - cylinder.origin()).cross(axis).magnitude() > tolerance.linear
    {
        return Err(unsupported(
            "noncoaxial cylinder/cone intersection requires bounded tracing",
        ));
    }
    let v = cylinder.radius() / cone.reference_radius();
    let center = cone.apex() + cone.axis() * v;
    let circle = Circle3::new(center, cone.x_radius() * v, cone.y_radius() * v, tolerance)?;
    let point = circle.evaluate(0.0)?;
    let uv = Surface3::Cylinder(cylinder)
        .project(point.point, tolerance)?
        .parameters;
    let rate = point
        .first_derivative
        .dot(Surface3::Cylinder(cylinder).evaluate(uv.0, uv.1)?.du)
        .signum();
    Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
        curve: circle.into(),
        first_pcurve: Line2::new(Point2::new(uv.0, uv.1), crate::Vec2::new(rate, 0.0))?.into(),
        second_pcurve: Line2::new(Point2::new(0.0, v), crate::Vec2::unit_x())?.into(),
        achieved_error: 0.0,
    }]))
}

fn cylinder_cylinder(
    first: super::Cylinder3,
    second: super::Cylinder3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let first_axis = first.axis();
    let second_axis = second.axis();
    let first_unit = first_axis.normalize();
    let second_unit = second_axis.normalize();
    if first_unit.dot(second_unit).abs() < 1.0 - tolerance.angular {
        let cosine = first_unit.dot(second_unit);
        let delta = second.origin() - first.origin();
        let along =
            (delta.dot(first_unit) - cosine * delta.dot(second_unit)) / (1.0 - cosine * cosine);
        let center = first.origin() + first_unit * along;
        let other = second.origin() + second_unit * (center - second.origin()).dot(second_unit);
        let roundoff =
            f64::EPSILON * 512.0 * center.to_vec().magnitude().max(first.radius()).max(1.0);
        if center.distance(other) <= roundoff
            && (first.radius() - second.radius()).abs() <= roundoff
        {
            // Equal-radius cylinders with intersecting axes factor into two
            // planar sections. Preserve those regular conics through the
            // tangencies where a four-parameter intersection loses rank.
            let range = super::ParameterRange::new(0.0, std::f64::consts::TAU)?;
            let mut curves = Vec::new();
            for normal in [first_unit + second_unit, first_unit - second_unit] {
                let frame = super::Frame3::from_z(center, normal)?;
                let plane = Plane3::new(center, frame.x(), frame.y(), tolerance)?;
                let IntersectionSet::Isolated(sections) = plane_cylinder(plane, first, tolerance)?
                else {
                    unreachable!()
                };
                for section in sections {
                    let mut points = (0..=64)
                        .map(|index| {
                            let point = section
                                .curve
                                .evaluate(range.start + range.span() * index as f64 / 64.0)?
                                .point;
                            let uv = Surface3::Cylinder(second)
                                .project(point, tolerance)?
                                .parameters;
                            Ok(Point2::new(uv.0, uv.1))
                        })
                        .collect::<KernelResult<Vec<_>>>()?;
                    unwrap_periodic_points(&mut points, Surface3::Cylinder(second).domains());
                    let guide = smooth_nurbs_guide(&points, range)?.into();
                    let second_pcurve = SurfaceProjectionPcurve2::new(
                        section.curve.clone(),
                        second.into(),
                        guide,
                        range,
                        tolerance,
                    )?
                    .into();
                    curves.push(SurfaceSurfaceCurve {
                        curve: section.curve,
                        first_pcurve: section.second_pcurve,
                        second_pcurve,
                        achieved_error: section.achieved_error,
                    });
                }
            }
            return Ok(IntersectionSet::Isolated(curves));
        }
        return Err(unsupported(
            "nonparallel cylinder intersection requires bounded general surface tracing",
        ));
    }
    let axial_offset = (second.origin() - first.origin()).dot(first_unit);
    let second_center = second.origin() - first_unit * axial_offset;
    let center_delta = second_center - first.origin();
    let distance = center_delta.magnitude();
    let first_radius = first.radius();
    let second_radius = second.radius();
    let linear = tolerance.linear_at_scale(
        first
            .origin()
            .to_vec()
            .magnitude()
            .max(second.origin().to_vec().magnitude())
            .max(first_radius)
            .max(second_radius),
    );
    if distance <= linear {
        return if (first_radius - second_radius).abs() <= linear {
            Ok(IntersectionSet::Coincident)
        } else {
            Ok(IntersectionSet::Empty)
        };
    }
    if distance > first_radius + second_radius + linear
        || distance < (first_radius - second_radius).abs() - linear
    {
        return Ok(IntersectionSet::Empty);
    }
    let along =
        (first_radius.powi(2) - second_radius.powi(2) + distance.powi(2)) / (2.0 * distance);
    let height_squared = first_radius.powi(2) - along.powi(2);
    if height_squared < -linear * linear {
        return Ok(IntersectionSet::Empty);
    }
    let base = first.origin() + center_delta * (along / distance);
    let perpendicular = first_unit.cross(center_delta).normalize();
    let height = height_squared.max(0.0).sqrt();
    let points = if height <= linear {
        vec![base]
    } else {
        vec![base + perpendicular * height, base - perpendicular * height]
    };
    let mut curves = Vec::with_capacity(points.len());
    for point in points {
        let first_projection = Surface3::Cylinder(first).project(point, tolerance)?;
        let second_projection = Surface3::Cylinder(second).project(point, tolerance)?;
        let second_rate = first_axis.dot(second_axis) / second_axis.magnitude2();
        let line: Curve3 = Line3::new(point, first_axis)?.into();
        curves.push(SurfaceSurfaceCurve {
            curve: line,
            first_pcurve: Line2::new(
                Point2::new(first_projection.parameters.0, first_projection.parameters.1),
                crate::Vec2::new(0.0, 1.0),
            )?
            .into(),
            second_pcurve: Line2::new(
                Point2::new(
                    second_projection.parameters.0,
                    second_projection.parameters.1,
                ),
                crate::Vec2::new(0.0, second_rate),
            )?
            .into(),
            achieved_error: first_projection.distance.max(second_projection.distance),
        });
    }
    Ok(IntersectionSet::Isolated(curves))
}

fn cylinder_torus(
    cylinder: super::Cylinder3,
    torus: super::Torus3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let cylinder_axis = cylinder.axis();
    let cylinder_unit = cylinder_axis.normalize();
    let torus_normal = torus.normal();
    let scale = cylinder
        .origin()
        .to_vec()
        .magnitude()
        .max(torus.center().to_vec().magnitude())
        .max(cylinder.radius())
        .max(torus.major_radius());
    let linear = tolerance.linear_at_scale(scale);
    let center_delta = torus.center() - cylinder.origin();
    let radial_delta = center_delta - cylinder_unit * center_delta.dot(cylinder_unit);
    if cylinder_unit.dot(torus_normal).abs() < 1.0 - tolerance.angular
        || radial_delta.magnitude() > linear
    {
        return Err(unsupported(
            "noncoaxial cylinder/torus intersection requires bounded general surface tracing",
        ));
    }

    let radial_cosine = (cylinder.radius() - torus.major_radius()) / torus.minor_radius();
    if radial_cosine.abs() > 1.0 + tolerance.relative * 64.0 {
        return Ok(IntersectionSet::Empty);
    }
    let angle = radial_cosine.clamp(-1.0, 1.0).acos();
    let minor_parameters = if angle.sin().abs() <= tolerance.angular {
        vec![angle]
    } else {
        vec![angle, -angle]
    };
    let mut curves = Vec::with_capacity(minor_parameters.len());
    for minor_parameter in minor_parameters {
        let center = torus.center() + torus_normal * (torus.minor_radius() * minor_parameter.sin());
        let circle = Circle3::new(center, cylinder.x_radius(), cylinder.y_radius(), tolerance)?;
        let point = circle.evaluate(0.0)?.point;
        let cylinder_v =
            (center - cylinder.origin()).dot(cylinder_axis) / cylinder_axis.magnitude2();
        let torus_projection = Surface3::Torus(torus).project(point, tolerance)?;
        let torus_evaluation = Surface3::Torus(torus)
            .evaluate(torus_projection.parameters.0, torus_projection.parameters.1)?;
        let curve_tangent = circle.evaluate(0.0)?.first_derivative;
        let torus_rate = if curve_tangent.dot(torus_evaluation.du) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        curves.push(SurfaceSurfaceCurve {
            curve: circle.into(),
            first_pcurve: Line2::new(Point2::new(0.0, cylinder_v), crate::Vec2::new(1.0, 0.0))?
                .into(),
            second_pcurve: Line2::new(
                Point2::new(torus_projection.parameters.0, minor_parameter),
                crate::Vec2::new(torus_rate, 0.0),
            )?
            .into(),
            achieved_error: torus_projection.distance,
        });
    }
    Ok(IntersectionSet::Isolated(curves))
}

fn plane_torus(
    plane: Plane3,
    torus: super::Torus3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let linear = tolerance.linear_at_scale(
        torus
            .center()
            .to_vec()
            .magnitude()
            .max(torus.major_radius()),
    );
    if plane.normal().dot(torus.normal()).abs() <= tolerance.angular
        && plane.project(torus.center())?.distance <= linear
    {
        // A plane through the rotation axis cuts two exact meridian circles.
        // Tracing these as general intersections creates duplicate spline
        // guides along existing circular sketch caps.
        let radial = torus.normal().cross(plane.normal()).normalize();
        let mut curves = Vec::new();
        for radial in [radial, -radial] {
            let center = torus.center() + radial * torus.major_radius();
            let circle = Circle3::new(
                center,
                radial * torus.minor_radius(),
                torus.normal() * torus.minor_radius(),
                tolerance,
            )?;
            let uv = plane.project(center)?.parameters;
            let angle = radial
                .dot(torus.y_major().normalize())
                .atan2(radial.dot(torus.x_major().normalize()));
            curves.push(SurfaceSurfaceCurve {
                curve: circle.into(),
                first_pcurve: SurfaceProjectionPcurve2::new(
                    circle.into(),
                    plane.into(),
                    super::Circle2::new(
                        Point2::new(uv.0, uv.1),
                        torus.minor_radius() / plane.u().magnitude(),
                    )?
                    .into(),
                    super::ParameterRange::new(0.0, std::f64::consts::TAU)?,
                    tolerance,
                )?
                .into(),
                second_pcurve: Line2::new(Point2::new(angle, 0.0), crate::Vec2::unit_y())?.into(),
                achieved_error: plane.project(center)?.distance,
            });
        }
        return Ok(IntersectionSet::Isolated(curves));
    }
    // Keep the general intersection dispatch's existing chart eligibility:
    // widening it changes Boolean split ordering and saved edge identities.
    let scale = plane
        .origin()
        .to_vec()
        .magnitude()
        .max(torus.center().to_vec().magnitude())
        .max(torus.major_radius());
    let u_scale = plane.u().magnitude();
    let v_scale = plane.v().magnitude();
    if (u_scale - v_scale).abs()
        > tolerance
            .linear_at_scale(scale)
            .max(u_scale.max(v_scale) * tolerance.relative * 64.0)
    {
        return Err(unsupported(
            "plane/torus circular intersection needs an equally scaled planar chart",
        ));
    }
    parallel_plane_torus_section(plane, torus, tolerance)
}

/// Exact parallel section, including anisotropic and skew planar charts.
/// Fillet supports opt in independently of Boolean intersection dispatch.
pub(crate) fn parallel_plane_torus_section(
    plane: Plane3,
    torus: super::Torus3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let torus_normal = torus.normal();
    if plane.normal().dot(torus_normal).abs() < 1.0 - tolerance.angular {
        return Err(unsupported(
            "oblique plane/torus intersection requires bounded general surface tracing",
        ));
    }
    let scale = plane
        .origin()
        .to_vec()
        .magnitude()
        .max(torus.center().to_vec().magnitude())
        .max(torus.major_radius());
    let linear = tolerance.linear_at_scale(scale);
    let height = (plane.origin() - torus.center()).dot(torus_normal);
    if height.abs() > torus.minor_radius() + linear {
        return Ok(IntersectionSet::Empty);
    }
    let u_scale = plane.u().magnitude();
    let v_scale = plane.v().magnitude();
    let circular_chart = (u_scale - v_scale).abs()
        <= linear.max(u_scale.max(v_scale) * tolerance.relative * 64.0)
        && plane.u().normalize().dot(plane.v().normalize()).abs() <= tolerance.angular;

    let sine = (height / torus.minor_radius()).clamp(-1.0, 1.0);
    let first = sine.asin();
    let second = std::f64::consts::PI - first;
    let minor_parameters = if (first - second).abs() <= tolerance.parameter
        || (first - second).abs() >= std::f64::consts::TAU - tolerance.parameter
    {
        vec![first]
    } else {
        vec![first, second]
    };
    let center = torus.center() + torus_normal * height;
    let center_uv = plane.project(center)?.parameters;
    let mut curves = Vec::with_capacity(minor_parameters.len());
    for minor_parameter in minor_parameters {
        let radius = torus.major_radius() + torus.minor_radius() * minor_parameter.cos();
        let x = plane.u().normalize();
        let y = plane.normal().cross(x).normalize();
        let circle = Circle3::new(center, x * radius, y * radius, tolerance)?;
        let evaluation = circle.evaluate(0.0)?;
        let torus_projection = Surface3::Torus(torus).project(evaluation.point, tolerance)?;
        let torus_evaluation =
            Surface3::Torus(torus).evaluate(torus_projection.parameters.0, minor_parameter)?;
        let torus_rate = if evaluation.first_derivative.dot(torus_evaluation.du) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let guide: Curve2 = super::Circle2::new(
            Point2::new(center_uv.0, center_uv.1),
            radius / ((u_scale + v_scale) * 0.5),
        )?
        .into();
        let first_pcurve = if circular_chart {
            guide
        } else {
            // The spatial section is a circle even when its planar UV image
            // is an ellipse. Keep the exact circle and its periodic range;
            // the projection differentiates the plane's affine inverse.
            SurfaceProjectionPcurve2::new(
                circle.into(),
                plane.into(),
                guide,
                super::ParameterRange::new(0.0, std::f64::consts::TAU)?,
                tolerance,
            )?
            .into()
        };
        curves.push(SurfaceSurfaceCurve {
            curve: circle.into(),
            first_pcurve,
            second_pcurve: Line2::new(
                Point2::new(torus_projection.parameters.0, minor_parameter),
                crate::Vec2::new(torus_rate, 0.0),
            )?
            .into(),
            achieved_error: torus_projection
                .distance
                .max(plane.project(evaluation.point)?.distance),
        });
    }
    Ok(IntersectionSet::Isolated(curves))
}

pub(crate) fn swap_surface_curves(
    curves: IntersectionSet<SurfaceSurfaceCurve>,
) -> IntersectionSet<SurfaceSurfaceCurve> {
    match curves {
        IntersectionSet::Isolated(curves) => IntersectionSet::Isolated(
            curves
                .into_iter()
                .map(|curve| SurfaceSurfaceCurve {
                    curve: curve.curve,
                    first_pcurve: curve.second_pcurve,
                    second_pcurve: curve.first_pcurve,
                    achieved_error: curve.achieved_error,
                })
                .collect(),
        ),
        IntersectionSet::Empty => IntersectionSet::Empty,
        IntersectionSet::Coincident => IntersectionSet::Coincident,
    }
}

fn plane_cylinder(
    plane: Plane3,
    cylinder: super::Cylinder3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let axis = cylinder.axis().normalize();
    let alignment = plane.normal().dot(axis).abs();
    if alignment <= tolerance.angular {
        let normal = plane.normal();
        let a = normal.dot(cylinder.x_radius());
        let b = normal.dot(cylinder.y_radius());
        let c = normal.dot(cylinder.origin() - plane.origin());
        let radial = a.hypot(b);
        let linear = tolerance.linear_at_scale(
            cylinder
                .origin()
                .to_vec()
                .magnitude()
                .max(cylinder.radius()),
        );
        if radial <= linear || c.abs() > radial + linear {
            return Ok(IntersectionSet::Empty);
        }
        let phase = b.atan2(a);
        // acos magnifies roundoff near a double root into two visibly
        // non-tangent rails. Preserve an exact tangency within arithmetic
        // precision, without merging distinct intersections at model tolerance.
        let roundoff = f64::EPSILON * 64.0 * cylinder.origin().to_vec().magnitude()
            .max(plane.origin().to_vec().magnitude()).max(radial).max(1.0);
        let offset = if (c.abs() - radial).abs() <= roundoff {
            if c < 0.0 { 0.0 } else { std::f64::consts::PI }
        } else {
            (-c / radial).clamp(-1.0, 1.0).acos()
        };
        let parameters = if offset.sin().abs() <= tolerance.angular {
            vec![phase + offset]
        } else {
            vec![phase + offset, phase - offset]
        };
        let mut curves = Vec::with_capacity(parameters.len());
        for u in parameters {
            let u = u.rem_euclid(std::f64::consts::TAU);
            let origin = cylinder.evaluate(u, 0.0)?.point;
            let line: Curve3 = Line3::new(origin, cylinder.axis())?.into();
            let plane_origin = plane.project(origin)?.parameters;
            let plane_next = plane.project(origin + cylinder.axis())?.parameters;
            curves.push(SurfaceSurfaceCurve {
                curve: line,
                first_pcurve: Line2::new(
                    Point2::new(plane_origin.0, plane_origin.1),
                    Point2::new(plane_next.0, plane_next.1)
                        - Point2::new(plane_origin.0, plane_origin.1),
                )?
                .into(),
                second_pcurve: Line2::new(Point2::new(u, 0.0), crate::Vec2::new(0.0, 1.0))?.into(),
                achieved_error: plane.project(origin)?.distance,
            });
        }
        return Ok(IntersectionSet::Isolated(curves));
    }
    if alignment < 1.0 - tolerance.angular {
        let denominator = plane.normal().dot(cylinder.axis());
        let range = super::ParameterRange::new(0.0, std::f64::consts::TAU)?;
        let mut plane_points = Vec::new();
        let mut cylinder_points = Vec::new();
        let mut achieved_error = 0.0_f64;
        for index in 0..=32 {
            let u = std::f64::consts::TAU * index as f64 / 32.0;
            let radial = cylinder.x_radius() * u.cos() + cylinder.y_radius() * u.sin();
            let v = -plane
                .normal()
                .dot(cylinder.origin() + radial - plane.origin())
                / denominator;
            let point = cylinder.evaluate(u, v)?.point;
            let projected = plane.project(point)?;
            achieved_error = achieved_error.max(projected.distance);
            plane_points.push(Point2::new(projected.parameters.0, projected.parameters.1));
            cylinder_points.push(Point2::new(u, v));
        }
        let plane_guide: Curve2 = smooth_nurbs_guide(&plane_points, range)?.into();
        let cylinder_guide: Curve2 = smooth_nurbs_guide(&cylinder_points, range)?.into();
        let implicit = SurfaceIntersectionCurve::new(
            Surface3::Plane(plane),
            Surface3::Cylinder(cylinder),
            plane_guide.clone(),
            cylinder_guide.clone(),
            range,
            tolerance,
            achieved_error,
        )?;
        let [first_pcurve, second_pcurve] = implicit.exact_pcurves();
        return Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
            curve: implicit.clone().into(),
            first_pcurve,
            second_pcurve,
            achieved_error,
        }]));
    }
    let denominator = plane.normal().dot(cylinder.axis());
    let v = plane.normal().dot(plane.origin() - cylinder.origin()) / denominator;
    let center = cylinder.origin() + cylinder.axis() * v;
    // Plane parameter axes retain application-scale lengths (for example, an
    // extruded side face commonly uses the profile edge and extrusion vector
    // directly).  The spatial section must therefore use the cylinder's
    // orthonormal radial frame, not `plane.u/v * radius`.
    let curve: Curve3 =
        Circle3::new(center, cylinder.x_radius(), cylinder.y_radius(), tolerance)?.into();
    let range = super::ParameterRange::new(0.0, std::f64::consts::TAU)?;
    let plane_center = plane.project(center)?.parameters;
    let plane_x = plane.project(center + cylinder.x_radius())?.parameters;
    let plane_y = plane.project(center + cylinder.y_radius())?.parameters;
    let guide_radius = [
        (plane_x.0 - plane_center.0).hypot(plane_x.1 - plane_center.1),
        (plane_y.0 - plane_center.0).hypot(plane_y.1 - plane_center.1),
    ]
    .into_iter()
    .fold(tolerance.parameter.max(f64::EPSILON), f64::max);
    // A scaled plane maps the spatial circle to an ellipse in UV.  Retain the
    // exact 3D circle and obtain its exact affine inverse on the plane; the
    // circle below is only the local branch guide for that inverse.
    let plane_pcurve: Curve2 = SurfaceProjectionPcurve2::new(
        curve.clone(),
        Surface3::Plane(plane),
        super::Circle2::new(Point2::new(plane_center.0, plane_center.1), guide_radius)?.into(),
        range,
        tolerance,
    )?
    .into();
    Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
        curve,
        first_pcurve: plane_pcurve,
        second_pcurve: Line2::new(Point2::new(0.0, v), crate::Vec2::new(1.0, 0.0))?.into(),
        achieved_error: 0.0,
    }]))
}

fn trace_regular_surface_intersections(
    first: &Surface3,
    second: &Surface3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let first_domains = bounded_surface_domains(first)?;
    let second_domains = bounded_surface_domains(second)?;
    trace_surface_intersections_in_domains(first, second, first_domains, second_domains, tolerance)
}

fn trace_surface_intersections_in_domains(
    first: &Surface3,
    second: &Surface3,
    first_domains: [super::ParameterRange; 2],
    second_domains: [super::ParameterRange; 2],
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let parameter_scale = first_domains
        .iter()
        .chain(&second_domains)
        .map(|range| range.span().abs())
        .fold(1.0_f64, f64::max);
    let mut seeds = Vec::<[f64; 4]>::new();
    let mut try_seed = |guess: [f64; 4]| -> KernelResult<()> {
        let mut candidates = Vec::new();
        if let Some(parameters) = correct_surface_pair(
            first,
            second,
            guess,
            first_domains,
            second_domains,
            tolerance,
        )? {
            candidates.push(parameters);
        }
        // A short branch can enter through a periodic chart seam and leave
        // through another trim before any interior grid seed reaches it.
        // Keep boundary guesses on their boundary during correction so they
        // cannot all drift onto a different branch of the intersection.
        for (index, range) in first_domains.iter().chain(&second_domains).enumerate() {
            if guess[index] != range.start && guess[index] != range.end {
                continue;
            }
            let mut gauge = [0.0; 4];
            gauge[index] = 1.0;
            if let Some(parameters) = correct_surface_pair_with_gauge(
                first,
                second,
                guess,
                gauge,
                first_domains,
                second_domains,
                tolerance,
            )? {
                candidates.push(parameters);
            }
        }
        for parameters in candidates {
            let (point, du, dv) = first.first_order(parameters[0], parameters[1])?;
            let (_, eu, ev) = second.first_order(parameters[2], parameters[3])?;
            // Both interior and boundary-constrained seeds must be regular.
            // A collapsed chart direction can otherwise manufacture tiny
            // intersection loops around an isolated contact point.
            let regular_chart = |u: Vec3, v: Vec3, ranges: [super::ParameterRange; 2]| {
                let u = u * ranges[0].span().abs();
                let v = v * ranges[1].span().abs();
                u.cross(v).magnitude() > tolerance.linear_at_scale(point.to_vec().magnitude()) * u.magnitude().max(v.magnitude())
            };
            if !regular_chart(du, dv, first_domains) || !regular_chart(eu, ev, second_domains) { continue; }
            if du.cross(dv).normalize().cross(eu.cross(ev).normalize()).magnitude() <= tolerance.angular * 8.0 { continue; }
            if seeds.iter().any(|existing| {
                first.evaluate(existing[0], existing[1]).is_ok_and(|value| {
                    value.point.distance(point)
                        <= tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0)) * 16.0
                })
            }) {
                continue;
            }
            seeds.push(parameters);
        }
        Ok(())
    };

    if matches!(first, Surface3::Nurbs(_) | Surface3::Offset(_))
        && matches!(second, Surface3::Nurbs(_) | Surface3::Offset(_))
    {
        let mut stack = Vec::new();
        let second_cells = rational_surface_cells(second)?;
        for first_cell in rational_surface_cells(first)? {
            for second_cell in &second_cells {
                stack.push((first_cell.clone(), second_cell.clone(), 0_u8));
            }
        }
        let mut visited = 0_usize;
        while let Some((first_cell, second_cell, depth)) = stack.pop() {
            visited += 1;
            if visited > 500_000 {
                return Err(KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "certified NURBS surface intersection exceeded its subdivision budget",
                ));
            }
            let first_bounds = first_cell.bounds()?;
            let second_bounds = second_cell.bounds()?;
            if surface_bounds_separated(first_bounds, second_bounds, tolerance.linear) {
                continue;
            }
            let first_flat = first_cell.directional_flatness()?;
            let second_flat = second_cell.directional_flatness()?;
            let linear = tolerance.linear_at_scale(
                surface_bounds_scale(first_bounds).max(surface_bounds_scale(second_bounds)),
            );
            let first_max_flat = first_flat[0].max(first_flat[1]);
            let second_max_flat = second_flat[0].max(second_flat[1]);
            if depth >= 48 || first_max_flat.max(second_max_flat) <= linear * 2.0 {
                try_seed([
                    first_cell.u_range.start + first_cell.u_range.span() * 0.5,
                    first_cell.v_range.start + first_cell.v_range.span() * 0.5,
                    second_cell.u_range.start + second_cell.u_range.span() * 0.5,
                    second_cell.v_range.start + second_cell.v_range.span() * 0.5,
                ])?;
                continue;
            }
            if first_max_flat >= second_max_flat {
                let (left, right) = first_cell.split_direction(first_flat);
                stack.push((right, second_cell.clone(), depth + 1));
                stack.push((left, second_cell, depth + 1));
            } else {
                let (left, right) = second_cell.split_direction(second_flat);
                stack.push((first_cell.clone(), right, depth + 1));
                stack.push((first_cell, left, depth + 1));
            }
        }
    } else {
        const DIVISIONS: usize = 3;
        for first_u in 0..=DIVISIONS {
            for first_v in 0..=DIVISIONS {
                for second_u in 0..=DIVISIONS {
                    for second_v in 0..=DIVISIONS {
                        try_seed([
                            first_domains[0].start
                                + first_domains[0].span() * first_u as f64 / DIVISIONS as f64,
                            first_domains[1].start
                                + first_domains[1].span() * first_v as f64 / DIVISIONS as f64,
                            second_domains[0].start
                                + second_domains[0].span() * second_u as f64 / DIVISIONS as f64,
                            second_domains[1].start
                                + second_domains[1].span() * second_v as f64 / DIVISIONS as f64,
                        ])?;
                    }
                }
            }
        }
    }
    if seeds.is_empty() {
        return Ok(IntersectionSet::Empty);
    }

    let nested_intersection = [first, second].iter().any(|surface| {
        surface.domains()[0].period.is_none()
            && matches!(surface, Surface3::RollingBall(blend) if matches!(blend.center_curve(), Curve3::SurfaceIntersection(_)))
    });
    let base_step = parameter_scale / if nested_intersection { 192.0 } else { 48.0 };
    let mut traced = Vec::<Vec<[f64; 4]>>::new();
    for seed in seeds {
        if traced.iter().any(|curve| {
            curve
                .iter()
                .any(|parameters| parameter_distance(*parameters, seed) <= base_step * 1.5)
        }) {
            continue;
        }
        let backward = trace_surface_intersection_direction(
            first,
            second,
            seed,
            -1.0,
            base_step,
            first_domains,
            second_domains,
            tolerance,
        )?;
        let forward = trace_surface_intersection_direction(
            first,
            second,
            seed,
            1.0,
            base_step,
            first_domains,
            second_domains,
            tolerance,
        )?;
        let mut parameters = backward.into_iter().rev().collect::<Vec<_>>();
        if parameters
            .last()
            .is_some_and(|last| parameter_distance(*last, seed) <= tolerance.parameter * 16.0)
        {
            parameters.pop();
        }
        parameters.push(seed);
        parameters.extend(forward);
        parameters.dedup_by(|first, second| {
            parameter_distance(*first, *second) <= tolerance.parameter * 16.0
        });
        // At a tangency the residual is quadratic in parameter error, so a
        // converged trace can still sit next to a trim corner. A nearby box
        // endpoint is usable only if it is a common point of both supports.
        let snap = |values: &mut [f64; 4]| -> KernelResult<()> {
            let endpoint_step = base_step * if nested_intersection { 2.0 } else { 0.1 };
            // Retain any trim already reached while snapping toward a nearby
            // corner. Moving off it can collapse a short seam-to-cap branch.
            let preserves_bounds = |candidate: [f64; 4], original: [f64; 4]| {
                first_domains
                    .iter()
                    .chain(&second_domains)
                    .enumerate()
                    .all(|(index, range)| {
                        [range.start, range.end].into_iter().all(|bound| {
                            (original[index] - bound).abs() > tolerance.parameter
                                || (candidate[index] - bound).abs() <= tolerance.parameter
                        })
                    })
            };
            // Correct all remaining coordinates with a nearby trim coordinate
            // fixed. Projecting a clamped point onto just the other support can
            // accept a tangential residual while leaving an endpoint short of
            // the trim in parameter space.
            for (index, range) in [
                first_domains[0],
                first_domains[1],
                second_domains[0],
                second_domains[1],
            ]
            .into_iter()
            .enumerate()
            {
                for bound in [range.start, range.end] {
                    if (values[index] - bound).abs() > endpoint_step {
                        continue;
                    }
                    let mut guide = *values;
                    guide[index] = bound;
                    let mut gauge = [0.0; 4];
                    gauge[index] = 1.0;
                    if let Some(endpoint) = correct_surface_pair_with_gauge(
                        first,
                        second,
                        guide,
                        gauge,
                        first_domains,
                        second_domains,
                        tolerance,
                    )? {
                        if parameter_distance(endpoint, *values) <= endpoint_step
                            && preserves_bounds(endpoint, *values)
                        {
                            *values = endpoint;
                        }
                    }
                }
            }
            let mut candidate = *values;
            let mut changed = [false; 2];
            for (index, range) in [
                first_domains[0],
                first_domains[1],
                second_domains[0],
                second_domains[1],
            ]
            .into_iter()
            .enumerate()
            {
                for bound in [range.start, range.end] {
                    if (candidate[index] - bound).abs() <= endpoint_step {
                        candidate[index] = bound;
                        changed[index / 2] = true;
                    }
                }
            }
            for side in 0..2 {
                if !changed[side] { continue; }
                let mut endpoint = candidate;
                let (point, projection) = if side == 0 {
                    let point = first.evaluate(candidate[0], candidate[1])?.point;
                    let projection = second.project(point, tolerance)?;
                    endpoint[2] = projection.parameters.0;
                    endpoint[3] = projection.parameters.1;
                    (point, projection)
                } else {
                    let point = second.evaluate(candidate[2], candidate[3])?.point;
                    let projection = first.project(point, tolerance)?;
                    endpoint[0] = projection.parameters.0;
                    endpoint[1] = projection.parameters.1;
                    (point, projection)
                };
                for (index, domain) in first.domains().into_iter().chain(second.domains()).enumerate() {
                    if let Some(period) = domain.period {
                        endpoint[index] += ((values[index] - endpoint[index]) / period).round() * period;
                    }
                }
                if projection.distance <= f64::EPSILON * point.to_vec().magnitude().max(1.0) * 256.0
                    && preserves_bounds(endpoint, *values)
                    && parameters_in_ranges(
                        endpoint,
                        first_domains,
                        second_domains,
                        tolerance.parameter,
                    )
                {
                    *values = endpoint;
                    break;
                }
            }
            Ok(())
        };
        if let Some(value) = parameters.first_mut() {
            snap(value)?;
        }
        if let Some(value) = parameters.last_mut() {
            snap(value)?;
        }
        // Snapping both ends of a short tangency trace to the same trim
        // corner can leave only A-B-A. This retraces one segment, rather than
        // defining a closed intersection, and has a zero tangent at its turn.
        if parameters.len() == 3
            && parameter_distance(parameters[0], parameters[2]) <= tolerance.parameter * 16.0
        {
            continue;
        }
        if parameters.len() >= 2 {
            // A tangential contact at a trim corner can converge to several
            // parameter samples of the same spatial point. It has no curve
            // extent and must not become a constant, singular guide spline.
            let origin = first.evaluate(parameters[0][0], parameters[0][1])?.point;
            let linear = tolerance.linear_at_scale(origin.to_vec().magnitude().max(1.0));
            let mut extent = 0.0_f64;
            for value in &parameters {
                extent = extent.max(origin.distance(first.evaluate(value[0], value[1])?.point));
                extent = extent.max(origin.distance(second.evaluate(value[2], value[3])?.point));
            }
            if extent > linear {
                traced.push(parameters);
            }
        }
    }

    let curves = traced
        .into_iter()
        .map(|parameters| {
            let mut first_points = parameters
                .iter()
                .map(|value| Point2::new(value[0], value[1]))
                .collect::<Vec<_>>();
            let mut second_points = parameters
                .iter()
                .map(|value| Point2::new(value[2], value[3]))
                .collect::<Vec<_>>();
            unwrap_periodic_points(&mut first_points, first.domains());
            unwrap_periodic_points(&mut second_points, second.domains());
            let range = super::ParameterRange::new(0.0, 1.0)?;
            let guide = |points: &[Point2]| {
                if nested_intersection {
                    let parameters = (0..points.len()).map(|index| index as f64 / (points.len() - 1) as f64).collect::<Vec<_>>();
                    interpolating_nurbs_guide(points, &parameters)
                } else { smooth_nurbs_guide(points, range) }
            };
            let first_pcurve = guide(&first_points)?;
            let second_pcurve = guide(&second_points)?;
            let achieved_error = parameters.iter().try_fold(0.0_f64, |error, value| {
                let first = first.evaluate(value[0], value[1])?.point;
                let second = second.evaluate(value[2], value[3])?.point;
                Ok::<_, KernelError>(error.max(first.distance(second)))
            })?;
            let implicit = SurfaceIntersectionCurve::new(
                first.clone(),
                second.clone(),
                first_pcurve.clone().into(),
                second_pcurve.clone().into(),
                range,
                tolerance,
                achieved_error,
            )?;
            // Trims and spatial edges must use the same corrected intersection;
            // the tracing splines only identify its branch.
            let [first_pcurve, second_pcurve] = implicit.exact_pcurves();
            Ok(SurfaceSurfaceCurve {
                curve: implicit.into(),
                first_pcurve,
                second_pcurve,
                achieved_error,
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    Ok(if curves.is_empty() {
        IntersectionSet::Empty
    } else {
        IntersectionSet::Isolated(curves)
    })
}

fn bounded_surface_domains(surface: &Surface3) -> KernelResult<[super::ParameterRange; 2]> {
    let domains = surface.domains();
    let DomainBounds::Bounded(first) = domains[0].bounds else {
        return Err(unsupported(
            "general surface tracing requires a bounded first parameter domain",
        ));
    };
    let DomainBounds::Bounded(second) = domains[1].bounds else {
        return Err(unsupported(
            "general surface tracing requires a bounded second parameter domain",
        ));
    };
    Ok([first, second])
}

#[derive(Clone)]
struct RationalSurfaceCell {
    u_range: super::ParameterRange,
    v_range: super::ParameterRange,
    controls: Vec<Vec<Vec4>>,
    padding: f64,
}

impl From<super::geometry::BezierSurfacePatch> for RationalSurfaceCell {
    fn from(value: super::geometry::BezierSurfacePatch) -> Self {
        Self {
            u_range: value.u_range,
            v_range: value.v_range,
            controls: value.controls,
            padding: 0.0,
        }
    }
}

impl RationalSurfaceCell {
    fn euclidean(control: Vec4) -> KernelResult<Point3> {
        if control.w <= 0.0 || !control.w.is_finite() {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "rational Bézier surface has a nonpositive homogeneous weight",
            ));
        }
        Ok(Point3::new(
            control.x / control.w,
            control.y / control.w,
            control.z / control.w,
        ))
    }

    fn bounds(&self) -> KernelResult<[Point3; 2]> {
        self.controls
            .iter()
            .flatten()
            .try_fold(
                [
                    Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                    Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
                ],
                |mut bounds, control| {
                    let point = Self::euclidean(*control)?;
                    bounds[0].x = bounds[0].x.min(point.x);
                    bounds[0].y = bounds[0].y.min(point.y);
                    bounds[0].z = bounds[0].z.min(point.z);
                    bounds[1].x = bounds[1].x.max(point.x);
                    bounds[1].y = bounds[1].y.max(point.y);
                    bounds[1].z = bounds[1].z.max(point.z);
                    Ok(bounds)
                },
            )
            .map(|mut bounds| {
                let padding = crate::Vec3::new(self.padding, self.padding, self.padding);
                bounds[0] -= padding;
                bounds[1] += padding;
                bounds
            })
    }

    fn directional_flatness(&self) -> KernelResult<[f64; 2]> {
        let control_flatness = |controls: Vec<Vec4>| -> KernelResult<f64> {
            let points = controls
                .into_iter()
                .map(Self::euclidean)
                .collect::<KernelResult<Vec<_>>>()?;
            let direction = points[points.len() - 1] - points[0];
            let length = direction.magnitude();
            if length <= f64::EPSILON {
                return Ok(points
                    .iter()
                    .map(|point| point.distance(points[0]))
                    .fold(0.0, f64::max));
            }
            Ok(points
                .iter()
                .map(|point| (*point - points[0]).cross(direction).magnitude() / length)
                .fold(0.0, f64::max))
        };
        let u = self
            .controls
            .iter()
            .map(|row| control_flatness(row.clone()))
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .fold(0.0, f64::max);
        let v = (0..self.controls[0].len())
            .map(|column| control_flatness(self.controls.iter().map(|row| row[column]).collect()))
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .fold(0.0, f64::max);
        Ok([u, v])
    }

    fn split_direction(self, flatness: [f64; 2]) -> (Self, Self) {
        if flatness[0] >= flatness[1] {
            self.split_u()
        } else {
            self.split_v()
        }
    }

    fn split_u(self) -> (Self, Self) {
        let (left_rows, right_rows): (Vec<_>, Vec<_>) = self
            .controls
            .into_iter()
            .map(split_homogeneous_values)
            .unzip();
        let middle = (self.u_range.start + self.u_range.end) * 0.5;
        (
            Self {
                u_range: super::ParameterRange {
                    start: self.u_range.start,
                    end: middle,
                },
                v_range: self.v_range,
                controls: left_rows,
                padding: self.padding,
            },
            Self {
                u_range: super::ParameterRange {
                    start: middle,
                    end: self.u_range.end,
                },
                v_range: self.v_range,
                controls: right_rows,
                padding: self.padding,
            },
        )
    }

    fn split_v(self) -> (Self, Self) {
        let width = self.controls[0].len();
        let (left_columns, right_columns): (Vec<_>, Vec<_>) = (0..width)
            .map(|u| split_homogeneous_values(self.controls.iter().map(|row| row[u]).collect()))
            .unzip();
        let left_height = left_columns[0].len();
        let right_height = right_columns[0].len();
        let left = (0..left_height)
            .map(|v| left_columns.iter().map(|column| column[v]).collect())
            .collect();
        let right = (0..right_height)
            .map(|v| right_columns.iter().map(|column| column[v]).collect())
            .collect();
        let middle = (self.v_range.start + self.v_range.end) * 0.5;
        (
            Self {
                u_range: self.u_range,
                v_range: super::ParameterRange {
                    start: self.v_range.start,
                    end: middle,
                },
                controls: left,
                padding: self.padding,
            },
            Self {
                u_range: self.u_range,
                v_range: super::ParameterRange {
                    start: middle,
                    end: self.v_range.end,
                },
                controls: right,
                padding: self.padding,
            },
        )
    }
}

fn rational_surface_cells(surface: &Surface3) -> KernelResult<Vec<RationalSurfaceCell>> {
    match surface {
        Surface3::Nurbs(surface) => Ok(surface
            .bezier_patches()?
            .into_iter()
            .map(RationalSurfaceCell::from)
            .collect()),
        Surface3::Offset(offset) => {
            let mut cells = rational_surface_cells(offset.base())?;
            for cell in &mut cells {
                cell.padding += offset.maximum_distance();
            }
            Ok(cells)
        }
        _ => Err(unsupported(
            "certified subdivision requires a NURBS-backed bounded surface",
        )),
    }
}

fn split_homogeneous_values(values: Vec<Vec4>) -> (Vec<Vec4>, Vec<Vec4>) {
    let degree = values.len() - 1;
    let mut levels = vec![values];
    for level in 1..=degree {
        levels.push(
            levels[level - 1]
                .windows(2)
                .map(|pair| (pair[0] + pair[1]) * 0.5)
                .collect(),
        );
    }
    let left = (0..=degree).map(|level| levels[level][0]).collect();
    let right = (0..=degree)
        .map(|level| levels[degree - level][level])
        .collect();
    (left, right)
}

fn surface_bounds_separated(first: [Point3; 2], second: [Point3; 2], tolerance: f64) -> bool {
    first[1].x < second[0].x - tolerance
        || second[1].x < first[0].x - tolerance
        || first[1].y < second[0].y - tolerance
        || second[1].y < first[0].y - tolerance
        || first[1].z < second[0].z - tolerance
        || second[1].z < first[0].z - tolerance
}

fn surface_bounds_scale(bounds: [Point3; 2]) -> f64 {
    bounds[0]
        .to_vec()
        .magnitude()
        .max(bounds[1].to_vec().magnitude())
        .max(1.0)
}

fn correct_surface_pair(
    first: &Surface3,
    second: &Surface3,
    parameters: [f64; 4],
    first_domains: [super::ParameterRange; 2],
    second_domains: [super::ParameterRange; 2],
    tolerance: ModelTolerance,
) -> KernelResult<Option<[f64; 4]>> {
    let mut parameters = clamp_parameters(parameters, first_domains, second_domains);
    for _ in 0..32 {
        let (first_point, first_du, first_dv) = first.first_order(parameters[0], parameters[1])?;
        let (second_point, second_du, second_dv) =
            second.first_order(parameters[2], parameters[3])?;
        let residual = first_point - second_point;
        let scale = first_point
            .to_vec()
            .magnitude()
            .max(second_point.to_vec().magnitude())
            .max(1.0);
        // A model-tolerance residual near tangency can manufacture a tiny
        // intersection branch. Match the accuracy of implicit evaluation.
        if residual.magnitude() <= f64::EPSILON * scale * 2048.0 {
            return Ok(Some(parameters));
        }
        let mut jacobian = [
            [first_du.x, first_dv.x, -second_du.x, -second_dv.x],
            [first_du.y, first_dv.y, -second_du.y, -second_dv.y],
            [first_du.z, first_dv.z, -second_du.z, -second_dv.z],
        ];
        let mut delta = [0.0; 4];
        // A clamped parameter cannot contribute an outward Newton step.
        // Re-solve with those columns fixed, or a narrow trim can keep
        // absorbing most of the correction without reducing the residual.
        // Each pass fixes at least one of the four parameters.
        for _ in 0..=4 {
            let normal = multiply_3x4_by_transpose(jacobian);
            let Some(lambda) = solve_linear_3(normal, [-residual.x, -residual.y, -residual.z])
            else {
                return Ok(None);
            };
            delta = std::array::from_fn(|column| {
                (0..3)
                    .map(|row| jacobian[row][column] * lambda[row])
                    .sum::<f64>()
            });
            let mut constrained = false;
            for (index, range) in first_domains.iter().chain(&second_domains).enumerate() {
                if (parameters[index] <= range.start.min(range.end) + tolerance.parameter
                    && delta[index] < 0.0)
                    || (parameters[index] >= range.start.max(range.end) - tolerance.parameter
                        && delta[index] > 0.0)
                {
                    for row in &mut jacobian {
                        row[index] = 0.0;
                    }
                    constrained = true;
                }
            }
            if !constrained {
                break;
            }
        }
        let mut damping = 1.0;
        let mut accepted = false;
        for _ in 0..12 {
            let candidate = clamp_parameters(
                std::array::from_fn(|index| parameters[index] + delta[index] * damping),
                first_domains,
                second_domains,
            );
            if parameters_in_ranges(
                candidate,
                first_domains,
                second_domains,
                tolerance.parameter,
            ) {
                let candidate_residual = first.first_order(candidate[0], candidate[1])?.0
                    - second.first_order(candidate[2], candidate[3])?.0;
                if candidate_residual.magnitude2() < residual.magnitude2() {
                    parameters = candidate;
                    accepted = true;
                    break;
                }
            }
            damping *= 0.5;
        }
        if !accepted {
            return Ok(None);
        }
    }
    Ok(None)
}

fn trace_surface_intersection_direction(
    first: &Surface3,
    second: &Surface3,
    seed: [f64; 4],
    direction: f64,
    step: f64,
    first_domains: [super::ParameterRange; 2],
    second_domains: [super::ParameterRange; 2],
    tolerance: ModelTolerance,
) -> KernelResult<Vec<[f64; 4]>> {
    let mut result = Vec::new();
    let mut current = seed;
    let mut previous_tangent = None;
    let domains = [first.domains(), second.domains()].concat();
    for _ in 0..512 {
        let (_, first_du, first_dv) = first.first_order(current[0], current[1])?;
        let (_, second_du, second_dv) = second.first_order(current[2], current[3])?;
        let jacobian = [
            [first_du.x, first_dv.x, -second_du.x, -second_dv.x],
            [first_du.y, first_dv.y, -second_du.y, -second_dv.y],
            [first_du.z, first_dv.z, -second_du.z, -second_dv.z],
        ];
        let mut tangent = null_vector_3x4(jacobian);
        let magnitude = tangent
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if magnitude <= tolerance.parameter {
            break;
        }
        tangent = tangent.map(|value| value / magnitude);
        if let Some(previous) = previous_tangent {
            if tangent
                .iter()
                .zip(previous)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                < 0.0
            {
                tangent = tangent.map(|value| -value);
            }
        } else {
            tangent = tangent.map(|value| value * direction);
        }
        // Near a turn, the tangent plane can meet the intersection far beyond
        // the predictor. Keep corrections local so the guide does not skip
        // that turn and acquire a nearly singular parameterization.
        let mut local_step = step;
        let mut next = None;
        for _ in 0..16 {
            let predicted =
                std::array::from_fn(|index| current[index] + tangent[index] * local_step);
            let corrected = correct_surface_pair_with_gauge(
                first,
                second,
                predicted,
                tangent,
                first_domains,
                second_domains,
                tolerance,
            )?;
            match corrected {
                Some(value) if parameter_distance(value, current) > local_step * 2.0 => {
                    local_step *= 0.5;
                }
                value => {
                    next = value;
                    break;
                }
            }
        }
        let Some(next) = next else {
            // Complete the intersection at a box boundary by solving both
            // surfaces with that parameter fixed. Clamping a prediction and
            // projecting onto only one support does not give a common point.
            let mut closest = None::<(f64, [f64; 4])>;
            for (index, range) in [
                first_domains[0],
                first_domains[1],
                second_domains[0],
                second_domains[1],
            ]
            .into_iter()
            .enumerate()
            {
                if tangent[index].abs() <= tolerance.parameter {
                    continue;
                }
                let bound = if tangent[index] > 0.0 {
                    range.start.max(range.end)
                } else {
                    range.start.min(range.end)
                };
                let travel = (bound - current[index]) / tangent[index];
                if travel < -tolerance.parameter || travel > step * 2.0 {
                    continue;
                }
                let mut guide = std::array::from_fn(|axis| current[axis] + tangent[axis] * travel);
                guide[index] = bound;
                let mut gauge = [0.0; 4];
                gauge[index] = 1.0;
                if let Some(endpoint) = correct_surface_pair_with_gauge(
                    first,
                    second,
                    guide,
                    gauge,
                    first_domains,
                    second_domains,
                    tolerance,
                )? {
                    let distance = parameter_distance(endpoint, current);
                    if distance > tolerance.parameter * 16.0
                        && closest.as_ref().is_none_or(|(best, _)| distance < *best)
                    {
                        closest = Some((distance, endpoint));
                    }
                }
            }
            if let Some((_, endpoint)) = closest {
                result.push(endpoint);
            }
            break;
        };
        if !parameters_in_ranges(
            next,
            first_domains,
            second_domains,
            tolerance.parameter * 8.0,
        ) {
            break;
        }
        if parameter_distance(next, current) <= tolerance.parameter * 16.0 {
            break;
        }
        result.push(next);
        // Stop on the first trim reached, excluding full-period seams.
        // Continuing along a trim can switch branches at a collapsing blend
        // and corrupt the curve guide.
        if first_domains.iter().chain(&second_domains).enumerate().any(|(index, range)| {
            if domains[index].period.is_some_and(|period| range.span().abs() >= period - tolerance.parameter * 8.0) { return false; }
            [range.start, range.end].into_iter().any(|bound| {
                (next[index] - bound).abs() <= tolerance.parameter * 8.0
                    && (current[index] - bound).abs() > tolerance.parameter * 8.0
            })
        }) {
            break;
        }
        let (_, du, dv) = first.first_order(next[0], next[1])?;
        let (_, eu, ev) = second.first_order(next[2], next[3])?;
        if du.cross(dv).magnitude2() == 0.0 || eu.cross(ev).magnitude2() == 0.0
            || du.cross(dv).normalize().cross(eu.cross(ev).normalize()).magnitude() <= tolerance.angular * 8.0
        {
            break;
        }
        if result.len() > 12 && parameter_distance(next, seed) <= step * 0.75 {
            break;
        }
        current = next;
        previous_tangent = Some(tangent);
    }
    Ok(result)
}

pub(crate) fn correct_surface_pair_with_gauge(
    first: &Surface3,
    second: &Surface3,
    guide: [f64; 4],
    tangent: [f64; 4],
    first_domains: [super::ParameterRange; 2],
    second_domains: [super::ParameterRange; 2],
    tolerance: ModelTolerance,
) -> KernelResult<Option<[f64; 4]>> {
    let mut parameters = clamp_parameters(guide, first_domains, second_domains);
    for _ in 0..24 {
        let (first_point, first_du, first_dv) = first.first_order(parameters[0], parameters[1])?;
        let (second_point, second_du, second_dv) =
            second.first_order(parameters[2], parameters[3])?;
        let residual = first_point - second_point;
        let gauge = parameters
            .iter()
            .zip(guide)
            .zip(tangent)
            .map(|((value, guide), tangent)| (value - guide) * tangent)
            .sum::<f64>();
        // The supports can themselves contain corrected implicit curves;
        // demanding a tighter residual than their evaluation stops a valid trace.
        if residual.magnitude() <= f64::EPSILON * first_point.to_vec().magnitude().max(1.0) * 2048.0
            && gauge.abs()
                <= f64::EPSILON * guide.iter().map(|v| v.abs()).fold(1.0_f64, f64::max) * 2048.0
        {
            return Ok(Some(parameters));
        }
        let matrix = [
            [first_du.x, first_dv.x, -second_du.x, -second_dv.x],
            [first_du.y, first_dv.y, -second_du.y, -second_dv.y],
            [first_du.z, first_dv.z, -second_du.z, -second_dv.z],
            tangent,
        ];
        let Some(delta) = solve_linear_4(matrix, [-residual.x, -residual.y, -residual.z, -gauge])
        else {
            return Ok(None);
        };
        let candidate = clamp_parameters(
            std::array::from_fn(|index| parameters[index] + delta[index]),
            first_domains,
            second_domains,
        );
        if !parameters_in_ranges(
            candidate,
            first_domains,
            second_domains,
            tolerance.parameter * 8.0,
        ) {
            return Ok(None);
        }
        parameters = candidate;
    }
    Ok(None)
}

/// Builds a smooth parameter-space guide for an exact implicit intersection.
///
/// The guide selects the local Newton branch; it is never used as the modeled
/// intersection itself. A clamped cubic guide is important here because a
/// piecewise-linear parameterization injects artificial tangent jumps into an
/// otherwise regular intersection, preventing certified boundary integration.
pub(crate) fn smooth_nurbs_guide(
    points: &[Point2],
    range: super::ParameterRange,
) -> KernelResult<NurbsCurve2> {
    // Coedges can run backwards, but a NURBS knot vector must increase.
    // Reverse both the samples and range to preserve their parameter values.
    if range.span() < 0.0 {
        let reversed = points.iter().rev().copied().collect::<Vec<_>>();
        return smooth_nurbs_guide(&reversed, range.reversed());
    }
    if points.len() < 2 {
        return Err(unsupported(
            "surface-intersection guide needs at least two points",
        ));
    }
    let degree = 3.min(points.len() - 1);
    let mut knots = vec![range.start; degree + 1];
    let internal_count = points.len() - degree - 1;
    knots.extend(
        (1..=internal_count)
            .map(|index| range.start + range.span() * index as f64 / (internal_count + 1) as f64),
    );
    knots.extend(vec![range.end; degree + 1]);
    NurbsCurve2::new(degree, points.to_vec(), vec![1.0; points.len()], knots)
}

/// Preserve each traced sample while making the guide tangent continuous.
/// Monotone Hermite slopes keep a short endpoint span from overshooting its trim.
pub(crate) fn smooth_interpolating_guide(curve: &NurbsCurve2) -> KernelResult<Curve2> {
    if curve.degree() != 1 { return Ok(curve.clone().into()); }
    let points = curve.control_points();
    let parameters = &curve.knots()[1..curve.knots().len() - 1];
    let slopes = points.windows(2).zip(parameters.windows(2)).map(|(points, parameters)| {
        (points[1] - points[0]) / (parameters[1] - parameters[0])
    }).collect::<Vec<_>>();
    let tangents = (0..points.len()).map(|index| {
        if index == 0 { return slopes[0]; }
        if index == points.len() - 1 { return slopes[index - 1]; }
        let mut tangent = crate::Vec2::new(0.0, 0.0);
        for axis in 0..2 {
            let a = slopes[index - 1][axis];
            let b = slopes[index][axis];
            tangent[axis] = if a * b > 0.0 { 2.0 * a * b / (a + b) } else { 0.0 };
        }
        tangent
    }).collect::<Vec<_>>();
    let mut controls = vec![points[0]];
    for index in 0..points.len() - 1 {
        let step = (parameters[index + 1] - parameters[index]) / 3.0;
        controls.extend([points[index] + tangents[index] * step,
            points[index + 1] - tangents[index + 1] * step, points[index + 1]]);
    }
    let mut knots = vec![parameters[0]; 4];
    for parameter in &parameters[1..parameters.len() - 1] { knots.extend([*parameter; 3]); }
    knots.extend([*parameters.last().unwrap(); 4]);
    let weights = vec![1.0; controls.len()];
    Ok(NurbsCurve2::new(3, controls, weights, knots)?.into())
}

/// A repeated cubic control net includes one control beyond each end, so
/// uniform unclamped knots preserve its exact period and UV winding. Clamping
/// that net changes the interior spacing and compresses the traversal.
pub(crate) fn periodic_nurbs_guide(
    points: &[Point2],
    range: super::ParameterRange,
) -> KernelResult<NurbsCurve2> {
    let step = range.span() / (points.len() - 3) as f64;
    let knots = (0..points.len() + 4)
        .map(|i| range.start + (i as f64 - 3.0) * step)
        .collect();
    NurbsCurve2::new(3, points.to_vec(), vec![1.0; points.len()], knots)
}

/// Lift sampled UV values into one continuous copy of every periodic chart.
/// Surface projection returns principal parameters, which may jump by a full
/// period along an otherwise smooth edge; feeding that jump to a spline guide
/// creates a false high-speed excursion through the middle of the chart.
pub(crate) fn unwrap_periodic_points(points: &mut [Point2], domains: [super::ParameterDomain; 2]) {
    for index in 1..points.len() {
        let previous = points[index - 1];
        if let Some(period) = domains[0].period {
            points[index].x += ((previous.x - points[index].x) / period).round() * period;
        }
        if let Some(period) = domains[1].period {
            points[index].y += ((previous.y - points[index].y) / period).round() * period;
        }
    }
}

/// Builds an interpolating piecewise-linear guide for difficult local inverse
/// branches. The guide is not modeled geometry; exact evaluation is still
/// corrected against both retained surfaces.
pub(crate) fn interpolating_nurbs_guide(
    points: &[Point2],
    parameters: &[f64],
) -> KernelResult<NurbsCurve2> {
    if points.len() < 2 {
        return Err(unsupported(
            "surface-intersection guide needs at least two points",
        ));
    }
    if parameters.len() != points.len()
        || parameters.iter().any(|parameter| !parameter.is_finite())
        || parameters.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(unsupported(
            "interpolating surface guide parameters must strictly increase",
        ));
    }
    let mut knots = vec![parameters[0]; 2];
    knots.extend(parameters[1..parameters.len() - 1].iter().copied());
    knots.extend(vec![parameters[parameters.len() - 1]; 2]);
    NurbsCurve2::new(1, points.to_vec(), vec![1.0; points.len()], knots)
}

fn multiply_3x4_by_transpose(matrix: [[f64; 4]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..4)
                .map(|index| matrix[row][index] * matrix[column][index])
                .sum()
        })
    })
}

pub(crate) fn solve_linear_3(mut matrix: [[f64; 3]; 3], mut right: [f64; 3]) -> Option<[f64; 3]> {
    for column in 0..3 {
        let pivot = (column..3).max_by(|first, second| {
            matrix[*first][column]
                .abs()
                .total_cmp(&matrix[*second][column].abs())
        })?;
        if matrix[pivot][column].abs() <= f64::EPSILON * 256.0 {
            return None;
        }
        matrix.swap(column, pivot);
        right.swap(column, pivot);
        let divisor = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= divisor;
        }
        right[column] /= divisor;
        for row in 0..3 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            for index in column..3 {
                matrix[row][index] -= factor * matrix[column][index];
            }
            right[row] -= factor * right[column];
        }
    }
    Some(right)
}

fn solve_linear_4(mut matrix: [[f64; 4]; 4], mut right: [f64; 4]) -> Option<[f64; 4]> {
    for column in 0..4 {
        let pivot = (column..4).max_by(|first, second| {
            matrix[*first][column]
                .abs()
                .total_cmp(&matrix[*second][column].abs())
        })?;
        if matrix[pivot][column].abs() <= f64::EPSILON * 256.0 {
            return None;
        }
        matrix.swap(column, pivot);
        right.swap(column, pivot);
        let divisor = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= divisor;
        }
        right[column] /= divisor;
        for row in 0..4 {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            for index in column..4 {
                matrix[row][index] -= factor * matrix[column][index];
            }
            right[row] -= factor * right[column];
        }
    }
    Some(right)
}

fn null_vector_3x4(matrix: [[f64; 4]; 3]) -> [f64; 4] {
    std::array::from_fn(|column| {
        let mut minor = [[0.0; 3]; 3];
        for row in 0..3 {
            let mut target = 0;
            for source in 0..4 {
                if source != column {
                    minor[row][target] = matrix[row][source];
                    target += 1;
                }
            }
        }
        let determinant = minor[0][0] * (minor[1][1] * minor[2][2] - minor[1][2] * minor[2][1])
            - minor[0][1] * (minor[1][0] * minor[2][2] - minor[1][2] * minor[2][0])
            + minor[0][2] * (minor[1][0] * minor[2][1] - minor[1][1] * minor[2][0]);
        if column % 2 == 0 {
            determinant
        } else {
            -determinant
        }
    })
}

fn parameters_in_ranges(
    parameters: [f64; 4],
    first: [super::ParameterRange; 2],
    second: [super::ParameterRange; 2],
    tolerance: f64,
) -> bool {
    [first[0], first[1], second[0], second[1]]
        .into_iter()
        .zip(parameters)
        .all(|(range, value)| {
            value >= range.start.min(range.end) - tolerance
                && value <= range.start.max(range.end) + tolerance
        })
}

fn clamp_parameters(
    parameters: [f64; 4],
    first: [super::ParameterRange; 2],
    second: [super::ParameterRange; 2],
) -> [f64; 4] {
    let ranges = [first[0], first[1], second[0], second[1]];
    std::array::from_fn(|index| {
        parameters[index].clamp(
            ranges[index].start.min(ranges[index].end),
            ranges[index].start.max(ranges[index].end),
        )
    })
}

fn parameter_distance(first: [f64; 4], second: [f64; 4]) -> f64 {
    first
        .iter()
        .zip(second)
        .map(|(first, second)| (first - second).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn line_line(
    first: Line3,
    second: Line3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveCurveHit>> {
    let p = first.origin();
    let q = second.origin();
    let u = first.direction();
    let v = second.direction();
    let a = u.magnitude2();
    let b = u.dot(v);
    let c = v.magnitude2();
    let delta = p - q;
    let d = u.dot(delta);
    let e = v.dot(delta);
    let denominator = a * c - b * b;
    let scale = p.to_vec().magnitude().max(q.to_vec().magnitude()).max(1.0);
    let linear = tolerance.linear_at_scale(scale);
    if denominator.abs() <= tolerance.angular * a * c {
        let distance = delta.cross(u).magnitude() / u.magnitude();
        return Ok(if distance <= linear {
            IntersectionSet::Coincident
        } else {
            IntersectionSet::Empty
        });
    }
    let first_parameter = (b * e - c * d) / denominator;
    let second_parameter = (a * e - b * d) / denominator;
    let first_point = first.evaluate(first_parameter)?.point;
    let second_point = second.evaluate(second_parameter)?.point;
    let error = first_point.distance(second_point);
    if error > linear {
        return Ok(IntersectionSet::Empty);
    }
    Ok(IntersectionSet::Isolated(vec![CurveCurveHit {
        point: Point3::from_vec((first_point.to_vec() + second_point.to_vec()) * 0.5),
        first_parameter,
        second_parameter,
        kind: IntersectionKind::Transverse,
        multiplicity: 1,
        achieved_error: error,
    }]))
}

fn line_circle(
    line: Line3,
    circle: Circle3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveCurveHit>> {
    let normal = circle.normal();
    let plane_distance = (line.origin() - circle.center()).dot(normal);
    let denominator = line.direction().dot(normal);
    let scale = circle
        .center()
        .to_vec()
        .magnitude()
        .max(circle.radius())
        .max(1.0);
    let linear = tolerance.linear_at_scale(scale);
    if denominator.abs() > tolerance.angular * line.direction().magnitude() {
        let parameter = -plane_distance / denominator;
        let point = line.evaluate(parameter)?.point;
        let radial_error = ((point - circle.center()).magnitude() - circle.radius()).abs();
        if radial_error > linear {
            return Ok(IntersectionSet::Empty);
        }
        let circle_parameter = circle.project(point)?.parameter;
        return Ok(IntersectionSet::Isolated(vec![CurveCurveHit {
            point,
            first_parameter: parameter,
            second_parameter: circle_parameter,
            kind: IntersectionKind::Transverse,
            multiplicity: 1,
            achieved_error: radial_error,
        }]));
    }
    if plane_distance.abs() > linear {
        return Ok(IntersectionSet::Empty);
    }
    let offset = line.origin() - circle.center();
    let direction = line.direction();
    let a = direction.magnitude2();
    let closest_parameter = -offset.dot(direction) / a;
    let closest = line.evaluate(closest_parameter)?.point;
    // The expanded quadratic loses precision when the line's origin is far
    // from a tangency. Measure the radial gap at the closest point instead.
    let radial_gap = circle.radius().powi(2) - (closest - circle.center()).magnitude2();
    let gap_tolerance = linear * linear * 0.25;
    if radial_gap < -gap_tolerance {
        return Ok(IntersectionSet::Empty);
    }
    let tangent = radial_gap.abs() <= gap_tolerance;
    let step = (radial_gap.max(0.0) / a).sqrt();
    let parameters = if tangent {
        vec![closest_parameter]
    } else {
        vec![closest_parameter - step, closest_parameter + step]
    };
    let mut hits = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        let point = line.evaluate(parameter)?.point;
        let circle_parameter = circle.project(point)?.parameter;
        let circle_point = circle.evaluate(circle_parameter)?.point;
        hits.push(CurveCurveHit {
            point,
            first_parameter: parameter,
            second_parameter: circle_parameter,
            kind: if tangent {
                IntersectionKind::Tangent
            } else {
                IntersectionKind::Transverse
            },
            multiplicity: if tangent { 2 } else { 1 },
            achieved_error: point.distance(circle_point),
        });
    }
    Ok(IntersectionSet::Isolated(hits))
}

fn circle_circle(
    first: Circle3,
    second: Circle3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveCurveHit>> {
    let first_plane = Plane3::new(
        first.center(),
        first.x_radius(),
        first.y_radius(),
        tolerance,
    )?;
    let second_plane = Plane3::new(
        second.center(),
        second.x_radius(),
        second.y_radius(),
        tolerance,
    )?;
    match plane_plane(first_plane, second_plane, tolerance)? {
        IntersectionSet::Empty => Ok(IntersectionSet::Empty),
        IntersectionSet::Isolated(curves) => {
            let Some(intersection) = curves.first() else {
                return Ok(IntersectionSet::Empty);
            };
            let Curve3::Line(line) = &intersection.curve else {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "plane intersection did not yield a line",
                ));
            };
            let candidates = line_circle(*line, first, tolerance)?;
            filter_circle_candidates(candidates, second, tolerance)
        }
        IntersectionSet::Coincident => coplanar_circle_circle(first, second, tolerance),
    }
}

fn coplanar_circle_circle(
    first: Circle3,
    second: Circle3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveCurveHit>> {
    let center_delta = second.center() - first.center();
    let distance = center_delta.magnitude();
    let first_radius = first.radius();
    let second_radius = second.radius();
    let linear = tolerance.linear_at_scale(first_radius.max(second_radius).max(1.0));
    if distance <= linear && (first_radius - second_radius).abs() <= linear {
        return Ok(IntersectionSet::Coincident);
    }
    if distance <= linear
        || distance > first_radius + second_radius + linear
        || distance < (first_radius - second_radius).abs() - linear
    {
        return Ok(IntersectionSet::Empty);
    }
    let along =
        (first_radius.powi(2) - second_radius.powi(2) + distance.powi(2)) / (2.0 * distance);
    let height_squared = first_radius.powi(2) - along.powi(2);
    let tangent = height_squared.abs() <= linear * linear;
    let base = first.center() + center_delta * (along / distance);
    let perpendicular = first.normal().cross(center_delta).normalize();
    let height = height_squared.max(0.0).sqrt();
    let points = if tangent {
        vec![base]
    } else {
        vec![base + perpendicular * height, base - perpendicular * height]
    };
    let mut hits = Vec::with_capacity(points.len());
    for point in points {
        let first_projection = first.project(point)?;
        let second_projection = second.project(point)?;
        hits.push(CurveCurveHit {
            point,
            first_parameter: first_projection.parameter,
            second_parameter: second_projection.parameter,
            kind: if tangent {
                IntersectionKind::Tangent
            } else {
                IntersectionKind::Transverse
            },
            multiplicity: if tangent { 2 } else { 1 },
            achieved_error: first_projection.distance.max(second_projection.distance),
        });
    }
    Ok(IntersectionSet::Isolated(hits))
}

fn filter_circle_candidates(
    candidates: IntersectionSet<CurveCurveHit>,
    circle: Circle3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveCurveHit>> {
    let IntersectionSet::Isolated(candidates) = candidates else {
        return Ok(IntersectionSet::Empty);
    };
    let linear = tolerance.linear_at_scale(circle.radius());
    let mut hits = Vec::new();
    for candidate in candidates {
        let projection = circle.project(candidate.point)?;
        if projection.distance <= linear {
            hits.push(CurveCurveHit {
                point: candidate.point,
                first_parameter: candidate.second_parameter,
                second_parameter: projection.parameter,
                kind: candidate.kind,
                multiplicity: candidate.multiplicity,
                achieved_error: candidate.achieved_error.max(projection.distance),
            });
        }
    }
    Ok(if hits.is_empty() {
        IntersectionSet::Empty
    } else {
        IntersectionSet::Isolated(hits)
    })
}

fn line_plane(
    line: Line3,
    plane: Plane3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveSurfaceHit>> {
    let normal = plane.normal();
    let denominator = normal.dot(line.direction());
    let signed_distance = normal.dot(line.origin() - plane.origin());
    let linear = tolerance.linear_at_scale(line.origin().to_vec().magnitude().max(1.0));
    if denominator.abs() <= tolerance.angular * line.direction().magnitude() {
        return Ok(if signed_distance.abs() <= linear {
            IntersectionSet::Coincident
        } else {
            IntersectionSet::Empty
        });
    }
    let parameter = -signed_distance / denominator;
    let point = line.evaluate(parameter)?.point;
    let projection = plane.project(point)?;
    Ok(IntersectionSet::Isolated(vec![CurveSurfaceHit {
        point,
        curve_parameter: parameter,
        surface_parameters: projection.parameters,
        kind: IntersectionKind::Transverse,
        multiplicity: 1,
        achieved_error: projection.distance,
    }]))
}

fn circle_plane(
    circle: Circle3,
    plane: Plane3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<CurveSurfaceHit>> {
    let circle_surface = Plane3::new(
        circle.center(),
        circle.x_radius(),
        circle.y_radius(),
        tolerance,
    )?;
    match plane_plane(circle_surface, plane, tolerance)? {
        IntersectionSet::Empty => Ok(IntersectionSet::Empty),
        IntersectionSet::Coincident => Ok(IntersectionSet::Coincident),
        IntersectionSet::Isolated(curves) => {
            let Some(intersection) = curves.first() else {
                return Ok(IntersectionSet::Empty);
            };
            let Curve3::Line(line) = &intersection.curve else {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "plane intersection did not yield a line",
                ));
            };
            let curve_hits = line_circle(*line, circle, tolerance)?;
            let IntersectionSet::Isolated(curve_hits) = curve_hits else {
                return Ok(IntersectionSet::Empty);
            };
            let mut hits = Vec::with_capacity(curve_hits.len());
            for hit in curve_hits {
                let surface = plane.project(hit.point)?;
                hits.push(CurveSurfaceHit {
                    point: hit.point,
                    curve_parameter: hit.second_parameter,
                    surface_parameters: surface.parameters,
                    kind: hit.kind,
                    multiplicity: hit.multiplicity,
                    achieved_error: hit.achieved_error.max(surface.distance),
                });
            }
            Ok(IntersectionSet::Isolated(hits))
        }
    }
}

fn plane_plane(
    first: Plane3,
    second: Plane3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let first_normal = first.normal();
    let second_normal = second.normal();
    let direction = first_normal.cross(second_normal);
    let scale = first
        .origin()
        .to_vec()
        .magnitude()
        .max(second.origin().to_vec().magnitude())
        .max(1.0);
    let linear = tolerance.linear_at_scale(scale);
    if direction.magnitude() <= tolerance.angular {
        let distance = (second.origin() - first.origin()).dot(first_normal).abs();
        return Ok(if distance <= linear {
            IntersectionSet::Coincident
        } else {
            IntersectionSet::Empty
        });
    }
    let d1 = first_normal.dot(first.origin().to_vec());
    let d2 = second_normal.dot(second.origin().to_vec());
    let denominator = direction.magnitude2();
    let point = Point3::from_vec(
        (second_normal.cross(direction) * d1 + direction.cross(first_normal) * d2) / denominator,
    );
    let line = Line3::new(point, direction)?;
    let first_origin_uv = first.project(point)?.parameters;
    let first_next_uv = first.project(point + direction)?.parameters;
    let second_origin_uv = second.project(point)?.parameters;
    let second_next_uv = second.project(point + direction)?.parameters;
    Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
        curve: line.into(),
        first_pcurve: Line2::new(
            Point2::new(first_origin_uv.0, first_origin_uv.1),
            Point2::new(first_next_uv.0, first_next_uv.1)
                - Point2::new(first_origin_uv.0, first_origin_uv.1),
        )?
        .into(),
        second_pcurve: Line2::new(
            Point2::new(second_origin_uv.0, second_origin_uv.1),
            Point2::new(second_next_uv.0, second_next_uv.1)
                - Point2::new(second_origin_uv.0, second_origin_uv.1),
        )?
        .into(),
        achieved_error: first
            .project(point)?
            .distance
            .max(second.project(point)?.distance),
    }]))
}

fn swap_curve_hits(set: IntersectionSet<CurveCurveHit>) -> IntersectionSet<CurveCurveHit> {
    match set {
        IntersectionSet::Isolated(hits) => IntersectionSet::Isolated(
            hits.into_iter()
                .map(|hit| CurveCurveHit {
                    first_parameter: hit.second_parameter,
                    second_parameter: hit.first_parameter,
                    ..hit
                })
                .collect(),
        ),
        IntersectionSet::Empty => IntersectionSet::Empty,
        IntersectionSet::Coincident => IntersectionSet::Coincident,
    }
}

fn unsupported(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::UnsupportedGeometry, message)
}
