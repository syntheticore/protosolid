use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3, Vec4};

use super::{
    DiagnosticKind, DomainBounds, KernelError, KernelResult, ModelTolerance, ParameterDomain,
    ParameterRange,
};

fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn finite_vector(vector: Vec3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}

/// A finite directed axis with a unit direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis3 {
    origin: Point3,
    direction: Vec3,
}

impl Axis3 {
    pub fn new(origin: Point3, direction: Vec3) -> KernelResult<Self> {
        if !finite_point(origin) || !finite_vector(direction) || direction.magnitude2() == 0.0 {
            return Err(invalid(
                "an axis requires a finite origin and nonzero direction",
            ));
        }
        Ok(Self {
            origin,
            direction: direction.normalize(),
        })
    }

    pub fn through(start: Point3, end: Point3) -> KernelResult<Self> {
        Self::new(start, end - start)
    }

    pub fn origin(self) -> Point3 {
        self.origin
    }

    pub fn direction(self) -> Vec3 {
        self.direction
    }

    pub fn point_at(self, distance: f64) -> KernelResult<Point3> {
        if !distance.is_finite() {
            return Err(invalid("axis distance must be finite"));
        }
        Ok(self.origin + self.direction * distance)
    }
}

/// A right-handed orthonormal coordinate frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame3 {
    origin: Point3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
}

impl Frame3 {
    pub fn new(origin: Point3, x: Vec3, y: Vec3, tolerance: ModelTolerance) -> KernelResult<Self> {
        if !finite_point(origin) || !finite_vector(x) || !finite_vector(y) {
            return Err(invalid("coordinate frame must be finite"));
        }
        let x_length = x.magnitude();
        let y_length = y.magnitude();
        if x_length == 0.0
            || y_length == 0.0
            || x.dot(y).abs() > tolerance.angular * x_length * y_length
        {
            return Err(invalid(
                "coordinate frame axes must be nonzero and perpendicular",
            ));
        }
        let x = x / x_length;
        let y = y / y_length;
        Ok(Self {
            origin,
            x,
            y,
            z: x.cross(y).normalize(),
        })
    }

    /// Build a deterministic frame around a desired Z direction.
    pub fn from_z(origin: Point3, z: Vec3) -> KernelResult<Self> {
        let axis = Axis3::new(origin, z)?;
        let z = axis.direction();
        let absolute = [z.x.abs(), z.y.abs(), z.z.abs()];
        let reference = if absolute[0] <= absolute[1] && absolute[0] <= absolute[2] {
            Vec3::unit_x()
        } else if absolute[1] <= absolute[2] {
            Vec3::unit_y()
        } else {
            Vec3::unit_z()
        };
        let x = reference.cross(z).normalize();
        let y = z.cross(x).normalize();
        Ok(Self { origin, x, y, z })
    }

    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn x(self) -> Vec3 {
        self.x
    }
    pub fn y(self) -> Vec3 {
        self.y
    }
    pub fn z(self) -> Vec3 {
        self.z
    }
    pub fn axis(self) -> Axis3 {
        Axis3 {
            origin: self.origin,
            direction: self.z,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve2Evaluation {
    pub point: Point2,
    pub first_derivative: Vec2,
    pub second_derivative: Vec2,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line2 {
    origin: Point2,
    direction: Vec2,
}

impl Line2 {
    pub fn new(origin: Point2, direction: Vec2) -> KernelResult<Self> {
        if !origin.x.is_finite()
            || !origin.y.is_finite()
            || !direction.x.is_finite()
            || !direction.y.is_finite()
            || direction.magnitude2() == 0.0
        {
            return Err(invalid(
                "a 2D line requires a finite origin and nonzero direction",
            ));
        }
        Ok(Self { origin, direction })
    }

    pub fn origin(self) -> Point2 {
        self.origin
    }
    pub fn direction(self) -> Vec2 {
        self.direction
    }

    fn evaluate(self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        Ok(Curve2Evaluation {
            point: self.origin + self.direction * parameter,
            first_derivative: self.direction,
            second_derivative: Vec2::new(0.0, 0.0),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle2 {
    center: Point2,
    radius: f64,
}

impl Circle2 {
    pub fn new(center: Point2, radius: f64) -> KernelResult<Self> {
        if !center.x.is_finite() || !center.y.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return Err(invalid(
                "a 2D circle requires a finite center and positive radius",
            ));
        }
        Ok(Self { center, radius })
    }

    pub fn center(self) -> Point2 {
        self.center
    }
    pub fn radius(self) -> f64 {
        self.radius
    }

    fn evaluate(self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        let (sin, cos) = parameter.sin_cos();
        let radial = Vec2::new(cos, sin) * self.radius;
        Ok(Curve2Evaluation {
            point: self.center + radial,
            first_derivative: Vec2::new(-sin, cos) * self.radius,
            second_derivative: -radial,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve2 {
    degree: usize,
    control_points: Vec<Point2>,
    weights: Vec<f64>,
    knots: Vec<f64>,
}

impl NurbsCurve2 {
    pub fn new(
        degree: usize,
        control_points: Vec<Point2>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    ) -> KernelResult<Self> {
        validate_nurbs(degree, control_points.len(), &weights, &knots)?;
        if control_points
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            return Err(invalid("2D NURBS control points must be finite"));
        }
        Ok(Self {
            degree,
            control_points,
            weights,
            knots,
        })
    }

    pub fn degree(&self) -> usize {
        self.degree
    }
    pub fn control_points(&self) -> &[Point2] {
        &self.control_points
    }
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    pub fn domain(&self) -> ParameterDomain {
        ParameterDomain {
            bounds: DomainBounds::Bounded(ParameterRange {
                start: self.knots[self.degree],
                end: self.knots[self.control_points.len()],
            }),
            period: None,
        }
    }

    fn homogeneous_controls(&self) -> Vec<Vec3> {
        self.control_points
            .iter()
            .zip(&self.weights)
            .map(|(point, weight)| Vec3::new(point.x * weight, point.y * weight, *weight))
            .collect()
    }

    fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        let start = self.knots[self.degree];
        let end = self.knots[self.control_points.len()];
        if parameter < start || parameter > end {
            return Err(invalid("NURBS parameter lies outside its natural domain"));
        }
        let controls = self.homogeneous_controls();
        let value = de_boor_vec3(self.degree, &self.knots, &controls, parameter)?;
        if value.z == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "2D NURBS has zero homogeneous weight",
            ));
        }
        let first_curve = derivative_controls3(self.degree, &self.knots, &controls);
        let first = if let Some((degree, knots, controls)) = first_curve.as_ref() {
            de_boor_vec3(*degree, knots, controls, parameter)?
        } else {
            Vec3::new(0.0, 0.0, 0.0)
        };
        let second_curve = first_curve
            .as_ref()
            .and_then(|(degree, knots, controls)| derivative_controls3(*degree, knots, controls));
        let second = if let Some((degree, knots, controls)) = second_curve.as_ref() {
            de_boor_vec3(*degree, knots, controls, parameter)?
        } else {
            Vec3::new(0.0, 0.0, 0.0)
        };
        let point_vector = value.truncate() / value.z;
        let first_derivative = (first.truncate() - point_vector * first.z) / value.z;
        let second_derivative =
            (second.truncate() - first_derivative * (2.0 * first.z) - point_vector * second.z)
                / value.z;
        Ok(Curve2Evaluation {
            point: Point2::from_vec(point_vector),
            first_derivative,
            second_derivative,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Curve2 {
    Line(Line2),
    Circle(Circle2),
    Nurbs(NurbsCurve2),
}

impl Curve2 {
    pub fn domain(&self) -> ParameterDomain {
        match self {
            Self::Line(_) => ParameterDomain::unbounded(),
            Self::Circle(_) => ParameterDomain {
                bounds: DomainBounds::Bounded(ParameterRange {
                    start: 0.0,
                    end: std::f64::consts::TAU,
                }),
                period: Some(std::f64::consts::TAU),
            },
            Self::Nurbs(curve) => curve.domain(),
        }
    }

    pub fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        match self {
            Self::Line(line) => line.evaluate(parameter),
            Self::Circle(circle) => circle.evaluate(parameter),
            Self::Nurbs(curve) => curve.evaluate(parameter),
        }
    }

    /// Contribution to signed UV area using Green's theorem.
    pub fn signed_area_contribution(
        &self,
        range: ParameterRange,
        tolerance: f64,
    ) -> KernelResult<f64> {
        match self {
            Self::Line(line) => {
                let cross = line.origin.x * line.direction.y - line.origin.y * line.direction.x;
                Ok(0.5 * cross * range.span())
            }
            Self::Circle(circle) => {
                let start = range.start;
                let end = range.end;
                let integral = circle.center.x * circle.radius * (end.sin() - start.sin())
                    + circle.center.y * circle.radius * (start.cos() - end.cos())
                    + circle.radius * circle.radius * range.span();
                Ok(0.5 * integral)
            }
            Self::Nurbs(curve) => {
                let integrand = |parameter: f64| -> KernelResult<f64> {
                    let evaluation = self.evaluate(parameter)?;
                    Ok(0.5
                        * (evaluation.point.x * evaluation.first_derivative.y
                            - evaluation.point.y * evaluation.first_derivative.x))
                };
                let coordinate_scale = curve
                    .control_points()
                    .iter()
                    .map(|point| point.x.abs().max(point.y.abs()))
                    .fold(1.0_f64, f64::max);
                let absolute_tolerance =
                    tolerance.max(f64::EPSILON * coordinate_scale.powi(2) * 4096.0);
                let mut breaks = curve
                    .knots()
                    .iter()
                    .copied()
                    .filter(|value| {
                        *value >= range.start.min(range.end) && *value <= range.start.max(range.end)
                    })
                    .collect::<Vec<_>>();
                breaks.extend([range.start, range.end]);
                breaks.sort_by(f64::total_cmp);
                breaks.dedup_by(|first, second| (*first - *second).abs() <= f64::EPSILON * 64.0);
                if range.span() < 0.0 {
                    breaks.reverse();
                }
                let interval_tolerance = absolute_tolerance / breaks.len().max(2) as f64;
                breaks.windows(2).try_fold(0.0, |sum, interval| {
                    if (interval[1] - interval[0]).abs() <= f64::EPSILON * 64.0 {
                        Ok(sum)
                    } else {
                        Ok(sum
                            + adaptive_simpson(
                                &integrand,
                                interval[0],
                                interval[1],
                                interval_tolerance,
                                24,
                            )?)
                    }
                })
            }
        }
    }
}

fn adaptive_simpson<F>(
    function: &F,
    start: f64,
    end: f64,
    tolerance: f64,
    maximum_depth: u8,
) -> KernelResult<f64>
where
    F: Fn(f64) -> KernelResult<f64>,
{
    let middle = (start + end) * 0.5;
    let start_value = function(start)?;
    let middle_value = function(middle)?;
    let end_value = function(end)?;
    let whole = (end - start) * (start_value + 4.0 * middle_value + end_value) / 6.0;
    adaptive_simpson_interval(
        function,
        start,
        end,
        start_value,
        middle_value,
        end_value,
        whole,
        tolerance,
        0,
        maximum_depth,
    )
}

#[allow(clippy::too_many_arguments)]
fn adaptive_simpson_interval<F>(
    function: &F,
    start: f64,
    end: f64,
    start_value: f64,
    middle_value: f64,
    end_value: f64,
    whole: f64,
    tolerance: f64,
    depth: u8,
    maximum_depth: u8,
) -> KernelResult<f64>
where
    F: Fn(f64) -> KernelResult<f64>,
{
    let middle = (start + end) * 0.5;
    let left_middle = (start + middle) * 0.5;
    let right_middle = (middle + end) * 0.5;
    let left_middle_value = function(left_middle)?;
    let right_middle_value = function(right_middle)?;
    let left = (middle - start) * (start_value + 4.0 * left_middle_value + middle_value) / 6.0;
    let right = (end - middle) * (middle_value + 4.0 * right_middle_value + end_value) / 6.0;
    let error = (left + right - whole).abs() / 15.0;
    if error <= tolerance {
        return Ok(left + right + (left + right - whole) / 15.0);
    }
    if depth >= maximum_depth {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "adaptive p-curve area integration did not converge",
        ));
    }
    Ok(adaptive_simpson_interval(
        function,
        start,
        middle,
        start_value,
        left_middle_value,
        middle_value,
        left,
        tolerance * 0.5,
        depth + 1,
        maximum_depth,
    )? + adaptive_simpson_interval(
        function,
        middle,
        end,
        middle_value,
        right_middle_value,
        end_value,
        right,
        tolerance * 0.5,
        depth + 1,
        maximum_depth,
    )?)
}

impl From<Line2> for Curve2 {
    fn from(value: Line2) -> Self {
        Self::Line(value)
    }
}
impl From<Circle2> for Curve2 {
    fn from(value: Circle2) -> Self {
        Self::Circle(value)
    }
}
impl From<NurbsCurve2> for Curve2 {
    fn from(value: NurbsCurve2) -> Self {
        Self::Nurbs(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveEvaluation {
    pub point: Point3,
    pub first_derivative: Vec3,
    pub second_derivative: Vec3,
}

impl CurveEvaluation {
    pub fn tangent(self) -> KernelResult<Vec3> {
        if self.first_derivative.magnitude2() == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "curve tangent is singular at the requested parameter",
            ));
        }
        Ok(self.first_derivative.normalize())
    }

    pub fn curvature(self) -> KernelResult<f64> {
        let speed = self.first_derivative.magnitude();
        if speed == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "curve curvature is undefined at a singular parameter",
            ));
        }
        Ok(self
            .first_derivative
            .cross(self.second_derivative)
            .magnitude()
            / speed.powi(3))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceEvaluation {
    pub point: Point3,
    pub du: Vec3,
    pub dv: Vec3,
    pub duu: Vec3,
    pub duv: Vec3,
    pub dvv: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceCurvature {
    pub gaussian: f64,
    pub mean: f64,
    pub principal: [f64; 2],
}

impl SurfaceEvaluation {
    pub fn normal(self) -> KernelResult<Vec3> {
        let normal = self.du.cross(self.dv);
        if normal.magnitude2() == 0.0 || !finite_vector(normal) {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "surface derivatives are singular at the requested parameter",
            ));
        }
        Ok(normal.normalize())
    }

    pub fn curvature(self) -> KernelResult<SurfaceCurvature> {
        let normal = self.normal()?;
        let first = [
            self.du.dot(self.du),
            self.du.dot(self.dv),
            self.dv.dot(self.dv),
        ];
        let determinant = first[0] * first[2] - first[1] * first[1];
        if determinant <= 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "surface first fundamental form is singular",
            ));
        }
        let second = [
            normal.dot(self.duu),
            normal.dot(self.duv),
            normal.dot(self.dvv),
        ];
        let gaussian = (second[0] * second[2] - second[1] * second[1]) / determinant;
        let mean = (first[0] * second[2] - 2.0 * first[1] * second[1] + first[2] * second[0])
            / (2.0 * determinant);
        let discriminant = (mean * mean - gaussian).max(0.0).sqrt();
        Ok(SurfaceCurvature {
            gaussian,
            mean,
            principal: [mean - discriminant, mean + discriminant],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection1 {
    pub parameter: f64,
    pub point: Point3,
    pub distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection2 {
    pub parameters: (f64, f64),
    pub point: Point3,
    pub distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line3 {
    origin: Point3,
    direction: Vec3,
}

impl Line3 {
    pub fn new(origin: Point3, direction: Vec3) -> KernelResult<Self> {
        if !finite_point(origin) || !finite_vector(direction) || direction.magnitude2() == 0.0 {
            return Err(invalid(
                "a line requires a finite origin and nonzero finite direction",
            ));
        }
        Ok(Self { origin, direction })
    }

    pub fn through(start: Point3, end: Point3) -> KernelResult<Self> {
        Self::new(start, end - start)
    }

    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn direction(self) -> Vec3 {
        self.direction
    }

    pub(crate) fn evaluate(self, parameter: f64) -> KernelResult<CurveEvaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        Ok(CurveEvaluation {
            point: self.origin + self.direction * parameter,
            first_derivative: self.direction,
            second_derivative: Vec3::new(0.0, 0.0, 0.0),
        })
    }

    pub(crate) fn project(self, point: Point3) -> KernelResult<Projection1> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let parameter = (point - self.origin).dot(self.direction) / self.direction.magnitude2();
        let projected = self.origin + self.direction * parameter;
        Ok(Projection1 {
            parameter,
            point: projected,
            distance: projected.distance(point),
        })
    }
}

/// An exact circle stored as two radius vectors. This retains its parameter frame
/// and avoids silently choosing a new seam during regeneration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle3 {
    center: Point3,
    x_radius: Vec3,
    y_radius: Vec3,
}

/// Validated non-uniform rational B-spline curve in homogeneous form.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve3 {
    degree: usize,
    control_points: Vec<Point3>,
    weights: Vec<f64>,
    knots: Vec<f64>,
}

impl NurbsCurve3 {
    pub fn new(
        degree: usize,
        control_points: Vec<Point3>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    ) -> KernelResult<Self> {
        validate_nurbs(degree, control_points.len(), &weights, &knots)?;
        if control_points.iter().any(|point| !finite_point(*point)) {
            return Err(invalid("NURBS control points must be finite"));
        }
        Ok(Self {
            degree,
            control_points,
            weights,
            knots,
        })
    }

    pub fn degree(&self) -> usize {
        self.degree
    }
    pub fn control_points(&self) -> &[Point3] {
        &self.control_points
    }
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    pub fn domain(&self) -> ParameterDomain {
        ParameterDomain {
            bounds: DomainBounds::Bounded(ParameterRange {
                start: self.knots[self.degree],
                end: self.knots[self.control_points.len()],
            }),
            period: None,
        }
    }

    fn homogeneous_controls(&self) -> Vec<Vec4> {
        self.control_points
            .iter()
            .zip(&self.weights)
            .map(|(point, weight)| {
                Vec4::new(
                    point.x * weight,
                    point.y * weight,
                    point.z * weight,
                    *weight,
                )
            })
            .collect()
    }

    fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        let domain_start = self.knots[self.degree];
        let domain_end = self.knots[self.control_points.len()];
        if parameter < domain_start || parameter > domain_end {
            return Err(invalid("NURBS parameter lies outside its natural domain"));
        }
        let controls = self.homogeneous_controls();
        let value = de_boor_vec4(self.degree, &self.knots, &controls, parameter)?;
        if value.w == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "NURBS has zero homogeneous weight",
            ));
        }
        let first_curve = derivative_controls(self.degree, &self.knots, &controls);
        let first = if let Some((degree, knots, controls)) = first_curve.as_ref() {
            de_boor_vec4(*degree, knots, controls, parameter)?
        } else {
            Vec4::new(0.0, 0.0, 0.0, 0.0)
        };
        let second_curve = first_curve
            .as_ref()
            .and_then(|(degree, knots, controls)| derivative_controls(*degree, knots, controls));
        let second = if let Some((degree, knots, controls)) = second_curve.as_ref() {
            de_boor_vec4(*degree, knots, controls, parameter)?
        } else {
            Vec4::new(0.0, 0.0, 0.0, 0.0)
        };

        let point_vector = value.truncate() / value.w;
        let first_derivative = (first.truncate() - point_vector * first.w) / value.w;
        let second_derivative =
            (second.truncate() - first_derivative * (2.0 * first.w) - point_vector * second.w)
                / value.w;
        Ok(CurveEvaluation {
            point: Point3::from_vec(point_vector),
            first_derivative,
            second_derivative,
        })
    }

    fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection1> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let start = self.knots[self.degree];
        let end = self.knots[self.control_points.len()];
        let mut candidates = vec![start, end];
        for span in self.knots.windows(2) {
            let low = span[0].max(start);
            let high = span[1].min(end);
            if high > low {
                for index in 1..=4 {
                    candidates.push(low + (high - low) * index as f64 / 5.0);
                }
            }
        }
        let mut parameter = candidates
            .into_iter()
            .map(|candidate| {
                self.evaluate(candidate)
                    .map(|evaluation| (candidate, evaluation.point.distance2(point)))
            })
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|candidate| candidate.0)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "NURBS projection has no initial candidate",
                )
            })?;

        let mut converged = false;
        for _ in 0..64 {
            let evaluation = self.evaluate(parameter)?;
            let residual = evaluation.point - point;
            let function = residual.dot(evaluation.first_derivative);
            let derivative = evaluation.first_derivative.magnitude2()
                + residual.dot(evaluation.second_derivative);
            if derivative.abs() <= f64::EPSILON {
                break;
            }
            let previous_distance = residual.magnitude2();
            let mut step = -function / derivative;
            let mut next = (parameter + step).clamp(start, end);
            for _ in 0..12 {
                let next_distance = self.evaluate(next)?.point.distance2(point);
                if next_distance <= previous_distance {
                    break;
                }
                step *= 0.5;
                next = (parameter + step).clamp(start, end);
            }
            if (next - parameter).abs() <= tolerance.parameter {
                parameter = next;
                converged = true;
                break;
            }
            parameter = next;
        }
        if !converged {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "NURBS closest-point refinement did not converge",
            ));
        }
        let projected = self.evaluate(parameter)?.point;
        Ok(Projection1 {
            parameter,
            point: projected,
            distance: projected.distance(point),
        })
    }
}

fn validate_nurbs(
    degree: usize,
    control_count: usize,
    weights: &[f64],
    knots: &[f64],
) -> KernelResult<()> {
    if degree == 0 || control_count < degree + 1 {
        return Err(invalid(
            "NURBS degree must be positive and smaller than its control-point count",
        ));
    }
    if weights.len() != control_count
        || weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
    {
        return Err(invalid(
            "NURBS requires one finite positive weight per control point",
        ));
    }
    if knots.len() != control_count + degree + 1
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Err(invalid(
            "NURBS knot vector has invalid length, values, or ordering",
        ));
    }
    let mut run = 1usize;
    for pair in knots.windows(2) {
        if pair[0] == pair[1] {
            run += 1;
            if run > degree + 1 {
                return Err(invalid(
                    "NURBS knot multiplicity may not exceed degree plus one",
                ));
            }
        } else {
            run = 1;
        }
    }
    if knots[degree] >= knots[control_count] {
        return Err(invalid(
            "NURBS active parameter domain must have positive length",
        ));
    }
    Ok(())
}

fn find_span(degree: usize, knots: &[f64], control_count: usize, parameter: f64) -> usize {
    let last = control_count - 1;
    if parameter >= knots[control_count] {
        return last;
    }
    let mut low = degree;
    let mut high = control_count;
    let mut middle = (low + high) / 2;
    while parameter < knots[middle] || parameter >= knots[middle + 1] {
        if parameter < knots[middle] {
            high = middle;
        } else {
            low = middle;
        }
        middle = (low + high) / 2;
    }
    middle
}

fn de_boor_vec4(
    degree: usize,
    knots: &[f64],
    controls: &[Vec4],
    parameter: f64,
) -> KernelResult<Vec4> {
    if degree == 0 {
        let span = find_span(0, knots, controls.len(), parameter);
        return Ok(controls[span]);
    }
    let span = find_span(degree, knots, controls.len(), parameter);
    let mut values: Vec<Vec4> = (0..=degree).map(|j| controls[span - degree + j]).collect();
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let index = span - degree + j;
            let denominator = knots[index + degree + 1 - level] - knots[index];
            if denominator == 0.0 {
                values[j] = values[j - 1];
            } else {
                let alpha = (parameter - knots[index]) / denominator;
                values[j] = values[j - 1] * (1.0 - alpha) + values[j] * alpha;
            }
        }
    }
    let result = values[degree];
    if !result.x.is_finite()
        || !result.y.is_finite()
        || !result.z.is_finite()
        || !result.w.is_finite()
    {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "NURBS evaluation produced a non-finite value",
        ));
    }
    Ok(result)
}

fn derivative_controls(
    degree: usize,
    knots: &[f64],
    controls: &[Vec4],
) -> Option<(usize, Vec<f64>, Vec<Vec4>)> {
    if degree == 0 || controls.len() < 2 {
        return None;
    }
    let derivative = (0..controls.len() - 1)
        .map(|index| {
            let denominator = knots[index + degree + 1] - knots[index + 1];
            if denominator == 0.0 {
                Vec4::new(0.0, 0.0, 0.0, 0.0)
            } else {
                (controls[index + 1] - controls[index]) * (degree as f64 / denominator)
            }
        })
        .collect();
    Some((degree - 1, knots[1..knots.len() - 1].to_vec(), derivative))
}

fn de_boor_vec3(
    degree: usize,
    knots: &[f64],
    controls: &[Vec3],
    parameter: f64,
) -> KernelResult<Vec3> {
    if degree == 0 {
        let span = find_span(0, knots, controls.len(), parameter);
        return Ok(controls[span]);
    }
    let span = find_span(degree, knots, controls.len(), parameter);
    let mut values: Vec<Vec3> = (0..=degree).map(|j| controls[span - degree + j]).collect();
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let index = span - degree + j;
            let denominator = knots[index + degree + 1 - level] - knots[index];
            if denominator == 0.0 {
                values[j] = values[j - 1];
            } else {
                let alpha = (parameter - knots[index]) / denominator;
                values[j] = values[j - 1] * (1.0 - alpha) + values[j] * alpha;
            }
        }
    }
    let result = values[degree];
    if !finite_vector(result) {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "2D NURBS evaluation produced a non-finite value",
        ));
    }
    Ok(result)
}

fn derivative_controls3(
    degree: usize,
    knots: &[f64],
    controls: &[Vec3],
) -> Option<(usize, Vec<f64>, Vec<Vec3>)> {
    if degree == 0 || controls.len() < 2 {
        return None;
    }
    let derivative = (0..controls.len() - 1)
        .map(|index| {
            let denominator = knots[index + degree + 1] - knots[index + 1];
            if denominator == 0.0 {
                Vec3::new(0.0, 0.0, 0.0)
            } else {
                (controls[index + 1] - controls[index]) * (degree as f64 / denominator)
            }
        })
        .collect();
    Some((degree - 1, knots[1..knots.len() - 1].to_vec(), derivative))
}

impl Circle3 {
    pub fn new(
        center: Point3,
        x_radius: Vec3,
        y_radius: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !finite_point(center) || !finite_vector(x_radius) || !finite_vector(y_radius) {
            return Err(invalid("circle frame must be finite"));
        }
        let x_length = x_radius.magnitude();
        let y_length = y_radius.magnitude();
        let scale = x_length.max(y_length);
        if x_length <= tolerance.linear_at_scale(scale)
            || y_length <= tolerance.linear_at_scale(scale)
        {
            return Err(invalid("circle radius must exceed model tolerance"));
        }
        if (x_length - y_length).abs() > tolerance.linear_at_scale(scale)
            || (x_radius.angle(y_radius).0 - std::f64::consts::FRAC_PI_2).abs() > tolerance.angular
        {
            return Err(invalid(
                "circle radius vectors must be perpendicular and equally long",
            ));
        }
        Ok(Self {
            center,
            x_radius,
            y_radius,
        })
    }

    pub fn from_axis(axis: Axis3, radius: f64, tolerance: ModelTolerance) -> KernelResult<Self> {
        if !radius.is_finite() || radius <= tolerance.linear_at_scale(radius) {
            return Err(invalid("circle radius must exceed model tolerance"));
        }
        let frame = Frame3::from_z(axis.origin(), axis.direction())?;
        Self::new(
            axis.origin(),
            frame.x() * radius,
            frame.y() * radius,
            tolerance,
        )
    }

    pub fn center(self) -> Point3 {
        self.center
    }
    pub fn radius(self) -> f64 {
        self.x_radius.magnitude()
    }
    pub fn x_radius(self) -> Vec3 {
        self.x_radius
    }
    pub fn y_radius(self) -> Vec3 {
        self.y_radius
    }
    pub fn normal(self) -> Vec3 {
        self.x_radius.cross(self.y_radius).normalize()
    }

    pub(crate) fn evaluate(self, parameter: f64) -> KernelResult<CurveEvaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        let (sin, cos) = parameter.sin_cos();
        let radial = self.x_radius * cos + self.y_radius * sin;
        let tangent = -self.x_radius * sin + self.y_radius * cos;
        Ok(CurveEvaluation {
            point: self.center + radial,
            first_derivative: tangent,
            second_derivative: -radial,
        })
    }

    pub(crate) fn project(self, point: Point3) -> KernelResult<Projection1> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.center;
        let x = delta.dot(self.x_radius) / self.x_radius.magnitude2();
        let y = delta.dot(self.y_radius) / self.y_radius.magnitude2();
        if x == 0.0 && y == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "circle projection is not unique on its axis",
            ));
        }
        let parameter = y.atan2(x).rem_euclid(std::f64::consts::TAU);
        let projected = self.evaluate(parameter)?.point;
        Ok(Projection1 {
            parameter,
            point: projected,
            distance: projected.distance(point),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Curve3 {
    Line(Line3),
    Circle(Circle3),
    Nurbs(NurbsCurve3),
}

impl Curve3 {
    pub fn domain(&self) -> ParameterDomain {
        match self {
            Self::Line(_) => ParameterDomain::unbounded(),
            Self::Circle(_) => ParameterDomain {
                bounds: DomainBounds::Bounded(ParameterRange {
                    start: 0.0,
                    end: std::f64::consts::TAU,
                }),
                period: Some(std::f64::consts::TAU),
            },
            Self::Nurbs(curve) => curve.domain(),
        }
    }

    pub fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        match self {
            Self::Line(line) => line.evaluate(parameter),
            Self::Circle(circle) => circle.evaluate(parameter),
            Self::Nurbs(curve) => curve.evaluate(parameter),
        }
    }

    pub fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection1> {
        match self {
            Self::Line(line) => line.project(point),
            Self::Circle(circle) => circle.project(point),
            Self::Nurbs(curve) => curve.project(point, tolerance),
        }
    }
}

impl From<Line3> for Curve3 {
    fn from(value: Line3) -> Self {
        Self::Line(value)
    }
}
impl From<Circle3> for Curve3 {
    fn from(value: Circle3) -> Self {
        Self::Circle(value)
    }
}
impl From<NurbsCurve3> for Curve3 {
    fn from(value: NurbsCurve3) -> Self {
        Self::Nurbs(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane3 {
    origin: Point3,
    u: Vec3,
    v: Vec3,
}

impl Plane3 {
    pub fn xy() -> Self {
        Self {
            origin: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
        }
    }

    pub fn xz() -> Self {
        Self {
            origin: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 0.0, 1.0),
        }
    }

    pub fn yz() -> Self {
        Self {
            origin: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(0.0, 1.0, 0.0),
            v: Vec3::new(0.0, 0.0, 1.0),
        }
    }

    pub fn new(origin: Point3, u: Vec3, v: Vec3, tolerance: ModelTolerance) -> KernelResult<Self> {
        if !finite_point(origin) || !finite_vector(u) || !finite_vector(v) {
            return Err(invalid("plane frame must be finite"));
        }
        let normal = u.cross(v);
        let scale = u.magnitude().max(v.magnitude());
        if normal.magnitude() <= tolerance.linear_at_scale(scale).powi(2) {
            return Err(invalid("plane parameter axes must be linearly independent"));
        }
        Ok(Self { origin, u, v })
    }

    pub fn from_frame(frame: Frame3) -> Self {
        Self {
            origin: frame.origin(),
            u: frame.x(),
            v: frame.y(),
        }
    }

    pub fn from_origin_normal(origin: Point3, normal: Vec3) -> KernelResult<Self> {
        Ok(Self::from_frame(Frame3::from_z(origin, normal)?))
    }

    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn u(self) -> Vec3 {
        self.u
    }
    pub fn v(self) -> Vec3 {
        self.v
    }
    pub fn normal(self) -> Vec3 {
        self.u.cross(self.v).normalize()
    }

    pub(crate) fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let zero = Vec3::new(0.0, 0.0, 0.0);
        Ok(SurfaceEvaluation {
            point: self.origin + self.u * u + self.v * v,
            du: self.u,
            dv: self.v,
            duu: zero,
            duv: zero,
            dvv: zero,
        })
    }

    pub(crate) fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.origin;
        let uu = self.u.dot(self.u);
        let uv = self.u.dot(self.v);
        let vv = self.v.dot(self.v);
        let determinant = uu * vv - uv * uv;
        if determinant == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "plane frame is singular",
            ));
        }
        let du = delta.dot(self.u);
        let dv = delta.dot(self.v);
        let u = (du * vv - dv * uv) / determinant;
        let v = (dv * uu - du * uv) / determinant;
        let projected = self.origin + self.u * u + self.v * v;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder3 {
    origin: Point3,
    axis: Vec3,
    x_radius: Vec3,
    y_radius: Vec3,
}

/// Circular cone parameterized from its apex (`v = 0`) through a reference
/// circle (`v = 1`). Trims select the finite sheet used by topology.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cone3 {
    apex: Point3,
    axis: Vec3,
    x_radius: Vec3,
    y_radius: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere3 {
    center: Point3,
    x_radius: Vec3,
    y_radius: Vec3,
    z_radius: Vec3,
}

/// A regular ring torus. The minor radius is strictly smaller than the major
/// radius so the natural surface cannot self-intersect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Torus3 {
    center: Point3,
    x_major: Vec3,
    y_major: Vec3,
    minor_radius: f64,
}

/// Validated tensor-product rational B-spline surface.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurface3 {
    degrees: (usize, usize),
    control_points: Vec<Vec<Point3>>,
    weights: Vec<Vec<f64>>,
    knots: (Vec<f64>, Vec<f64>),
}

impl NurbsSurface3 {
    pub fn new(
        degrees: (usize, usize),
        control_points: Vec<Vec<Point3>>,
        weights: Vec<Vec<f64>>,
        knots: (Vec<f64>, Vec<f64>),
    ) -> KernelResult<Self> {
        if control_points.is_empty() || control_points[0].is_empty() {
            return Err(invalid("NURBS surface control net may not be empty"));
        }
        let v_count = control_points.len();
        let u_count = control_points[0].len();
        if control_points.iter().any(|row| row.len() != u_count)
            || weights.len() != v_count
            || weights.iter().any(|row| row.len() != u_count)
        {
            return Err(invalid(
                "NURBS surface control points and weights must form matching rectangles",
            ));
        }
        let flat_weights: Vec<f64> = weights.iter().flatten().copied().collect();
        validate_nurbs(degrees.0, u_count, &flat_weights[..u_count], &knots.0)?;
        validate_nurbs(
            degrees.1,
            v_count,
            &weights.iter().map(|row| row[0]).collect::<Vec<_>>(),
            &knots.1,
        )?;
        if flat_weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
            || control_points
                .iter()
                .flatten()
                .any(|point| !finite_point(*point))
        {
            return Err(invalid(
                "NURBS surface controls and weights must be finite with positive weights",
            ));
        }
        Ok(Self {
            degrees,
            control_points,
            weights,
            knots,
        })
    }

    pub fn degrees(&self) -> (usize, usize) {
        self.degrees
    }
    pub fn control_points(&self) -> &[Vec<Point3>] {
        &self.control_points
    }
    pub fn weights(&self) -> &[Vec<f64>] {
        &self.weights
    }
    pub fn knots(&self) -> (&[f64], &[f64]) {
        (&self.knots.0, &self.knots.1)
    }

    pub fn domains(&self) -> [ParameterDomain; 2] {
        [
            ParameterDomain {
                bounds: DomainBounds::Bounded(ParameterRange {
                    start: self.knots.0[self.degrees.0],
                    end: self.knots.0[self.control_points[0].len()],
                }),
                period: None,
            },
            ParameterDomain {
                bounds: DomainBounds::Bounded(ParameterRange {
                    start: self.knots.1[self.degrees.1],
                    end: self.knots.1[self.control_points.len()],
                }),
                period: None,
            },
        ]
    }

    fn homogeneous_rows(&self) -> Vec<Vec<Vec4>> {
        self.control_points
            .iter()
            .zip(&self.weights)
            .map(|(points, weights)| {
                points
                    .iter()
                    .zip(weights)
                    .map(|(point, weight)| {
                        Vec4::new(
                            point.x * weight,
                            point.y * weight,
                            point.z * weight,
                            *weight,
                        )
                    })
                    .collect()
            })
            .collect()
    }

    fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        let domains = self.domains();
        let in_domain = |domain: ParameterDomain, value: f64| match domain.bounds {
            super::DomainBounds::Bounded(range) => range.start <= value && value <= range.end,
            _ => true,
        };
        if !u.is_finite()
            || !v.is_finite()
            || !in_domain(domains[0], u)
            || !in_domain(domains[1], v)
        {
            return Err(invalid(
                "NURBS surface parameters lie outside its natural domain",
            ));
        }
        let rows = self.homogeneous_rows();
        let along_u: Vec<Vec4> = rows
            .iter()
            .map(|row| de_boor_vec4(self.degrees.0, &self.knots.0, row, u))
            .collect::<KernelResult<_>>()?;
        let value = de_boor_vec4(self.degrees.1, &self.knots.1, &along_u, v)?;

        let u_rows: Vec<Vec4> = rows
            .iter()
            .map(|row| evaluate_derivative_vec4(self.degrees.0, &self.knots.0, row, u, 1))
            .collect::<KernelResult<_>>()?;
        let uu_rows: Vec<Vec4> = rows
            .iter()
            .map(|row| evaluate_derivative_vec4(self.degrees.0, &self.knots.0, row, u, 2))
            .collect::<KernelResult<_>>()?;
        let du = de_boor_vec4(self.degrees.1, &self.knots.1, &u_rows, v)?;
        let duu = de_boor_vec4(self.degrees.1, &self.knots.1, &uu_rows, v)?;
        let dv = evaluate_derivative_vec4(self.degrees.1, &self.knots.1, &along_u, v, 1)?;
        let dvv = evaluate_derivative_vec4(self.degrees.1, &self.knots.1, &along_u, v, 2)?;
        let duv = evaluate_derivative_vec4(self.degrees.1, &self.knots.1, &u_rows, v, 1)?;
        if value.w == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "NURBS surface has zero homogeneous weight",
            ));
        }

        let point = value.truncate() / value.w;
        let rational_du = (du.truncate() - point * du.w) / value.w;
        let rational_dv = (dv.truncate() - point * dv.w) / value.w;
        let rational_duu = (duu.truncate() - rational_du * (2.0 * du.w) - point * duu.w) / value.w;
        let rational_duv =
            (duv.truncate() - rational_du * dv.w - rational_dv * du.w - point * duv.w) / value.w;
        let rational_dvv = (dvv.truncate() - rational_dv * (2.0 * dv.w) - point * dvv.w) / value.w;
        Ok(SurfaceEvaluation {
            point: Point3::from_vec(point),
            du: rational_du,
            dv: rational_dv,
            duu: rational_duu,
            duv: rational_duv,
            dvv: rational_dvv,
        })
    }

    fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let domains = self.domains();
        let super::DomainBounds::Bounded(u_range) = domains[0].bounds else {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "NURBS surface has an unbounded U domain",
            ));
        };
        let super::DomainBounds::Bounded(v_range) = domains[1].bounds else {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "NURBS surface has an unbounded V domain",
            ));
        };
        let axis_candidates = |knots: &[f64], range: super::ParameterRange| {
            let mut values = vec![range.start, range.end];
            for span in knots.windows(2) {
                let low = span[0].max(range.start);
                let high = span[1].min(range.end);
                if high > low {
                    values.push((low + high) * 0.5);
                }
            }
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            values.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
            values
        };
        let u_candidates = axis_candidates(&self.knots.0, u_range);
        let v_candidates = axis_candidates(&self.knots.1, v_range);
        let mut best: Option<((f64, f64), f64)> = None;
        for u in &u_candidates {
            for v in &v_candidates {
                let distance = self.evaluate(*u, *v)?.point.distance2(point);
                if best.map(|candidate| distance < candidate.1).unwrap_or(true) {
                    best = Some(((*u, *v), distance));
                }
            }
        }
        let (mut u, mut v) = best
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "NURBS surface projection has no initial candidate",
                )
            })?
            .0;

        let mut converged = false;
        for _ in 0..80 {
            let evaluation = self.evaluate(u, v)?;
            let residual = evaluation.point - point;
            let gradient = [residual.dot(evaluation.du), residual.dot(evaluation.dv)];
            let h00 = evaluation.du.magnitude2() + residual.dot(evaluation.duu);
            let h01 = evaluation.du.dot(evaluation.dv) + residual.dot(evaluation.duv);
            let h11 = evaluation.dv.magnitude2() + residual.dot(evaluation.dvv);
            let determinant = h00 * h11 - h01 * h01;
            if determinant.abs() <= f64::EPSILON {
                break;
            }
            let mut du = (-gradient[0] * h11 + gradient[1] * h01) / determinant;
            let mut dv = (-gradient[1] * h00 + gradient[0] * h01) / determinant;
            let previous_distance = residual.magnitude2();
            let mut next_u = (u + du).clamp(u_range.start, u_range.end);
            let mut next_v = (v + dv).clamp(v_range.start, v_range.end);
            for _ in 0..12 {
                let next_distance = self.evaluate(next_u, next_v)?.point.distance2(point);
                if next_distance <= previous_distance {
                    break;
                }
                du *= 0.5;
                dv *= 0.5;
                next_u = (u + du).clamp(u_range.start, u_range.end);
                next_v = (v + dv).clamp(v_range.start, v_range.end);
            }
            if (next_u - u).abs() <= tolerance.parameter
                && (next_v - v).abs() <= tolerance.parameter
            {
                u = next_u;
                v = next_v;
                converged = true;
                break;
            }
            u = next_u;
            v = next_v;
        }
        if !converged {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "NURBS surface closest-point refinement did not converge",
            ));
        }
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

fn evaluate_derivative_vec4(
    degree: usize,
    knots: &[f64],
    controls: &[Vec4],
    parameter: f64,
    order: usize,
) -> KernelResult<Vec4> {
    if order == 0 {
        return de_boor_vec4(degree, knots, controls, parameter);
    }
    let mut current_degree = degree;
    let mut current_knots = knots.to_vec();
    let mut current_controls = controls.to_vec();
    for _ in 0..order {
        let Some((next_degree, next_knots, next_controls)) =
            derivative_controls(current_degree, &current_knots, &current_controls)
        else {
            return Ok(Vec4::new(0.0, 0.0, 0.0, 0.0));
        };
        current_degree = next_degree;
        current_knots = next_knots;
        current_controls = next_controls;
    }
    de_boor_vec4(current_degree, &current_knots, &current_controls, parameter)
}

impl Cylinder3 {
    pub fn new(
        origin: Point3,
        axis: Vec3,
        x_radius: Vec3,
        y_radius: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let circle = Circle3::new(origin, x_radius, y_radius, tolerance)?;
        if !finite_vector(axis) || axis.magnitude() <= tolerance.linear {
            return Err(invalid("cylinder axis must be finite and nonzero"));
        }
        if axis
            .angle(circle.normal())
            .0
            .min(axis.angle(-circle.normal()).0)
            > tolerance.angular
        {
            return Err(invalid(
                "cylinder axis must be perpendicular to its radial frame",
            ));
        }
        Ok(Self {
            origin,
            axis,
            x_radius,
            y_radius,
        })
    }

    pub fn from_axis(
        axis: Axis3,
        axial_scale: f64,
        radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !axial_scale.is_finite()
            || axial_scale.abs() <= tolerance.linear_at_scale(axial_scale)
            || !radius.is_finite()
            || radius <= tolerance.linear_at_scale(radius)
        {
            return Err(invalid("cylinder axial scale must exceed model tolerance"));
        }
        let frame = Frame3::from_z(axis.origin(), axis.direction())?;
        Self::new(
            axis.origin(),
            axis.direction() * axial_scale,
            frame.x() * radius,
            frame.y() * radius,
            tolerance,
        )
    }

    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn axis(self) -> Vec3 {
        self.axis
    }
    pub fn radius(self) -> f64 {
        self.x_radius.magnitude()
    }
    pub fn x_radius(self) -> Vec3 {
        self.x_radius
    }
    pub fn y_radius(self) -> Vec3 {
        self.y_radius
    }

    pub(crate) fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let (sin, cos) = u.sin_cos();
        let radial = self.x_radius * cos + self.y_radius * sin;
        let tangent = -self.x_radius * sin + self.y_radius * cos;
        let zero = Vec3::new(0.0, 0.0, 0.0);
        Ok(SurfaceEvaluation {
            point: self.origin + radial + self.axis * v,
            du: tangent,
            dv: self.axis,
            duu: -radial,
            duv: zero,
            dvv: zero,
        })
    }

    fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.origin;
        let v = delta.dot(self.axis) / self.axis.magnitude2();
        let radial = delta - self.axis * v;
        let x = radial.dot(self.x_radius) / self.x_radius.magnitude2();
        let y = radial.dot(self.y_radius) / self.y_radius.magnitude2();
        if x == 0.0 && y == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "cylinder projection is not unique on its axis",
            ));
        }
        let u = y.atan2(x).rem_euclid(std::f64::consts::TAU);
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

impl Cone3 {
    pub fn new(
        apex: Point3,
        axis: Vec3,
        x_radius: Vec3,
        y_radius: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let reference = Circle3::new(apex + axis, x_radius, y_radius, tolerance)?;
        if !finite_vector(axis)
            || axis.magnitude() <= tolerance.linear
            || axis
                .angle(reference.normal())
                .0
                .min(axis.angle(-reference.normal()).0)
                > tolerance.angular
        {
            return Err(invalid(
                "cone axis must be finite, nonzero, and perpendicular to its reference circle",
            ));
        }
        Ok(Self {
            apex,
            axis,
            x_radius,
            y_radius,
        })
    }

    pub fn from_axis(
        axis: Axis3,
        height: f64,
        reference_radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !height.is_finite()
            || height.abs() <= tolerance.linear_at_scale(height)
            || !reference_radius.is_finite()
            || reference_radius <= tolerance.linear_at_scale(reference_radius)
        {
            return Err(invalid("cone height must exceed model tolerance"));
        }
        let frame = Frame3::from_z(axis.origin(), axis.direction())?;
        Self::new(
            axis.origin(),
            axis.direction() * height,
            frame.x() * reference_radius,
            frame.y() * reference_radius,
            tolerance,
        )
    }

    pub fn apex(self) -> Point3 {
        self.apex
    }
    pub fn axis(self) -> Vec3 {
        self.axis
    }
    pub fn x_radius(self) -> Vec3 {
        self.x_radius
    }
    pub fn y_radius(self) -> Vec3 {
        self.y_radius
    }
    pub fn reference_radius(self) -> f64 {
        self.x_radius.magnitude()
    }

    fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let (sin, cos) = u.sin_cos();
        let radial = self.x_radius * cos + self.y_radius * sin;
        let tangent = -self.x_radius * sin + self.y_radius * cos;
        let zero = Vec3::new(0.0, 0.0, 0.0);
        Ok(SurfaceEvaluation {
            point: self.apex + (self.axis + radial) * v,
            du: tangent * v,
            dv: self.axis + radial,
            duu: -radial * v,
            duv: tangent,
            dvv: zero,
        })
    }

    fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.apex;
        let x = delta.dot(self.x_radius) / self.x_radius.magnitude2();
        let y = delta.dot(self.y_radius) / self.y_radius.magnitude2();
        let u = if x == 0.0 && y == 0.0 {
            0.0
        } else {
            y.atan2(x).rem_euclid(std::f64::consts::TAU)
        };
        let (sin, cos) = u.sin_cos();
        let generator = self.axis + self.x_radius * cos + self.y_radius * sin;
        let v = delta.dot(generator) / generator.magnitude2();
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

impl Sphere3 {
    pub fn new(
        center: Point3,
        x_radius: Vec3,
        y_radius: Vec3,
        z_radius: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let equator = Circle3::new(center, x_radius, y_radius, tolerance)?;
        let radius = equator.radius();
        if !finite_vector(z_radius)
            || (z_radius.magnitude() - radius).abs() > tolerance.linear_at_scale(radius)
            || z_radius
                .angle(equator.normal())
                .0
                .min(z_radius.angle(-equator.normal()).0)
                > tolerance.angular
        {
            return Err(invalid(
                "sphere frame requires three perpendicular radius vectors of equal length",
            ));
        }
        Ok(Self {
            center,
            x_radius,
            y_radius,
            z_radius,
        })
    }

    pub fn from_radius(
        center: Point3,
        radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !radius.is_finite() || radius <= tolerance.linear_at_scale(radius) {
            return Err(invalid("sphere radius must exceed model tolerance"));
        }
        Self::new(
            center,
            Vec3::unit_x() * radius,
            Vec3::unit_y() * radius,
            Vec3::unit_z() * radius,
            tolerance,
        )
    }

    pub fn center(self) -> Point3 {
        self.center
    }
    pub fn radius(self) -> f64 {
        self.x_radius.magnitude()
    }
    pub fn x_radius(self) -> Vec3 {
        self.x_radius
    }
    pub fn y_radius(self) -> Vec3 {
        self.y_radius
    }
    pub fn z_radius(self) -> Vec3 {
        self.z_radius
    }

    fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let (u_sin, u_cos) = u.sin_cos();
        let (v_sin, v_cos) = v.sin_cos();
        let radial = self.x_radius * u_cos + self.y_radius * u_sin;
        let tangent = -self.x_radius * u_sin + self.y_radius * u_cos;
        Ok(SurfaceEvaluation {
            point: self.center + radial * v_cos + self.z_radius * v_sin,
            du: tangent * v_cos,
            dv: -radial * v_sin + self.z_radius * v_cos,
            duu: -radial * v_cos,
            duv: -tangent * v_sin,
            dvv: -radial * v_cos - self.z_radius * v_sin,
        })
    }

    fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.center;
        if delta.magnitude2() == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "sphere projection is not unique at its center",
            ));
        }
        let radius = self.radius();
        let x = delta.dot(self.x_radius) / radius.powi(2);
        let y = delta.dot(self.y_radius) / radius.powi(2);
        let z = delta.dot(self.z_radius) / radius.powi(2);
        let u = y.atan2(x).rem_euclid(std::f64::consts::TAU);
        let v = z.atan2((x * x + y * y).sqrt());
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

impl Torus3 {
    pub fn new(
        center: Point3,
        x_major: Vec3,
        y_major: Vec3,
        minor_radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let major = Circle3::new(center, x_major, y_major, tolerance)?;
        if !minor_radius.is_finite()
            || minor_radius <= tolerance.linear
            || minor_radius >= major.radius() - tolerance.linear_at_scale(major.radius())
        {
            return Err(invalid(
                "a regular torus needs a positive minor radius strictly smaller than its major radius",
            ));
        }
        Ok(Self {
            center,
            x_major,
            y_major,
            minor_radius,
        })
    }

    pub fn from_axis(
        axis: Axis3,
        major_radius: f64,
        minor_radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !major_radius.is_finite() || major_radius <= tolerance.linear_at_scale(major_radius) {
            return Err(invalid("torus major radius must exceed model tolerance"));
        }
        let frame = Frame3::from_z(axis.origin(), axis.direction())?;
        Self::new(
            axis.origin(),
            frame.x() * major_radius,
            frame.y() * major_radius,
            minor_radius,
            tolerance,
        )
    }

    pub fn center(self) -> Point3 {
        self.center
    }
    pub fn major_radius(self) -> f64 {
        self.x_major.magnitude()
    }
    pub fn minor_radius(self) -> f64 {
        self.minor_radius
    }
    pub fn x_major(self) -> Vec3 {
        self.x_major
    }
    pub fn y_major(self) -> Vec3 {
        self.y_major
    }
    pub fn normal(self) -> Vec3 {
        self.x_major.cross(self.y_major).normalize()
    }

    fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let major = self.major_radius();
        let (u_sin, u_cos) = u.sin_cos();
        let (v_sin, v_cos) = v.sin_cos();
        let radial_unit = (self.x_major * u_cos + self.y_major * u_sin) / major;
        let tangent_unit = (-self.x_major * u_sin + self.y_major * u_cos) / major;
        let normal = self.normal();
        let radial_distance = major + self.minor_radius * v_cos;
        Ok(SurfaceEvaluation {
            point: self.center
                + radial_unit * radial_distance
                + normal * (self.minor_radius * v_sin),
            du: tangent_unit * radial_distance,
            dv: radial_unit * (-self.minor_radius * v_sin) + normal * (self.minor_radius * v_cos),
            duu: -radial_unit * radial_distance,
            duv: tangent_unit * (-self.minor_radius * v_sin),
            dvv: radial_unit * (-self.minor_radius * v_cos) - normal * (self.minor_radius * v_sin),
        })
    }

    fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.center;
        let major = self.major_radius();
        let x = delta.dot(self.x_major) / major.powi(2);
        let y = delta.dot(self.y_major) / major.powi(2);
        if x == 0.0 && y == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "torus projection is not unique on its axis",
            ));
        }
        let u = y.atan2(x).rem_euclid(std::f64::consts::TAU);
        let (sin, cos) = u.sin_cos();
        let radial = (self.x_major * cos + self.y_major * sin) / major;
        let tube_center = self.center + radial * major;
        let tube_delta = point - tube_center;
        let v = tube_delta
            .dot(self.normal())
            .atan2(tube_delta.dot(radial))
            .rem_euclid(std::f64::consts::TAU);
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Surface3 {
    Plane(Plane3),
    Cylinder(Cylinder3),
    Cone(Cone3),
    Sphere(Sphere3),
    Torus(Torus3),
    Nurbs(NurbsSurface3),
}

impl Surface3 {
    pub fn domains(&self) -> [ParameterDomain; 2] {
        match self {
            Self::Plane(_) => [ParameterDomain::unbounded(), ParameterDomain::unbounded()],
            Self::Cylinder(_) => [
                ParameterDomain {
                    bounds: DomainBounds::Bounded(ParameterRange {
                        start: 0.0,
                        end: std::f64::consts::TAU,
                    }),
                    period: Some(std::f64::consts::TAU),
                },
                ParameterDomain::unbounded(),
            ],
            Self::Cone(_) => [
                ParameterDomain {
                    bounds: DomainBounds::Bounded(ParameterRange {
                        start: 0.0,
                        end: std::f64::consts::TAU,
                    }),
                    period: Some(std::f64::consts::TAU),
                },
                ParameterDomain::unbounded(),
            ],
            Self::Sphere(_) => [
                ParameterDomain {
                    bounds: DomainBounds::Bounded(ParameterRange {
                        start: 0.0,
                        end: std::f64::consts::TAU,
                    }),
                    period: Some(std::f64::consts::TAU),
                },
                ParameterDomain {
                    bounds: DomainBounds::Bounded(ParameterRange {
                        start: -std::f64::consts::FRAC_PI_2,
                        end: std::f64::consts::FRAC_PI_2,
                    }),
                    period: None,
                },
            ],
            Self::Torus(_) => [
                ParameterDomain {
                    bounds: DomainBounds::Bounded(ParameterRange {
                        start: 0.0,
                        end: std::f64::consts::TAU,
                    }),
                    period: Some(std::f64::consts::TAU),
                },
                ParameterDomain {
                    bounds: DomainBounds::Bounded(ParameterRange {
                        start: 0.0,
                        end: std::f64::consts::TAU,
                    }),
                    period: Some(std::f64::consts::TAU),
                },
            ],
            Self::Nurbs(surface) => surface.domains(),
        }
    }

    pub fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        match self {
            Self::Plane(plane) => plane.evaluate(u, v),
            Self::Cylinder(cylinder) => cylinder.evaluate(u, v),
            Self::Cone(cone) => cone.evaluate(u, v),
            Self::Sphere(sphere) => sphere.evaluate(u, v),
            Self::Torus(torus) => torus.evaluate(u, v),
            Self::Nurbs(surface) => surface.evaluate(u, v),
        }
    }

    pub fn normal(&self, u: f64, v: f64) -> KernelResult<Vec3> {
        match self {
            Self::Sphere(sphere) => {
                let radial = sphere.evaluate(u, v)?.point - sphere.center();
                if radial.magnitude2() == 0.0 {
                    return Err(KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "sphere normal is singular at its center",
                    ));
                }
                Ok(radial.normalize())
            }
            _ => self.evaluate(u, v)?.normal(),
        }
    }

    pub fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection2> {
        match self {
            Self::Plane(plane) => plane.project(point),
            Self::Cylinder(cylinder) => cylinder.project(point),
            Self::Cone(cone) => cone.project(point),
            Self::Sphere(sphere) => sphere.project(point),
            Self::Torus(torus) => torus.project(point),
            Self::Nurbs(surface) => surface.project(point, tolerance),
        }
    }
}

impl From<Plane3> for Surface3 {
    fn from(value: Plane3) -> Self {
        Self::Plane(value)
    }
}
impl From<Cylinder3> for Surface3 {
    fn from(value: Cylinder3) -> Self {
        Self::Cylinder(value)
    }
}
impl From<Cone3> for Surface3 {
    fn from(value: Cone3) -> Self {
        Self::Cone(value)
    }
}
impl From<Sphere3> for Surface3 {
    fn from(value: Sphere3) -> Self {
        Self::Sphere(value)
    }
}
impl From<Torus3> for Surface3 {
    fn from(value: Torus3) -> Self {
        Self::Torus(value)
    }
}
impl From<NurbsSurface3> for Surface3 {
    fn from(value: NurbsSurface3) -> Self {
        Self::Nurbs(value)
    }
}
