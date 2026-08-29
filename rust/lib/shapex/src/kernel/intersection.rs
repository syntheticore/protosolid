use cgmath::prelude::*;

use crate::{Point2, Point3, Vec4};

use super::{
    Circle3, Curve2, Curve3, DiagnosticKind, DomainBounds, KernelError, KernelResult, Line2, Line3,
    ModelTolerance, NurbsCurve2, Plane3, Surface3, SurfaceIntersectionCurve,
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
            (Self::Nurbs(_) | Self::Offset(_), Self::Nurbs(_) | Self::Offset(_)) => {
                trace_regular_surface_intersections(self, other, tolerance)
            }
            _ => Err(unsupported(
                "this surface/surface intersection pair is not yet certified",
            )),
        }
    }
}

fn trace_regular_surface_intersections(
    first: &Surface3,
    second: &Surface3,
    tolerance: ModelTolerance,
) -> KernelResult<IntersectionSet<SurfaceSurfaceCurve>> {
    let first_domains = bounded_surface_domains(first)?;
    let second_domains = bounded_surface_domains(second)?;
    let parameter_scale = first_domains
        .iter()
        .chain(&second_domains)
        .map(|range| range.span().abs())
        .fold(1.0_f64, f64::max);
    let mut seeds = Vec::<[f64; 4]>::new();
    let mut try_seed = |guess: [f64; 4]| -> KernelResult<()> {
        let Some(parameters) = correct_surface_pair(first, second, guess, tolerance)? else {
            return Ok(());
        };
        let point = first.evaluate(parameters[0], parameters[1])?.point;
        if seeds.iter().any(|existing| {
            first.evaluate(existing[0], existing[1]).is_ok_and(|value| {
                value.point.distance(point)
                    <= tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0)) * 16.0
            })
        }) {
            return Ok(());
        }
        seeds.push(parameters);
        Ok(())
    };

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
    if seeds.is_empty() {
        return Ok(IntersectionSet::Empty);
    }

    let base_step = parameter_scale / 96.0;
    let mut traced = Vec::<Vec<[f64; 4]>>::new();
    for seed in seeds {
        if traced.iter().any(|curve| {
            curve
                .iter()
                .any(|parameters| parameter_distance(*parameters, seed) <= base_step * 1.5)
        }) {
            continue;
        }
        let backward =
            trace_surface_intersection_direction(first, second, seed, -1.0, base_step, tolerance)?;
        let forward =
            trace_surface_intersection_direction(first, second, seed, 1.0, base_step, tolerance)?;
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
        if parameters.len() >= 2 {
            traced.push(parameters);
        }
    }

    let curves = traced
        .into_iter()
        .map(|parameters| {
            let first_points = parameters
                .iter()
                .map(|value| Point2::new(value[0], value[1]))
                .collect::<Vec<_>>();
            let second_points = parameters
                .iter()
                .map(|value| Point2::new(value[2], value[3]))
                .collect::<Vec<_>>();
            let first_pcurve = linear_nurbs_guide(&first_points)?;
            let second_pcurve = linear_nurbs_guide(&second_points)?;
            let range = super::ParameterRange::new(0.0, 1.0)?;
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
            Ok(SurfaceSurfaceCurve {
                curve: implicit.into(),
                first_pcurve: first_pcurve.into(),
                second_pcurve: second_pcurve.into(),
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
                cell.padding += offset.distance().abs();
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
    tolerance: ModelTolerance,
) -> KernelResult<Option<[f64; 4]>> {
    let first_domains = bounded_surface_domains(first)?;
    let second_domains = bounded_surface_domains(second)?;
    let mut parameters = clamp_parameters(parameters, first_domains, second_domains);
    for _ in 0..32 {
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
            let regularity = first_value
                .normal()?
                .cross(second_value.normal()?)
                .magnitude();
            return Ok((regularity > tolerance.angular * 8.0).then_some(parameters));
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
        let Some(lambda) = solve_linear_3(normal, [-residual.x, -residual.y, -residual.z]) else {
            return Ok(None);
        };
        let delta: [f64; 4] = std::array::from_fn(|column| {
            (0..3)
                .map(|row| jacobian[row][column] * lambda[row])
                .sum::<f64>()
        });
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
                let candidate_residual = first.evaluate(candidate[0], candidate[1])?.point
                    - second.evaluate(candidate[2], candidate[3])?.point;
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
    tolerance: ModelTolerance,
) -> KernelResult<Vec<[f64; 4]>> {
    let first_domains = bounded_surface_domains(first)?;
    let second_domains = bounded_surface_domains(second)?;
    let mut result = Vec::new();
    let mut current = seed;
    let mut previous_tangent = None;
    for _ in 0..2048 {
        let first_value = first.evaluate(current[0], current[1])?;
        let second_value = second.evaluate(current[2], current[3])?;
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
        let predicted = std::array::from_fn(|index| current[index] + tangent[index] * step);
        let Some(next) =
            correct_surface_pair_with_gauge(first, second, predicted, tangent, tolerance)?
        else {
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
        if result.len() > 12 && parameter_distance(next, seed) <= step * 0.75 {
            break;
        }
        current = next;
        previous_tangent = Some(tangent);
        if parameter_on_boundary(current, first_domains, second_domains, step * 0.5) {
            break;
        }
    }
    Ok(result)
}

fn correct_surface_pair_with_gauge(
    first: &Surface3,
    second: &Surface3,
    guide: [f64; 4],
    tangent: [f64; 4],
    tolerance: ModelTolerance,
) -> KernelResult<Option<[f64; 4]>> {
    let first_domains = bounded_surface_domains(first)?;
    let second_domains = bounded_surface_domains(second)?;
    let mut parameters = clamp_parameters(guide, first_domains, second_domains);
    for _ in 0..24 {
        let first_value = first.evaluate(parameters[0], parameters[1])?;
        let second_value = second.evaluate(parameters[2], parameters[3])?;
        let residual = first_value.point - second_value.point;
        let gauge = parameters
            .iter()
            .zip(guide)
            .zip(tangent)
            .map(|((value, guide), tangent)| (value - guide) * tangent)
            .sum::<f64>();
        if residual.magnitude()
            <= tolerance.linear_at_scale(first_value.point.to_vec().magnitude().max(1.0))
            && gauge.abs() <= tolerance.parameter * 8.0
        {
            return Ok(Some(parameters));
        }
        let matrix = [
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

fn linear_nurbs_guide(points: &[Point2]) -> KernelResult<NurbsCurve2> {
    if points.len() < 2 {
        return Err(unsupported(
            "surface-intersection guide needs at least two points",
        ));
    }
    let mut knots = vec![0.0, 0.0];
    knots.extend((1..points.len() - 1).map(|index| index as f64 / (points.len() - 1) as f64));
    knots.extend([1.0, 1.0]);
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

fn solve_linear_3(mut matrix: [[f64; 3]; 3], mut right: [f64; 3]) -> Option<[f64; 3]> {
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

fn parameter_on_boundary(
    parameters: [f64; 4],
    first: [super::ParameterRange; 2],
    second: [super::ParameterRange; 2],
    tolerance: f64,
) -> bool {
    [first[0], first[1], second[0], second[1]]
        .into_iter()
        .zip(parameters)
        .any(|(range, value)| {
            (value - range.start).abs() <= tolerance || (value - range.end).abs() <= tolerance
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
