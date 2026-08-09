use cgmath::prelude::*;

use crate::{Point2, Point3};

use super::{
    Circle3, Curve2, Curve3, DiagnosticKind, KernelError, KernelResult, Line2, Line3,
    ModelTolerance, Plane3, Surface3,
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
        match (self, other) {
            (Self::Plane(first), Self::Plane(second)) => plane_plane(*first, *second, tolerance),
            _ => Err(unsupported(
                "this surface/surface intersection pair is not yet certified",
            )),
        }
    }
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
    let b = 2.0 * offset.dot(direction);
    let c = offset.magnitude2() - circle.radius().powi(2);
    let discriminant = b * b - 4.0 * a * c;
    let discriminant_tolerance = linear * linear * a;
    if discriminant < -discriminant_tolerance {
        return Ok(IntersectionSet::Empty);
    }
    let tangent = discriminant.abs() <= discriminant_tolerance;
    let root = discriminant.max(0.0).sqrt();
    let parameters = if tangent {
        vec![-b / (2.0 * a)]
    } else {
        vec![(-b - root) / (2.0 * a), (-b + root) / (2.0 * a)]
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
