//! Standalone planar curve queries. Parameters always belong to the basis curve;
//! a trim supplies orientation and bounds, without changing that parameterization.
use super::wire::{curve_loop_contains_point, curve_pair_intersection};
use super::{
    invalid, Curve2, Curve2Evaluation, DiagnosticKind, KernelError, KernelResult, ModelTolerance,
    ParameterRange, Profile, TessellationOptions, TrimmedCurve2, Wire2,
};
use crate::{Point2, Vec2};
use cgmath::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveProjection2 {
    pub parameter: f64,
    pub point: Point2,
    pub distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointClassification {
    Inside,
    Outside,
    Boundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveContact {
    Crossing,
    Tangent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CurveIntersection2 {
    Point {
        point: Point2,
        first_parameter: f64,
        second_parameter: f64,
        contact: CurveContact,
    },
    Overlap {
        first: ParameterRange,
        second: ParameterRange,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Polyline2 {
    pub parameters: Vec<f64>,
    pub points: Vec<Point2>,
}

/// Adaptive cubic approximation. The error is the largest validation residual,
/// not a certificate for arbitrary procedural evaluators.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveApproximation2 {
    pub curve: TrimmedCurve2,
    pub estimated_error: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RigidTransform2 {
    angle: f64,
    translation: Vec2,
}
impl RigidTransform2 {
    pub fn new(angle: f64, translation: Vec2) -> KernelResult<Self> {
        if !angle.is_finite() || !translation.x.is_finite() || !translation.y.is_finite() {
            return Err(invalid("planar transform must be finite"));
        }
        Ok(Self { angle, translation })
    }
    pub fn angle(self) -> f64 {
        self.angle
    }
    pub fn translation(self) -> Vec2 {
        self.translation
    }
    pub fn transform_vector(self, v: Vec2) -> Vec2 {
        let (s, c) = self.angle.sin_cos();
        Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
    }
    pub fn transform_point(self, p: Point2) -> Point2 {
        Point2::from_vec(self.transform_vector(p.to_vec()) + self.translation)
    }
    pub fn inverse(self) -> Self {
        let mut inverse = Self {
            angle: -self.angle,
            translation: Vec2::new(0.0, 0.0),
        };
        inverse.translation = -inverse.transform_vector(self.translation);
        inverse
    }
    pub fn then(self, next: Self) -> Self {
        Self {
            angle: self.angle + next.angle,
            translation: next.transform_vector(self.translation) + next.translation,
        }
    }
}

impl Curve2 {
    pub fn closest_point(
        &self,
        point: Point2,
        tolerance: ModelTolerance,
    ) -> KernelResult<CurveProjection2> {
        tolerance.validate()?;
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(invalid("query point must be finite"));
        }
        if let Self::Line(line) = self {
            let parameter =
                (point - line.origin()).dot(line.direction()) / line.direction().magnitude2();
            let projected = line.origin() + line.direction() * parameter;
            return Ok(CurveProjection2 {
                parameter,
                point: projected,
                distance: projected.distance(point),
            });
        }
        let super::DomainBounds::Bounded(range) = self.domain().bounds else {
            return Err(super::unsupported(
                "unbounded derived curves require a trim for closest-point queries",
            ));
        };
        TrimmedCurve2::new(self.clone(), range)?.closest_point(point, tolerance)
    }
}

/// Exact parallel curve. Positive distance is to the left of increasing basis
/// parameter. A reversed trimmed use negates the requested offset distance.
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetCurve2 {
    base: Box<Curve2>,
    distance: f64,
}

impl OffsetCurve2 {
    pub fn new(base: Curve2, distance: f64) -> KernelResult<Self> {
        if !distance.is_finite() {
            return Err(invalid("offset distance must be finite"));
        }
        if let Curve2::Offset(offset) = base {
            return Self::new(*offset.base, distance + offset.distance);
        }
        Ok(Self {
            base: Box::new(base),
            distance,
        })
    }
    pub fn base(&self) -> &Curve2 {
        &self.base
    }
    pub fn distance(&self) -> f64 {
        self.distance
    }
    pub(crate) fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        let value = self.base.evaluate(parameter)?;
        let v = value.first_derivative;
        let a = value.second_derivative;
        let speed = v.magnitude();
        if speed == 0.0 {
            return Err(invalid("offset has no normal at a stationary basis point"));
        }
        let spatial = super::Plane3::xy().lift_curve(&self.base, ModelTolerance::default())?;
        let third = spatial.third_derivative(parameter)?;
        let j = Vec2::new(third.x, third.y);
        let alpha = v.dot(a);
        let beta = a.dot(a) + v.dot(j);
        let rotate = |v: Vec2| Vec2::new(-v.y, v.x);
        Ok(Curve2Evaluation {
            point: value.point + rotate(v / speed) * self.distance,
            first_derivative: v + rotate(a / speed - v * (alpha / speed.powi(3))) * self.distance,
            second_derivative: a + rotate(
                j / speed - a * (2.0 * alpha / speed.powi(3)) - v * (beta / speed.powi(3))
                    + v * (3.0 * alpha * alpha / speed.powi(5)),
            ) * self.distance,
        })
    }
}

impl From<OffsetCurve2> for Curve2 {
    fn from(value: OffsetCurve2) -> Self {
        Self::Offset(value)
    }
}

/// Exact orthogonal image retaining the spatial curve's parameterization.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedCurve2 {
    curve: Box<super::Curve3>,
    plane: super::Plane3,
    range: ParameterRange,
}
impl ProjectedCurve2 {
    pub fn curve(&self) -> &super::Curve3 {
        &self.curve
    }
    pub fn plane(&self) -> super::Plane3 {
        self.plane
    }
    pub fn range(&self) -> ParameterRange {
        self.range
    }
    pub(crate) fn evaluate(&self, u: f64) -> KernelResult<Curve2Evaluation> {
        let value = self.curve.evaluate(u)?;
        let vector = |v: crate::Vec3| {
            self.plane
                .coordinates(self.plane.origin() + v)
                .map(|p| p.to_vec())
        };
        Ok(Curve2Evaluation {
            point: self.plane.coordinates(value.point)?,
            first_derivative: vector(value.first_derivative)?,
            second_derivative: vector(value.second_derivative)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaneCurveProjection {
    Point(Point2),
    Curve(TrimmedCurve2),
}

impl super::Plane3 {
    pub fn lift_trimmed_curve(
        self,
        curve: &TrimmedCurve2,
        tolerance: ModelTolerance,
    ) -> KernelResult<super::Curve3> {
        if !curve.curve().is_derived() {
            return self.lift_curve(curve.curve(), tolerance);
        }
        Ok(super::SurfaceCurve3::new(
            super::Surface3::Plane(self),
            curve.curve().clone(),
            curve.range(),
            tolerance,
        )?
        .into())
    }
    /// Express a coplanar curve in this plane's chart. This does not project
    /// off-plane geometry, and requires a geometric coplanarity certificate.
    pub fn express_curve(
        self,
        curve: &super::Curve3,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<TrimmedCurve2> {
        let normal = self.normal();
        let point_on =
            |point: crate::Point3| (point - self.origin()).dot(normal).abs() <= tolerance.linear;
        let plane_on = |surface: &super::Surface3| match surface {
            super::Surface3::Plane(plane) => {
                point_on(plane.origin())
                    && plane.normal().cross(normal).magnitude() <= tolerance.angular
            }
            _ => false,
        };
        let coplanar = match curve {
            super::Curve3::Line(line) => {
                point_on(line.origin())
                    && line.direction().dot(normal).abs()
                        <= tolerance.angular * line.direction().magnitude()
            }
            super::Curve3::Circle(circle) => {
                point_on(circle.center())
                    && circle.x_radius().dot(normal).abs() <= tolerance.linear
                    && circle.y_radius().dot(normal).abs() <= tolerance.linear
            }
            super::Curve3::Nurbs(curve) => curve.control_points().iter().all(|p| point_on(*p)),
            super::Curve3::SurfaceCurve(curve) => plane_on(curve.surface()),
            super::Curve3::SurfaceIntersection(curve) => {
                plane_on(curve.first_surface()) || plane_on(curve.second_surface())
            }
        };
        if !coplanar {
            return Err(invalid(
                "curve has no coplanarity certificate for this plane",
            ));
        }
        match self.project_curve(curve, range, tolerance)? {
            PlaneCurveProjection::Curve(curve) => Ok(curve),
            PlaneCurveProjection::Point(_) => Err(invalid("coplanar curve collapsed to a point")),
        }
    }
    /// Orthogonal image retaining the spatial curve's parameter, including
    /// circles whose axes differ from the plane's coordinate frame.
    pub(crate) fn parameterized_curve_image(
        self,
        curve: &super::Curve3,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Curve2> {
        if let Some(image) = self.flatten_curve(curve, tolerance)? {
            return Ok(image);
        }
        if let super::Curve3::SurfaceIntersection(intersection) = curve {
            let exact = intersection.exact_pcurves();
            if intersection.first_surface() == &super::Surface3::Plane(self) {
                return Ok(exact[0].clone());
            }
            if intersection.second_surface() == &super::Surface3::Plane(self) {
                return Ok(exact[1].clone());
            }
        }
        Ok(Curve2::Projected(ProjectedCurve2 { curve: Box::new(curve.clone()), plane: self, range }))
    }

    pub fn project_curve(
        self,
        curve: &super::Curve3,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<PlaneCurveProjection> {
        if !curve.domain().contains_range(range, tolerance.parameter) {
            return Err(invalid("projection trim lies outside the spatial curve"));
        }
        let point = |p| self.coordinates(p);
        let image = || {
            TrimmedCurve2::new(
                Curve2::Projected(ProjectedCurve2 {
                    curve: Box::new(curve.clone()),
                    plane: self,
                    range,
                }),
                range,
            )
        };
        let result = match curve {
            super::Curve3::Line(line) => {
                let origin = point(line.origin())?;
                let direction = point(line.origin() + line.direction())? - origin;
                if direction.magnitude() * range.span().abs() <= tolerance.linear {
                    return Ok(PlaneCurveProjection::Point(origin));
                }
                TrimmedCurve2::new(super::Line2::new(origin, direction)?, range)?
            }
            super::Curve3::Circle(circle) => {
                let center = point(circle.center())?;
                let x = point(circle.center() + circle.x_radius())? - center;
                let y = point(circle.center() + circle.y_radius())? - center;
                if (x.magnitude() - y.magnitude()).abs() <= tolerance.linear
                    && x.dot(y).abs() <= tolerance.angular * x.magnitude() * y.magnitude()
                {
                    let angle = x.y.atan2(x.x);
                    let sign = (x.x * y.y - x.y * y.x).signum();
                    TrimmedCurve2::new(
                        super::Circle2::new(center, x.magnitude())?,
                        ParameterRange::new(angle + sign * range.start, angle + sign * range.end)?,
                    )?
                } else {
                    image()?
                }
            }
            super::Curve3::Nurbs(curve) => TrimmedCurve2::new(
                super::NurbsCurve2::new(
                    curve.degree(),
                    curve
                        .control_points()
                        .iter()
                        .map(|p| point(*p))
                        .collect::<KernelResult<_>>()?,
                    curve.weights().to_vec(),
                    curve.knots().to_vec(),
                )?,
                range,
            )?,
            _ => image()?,
        };
        Ok(PlaneCurveProjection::Curve(result))
    }
}

impl super::Curve3 {
    pub fn transformed(
        &self,
        transform: super::RigidTransform3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        super::transform::transform_curve(self.clone(), transform, tolerance)
    }
    pub fn closest_point(
        &self,
        point: crate::Point3,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<super::Projection1> {
        self.project_on_trim(point, range, tolerance)
    }
}

impl TrimmedCurve2 {
    pub fn transformed(
        &self,
        transform: RigidTransform2,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let (s, c) = transform.angle.sin_cos();
        let rotation = crate::Matrix3::from_cols(
            crate::Vec3::new(c, s, 0.0),
            crate::Vec3::new(-s, c, 0.0),
            crate::Vec3::new(0.0, 0.0, 1.0),
        );
        let rigid = super::RigidTransform3::new(
            rotation,
            crate::Vec3::new(transform.translation.x, transform.translation.y, 0.0),
            tolerance,
        )?;
        let spatial = super::Plane3::xy()
            .lift_trimmed_curve(self, tolerance)?
            .transformed(rigid, tolerance)?;
        super::Plane3::xy().express_curve(&spatial, self.range(), tolerance)
    }
    pub fn approximate_nurbs(
        &self,
        tolerance: ModelTolerance,
    ) -> KernelResult<CurveApproximation2> {
        tolerance.validate()?;
        if matches!(self.curve(), Curve2::Nurbs(_)) {
            return Ok(CurveApproximation2 {
                curve: self.clone(),
                estimated_error: 0.0,
            });
        }
        fn fit(
            curve: &Curve2,
            a: f64,
            b: f64,
            tolerance: f64,
            depth: u8,
            spans: &mut Vec<(f64, [Point2; 4], f64)>,
        ) -> KernelResult<()> {
            let left = curve.evaluate(a)?;
            let right = curve.evaluate(b)?;
            let controls = [
                left.point,
                left.point + left.first_derivative * ((b - a) / 3.0),
                right.point - right.first_derivative * ((b - a) / 3.0),
                right.point,
            ];
            let mut error = 0.0_f64;
            for i in 1..16 {
                let t = i as f64 / 16.0;
                let s = 1.0 - t;
                let point = Point2::from_vec(
                    controls[0].to_vec() * s.powi(3)
                        + controls[1].to_vec() * (3.0 * s * s * t)
                        + controls[2].to_vec() * (3.0 * s * t * t)
                        + controls[3].to_vec() * t.powi(3),
                );
                error = error.max(point.distance(curve.evaluate(a + (b - a) * t)?.point));
            }
            if error <= tolerance {
                spans.push((b, controls, error));
                return Ok(());
            }
            if depth == 24 {
                return Err(KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "NURBS approximation did not converge",
                ));
            }
            let middle = (a + b) * 0.5;
            fit(curve, a, middle, tolerance, depth + 1, spans)?;
            fit(curve, middle, b, tolerance, depth + 1, spans)
        }
        let range = self.range();
        let low = range.start.min(range.end);
        let high = range.start.max(range.end);
        let mut breaks = self.curve().integration_breakpoints(range);
        breaks.extend([low, high]);
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        let mut spans = Vec::new();
        for pair in breaks.windows(2) {
            fit(
                self.curve(),
                pair[0],
                pair[1],
                tolerance.linear,
                0,
                &mut spans,
            )?;
        }
        let mut controls = vec![spans[0].1[0]];
        let mut knots = vec![low; 4];
        let mut estimated_error = 0.0_f64;
        for (index, (end, points, error)) in spans.iter().enumerate() {
            controls.extend_from_slice(&points[1..]);
            knots.extend(vec![*end; if index + 1 == spans.len() { 4 } else { 3 }]);
            estimated_error = estimated_error.max(*error);
        }
        let weights = vec![1.0; controls.len()];
        Ok(CurveApproximation2 {
            curve: Self::new(super::NurbsCurve2::new(3, controls, weights, knots)?, range)?,
            estimated_error,
        })
    }

    pub fn self_intersections(
        &self,
        tolerance: ModelTolerance,
    ) -> KernelResult<Vec<CurveIntersection2>> {
        tolerance.validate()?;
        if matches!(self.curve(), Curve2::Line(_) | Curve2::Circle(_)) {
            return Ok(Vec::new());
        }
        let range = self.range();
        let pieces = if let Curve2::Nurbs(curve) = self.curve() {
            super::wire::simple_nurbs_pieces(curve, range, tolerance)?
                .into_iter()
                .map(|(range, _)| Self::new(self.curve().clone(), range))
                .collect::<KernelResult<Vec<_>>>()?
        } else {
            let mut cuts = self.curve().integration_breakpoints(range);
            cuts.extend((1..32).map(|i| range.start + range.span() * i as f64 / 32.0));
            self.split_at(cuts, tolerance)?
        };
        let mut result = Vec::new();
        for i in 0..pieces.len() {
            for j in 0..i {
                for hit in pieces[i].intersections(&pieces[j], tolerance)? {
                    if let CurveIntersection2::Point {
                        point,
                        first_parameter,
                        second_parameter,
                        ..
                    } = &hit
                    {
                        let middle = (first_parameter + second_parameter) * 0.5;
                        let speed = [*first_parameter, middle, *second_parameter].into_iter()
                            .map(|parameter| self.curve().first_order(parameter).map(|value| value.1.magnitude()))
                            .collect::<KernelResult<Vec<_>>>()?.into_iter().fold(0.0_f64, f64::max);
                        // A curve meets its own diagonal everywhere. The
                        // intersection search band must not turn neighboring
                        // parameters of that local arc into a self-crossing.
                        if (first_parameter - second_parameter).abs() * speed <= tolerance.linear * 8.0 {
                            continue;
                        }
                        if [pieces[i].range().start, pieces[i].range().end].contains(first_parameter)
                            && [pieces[j].range().start, pieces[j].range().end].contains(second_parameter)
                            && self.curve().first_order(*first_parameter)?.0
                                .distance(self.curve().first_order(*second_parameter)?.0) > tolerance.linear {
                            continue;
                        }
                        if i == j + 1
                            && point.distance(pieces[i].start()?) <= tolerance.linear * 8.0
                        {
                            continue;
                        }
                        if j == 0
                            && i + 1 == pieces.len()
                            && self.start()?.distance(self.end()?) <= tolerance.linear
                            && point.distance(self.start()?) <= tolerance.linear * 8.0
                        {
                            continue;
                        }
                        let seam = (*first_parameter - range.end).abs() <= tolerance.parameter
                            && (*second_parameter - range.start).abs() <= tolerance.parameter;
                        if (first_parameter - second_parameter).abs() <= tolerance.parameter || seam
                        {
                            continue;
                        }
                        if result.iter().any(|previous| match previous {
                            CurveIntersection2::Point {
                                first_parameter: a,
                                second_parameter: b,
                                ..
                            } => {
                                (*a - first_parameter).abs() <= tolerance.parameter * 8.0
                                    && (*b - second_parameter).abs() <= tolerance.parameter * 8.0
                            }
                            _ => false,
                        }) {
                            continue;
                        }
                    }
                    result.push(hit);
                }
            }
        }
        Ok(result)
    }

    pub fn offset(&self, distance: f64) -> KernelResult<Self> {
        let signed = distance * self.range().span().signum();
        let curve: Curve2 = match self.curve() {
            Curve2::Line(line) => {
                let v = line.direction().normalize();
                super::Line2::new(
                    line.origin() + Vec2::new(-v.y, v.x) * signed,
                    line.direction(),
                )?
                .into()
            }
            Curve2::Circle(circle) => {
                let radius = circle.radius() - signed;
                if radius <= 0.0 {
                    return Err(invalid("offset collapses the circular curve"));
                }
                super::Circle2::new(circle.center(), radius)?.into()
            }
            _ => OffsetCurve2::new(self.curve().clone(), signed)?.into(),
        };
        Self::new(curve, self.range())
    }
}

impl TrimmedCurve2 {
    pub fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        if !self.range().contains(parameter, ModelTolerance::default()) {
            return Err(invalid("parameter lies outside the trimmed curve"));
        }
        self.curve().evaluate(parameter)
    }

    /// Globally closest retained point, including endpoints and stationary points.
    pub fn closest_point(
        &self,
        point: Point2,
        tolerance: ModelTolerance,
    ) -> KernelResult<CurveProjection2> {
        tolerance.validate()?;
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(invalid("query point must be finite"));
        }
        let range = self.range();
        let low = range.start.min(range.end);
        let high = range.start.max(range.end);
        let make = |parameter| -> KernelResult<CurveProjection2> {
            let value = self.curve().evaluate(parameter)?.point;
            Ok(CurveProjection2 {
                parameter,
                point: value,
                distance: value.distance(point),
            })
        };
        let mut best = make(range.start)?;
        let mut accept = |parameter| -> KernelResult<()> {
            let candidate = make(parameter)?;
            if candidate.distance < best.distance {
                best = candidate;
            }
            Ok(())
        };
        accept(range.end)?;
        match self.curve() {
            Curve2::Line(line) => accept(
                ((point - line.origin()).dot(line.direction()) / line.direction().magnitude2())
                    .clamp(low, high),
            )?,
            Curve2::Circle(circle) => {
                let radial = point - circle.center();
                let angle = radial.y.atan2(radial.x);
                let period = std::f64::consts::TAU;
                for k in ((low - angle) / period).floor() as i64
                    ..=((high - angle) / period).ceil() as i64
                {
                    accept((angle + k as f64 * period).clamp(low, high))?;
                }
            }
            _ => {
                // Bracket every stationary point on adaptive geometric spans.
                let samples = self.tessellate(TessellationOptions::new(
                    tolerance.linear.sqrt().max(tolerance.linear),
                    0.05,
                    24,
                )?)?;
                let derivative = |u| -> KernelResult<f64> {
                    let value = self.curve().evaluate(u)?;
                    Ok((value.point - point).dot(value.first_derivative))
                };
                for pair in samples.parameters.windows(2) {
                    let mut a = pair[0].min(pair[1]);
                    let mut b = pair[0].max(pair[1]);
                    let mut fa = derivative(a)?;
                    accept(a)?;
                    accept(b)?;
                    if fa * derivative(b)? > 0.0 {
                        continue;
                    }
                    for _ in 0..64 {
                        let middle = (a + b) * 0.5;
                        let fm = derivative(middle)?;
                        if fa * fm <= 0.0 {
                            b = middle;
                        } else {
                            a = middle;
                            fa = fm;
                        }
                        if b - a <= tolerance.parameter {
                            break;
                        }
                    }
                    accept((a + b) * 0.5)?;
                }
            }
        }
        Ok(best)
    }

    pub fn length(&self, tolerance: ModelTolerance) -> KernelResult<f64> {
        tolerance.validate()?;
        let range = self.range();
        Ok(super::integration::adaptive_gauss_kronrod_partitioned(
            range.start,
            range.end,
            |u| Ok([self.curve().evaluate(u)?.first_derivative.magnitude()]),
            [tolerance.linear],
            24,
            "curve length integration did not converge",
            &self.curve().integration_breakpoints(range),
        )?[0]
            .abs())
    }

    pub fn split_at(
        &self,
        parameters: impl IntoIterator<Item = f64>,
        tolerance: ModelTolerance,
    ) -> KernelResult<Vec<Self>> {
        tolerance.validate()?;
        let range = self.range();
        let mut values = vec![range.start, range.end];
        for value in parameters {
            if !value.is_finite() || !range.contains(value, tolerance) {
                return Err(invalid("split parameter lies outside the trim"));
            }
            if (value - range.start).abs() > tolerance.parameter
                && (value - range.end).abs() > tolerance.parameter
            {
                values.push(value);
            }
        }
        values.sort_by(f64::total_cmp);
        values.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
        if range.span() < 0.0 {
            values.reverse();
        }
        values
            .windows(2)
            .map(|pair| Self::new(self.curve().clone(), ParameterRange::new(pair[0], pair[1])?))
            .collect()
    }

    pub fn intersections(
        &self,
        other: &Self,
        tolerance: ModelTolerance,
    ) -> KernelResult<Vec<CurveIntersection2>> {
        tolerance.validate()?;
        if self.curve() == other.curve() && self.curve().domain().period.is_none() {
            let a = self.range();
            let b = other.range();
            let low = a.start.min(a.end).max(b.start.min(b.end));
            let high = a.start.max(a.end).min(b.start.max(b.end));
            if high - low > tolerance.parameter {
                let range = if a.span() > 0.0 {
                    ParameterRange::new(low, high)?
                } else {
                    ParameterRange::new(high, low)?
                };
                return Ok(vec![CurveIntersection2::Overlap {
                    first: range,
                    second: range,
                }]);
            }
        }
        let intersection = curve_pair_intersection(self, other, tolerance)?;
        if !intersection.coincident && intersection.hits.is_empty() {
            if let (Curve2::Circle(a), Curve2::Circle(b)) = (self.curve(), other.curve()) {
                if a.center().distance(b.center()) <= tolerance.linear
                    && (a.radius() - b.radius()).abs() <= tolerance.linear
                {
                    let mut contacts = Vec::new();
                    for u in [self.range().start, self.range().end] {
                        for v in [other.range().start, other.range().end] {
                            let point = self.curve().evaluate(u)?.point;
                            if point.distance(other.curve().evaluate(v)?.point) <= tolerance.linear
                                && !contacts.iter().any(|hit| match hit {
                                    CurveIntersection2::Point { point: p, .. } => {
                                        point.distance(*p) <= tolerance.linear
                                    }
                                    _ => false,
                                })
                            {
                                contacts.push(CurveIntersection2::Point {
                                    point,
                                    first_parameter: u,
                                    second_parameter: v,
                                    contact: CurveContact::Tangent,
                                });
                            }
                        }
                    }
                    return Ok(contacts);
                }
            }
        }
        if intersection.coincident {
            if matches!(
                (self.curve(), other.curve()),
                (Curve2::Circle(_), Curve2::Circle(_))
            ) {
                let a = self.range();
                let b = other.range();
                let period = std::f64::consts::TAU;
                let low = a.start.min(a.end);
                let high = a.start.max(a.end);
                let mut overlaps = Vec::new();
                let first_shift = ((low - b.start.max(b.end)) / period).floor() as i64;
                let last_shift = ((high - b.start.min(b.end)) / period).ceil() as i64;
                for k in first_shift..=last_shift {
                    let shift = k as f64 * period;
                    let start = low.max(b.start.min(b.end) + shift);
                    let end = high.min(b.start.max(b.end) + shift);
                    if end - start > tolerance.parameter {
                        let (start, end) = if a.span() > 0.0 {
                            (start, end)
                        } else {
                            (end, start)
                        };
                        overlaps.push(CurveIntersection2::Overlap {
                            first: ParameterRange::new(start, end)?,
                            second: ParameterRange::new(start - shift, end - shift)?,
                        });
                    }
                }
                return Ok(overlaps);
            }
            let mut pairs = Vec::new();
            for u in [self.range().start, self.range().end] {
                let projection = other.closest_point(self.curve().evaluate(u)?.point, tolerance)?;
                if projection.distance <= tolerance.linear {
                    pairs.push((u, projection.parameter));
                }
            }
            for v in [other.range().start, other.range().end] {
                let projection = self.closest_point(other.curve().evaluate(v)?.point, tolerance)?;
                if projection.distance <= tolerance.linear {
                    pairs.push((projection.parameter, v));
                }
            }
            pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
            pairs.dedup_by(|a, b| (a.0 - b.0).abs() <= tolerance.parameter);
            if pairs.len() >= 2 {
                let a = pairs[0];
                let b = *pairs.last().unwrap();
                return Ok(vec![CurveIntersection2::Overlap {
                    first: ParameterRange::new(a.0, b.0)?,
                    second: ParameterRange::new(a.1, b.1)?,
                }]);
            }
        }
        intersection
            .hits
            .into_iter()
            .map(|hit| {
                // Analytic intersection solvers return canonical circle angles.
                // Public hits use the representative in each retained trim so
                // callers can pass the parameters straight to split_at.
                let first_parameter = self
                    .curve()
                    .domain()
                    .nearest(hit.first_parameter, self.range().middle());
                let second_parameter = other
                    .curve()
                    .domain()
                    .nearest(hit.second_parameter, other.range().middle());
                let a = self.curve().evaluate(first_parameter)?.first_derivative;
                let b = other.curve().evaluate(second_parameter)?.first_derivative;
                let tangent = (a.x * b.y - a.y * b.x).abs()
                    <= tolerance.angular * a.magnitude() * b.magnitude();
                Ok(CurveIntersection2::Point {
                    point: hit.point,
                    first_parameter,
                    second_parameter,
                    contact: if tangent {
                        CurveContact::Tangent
                    } else {
                        CurveContact::Crossing
                    },
                })
            })
            .collect()
    }

    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<Polyline2> {
        fn visit(
            curve: &Curve2,
            a: f64,
            b: f64,
            options: TessellationOptions,
            depth: u8,
            output: &mut Polyline2,
        ) -> KernelResult<()> {
            let left = curve.evaluate(a)?;
            let right = curve.evaluate(b)?;
            let chord = right.point - left.point;
            let mut error = 0.0_f64;
            for fraction in [0.25, 0.5, 0.75] {
                let value = curve.evaluate(a + (b - a) * fraction)?;
                let t = if chord.magnitude2() == 0.0 {
                    0.0
                } else {
                    ((value.point - left.point).dot(chord) / chord.magnitude2()).clamp(0.0, 1.0)
                };
                error = error.max(value.point.distance(left.point + chord * t));
            }
            let tangent = |v: Vec2| {
                let v = v * (b - a);
                if v.magnitude() <= f64::EPSILON.sqrt() * chord.magnitude().max(1.0) {
                    chord
                } else {
                    v
                }
            };
            let first = tangent(left.first_derivative);
            let last = tangent(right.first_derivative);
            let denom = first.magnitude() * last.magnitude();
            let angle = if denom > 0.0 {
                (first.dot(last) / denom).clamp(-1.0, 1.0).acos()
            } else {
                0.0
            };
            if error <= options.chord_tolerance && angle <= options.angular_tolerance {
                output.parameters.push(b);
                output.points.push(right.point);
                return Ok(());
            }
            if depth >= options.maximum_depth {
                return Err(KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "curve tessellation did not reach the requested tolerance",
                ));
            }
            let middle = (a + b) * 0.5;
            visit(curve, a, middle, options, depth + 1, output)?;
            visit(curve, middle, b, options, depth + 1, output)
        }
        let range = self.range();
        let mut output = Polyline2 {
            parameters: vec![range.start],
            points: vec![self.start()?],
        };
        let mut breaks = self.curve().integration_breakpoints(range);
        breaks.extend([range.start, range.end]);
        breaks.retain(|u| range.contains(*u, ModelTolerance::default()));
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        if range.span() < 0.0 {
            breaks.reverse();
        }
        for pair in breaks.windows(2) {
            visit(self.curve(), pair[0], pair[1], options, 0, &mut output)?;
        }
        Ok(output)
    }
}

impl Wire2 {
    /// Classify a point by the winding number of a closed wire, without
    /// requiring the wire to be simple. Retraced segments cancel.
    pub fn classify_point(
        &self,
        point: Point2,
        tolerance: ModelTolerance,
    ) -> KernelResult<PointClassification> {
        if !self.is_closed() {
            return Err(invalid("only a closed wire classifies points"));
        }
        classify_loop_point(self.segments(), point, tolerance)
    }
}

/// Points within tolerance of a segment lie on the boundary; others are
/// inside when the loop winds around them.
pub(super) fn classify_loop_point(
    segments: &[TrimmedCurve2],
    point: Point2,
    tolerance: ModelTolerance,
) -> KernelResult<PointClassification> {
    for segment in segments {
        if segment.closest_point(point, tolerance)?.distance <= tolerance.linear {
            return Ok(PointClassification::Boundary);
        }
    }
    Ok(if curve_loop_contains_point(segments, point, tolerance)? {
        PointClassification::Inside
    } else {
        PointClassification::Outside
    })
}

impl Profile {
    pub fn classify_point(&self, point: Point2) -> KernelResult<PointClassification> {
        let outer = self.outer().classify_point(point, self.tolerance())?;
        if outer != PointClassification::Inside {
            return Ok(outer);
        }
        for hole in self.holes() {
            match hole.classify_point(point, self.tolerance())? {
                PointClassification::Boundary => return Ok(PointClassification::Boundary),
                PointClassification::Inside => return Ok(PointClassification::Outside),
                _ => {}
            }
        }
        Ok(PointClassification::Inside)
    }
}
