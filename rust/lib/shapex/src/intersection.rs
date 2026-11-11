use cgmath::prelude::*;

use crate::{Point2, Point3, Vec3, Vec4};

use super::bounds::Bounds;
use super::geometry::de_casteljau_split;
use super::linalg::solve_linear;
use super::{
    unsupported, Circle3, Curve2, Curve3, DiagnosticKind, DomainBounds, KernelError, KernelResult, Line2, Line3,
    ModelTolerance, NurbsCurve2, Plane3, Surface3, SurfaceIntersectionCurve,
    SurfaceProjectionPcurve2,
};
use crate::geometry_error;

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
            let Some(delta) = solve_linear(matrix, [-residual.x, -residual.y, -residual.z])
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
                return Err(geometry_error("local surface intersection is tangential or singular"));
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
            solve_linear(normal, [-residual.x, -residual.y, -residual.z]).ok_or_else(|| {
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
    let axial_direction = slope * axial_direction;
    let a = radial_direction.magnitude2() - axial_direction.powi(2);
    // With a flat cone the two sheets are close together. Center the
    // quadratic at the line's crossing of the apex plane when the axial
    // term dominates, so the discriminant resolves both roots.
    let (radial_origin, axial_origin, offset) =
        if a < 0.0 {
            let offset = -(slope * axial_origin) / axial_direction;
            (radial_origin + radial_direction * offset, 0.0, offset)
        } else {
            (radial_origin, slope * axial_origin, 0.0)
        };
    let mut roots = solve_quadratic(
        a,
        2.0 * (radial_origin.dot(radial_direction) - axial_origin * axial_direction),
        radial_origin.magnitude2() - axial_origin.powi(2),
        tolerance.linear_at_scale(cone.reference_radius()),
    )?;
    if let IntersectionSet::Isolated(roots) = &mut roots {
        for (parameter, _) in roots {
            *parameter += offset;
        }
    }
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
        if let (Self::Nurbs(first), Self::Nurbs(second)) = (self, other) {
            if first.coincident_support(second, tolerance)? {
                return Ok(IntersectionSet::Coincident);
            }
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
            (Self::Plane(plane), Self::Sphere(sphere)) => plane_sphere(*plane, *sphere, tolerance),
            (Self::Sphere(sphere), Self::Plane(plane)) => {
                Ok(swap_surface_curves(plane_sphere(*plane, *sphere, tolerance)?))
            }
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

    pub(crate) fn intersect_section_plane(&self, plane: Plane3,
        domains: [super::ParameterRange; 2], plane_domains: [super::ParameterRange; 2],
        tolerance: ModelTolerance) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
        match self {
            Self::Cylinder(cylinder) => Ok(swap_surface_curves(plane_cylinder_impl(plane, *cylinder, tolerance, true)?)),
            Self::Revolved(revolved) => match regular_revolution_section(revolved, plane, tolerance)? {
                Some(curves) => Ok(IntersectionSet::Isolated(curves)),
                None => self.intersect_bounded(&Self::Plane(plane), domains, plane_domains, tolerance, true),
            },
            Self::Torus(torus) => match section_plane_torus(plane, *torus, domains, tolerance)? {
                Some(result) => Ok(swap_surface_curves(result)),
                None => self.intersect_bounded(&Self::Plane(plane), domains, plane_domains, tolerance, true),
            },
            _ => self.intersect_bounded(&Self::Plane(plane), domains, plane_domains, tolerance, true),
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
        let interpolate_guides = matches!(self, Self::Swept(_)) || matches!(other, Self::Swept(_));
        self.intersect_bounded(other, first_domains, second_domains, tolerance, interpolate_guides)
    }

    fn intersect_bounded(&self, other: &Self, first_domains: [super::ParameterRange; 2],
        second_domains: [super::ParameterRange; 2], tolerance: ModelTolerance,
        interpolate_guides: bool) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
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
                    interpolate_guides,
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
        // A planar contact rail retains its analytic curve under a normal
        // offset: every point moves by the same plane-normal translation.
        let mut curve = support.boundary_curve(guide.clone(), range, tolerance)?;
        if surface != base {
            let parameter = range.middle();
            let translation = surface.evaluate(parameter, side)?.point
                - base.evaluate(parameter, side)?.point;
            curve = super::transform::transform_curve(
                curve, super::RigidTransform3::from_translation(translation), tolerance,
            )?;
        }
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
    if plane.normal().cross(cone.axis().normalize()).magnitude() > tolerance.angular {
        let axial = normal.dot(cone.axis());
        let x = normal.dot(cone.x_radius());
        let y = normal.dot(cone.y_radius());
        let radial = x.hypot(y);
        if axial.abs() - radial <= tolerance.linear_at_scale(scale) {
            return Err(unsupported("open plane/cone intersection requires bounded tracing"));
        }
        let height = -apex_distance / axial;
        if height <= tolerance.parameter {
            return Err(unsupported("plane/cone section on the opposite sheet requires bounded tracing"));
        }
        // Project the rational unit circle from the cone apex onto the plane.
        // Align its first radius with the plane normal's radial component so
        // every control weight stays positive throughout the elliptic case.
        let (sin, cos) = y.atan2(x).sin_cos();
        let first = cone.x_radius() * cos + cone.y_radius() * sin;
        let second = -cone.x_radius() * sin + cone.y_radius() * cos;
        let mut controls = Vec::new();
        let mut weights = Vec::new();
        let quarter = std::f64::consts::FRAC_1_SQRT_2;
        for (u, v, weight) in [
            (1.0, 0.0, 1.0), (1.0, 1.0, quarter), (0.0, 1.0, 1.0),
            (-1.0, 1.0, quarter), (-1.0, 0.0, 1.0), (-1.0, -1.0, quarter),
            (0.0, -1.0, 1.0), (1.0, -1.0, quarter), (1.0, 0.0, 1.0),
        ] {
            let factor = 1.0 + radial / axial * u;
            controls.push(cone.apex() + (cone.axis() + first * u + second * v) * (height / factor));
            weights.push(weight * factor);
        }
        let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0];
        let curve: Curve3 = super::NurbsCurve3::new(2, controls, weights, knots)?.into();
        let range = super::ParameterRange::new(0.0, 4.0)?;
        let surface: Surface3 = cone.into();
        let mut points = Vec::new();
        let mut achieved_error = 0.0_f64;
        for index in 0..=16 {
            let point = curve.first_order(index as f64 / 4.0)?.0;
            let projection = surface.project(point, tolerance)?;
            achieved_error = achieved_error.max(projection.distance);
            points.push(Point2::new(projection.parameters.0, projection.parameters.1));
        }
        unwrap_periodic_points(&mut points, surface.domains());
        let guide = smooth_nurbs_guide(&points, range)?.into();
        let first_pcurve = plane.parameterized_curve_image(&curve, range, tolerance)?;
        let second_pcurve = SurfaceProjectionPcurve2::new(
            curve.clone(), surface, guide, range, tolerance,
        )?.into();
        return Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
            curve, first_pcurve, second_pcurve, achieved_error,
        }]));
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
    if axis.cross(cone.axis().normalize()).magnitude() > tolerance.angular {
        return Err(unsupported(
            "nonparallel cylinder/cone intersection requires bounded tracing",
        ));
    }
    let eccentricity = (cone.apex() - cylinder.origin()).cross(axis).magnitude();
    if eccentricity > tolerance.linear {
        // Each cylinder generator meets the positive cone sheet at the
        // axial height set by its radial distance from the cone axis.
        if (eccentricity - cylinder.radius()).abs() <= tolerance.linear {
            return Err(unsupported("cylinder/cone intersection through the cone apex requires bounded tracing"));
        }
        let mut cylinder_points = Vec::new();
        let mut cone_points = Vec::new();
        let mut achieved_error = 0.0_f64;
        let cone_surface: Surface3 = cone.into();
        for index in 0..=64 {
            let u = std::f64::consts::TAU * index as f64 / 64.0;
            let base = cylinder.evaluate(u, 0.0)?.point;
            let delta = base - cone.apex();
            let radial = delta - axis * delta.dot(axis);
            let cone_v = radial.magnitude() / cone.reference_radius();
            let cone_u = radial.dot(cone.y_radius()).atan2(radial.dot(cone.x_radius()));
            let cylinder_v = (cone.axis() * cone_v - delta).dot(axis) / cylinder.axis().magnitude();
            let point = cylinder.evaluate(u, cylinder_v)?.point;
            achieved_error = achieved_error.max(point.distance(cone_surface.first_order(cone_u, cone_v)?.0));
            cylinder_points.push(Point2::new(u, cylinder_v));
            cone_points.push(Point2::new(cone_u, cone_v));
        }
        unwrap_periodic_points(&mut cone_points, cone_surface.domains());
        let curve = SurfaceIntersectionCurve::new(
            cylinder.into(), cone_surface,
            closed_interpolating_guide(&cylinder_points)?.into(),
            closed_interpolating_guide(&cone_points)?.into(),
            super::ParameterRange::new(0.0, 1.0)?, tolerance, achieved_error,
        )?;
        let [first_pcurve, second_pcurve] = curve.exact_pcurves();
        return Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
            curve: curve.into(), first_pcurve, second_pcurve, achieved_error,
        }]));
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
    // Within tolerance of tangency the two sections bound no resolvable strip.
    let minor_parameters = if torus.minor_radius() * (1.0 - radial_cosine.abs()) <= linear {
        vec![if radial_cosine > 0.0 { 0.0 } else { std::f64::consts::PI }]
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

fn plane_sphere(
    plane: Plane3,
    sphere: super::Sphere3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let normal = plane.normal();
    let height = (plane.origin() - sphere.center()).dot(normal);
    let radius2 = (sphere.radius() - height.abs()) * (sphere.radius() + height.abs());
    // A tangent point has no separating curve or cap area.
    if radius2 <= tolerance.linear.powi(2) { return Ok(IntersectionSet::Empty); }
    let axis = sphere.z_radius().normalize();
    let mut curves = Vec::new();
    let pole_error = f64::EPSILON * sphere.center().to_vec().magnitude().max(sphere.radius()) * 128.0;
    let pole_heights = [-1.0, 1.0].map(|sign| (sign * sphere.z_radius().dot(normal) - height).abs());
    if pole_heights.iter().all(|distance| *distance <= pole_error) {
        let radial = axis.cross(normal).normalize();
        for radial in [radial, -radial] {
            let curve: Curve3 = Circle3::new(sphere.center(), -sphere.z_radius(),
                radial * sphere.radius(), tolerance)?.into();
            let range = super::ParameterRange::new(0.0, std::f64::consts::PI)?;
            let angle = radial.dot(sphere.y_radius()).atan2(radial.dot(sphere.x_radius()));
            let first_pcurve = plane.parameterized_curve_image(&curve, range, tolerance)?;
            // Retain an analytic circle on the plane, with a bounded linear
            // p-curve on the sphere to keep just this pole-to-pole arc.
            let second_pcurve = interpolating_nurbs_guide(&[
                Point2::new(angle, -std::f64::consts::FRAC_PI_2),
                Point2::new(angle, std::f64::consts::FRAC_PI_2),
            ], &[range.start, range.end])?.into();
            curves.push(SurfaceSurfaceCurve { first_pcurve, second_pcurve, curve, achieved_error: height.abs() });
        }
    } else {
        let center = sphere.center() + normal * height;
        let radius = radius2.sqrt();
        // If the circle passes through a pole, put the chart discontinuity at
        // the seam and take the longitude at either end from the one-sided tangent.
        let pole = pole_heights.iter().position(|distance| *distance <= pole_error);
        let radial = pole.map_or_else(|| plane.u().normalize(), |index| {
            let delta = sphere.z_radius() * if index == 0 { -1.0 } else { 1.0 };
            (delta - normal * delta.dot(normal)).normalize()
        });
        let circle: Curve3 = Circle3::new(center, radial * radius,
            normal.cross(radial).normalize() * radius, tolerance)?.into();
        let range = super::ParameterRange::new(0.0, std::f64::consts::TAU)?;
        let mut points = (0..=64).map(|index| {
            let sample = circle.evaluate(range.end * index as f64 / 64.0)?;
            if let Some(pole) = pole.filter(|_| index == 0 || index == 64) {
                let tangent = sample.first_derivative * if index == 0 { 1.0 } else { -1.0 };
                return Ok(Point2::new(tangent.dot(sphere.y_radius()).atan2(tangent.dot(sphere.x_radius())),
                    if pole == 0 { -std::f64::consts::FRAC_PI_2 } else { std::f64::consts::FRAC_PI_2 }));
            }
            let projection = Surface3::Sphere(sphere).project(sample.point, tolerance)?;
            Ok(Point2::new(projection.parameters.0, projection.parameters.1))
        }).collect::<KernelResult<Vec<_>>>()?;
        unwrap_periodic_points(&mut points, Surface3::Sphere(sphere).domains());
        let guide = smooth_nurbs_guide(&points, range)?.into();
        let pcurve = SurfaceProjectionPcurve2::new(circle.clone(), sphere.into(), guide, range, tolerance)?.into();
        curves.push(SurfaceSurfaceCurve { first_pcurve: plane.parameterized_curve_image(&circle, range, tolerance)?,
            second_pcurve: pcurve, curve: circle, achieved_error: 0.0 });
    }
    Ok(IntersectionSet::Isolated(curves))
}

/// When a plane crosses every parallel of a spline revolution transversely,
/// the two longitude solutions run continuously over the full meridian.
/// The meridian control hull proves that neither solution can turn or vanish.
fn regular_revolution_section(
    revolved: &super::RevolvedSurface3,
    plane: Plane3,
    tolerance: ModelTolerance,
) -> KernelResult<Option<Vec<SurfaceSurfaceCurve>>> {
    let Curve2::Nurbs(profile) = revolved.profile() else { return Ok(None); };
    let frame = revolved.frame();
    let normal = plane.normal();
    let x = normal.dot(frame.x());
    let y = normal.dot(frame.y());
    let axial = normal.dot(frame.z());
    let radial = x.hypot(y);
    let phase = y.atan2(x);
    let offset = normal.dot(frame.origin() - plane.origin());
    if profile.control_points().iter().any(|p| {
        (offset + axial * p.y).abs() + tolerance.linear >= radial * p.x
    }) {
        return Ok(None);
    }
    let range = revolved.profile_range();
    let range = super::ParameterRange::new(range.start.min(range.end), range.start.max(range.end))?;
    let mut spans = vec![range.start];
    spans.extend(revolved.profile().integration_breakpoints(range));
    spans.push(range.end);
    spans.sort_by(f64::total_cmp);
    spans.dedup();
    let parameters = spans.windows(2).flat_map(|span| {
        (0..32).map(move |index| span[0] + (span[1] - span[0]) * index as f64 / 32.0)
    }).chain([range.end]).collect::<Vec<_>>();
    let surface: Surface3 = revolved.clone().into();
    let curves = [-1.0, 1.0].into_iter().map(|sign| {
        let mut first = Vec::new();
        let mut second = Vec::new();
        let mut error = 0.0_f64;
        for &v in &parameters {
            let p = revolved.profile().first_order(v)?.0;
            let u = phase + sign * (-(offset + axial * p.y) / (radial * p.x)).acos();
            let projection = plane.project(surface.first_order(u, v)?.0)?;
            first.push(Point2::new(u, v));
            second.push(Point2::new(projection.parameters.0, projection.parameters.1));
            error = error.max(projection.distance);
        }
        let guide = |points: &[Point2]| {
            smooth_interpolating_guide(&interpolating_nurbs_guide(points, &parameters)?)
        };
        let curve = SurfaceIntersectionCurve::new(
            surface.clone(), plane.into(), guide(&first)?, guide(&second)?, range, tolerance, error,
        )?;
        let [first_pcurve, second_pcurve] = curve.exact_pcurves();
        Ok(SurfaceSurfaceCurve { curve: curve.into(), first_pcurve, second_pcurve, achieved_error: error })
    }).collect::<KernelResult<Vec<_>>>()?;
    Ok(Some(curves))
}

/// Solve the torus plane equation analytically for its minor angle. The
/// discriminant determines every branch interval, including arbitrarily thin
/// sections that a grid of numerical tracing seeds can miss.
fn section_plane_torus(plane: Plane3, torus: super::Torus3, domains: [super::ParameterRange; 2], tolerance: ModelTolerance)
    -> KernelResult<Option<IntersectionSet<SurfaceSurfaceCurve>>> {
    match plane_torus(plane, torus, tolerance) {
        Ok(result) => return Ok(Some(result)),
        Err(error) if error.diagnostics().iter().all(|d| d.kind == DiagnosticKind::UnsupportedGeometry) => {},
        Err(error) => return Err(error),
    }
    let n = plane.normal();
    let a = n.dot(torus.x_major().normalize());
    let b = n.dot(torus.y_major().normalize());
    let c = n.dot(torus.normal());
    let h = a.hypot(b);
    let d = n.dot(torus.center() - plane.origin());
    let major = torus.major_radius();
    let minor = torus.minor_radius();
    let scale = plane.origin().to_vec().magnitude().max(torus.center().to_vec().magnitude()).max(major);
    let roundoff = f64::EPSILON * scale * 128.0;
    // At a saddle the two branches meet at an extremum of the projected major circle.
    // The bounded tracer separates the incident arcs at that junction.
    if (d.abs() - (major * h - minor).abs()).abs() <= roundoff {
        return Ok(None);
    }
    let quadratic = (major - minor) * (major + minor);
    let reach = minor * (d * d + quadratic * c * c).sqrt();
    let large = (-major * d - reach.copysign(major * d)) / quadratic;
    let small = (d - minor * c) * (d + minor * c) / (quadratic * large);
    let low = large.min(small);
    let high = large.max(small);
    if low > h || high < -h { return Ok(Some(IntersectionSet::Empty)); }
    let phase = b.atan2(a);
    let tau = std::f64::consts::TAU;
    let mut cuts = vec![0.0, tau];
    for q in [low, high] {
        if q.abs() < h {
            let angle = (q / h).acos();
            cuts.extend([(phase - angle).rem_euclid(tau), (phase + angle).rem_euclid(tau)]);
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
    let mut intervals = cuts.windows(2).filter_map(|pair| {
        let u = (pair[0] + pair[1]) * 0.5;
        let q = a * u.cos() + b * u.sin();
        (q >= low && q <= high).then_some((pair[0], pair[1]))
    }).collect::<Vec<_>>();
    if intervals.len() > 1 && intervals[0].0 == 0.0 && intervals.last().unwrap().1 == tau {
        let first = intervals.remove(0);
        intervals.last_mut().unwrap().1 = first.1 + tau;
    }
    let mut curves = Vec::new();
    for (start, end) in intervals {
        let full = end - start >= tau - tolerance.parameter;
        let branches: &[f64] = if full { &[-1.0, 1.0] } else { &[1.0] };
        for sign in branches {
            const COUNT: usize = 64;
            let mut angles = (0..=COUNT).map(|index| tau * index as f64 / COUNT as f64).collect::<Vec<_>>();
            for v in [domains[1].start, domains[1].end] {
                let q = -(minor * c * v.sin() + d) / (major + minor * v.cos());
                if q.abs() > h { continue; }
                // A trim crossing at a branch turn is already sampled there.
                // Inverting a rounded cosine near the turn can insert two
                // nearly coincident samples with an undefined guide tangent.
                if !full && [low, high].iter().any(|root| {
                    (q - root).abs() <= f64::EPSILON * q.abs().max(root.abs()) * 32.0
                }) {
                    continue;
                }
                let angle = (q / h).acos();
                for u in [phase - angle, phase + angle] {
                    let u = (u - start).rem_euclid(tau) + start;
                    if u > end { continue; }
                    if full { angles.push(u - start); }
                    else {
                        let angle = (((start + end) * 0.5 - u) / ((end - start) * 0.5)).clamp(-1.0, 1.0).acos();
                        angles.extend([angle, tau - angle]);
                    }
                }
            }
            angles.sort_by(f64::total_cmp);
            angles.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
            let mut torus_points = Vec::new();
            let mut plane_points = Vec::new();
            let mut achieved_error = 0.0_f64;
            for angle in angles {
                let u = if full { start + angle } else {
                    (start + end) * 0.5 - (end - start) * 0.5 * angle.cos()
                };
                let q = a * u.cos() + b * u.sin();
                let discriminant = if !full && [0.0, std::f64::consts::PI, tau].contains(&angle) { 0.0 }
                    else { (quadratic * (q - low) * (high - q)).max(0.0) };
                let branch = if full { *sign } else if angle <= std::f64::consts::PI { 1.0 } else { -1.0 };
                let v = c.atan2(q) + branch * discriminant.sqrt().atan2(-(major * q + d));
                let point = Surface3::Torus(torus).evaluate(u, v)?.point;
                let projected = plane.project(point)?;
                achieved_error = achieved_error.max(projected.distance);
                torus_points.push(Point2::new(u, v));
                plane_points.push(Point2::new(projected.parameters.0, projected.parameters.1));
            }
            unwrap_periodic_points(&mut torus_points, Surface3::Torus(torus).domains());
            let range = super::ParameterRange::new(0.0, 1.0)?;
            let implicit = SurfaceIntersectionCurve::new(plane.into(), torus.into(),
                closed_interpolating_guide(&plane_points)?.into(),
                closed_interpolating_guide(&torus_points)?.into(), range, tolerance, achieved_error)?;
            let [first_pcurve, second_pcurve] = implicit.exact_pcurves();
            curves.push(SurfaceSurfaceCurve { curve: implicit.into(), first_pcurve, second_pcurve, achieved_error });
        }
    }
    Ok(Some(if curves.is_empty() { IntersectionSet::Empty } else { IntersectionSet::Isolated(curves) }))
}

/// Periodic Hermite guide with the same tangent at both seam representatives.
fn closed_interpolating_guide(points: &[Point2]) -> KernelResult<NurbsCurve2> {
    let count = points.len() - 1;
    let winding = points[count] - points[0];
    let slope = |index: usize| (points[index + 1] - points[index]) * count as f64;
    let tangent = |index: usize| {
        let before = if index == 0 || index == count { (points[0] - (points[count - 1] - winding)) * count as f64 }
            else { slope(index - 1) };
        let after = if index == count { slope(0) } else { slope(index) };
        crate::Vec2::new(
            if before.x * after.x > 0.0 { 2.0 * before.x * after.x / (before.x + after.x) } else { 0.0 },
            if before.y * after.y > 0.0 { 2.0 * before.y * after.y / (before.y + after.y) } else { 0.0 })
    };
    let mut controls = vec![points[0]];
    let mut knots = vec![0.0; 4];
    for index in 0..count {
        controls.extend([points[index] + tangent(index) / (3.0 * count as f64),
            points[index + 1] - tangent(index + 1) / (3.0 * count as f64), points[index + 1]]);
        if index + 1 < count { knots.extend([(index + 1) as f64 / count as f64; 3]); }
    }
    knots.extend([1.0; 4]);
    let weights = vec![1.0; controls.len()];
    NurbsCurve2::new(3, controls, weights, knots)
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
    if plane.normal().cross(torus_normal).magnitude() > tolerance.angular {
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
    // Within tolerance of the tangent height the two sections lie apart by
    // the root of that distance, yet bound no resolvable strip between them.
    let minor_parameters = if torus.minor_radius() - height.abs() <= linear {
        vec![std::f64::consts::FRAC_PI_2.copysign(height)]
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
    plane_cylinder_impl(plane, cylinder, tolerance, false)
}

fn plane_cylinder_impl(plane: Plane3, cylinder: super::Cylinder3,
    tolerance: ModelTolerance, rational_section: bool) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
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
    if plane.normal().cross(axis).magnitude() > tolerance.angular {
        let denominator = plane.normal().dot(cylinder.axis());
        if !rational_section {
            let range = super::ParameterRange::new(0.0, std::f64::consts::TAU)?;
            let mut plane_points = Vec::new();
            let mut cylinder_points = Vec::new();
            let mut achieved_error = 0.0_f64;
            for index in 0..=32 {
                let u = std::f64::consts::TAU * index as f64 / 32.0;
                let radial = cylinder.x_radius() * u.cos() + cylinder.y_radius() * u.sin();
                let v = -plane.normal().dot(cylinder.origin() + radial - plane.origin()) / denominator;
                let point = cylinder.evaluate(u, v)?.point;
                let projected = plane.project(point)?;
                achieved_error = achieved_error.max(projected.distance);
                plane_points.push(Point2::new(projected.parameters.0, projected.parameters.1));
                cylinder_points.push(Point2::new(u, v));
            }
            let implicit = SurfaceIntersectionCurve::new(plane.into(), cylinder.into(),
                smooth_nurbs_guide(&plane_points, range)?.into(),
                smooth_nurbs_guide(&cylinder_points, range)?.into(), range, tolerance, achieved_error)?;
            let [first_pcurve, second_pcurve] = implicit.exact_pcurves();
            return Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
                curve: implicit.into(), first_pcurve, second_pcurve, achieved_error,
            }]));
        }
        let center = cylinder.origin() + cylinder.axis()
            * plane.normal().dot(plane.origin() - cylinder.origin()) / denominator;
        let project = |radial: Vec3| radial - cylinder.axis() * plane.normal().dot(radial) / denominator;
        let x = project(cylinder.x_radius());
        let y = project(cylinder.y_radius());
        // An affine image of the exact rational circle is the section ellipse.
        // This avoids ill-conditioned four-parameter correction near a cap.
        let controls = [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0),
            (-1.0, 0.0), (-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (1.0, 0.0)]
            .map(|(a, b)| center + x * a + y * b).to_vec();
        let weight = std::f64::consts::FRAC_1_SQRT_2;
        let curve: Curve3 = super::NurbsCurve3::new(2, controls,
            vec![1.0, weight, 1.0, weight, 1.0, weight, 1.0, weight, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0])?.into();
        let range = super::ParameterRange::new(0.0, 4.0)?;
        let mut cylinder_points = Vec::new();
        let mut achieved_error = 0.0_f64;
        for index in 0..=32 {
            let point = curve.evaluate(range.end * index as f64 / 32.0)?.point;
            let projected = Surface3::Cylinder(cylinder).project(point, tolerance)?;
            achieved_error = achieved_error.max(projected.distance);
            cylinder_points.push(Point2::new(projected.parameters.0, projected.parameters.1));
        }
        unwrap_periodic_points(&mut cylinder_points, Surface3::Cylinder(cylinder).domains());
        let cylinder_guide: Curve2 = smooth_nurbs_guide(&cylinder_points, range)?.into();
        let second_pcurve = SurfaceProjectionPcurve2::new(curve.clone(), cylinder.into(),
            cylinder_guide, range, tolerance)?.into();
        let first_pcurve = plane.parameterized_curve_image(&curve, range, tolerance)?;
        return Ok(IntersectionSet::Isolated(vec![SurfaceSurfaceCurve {
            curve,
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
    trace_surface_intersections_in_domains(first, second, first_domains, second_domains, tolerance, false)
}

fn trace_surface_intersections_in_domains(
    first: &Surface3,
    second: &Surface3,
    first_domains: [super::ParameterRange; 2],
    second_domains: [super::ParameterRange; 2],
    tolerance: ModelTolerance,
    interpolate_guides: bool,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let poles = collapsed_plane_join_poles(first, second, first_domains, second_domains, tolerance)?;
    let parameter_scale = first_domains
        .iter()
        .chain(&second_domains)
        .map(|range| range.span().abs())
        .fold(1.0_f64, f64::max);
    let mut seeds = Vec::<[f64; 4]>::new();
    // Corrected seed points, kept to reject duplicates without re-evaluating.
    let mut seed_points = Vec::<Point3>::new();
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
            let (other_point, eu, ev) = second.first_order(parameters[2], parameters[3])?;
            // Both interior and boundary-constrained seeds must be regular.
            // A collapsed chart direction can otherwise manufacture tiny
            // intersection loops around an isolated contact point.
            let regular_chart = |u: Vec3, v: Vec3, ranges: [super::ParameterRange; 2]| {
                let u = u * ranges[0].span().abs();
                let v = v * ranges[1].span().abs();
                u.cross(v).magnitude() > tolerance.linear_at_scale(point.to_vec().magnitude()) * u.magnitude().max(v.magnitude())
            };
            if !regular_chart(du, dv, first_domains) || !regular_chart(eu, ev, second_domains) { continue; }
            let first_normal = du.cross(dv).normalize();
            let second_normal = eu.cross(ev).normalize();
            let angle = first_normal.cross(second_normal).magnitude();
            if angle <= tolerance.angular * 8.0 { continue; }
            if angle <= (f64::EPSILON * 2048.0).sqrt() * 8.0 {
                // A small angle alone also describes a real shallow crossing.
                // In the transverse tangent direction the local separation is
                // g + slope*t + curvature*t²/2. Only an unresolved double root
                // of this equation is a numerical seed beside a contact.
                let direction = (second_normal - first_normal * first_normal.dot(second_normal)).normalize();
                let along = |value: super::SurfaceEvaluation| {
                    let a = value.du.magnitude2();
                    let b = value.du.dot(value.dv);
                    let c = value.dv.magnitude2();
                    let x = value.du.dot(direction);
                    let y = value.dv.dot(direction);
                    let u = (c * x - b * y) / (a * c - b * b);
                    let v = (a * y - b * x) / (a * c - b * b);
                    (value.du * u + value.dv * v,
                        value.duu * (u * u) + value.duv * (2.0 * u * v) + value.dvv * (v * v))
                };
                let (first_velocity, first_acceleration) = along(first.evaluate(parameters[0], parameters[1])?);
                let (second_velocity, second_acceleration) = along(second.evaluate(parameters[2], parameters[3])?);
                let slope = first_normal.dot(first_velocity - second_velocity);
                let curvature = first_normal.dot(first_acceleration - second_acceleration);
                let discriminant = slope * slope - 2.0 * curvature * first_normal.dot(point - other_point);
                let roundoff = f64::EPSILON * point.to_vec().magnitude().max(other_point.to_vec().magnitude()).max(1.0) * 2048.0;
                if curvature != 0.0 && discriminant.abs() <= roundoff * curvature.abs() { continue; }
            }
            let duplicate = tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0)) * 16.0;
            if seed_points.iter().any(|existing| existing.distance(point) <= duplicate) {
                continue;
            }
            seeds.push(parameters);
            seed_points.push(point);
        }
        Ok(())
    };

    fn nurbs_backed(surface: &Surface3) -> bool {
        match surface {
            Surface3::Nurbs(_) => true,
            Surface3::Offset(offset) => nurbs_backed(offset.base()),
            _ => false,
        }
    }
    if nurbs_backed(first) && nurbs_backed(second)
    {
        let mut stack = Vec::new();
        let second_cells = rational_surface_cells(second, second_domains)?;
        for first_cell in rational_surface_cells(first, first_domains)? {
            for second_cell in &second_cells {
                let transverse = first_cell.ruled_direction().zip(second_cell.ruled_direction())
                    .map(|(first, second)| first.cross(second))
                    .filter(|normal| normal.magnitude() > tolerance.angular)
                    .map(|normal| normal.normalize());
                stack.push((first_cell.clone(), second_cell.clone(), transverse, 0_u8));
            }
        }
        let mut visited = 0_usize;
        while let Some((first_cell, second_cell, transverse, depth)) = stack.pop() {
            visited += 1;
            if visited > 500_000 {
                return Err(KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "certified NURBS surface intersection exceeded its subdivision budget",
                ));
            }
            let first_bounds = first_cell.bounds()?;
            let second_bounds = second_cell.bounds()?;
            if Bounds(first_bounds[0], first_bounds[1]).separated(Bounds(second_bounds[0], second_bounds[1]), tolerance.linear) {
                continue;
            }
            if depth == 0 && (first_cell.only_touches_extrusion(&second_cell, 0.0)
                || second_cell.only_touches_extrusion(&first_cell, 0.0)
                || first_cell.touches_extrusion_inside(first, &second_cell, tolerance)?
                || second_cell.touches_extrusion_inside(second, &first_cell, tolerance)?) {
                continue;
            }
            // Boxes cannot separate tilted, nearly parallel cells whose gap
            // grows only quadratically away from a tangential contact. Their
            // control hulls also bound them along either cell's normal.
            let separates = |normal| {
                let [first_low, first_high] = first_cell.extent_along(normal);
                let [second_low, second_high] = second_cell.extent_along(normal);
                first_high + tolerance.linear < second_low || second_high + tolerance.linear < first_low
            };
            if [transverse, first_cell.normal(first)?, second_cell.normal(second)?]
                .into_iter().flatten().any(separates)
            {
                continue;
            }
            // The normal tests can still leave long, oblique hulls whose
            // edges pass each other. Test cross-edge axes before subdividing.
            if first_cell.padding == 0.0 && second_cell.padding == 0.0 {
                let first_directions = first_cell.corner_directions();
                let second_directions = second_cell.corner_directions();
                let angular_squared = tolerance.angular.powi(2);
                if first_directions.into_iter().any(|a| {
                    second_directions.into_iter().any(|b| {
                        let normal = a.cross(b);
                        normal.magnitude2() > angular_squared * a.magnitude2() * b.magnitude2()
                            && separates(normal.normalize())
                    })
                }) {
                    continue;
                }
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
                stack.push((right, second_cell.clone(), transverse, depth + 1));
                stack.push((left, second_cell, transverse, depth + 1));
            } else {
                let (left, right) = second_cell.split_direction(second_flat);
                stack.push((first_cell.clone(), right, transverse, depth + 1));
                stack.push((first_cell, left, transverse, depth + 1));
            }
        }
    } else {
        // Project the helical chart samples for nearby seeds on small branches.
        // A long screw needs a comparable density on each turn.
        for (surface, other, ranges, other_ranges, reverse) in [
            (first, second, first_domains, second_domains, false),
            (second, first, second_domains, first_domains, true),
        ] {
            let Surface3::Swept(sweep) = surface else { continue; };
            let Some(rate) = sweep.helix_rate() else { continue; };
            let rail_divisions =
                ((rate.abs() * ranges[1].span().abs() / std::f64::consts::FRAC_PI_4).ceil() as usize).max(8);
            let domains = other.domains();
            for i in 0..=8 {
                let u = ranges[0].start + ranges[0].span() * i as f64 / 8.0;
                for j in 0..=rail_divisions {
                    let v = ranges[1].start + ranges[1].span() * j as f64 / rail_divisions as f64;
                    let point = surface.first_order(u, v)?.0;
                    // A sample can have no unique foot, such as on a torus axis.
                    let Ok(projection) = other.project(point, tolerance) else { continue; };
                    let (a, b) = projection.parameters;
                    let a = domains[0].nearest(a, other_ranges[0].middle());
                    let b = domains[1].nearest(b, other_ranges[1].middle());
                    try_seed(if reverse { [a, b, u, v] } else { [u, v, a, b] })?;
                }
            }
        }
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
        let mut covered = false;
        let neighborhood = base_step * 1.5;
        for curve in &traced {
            if curve.iter().any(|parameters| parameter_distance(*parameters, seed) <= neighborhood)
                && trace_reaches_seed(first, second, curve, seed, neighborhood, first_domains, second_domains, tolerance)? {
                covered = true;
                break;
            }
        }
        if covered {
            continue;
        }
        let (mut forward, mut closed) = trace_surface_intersection_direction(
            first,
            second,
            seed,
            1.0,
            base_step,
            first_domains,
            second_domains,
            tolerance,
            interpolate_guides,
        )?;
        let mut parameters = if closed {
            Vec::new()
        } else {
            let (backward, backward_closed) = trace_surface_intersection_direction(
                first, second, seed, -1.0, base_step, first_domains, second_domains,
                tolerance, interpolate_guides,
            )?;
            if backward_closed {
                forward = backward;
                closed = true;
                Vec::new()
            } else {
                backward.into_iter().rev().collect::<Vec<_>>()
            }
        };
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
        if poles.iter().any(|(_, _, pole, _, cutoff)| parameters.iter().all(|uv| {
            first.first_order(uv[0], uv[1]).is_ok_and(|value| value.0.distance(*pole) <= *cutoff)
        })) { continue; }
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
                    endpoint[index] = domain.nearest(endpoint[index], values[index]);
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
        if !closed {
            snap(parameters.first_mut().unwrap())?;
            snap(parameters.last_mut().unwrap())?;
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
                if interpolate_guides {
                    // A corrector can stall next to a contact pole and emit
                    // several samples of one model-space point. Microscopic
                    // guide spans there make the parameter gauge singular.
                    let point = |value: [f64; 4]| first.first_order(value[0], value[1]).map(|value| value.0);
                    let end = *parameters.last().unwrap();
                    let mut compact = vec![parameters[0]];
                    let mut previous = point(parameters[0])?;
                    for value in &parameters[1..parameters.len() - 1] {
                        let next = point(*value)?;
                        if next.distance(previous) > linear * 8.0 {
                            compact.push(*value);
                            previous = next;
                        }
                    }
                    if compact.len() > 1 && previous.distance(point(end)?) <= linear * 8.0 {
                        compact.pop();
                    }
                    compact.push(end);
                    parameters = compact;
                }
                traced.push(parameters);
            }
        }
    }

    // Blend jets use finite differences at their contact limits, where the
    // resulting normal derivatives cannot certify a stationary contact.
    let procedural = |surface: &Surface3| matches!(surface, Surface3::RollingBall(_) | Surface3::Blend(_))
        || matches!(surface, Surface3::Offset(value) if matches!(value.base(), Surface3::RollingBall(_) | Surface3::Blend(_)));
    let refine_contacts = !std::ptr::eq(first, second) && !procedural(first) && !procedural(second);
    let cross = |v: [f64; 4]| -> KernelResult<Vec3> {
        let (_, a, b) = first.first_order(v[0], v[1])?;
        let (_, c, d) = second.first_order(v[2], v[3])?;
        Ok(a.cross(b).normalize().cross(c.cross(d).normalize()))
    };
    let mut split_traces = Vec::new();
    for trace in traced {
        if !refine_contacts {
            split_traces.push(trace);
            continue;
        }
        let mut current = vec![trace[0]];
        let mut previous_normal_cross = cross(trace[0])?;
        for pair in trace.windows(2) {
            let normal_cross = cross(pair[1])?;
            // Across a saddle the oriented tangent reverses even when the
            // trace continues in the same direction.
            if previous_normal_cross.dot(normal_cross) < 0.0 {
                let seed = std::array::from_fn(|i| (pair[0][i] + pair[1][i]) * 0.5);
                if let Some(root) = refine_surface_tangency(first, second, seed, first_domains, second_domains, tolerance)? {
                    let chord = std::array::from_fn::<_, 4, _>(|i| pair[1][i] - pair[0][i]);
                    let length2 = chord.iter().map(|v| v * v).sum::<f64>();
                    let fraction = root.iter().zip(pair[0]).zip(chord).map(|((r, a), d)| (r - a) * d).sum::<f64>() / length2;
                    let closest = std::array::from_fn(|i| pair[0][i] + chord[i] * fraction);
                    let point = first.first_order(root[0], root[1])?.0;
                    if (0.0..=1.0).contains(&fraction)
                        && parameter_distance(root, closest) <= length2.sqrt() * 0.25
                        && point.distance(first.first_order(pair[0][0], pair[0][1])?.0) > tolerance.linear * 8.0
                        && point.distance(first.first_order(pair[1][0], pair[1][1])?.0) > tolerance.linear * 8.0 {
                        current.push(root);
                        split_traces.push(current);
                        current = vec![root];
                    }
                }
            }
            previous_normal_cross = normal_cross;
            current.push(pair[1]);
        }
        split_traces.push(current);
    }
    let traced = split_traces;
    let curves = traced
        .into_iter()
        .map(|parameters| {
            for (surface, plane, side) in [(first, second, 0), (second, first, 2)] {
                if let (Surface3::Nurbs(surface), Surface3::Plane(plane)) = (surface, plane) {
                    if let Some(curve) = isoparametric_plane_section(surface, *plane, &parameters, side, tolerance)? {
                        return Ok(if side == 0 { curve } else {
                            SurfaceSurfaceCurve { first_pcurve: curve.second_pcurve, second_pcurve: curve.first_pcurve, ..curve }
                        });
                    }
                }
            }
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
            let torus_plane = matches!((first, second), (Surface3::Plane(_), Surface3::Torus(_)) | (Surface3::Torus(_), Surface3::Plane(_)));
            let mut guide_parameters = vec![0.0];
            if torus_plane || interpolate_guides {
                for index in 1..first_points.len() {
                    let length = (first_points[index] - first_points[index - 1]).magnitude2()
                        + (second_points[index] - second_points[index - 1]).magnitude2();
                    guide_parameters.push(guide_parameters.last().unwrap() + length.sqrt());
                }
                let length = *guide_parameters.last().unwrap();
                for parameter in &mut guide_parameters { *parameter /= length; }
            }
            // Component-wise monotone smoothing can zero the entire tangent
            // when two traced chords have no coordinate moving in the same
            // direction. The exact corrector needs a nonzero guide gauge.
            let smooth = first_points.windows(3).zip(second_points.windows(3)).all(|(a, b)| {
                [a, b].into_iter().any(|p| {
                    let before = p[1] - p[0];
                    let after = p[2] - p[1];
                    before.x * after.x > 0.0 || before.y * after.y > 0.0
                })
            });
            let guide = |points: &[Point2]| {
                if nested_intersection && !interpolate_guides {
                    let parameters = (0..points.len()).map(|index| index as f64 / (points.len() - 1) as f64).collect::<Vec<_>>();
                    interpolating_nurbs_guide(points, &parameters)
                } else if torus_plane || interpolate_guides {
                    let linear = interpolating_nurbs_guide(points, &guide_parameters)?;
                    if !smooth { return Ok(linear); }
                    match smooth_interpolating_guide(&linear)? {
                        Curve2::Nurbs(curve) => Ok(curve),
                        _ => unreachable!(),
                    }
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

/// At an isolated tangency, position equations lose a constraint. Normal
/// equality supplies the missing differential information and isolates the
/// contact without accepting a square-root positional residual as a vertex.
fn refine_surface_tangency(first: &Surface3, second: &Surface3, mut parameters: [f64; 4],
    first_bounds: [super::ParameterRange; 2], second_bounds: [super::ParameterRange; 2],
    tolerance: ModelTolerance) -> KernelResult<Option<[f64; 4]>>
{
    let normal_jet = |value: super::SurfaceEvaluation| -> Option<(Vec3, Vec3, Vec3)> {
        let cross = value.du.cross(value.dv);
        let length = cross.magnitude();
        if length <= tolerance.linear_at_scale(value.point.to_vec().magnitude())
            * value.du.magnitude().max(value.dv.magnitude()) { return None; }
        let normal = cross / length;
        let derivative = |v: Vec3| (v - normal * normal.dot(v)) / length;
        Some((normal, derivative(value.duu.cross(value.dv) + value.du.cross(value.duv)),
            derivative(value.duv.cross(value.dv) + value.du.cross(value.dvv))))
    };
    for _ in 0..32 {
        let a = first.evaluate(parameters[0], parameters[1])?;
        let b = second.evaluate(parameters[2], parameters[3])?;
        let Some((n, nu, nv)) = normal_jet(a) else { return Ok(None); };
        let Some((m, mu, mv)) = normal_jet(b) else { return Ok(None); };
        let scale = a.point.to_vec().magnitude().max(b.point.to_vec().magnitude()).max(1.0);
        let position = a.point - b.point;
        let angle = n.cross(m);
        if position.magnitude() <= f64::EPSILON * scale * 2048.0
            && angle.magnitude() <= tolerance.angular * 8.0 { return Ok(Some(parameters)); }
        let normal_columns = [nu.cross(m), nv.cross(m), n.cross(mu), n.cross(mv)];
        let position_columns = [a.du, a.dv, -b.du, -b.dv];
        let columns: [[f64; 6]; 4] = std::array::from_fn(|i| {
            let p = position_columns[i];
            let n = normal_columns[i] * scale;
            [p.x, p.y, p.z, n.x, n.y, n.z]
        });
        let residual = [position.x, position.y, position.z, angle.x * scale, angle.y * scale, angle.z * scale];
        let matrix: [[f64; 4]; 4] = std::array::from_fn(|i| std::array::from_fn(|j| columns[i].iter().zip(columns[j]).map(|(a, b)| a * b).sum()));
        let rhs: [f64; 4] = std::array::from_fn(|i| -columns[i].iter().zip(residual).map(|(a, b)| a * b).sum::<f64>());
        let Some(step) = solve_linear(matrix, rhs) else { return Ok(None); };
        parameters = clamp_parameters(std::array::from_fn(|i| parameters[i] + step[i]), first_bounds, second_bounds);
    }
    Ok(None)
}

/// Replace an isoparametric plane section with its exact rational
/// curve. The control hull certifies planarity; trace samples only identify
/// which parameter is constant. This avoids repeated implicit corrections in
/// projected profiles and their later extrusions.
fn isoparametric_plane_section(surface: &super::NurbsSurface3, plane: Plane3,
    parameters: &[[f64; 4]], side: usize, tolerance: ModelTolerance) -> KernelResult<Option<SurfaceSurfaceCurve>> {
    let plane_normal = plane.normal();
    for axis in 0..2 {
        let constant = parameters[0][side + axis];
        let epsilon = f64::EPSILON * constant.abs().max(1.0) * 256.0;
        if parameters.iter().any(|uv| (uv[side + axis] - constant).abs() > epsilon) { continue; }
        let basis = surface.isoparametric_curve(axis, constant)?;
        let DomainBounds::Bounded(domain) = basis.domain().bounds else { unreachable!() };
        let start = parameters[0][side + 1 - axis].clamp(domain.start, domain.end);
        let end = parameters.last().unwrap()[side + 1 - axis].clamp(domain.start, domain.end);
        let range = super::ParameterRange::new(start, end)?;
        let basis = basis.restricted(range)?;
        let mut error = 0.0_f64;
        let planar = basis.control_points().iter().all(|point| {
            let distance = (*point - plane.origin()).dot(plane_normal).abs();
            error = error.max(distance);
            distance <= f64::EPSILON * point.to_vec().magnitude().max(1.0) * 256.0
        });
        if !planar { continue; }
        let curve: Curve3 = basis.reparameterized(range.start, range.span())?.into();
        let (origin, direction) = if axis == 0 {
            (Point2::new(constant, range.start), crate::Vec2::new(0.0, range.span()))
        } else { (Point2::new(range.start, constant), crate::Vec2::new(range.span(), 0.0)) };
        let first_pcurve = Line2::new(origin, direction)?.into();
        let second_pcurve = plane.parameterized_curve_image(&curve, super::ParameterRange::new(0.0, 1.0)?, tolerance)?;
        return Ok(Some(SurfaceSurfaceCurve { curve, first_pcurve, second_pcurve, achieved_error: error }));
    }
    Ok(None)
}

#[test]
fn trimmed_isoparametric_sections_retain_exact_curves() {
    let tolerance = ModelTolerance::default();
    let surface = super::NurbsSurface3::new((2, 1), [0.0, 1.0].into_iter().map(|y| {
        vec![Point3::new(0.0, y, 0.0), Point3::new(1.0, y, 2.0), Point3::new(3.0, y, 1.0)]
    }).collect(), vec![vec![1.0, 0.7, 1.0]; 2],
        (vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0])).unwrap();
    let plane = Plane3::new(Point3::new(0.0, 0.3, 0.0), Vec3::unit_x(), Vec3::unit_z(), tolerance).unwrap();
    for (start, end) in [(0.2, 0.8), (0.8, 0.2)] {
        let parameters = [[start, 0.3, 0.0, 0.0], [(start + end) * 0.5, 0.3, 0.0, 0.0], [end, 0.3, 0.0, 0.0]];
        let section = isoparametric_plane_section(&surface, plane, &parameters, 0, tolerance).unwrap().unwrap();
        assert!(matches!(section.curve, Curve3::Nurbs(_)));
        assert_eq!(section.curve.domain().bounds, DomainBounds::Bounded(super::ParameterRange::new(0.0, 1.0).unwrap()));
        for t in [0.0, 0.2, 0.7, 1.0] {
            let point = section.curve.evaluate(t).unwrap().point;
            let uv = section.first_pcurve.evaluate(t).unwrap().point;
            assert!(point.distance(Surface3::Nurbs(surface.clone()).evaluate(uv.x, uv.y).unwrap().point) < 1.0e-12);
            let uv = section.second_pcurve.evaluate(t).unwrap().point;
            assert!(point.distance(plane.point_at(uv)) < 1.0e-12);
        }
    }
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

impl RationalSurfaceCell {
    /// Certify contact with a translated curve strip by comparing corresponding
    /// Bernstein coefficients. A shared monotone coordinate fixes the curve
    /// parameter; the remaining transverse coordinate must stay on one side
    /// and have zero derivative at any shared boundary. Unlike sampled normals,
    /// this accounts for the whole patch, including its interior. `slack`
    /// widens only the transverse contact comparisons.
    fn only_touches_extrusion(&self, other: &Self, slack: f64) -> bool {
        if self.padding != 0.0 || other.padding != 0.0 { return false; }
        let grid = |cell: &Self, axis| -> Vec<Vec<Vec4>> {
            if axis == 1 { cell.controls.clone() } else {
                (0..cell.controls[0].len()).map(|u| cell.controls.iter().map(|row| row[u]).collect()).collect()
            }
        };
        let point = |control: Vec4| control.truncate() / control.w;
        let scale = self.controls.iter().flatten().chain(other.controls.iter().flatten())
            .map(|control| point(*control).magnitude()).fold(1.0_f64, f64::max);
        let roundoff = f64::EPSILON * scale * 256.0;
        let contact = roundoff.max(slack);
        // The strip's curve may run either way along the patch rows.
        for (other_axis, reversed) in [(0, false), (0, true), (1, false), (1, true)] {
            let mut strip = grid(other, other_axis);
            if strip[0].len() != 2 { continue; }
            if reversed { strip.reverse(); }
            let translation = point(strip[0][1]) - point(strip[0][0]);
            if translation.magnitude() <= roundoff || strip.iter().any(|row| {
                (row[0].w - row[1].w).abs() > f64::EPSILON * row[0].w * 256.0
                    || (point(row[1]) - point(row[0]) - translation).magnitude() > roundoff
            }) { continue; }
            let translation_direction = translation.normalize();
            for axis in 0..2 {
                let patch = grid(self, axis);
                if patch.len() != strip.len() { continue; }
                let width = patch[0].len();
                if width < 3 { continue; }
                // Factor the weights so every transverse column uses the
                // strip's same rational curve parameterization.
                if (0..width).any(|j| {
                    let ratio = patch[0][j].w / strip[0][0].w;
                    patch.iter().zip(&strip).any(|(row, base)| {
                        (row[j].w - base[0].w * ratio).abs() > f64::EPSILON * row[j].w * 256.0
                    })
                }) { continue; }
                let differences = patch.iter().zip(&strip).map(|(row, base)| {
                    row.iter().map(|control| point(*control) - point(base[0])).collect::<Vec<_>>()
                }).collect::<Vec<_>>();
                let Some(longitudinal) = differences.iter().flatten()
                    .map(|delta| translation_direction.cross(*delta))
                    .find(|normal| normal.magnitude() > roundoff).map(|normal| normal.normalize())
                else { continue; };
                if differences.iter().flatten().any(|delta| longitudinal.dot(*delta).abs() > roundoff) { continue; }
                let heights = strip.iter().map(|row| longitudinal.dot(point(row[0]))).collect::<Vec<_>>();
                if !heights.windows(2).all(|pair| pair[1] - pair[0] > roundoff)
                    && !heights.windows(2).all(|pair| pair[0] - pair[1] > roundoff) { continue; }
                let transverse = translation.cross(longitudinal).normalize();
                let residuals = differences.iter().map(|row| row.iter()
                    .map(|delta| transverse.dot(*delta)).collect::<Vec<_>>()).collect::<Vec<_>>();
                let low = residuals.iter().flatten().copied().fold(f64::INFINITY, f64::min);
                let high = residuals.iter().flatten().copied().fold(f64::NEG_INFINITY, f64::max);
                let sign = if low >= -contact { 1.0 } else if high <= contact { -1.0 } else { continue; };
                if residuals.iter().any(|row| !row.iter().any(|value| value * sign > contact)) { continue; }
                let tangent_boundaries = [(0, 1), (width - 1, width - 2)].into_iter().all(|(edge, adjacent)| {
                    !residuals.iter().all(|row| row[edge].abs() <= contact)
                        || residuals.iter().all(|row| row[adjacent].abs() <= contact)
                });
                if tangent_boundaries { return true; }
            }
        }
        false
    }

    /// A strip extruded from an interior parameter line of this patch touches
    /// it along that line, where no single Bernstein form is one-signed. The
    /// line is the patch silhouette along the strip's translation; split there,
    /// each half meets the strip only along its boundary. The split parameter
    /// carries bisection error, so the contact is certified at model tolerance.
    fn touches_extrusion_inside(&self, surface: &Surface3, other: &Self,
        tolerance: ModelTolerance) -> KernelResult<bool> {
        if self.padding != 0.0 { return Ok(false); }
        let translation = if other.controls.len() == 2 {
            Self::euclidean(other.controls[1][0])? - Self::euclidean(other.controls[0][0])?
        } else if other.controls[0].len() == 2 {
            Self::euclidean(other.controls[0][1])? - Self::euclidean(other.controls[0][0])?
        } else { return Ok(false); };
        let ranges = [self.u_range, self.v_range];
        for axis in 0..2 {
            let across = ranges[1 - axis].middle();
            let facing = |parameter: f64| -> KernelResult<f64> {
                let (u, v) = if axis == 0 { (parameter, across) } else { (across, parameter) };
                let (_, du, dv) = surface.first_order(u, v)?;
                Ok(du.cross(dv).dot(translation))
            };
            let (mut low, mut high) = (ranges[axis].start, ranges[axis].end);
            let mut low_value = facing(low)?;
            if low_value * facing(high)? >= 0.0 { continue; }
            for _ in 0..64 {
                let middle = (low + high) * 0.5;
                let value = facing(middle)?;
                if (value > 0.0) == (low_value > 0.0) { (low, low_value) = (middle, value); } else { high = middle; }
            }
            let fraction = ((low + high) * 0.5 - ranges[axis].start) / ranges[axis].span();
            let (left, right) = if axis == 0 { self.clone().split_u(fraction) } else { self.clone().split_v(fraction) };
            if left.only_touches_extrusion(other, tolerance.linear) && right.only_touches_extrusion(other, tolerance.linear) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn euclidean(control: Vec4) -> KernelResult<Point3> {
        if control.w <= 0.0 || !control.w.is_finite() {
            return Err(geometry_error("rational Bézier surface has a nonpositive homogeneous weight"));
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

    /// A separating-axis candidate at the cell center. A singular chart has
    /// no normal, so only its spatial box participates in this rejection.
    fn normal(&self, surface: &Surface3) -> KernelResult<Option<Vec3>> {
        if let Surface3::Offset(offset) = surface { return self.normal(offset.base()); }
        let (_, du, dv) = surface.first_order(self.u_range.middle(), self.v_range.middle())?;
        let normal = du.cross(dv);
        Ok((normal.magnitude2() > 0.0).then(|| normal.normalize()))
    }

    /// Interval of the cell's control hull, which contains the patch, along a
    /// unit direction.
    fn extent_along(&self, direction: Vec3) -> [f64; 2] {
        let (low, high) = self.controls.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), control| {
            let value = (control.x * direction.x + control.y * direction.y + control.z * direction.z) / control.w;
            (low.min(value), high.max(value))
        });
        [low - self.padding, high + self.padding]
    }

    // The common transverse axis of two extruded strips ignores their full
    // longitudinal spans, which otherwise keep distant control hulls in play.
    // This supplies a candidate axis only; the actual test uses both hulls.
    fn ruled_direction(&self) -> Option<Vec3> {
        let start = self.controls[0][0];
        let end = if self.controls.len() == 2 {
            self.controls[1][0]
        } else if self.controls[0].len() == 2 {
            self.controls[0][1]
        } else {
            return None;
        };
        let direction = end.truncate() / end.w - start.truncate() / start.w;
        (direction.magnitude2() > 0.0).then(|| direction.normalize())
    }

    fn corner_directions(&self) -> [Vec3; 2] {
        let point = |control: Vec4| control.truncate() / control.w;
        let origin = point(self.controls[0][0]);
        [
            point(*self.controls[0].last().unwrap()) - origin,
            point(self.controls.last().unwrap()[0]) - origin,
        ]
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
            self.split_u(0.5)
        } else {
            self.split_v(0.5)
        }
    }

    fn split_u(self, fraction: f64) -> (Self, Self) {
        let (left_rows, right_rows): (Vec<_>, Vec<_>) = self
            .controls
            .into_iter()
            .map(|row| de_casteljau_split(&row, fraction))
            .unzip();
        let middle = self.u_range.start + self.u_range.span() * fraction;
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

    fn split_v(self, fraction: f64) -> (Self, Self) {
        let width = self.controls[0].len();
        let (left_columns, right_columns): (Vec<_>, Vec<_>) = (0..width)
            .map(|u| de_casteljau_split(&self.controls.iter().map(|row| row[u]).collect::<Vec<_>>(), fraction))
            .unzip();
        let left_height = left_columns[0].len();
        let right_height = right_columns[0].len();
        let left = (0..left_height)
            .map(|v| left_columns.iter().map(|column| column[v]).collect())
            .collect();
        let right = (0..right_height)
            .map(|v| right_columns.iter().map(|column| column[v]).collect())
            .collect();
        let middle = self.v_range.start + self.v_range.span() * fraction;
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

fn rational_surface_cells(surface: &Surface3, bounds: [super::ParameterRange; 2]) -> KernelResult<Vec<RationalSurfaceCell>> {
    match surface {
        Surface3::Nurbs(surface) => {
            let mut cells = Vec::new();
            for patch in surface.bezier_patches()? {
                let start = [patch.u_range.start.max(bounds[0].start), patch.v_range.start.max(bounds[1].start)];
                let end = [patch.u_range.end.min(bounds[0].end), patch.v_range.end.min(bounds[1].end)];
                if (0..2).any(|axis| start[axis] >= end[axis]) { continue; }
                let bounds = [super::ParameterRange::new(start[0], end[0])?, super::ParameterRange::new(start[1], end[1])?];
                cells.push(RationalSurfaceCell {
                    u_range: bounds[0], v_range: bounds[1],
                    controls: patch.controls_in_bounds(bounds), padding: 0.0,
                });
            }
            Ok(cells)
        }
        Surface3::Offset(offset) => {
            let mut cells = rational_surface_cells(offset.base(), bounds)?;
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
            let Some(lambda) = solve_linear(normal, [-residual.x, -residual.y, -residual.z])
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

/// Test branch membership on the corrected supports, not by proximity to a
/// trace sample. A large face box can make the tracing step wider than the
/// separation between two different intersection branches.
fn trace_reaches_seed(
    first: &Surface3,
    second: &Surface3,
    trace: &[[f64; 4]],
    seed: [f64; 4],
    neighborhood: f64,
    first_domains: [super::ParameterRange; 2],
    second_domains: [super::ParameterRange; 2],
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let parameter_error = seed.iter().map(|v| v.abs()).fold(1.0_f64, f64::max)
        * f64::EPSILON * 4096.0;
    let (first_point, first_du, first_dv) = first.first_order(seed[0], seed[1])?;
    let (second_point, second_du, second_dv) = second.first_order(seed[2], seed[3])?;
    let first_normal = first_du.cross(first_dv).normalize();
    let second_normal = second_du.cross(second_dv).normalize();
    // Compare roots in model units: UV speed can vanish at a contact corner.
    // The corrector's residual gives only square-root positional accuracy
    // when the two surface normals become parallel.
    let tangency = first_normal.cross(second_normal).magnitude();
    let scale = first_point.to_vec().magnitude().max(second_point.to_vec().magnitude()).max(1.0);
    let roundoff = f64::EPSILON * scale * 2048.0;
    let positional_error = if tangency <= (roundoff / scale).sqrt() * 8.0 {
        (roundoff * scale).sqrt() * 8.0
    } else {
        roundoff / tangency
    }.max(tolerance.linear_at_scale(scale) * 8.0);
    let allowed = parameter_error.max(tolerance.parameter * 64.0);
    for pair in trace.windows(2) {
        let delta: [f64; 4] = std::array::from_fn(|i| pair[1][i] - pair[0][i]);
        let axis = (0..4).max_by(|a, b| delta[*a].abs().total_cmp(&delta[*b].abs())).unwrap();
        if delta[axis].abs() <= tolerance.parameter { continue; }
        let fraction = (seed[axis] - pair[0][axis]) / delta[axis];
        // Limit continuation to one sampled segment beyond either endpoint.
        // A large tracing step alone cannot justify a distant extrapolation.
        if !(-1.0..=2.0).contains(&fraction) { continue; }
        let guide = std::array::from_fn(|i| pair[0][i] + delta[i] * fraction);
        if parameter_distance(guide, seed) <= allowed { return Ok(true); }
        let nearest = std::array::from_fn(|i| pair[0][i] + delta[i] * fraction.clamp(0.0, 1.0));
        // A trim box can cut a short gap out of one support intersection.
        // Continue the local branch to a nearby seed beyond the sampled end;
        // requiring it to lie inside a segment retraces the overlapping branch.
        if parameter_distance(guide, nearest) > neighborhood
            || parameter_distance(guide, seed) > neighborhood { continue; }
        if parameters_in_ranges(guide, first_domains, second_domains, tolerance.parameter) {
            let a = first.first_order(guide[0], guide[1])?.0;
            let b = second.first_order(guide[2], guide[3])?.0;
            if a.distance(first_point) <= positional_error && b.distance(second_point) <= positional_error {
                return Ok(true);
            }
        }
        let mut gauge = [0.0; 4];
        gauge[axis] = 1.0;
        // Start on this trace and hold its monotone coordinate at the seed.
        // Starting Newton at the seed itself would trivially accept every
        // branch, since each seed already satisfies both support equations.
        let corrected = correct_surface_pair_with_gauge(
            first, second, guide, gauge, first_domains, second_domains, tolerance,
        )?;
        if let Some(parameters) = corrected {
            if parameter_distance(parameters, seed) <= allowed
                || (parameter_distance(parameters, seed) <= neighborhood
                    && first.first_order(parameters[0], parameters[1])?.0.distance(first_point) <= positional_error
                    && second.first_order(parameters[2], parameters[3])?.0.distance(second_point) <= positional_error) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn collapsed_plane_join_poles(
    first: &Surface3, second: &Surface3,
    first_domains: [super::ParameterRange; 2], second_domains: [super::ParameterRange; 2],
    tolerance: ModelTolerance,
) -> KernelResult<Vec<(usize, f64, Point3, (f64, f64), f64)>> {
    let mut poles = Vec::new();
    for (side, surface, other, domains) in [(0, first, second, first_domains), (1, second, first, second_domains)] {
        let (Surface3::RollingBall(blend), Surface3::Plane(plane)) = (surface, other) else { continue; };
        for u in [blend.center_range().start, blend.center_range().end] {
            if !domains[0].contains(u, tolerance) { continue; }
            let a = surface.first_order(u, 0.0)?.0;
            let b = surface.first_order(u, 1.0)?.0;
            let roundoff = f64::EPSILON * a.to_vec().magnitude().max(1.0) * 2048.0;
            let projection = plane.project(a)?;
            if a.distance(b) <= roundoff * 8.0 && projection.distance.abs() <= roundoff {
                poles.push((side, u, a, projection.parameters, (roundoff * blend.radius()).sqrt() * 2.0));
            }
        }
    }
    Ok(poles)
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
    adaptive: bool,
) -> KernelResult<(Vec<[f64; 4]>, bool)> {
    let poles = collapsed_plane_join_poles(first, second, first_domains, second_domains, tolerance)?;
    let mut result = Vec::new();
    let mut current = seed;
    let mut previous_tangent = None;
    let mut initial_tangent = [0.0; 4];
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
            initial_tangent = tangent;
        }
        // A long axial range must not set the step in an angular chart.
        // Otherwise a predictor can skip across the turn of a branch.
        let mut local_step = step;
        for (component, domain) in tangent.into_iter().zip(&domains) {
            if let Some(period) = domain.period {
                local_step = local_step.min(period / (48.0 * component.abs()));
            }
        }
        // Adaptive guides also need local steps in their nonperiodic charts.
        if adaptive || matches!((first, second), (Surface3::Plane(_), Surface3::Torus(_)) | (Surface3::Torus(_), Surface3::Plane(_))) {
            for (index, range) in first_domains.iter().chain(&second_domains).enumerate() {
                if tangent[index].abs() > tolerance.parameter {
                    local_step = local_step.min(range.span().abs() / (48.0 * tangent[index].abs()));
                }
            }
        }
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
                // Solve for the exact trim endpoint when the predictor
                // leaves the box rather than accumulating near-bound samples.
                None if !adaptive && !parameters_in_ranges(
                    predicted, first_domains, second_domains, tolerance.parameter,
                ) => break,
                None => { local_step *= 0.5; }
                Some(value) if parameter_distance(value, predicted) > local_step * 0.25 => {
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
        if let Some((side, u, _, other_uv, _)) = poles.iter().find(|(_, _, pole, _, cutoff)| {
            first.first_order(next[0], next[1]).is_ok_and(|value| value.0.distance(*pole) <= *cutoff)
        }) {
            // The entire transverse chart closes to one point here. Following
            // its numerical residual around the chart manufactures a loop.
            let mut endpoint = next;
            endpoint[side * 2] = *u;
            endpoint[(1 - side) * 2] = other_uv.0;
            endpoint[(1 - side) * 2 + 1] = other_uv.1;
            result.push(endpoint);
            break;
        }
        if parameter_distance(next, current) <= tolerance.parameter * 16.0 {
            break;
        }
        // A closed branch crosses the starting transverse plane in the
        // original direction. Confirm that the crossing is the starting root
        // before closing, then use the exact seed as the shared endpoint.
        let seed_plane = |point: [f64; 4]| {
            (0..4).map(|axis| (point[axis] - seed[axis]) * initial_tangent[axis]).sum::<f64>()
        };
        let before = seed_plane(current);
        let after = seed_plane(next);
        if before < 0.0 && after >= 0.0 && parameter_distance(next, seed) <= local_step * 2.0 {
            let fraction = -before / (after - before);
            let crossing = std::array::from_fn(|axis| current[axis] + (next[axis] - current[axis]) * fraction);
            if correct_surface_pair_with_gauge(
                first, second, crossing, initial_tangent, first_domains, second_domains, tolerance,
            )?.is_some_and(|root| parameter_distance(root, seed) <= tolerance.parameter * 64.0) {
                result.push(seed);
                return Ok((result, true));
            }
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
        current = next;
        previous_tangent = Some(tangent);
    }
    Ok((result, false))
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
        let Some(delta) = solve_linear(matrix, [-residual.x, -residual.y, -residual.z, -gauge])
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
        points[index] = super::nearest_uv(domains, points[index], points[index - 1]);
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
    // Tangency is decided on the linear distance between line and circle, as
    // the squared gap r² - d² shrinks with 2r·(r - d). A squared tolerance
    // splits solved tangencies into two hits a square root apart.
    let closest_distance = (closest - circle.center()).magnitude();
    let radial_gap = circle.radius() - closest_distance;
    if radial_gap < -linear {
        return Ok(IntersectionSet::Empty);
    }
    let tangent = radial_gap <= linear;
    let step = ((circle.radius() + closest_distance) * radial_gap.max(0.0) / a).sqrt();
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
                return Err(geometry_error("plane intersection did not yield a line"));
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
                return Err(geometry_error("plane intersection did not yield a line"));
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
