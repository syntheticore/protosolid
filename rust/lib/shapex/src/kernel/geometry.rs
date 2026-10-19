use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3, Vec4};

use super::linalg::solve_linear;
use super::{
    invalid, DiagnosticKind, DomainBounds, KernelError, KernelResult, ModelTolerance,
    ParameterDomain, ParameterRange,
};

#[cfg(test)]
#[path = "geometry_tests.rs"]
mod tests;

pub(crate) fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn finite_vector(vector: Vec3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
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

    /// Extract exact rational Bézier spans by raising every internal knot to
    /// full degree multiplicity. The returned homogeneous control polygons
    /// retain the original NURBS parameterization.
    pub(crate) fn bezier_spans(&self) -> KernelResult<Vec<(ParameterRange, Vec<Vec3>)>> {
        let mut controls = self.homogeneous_controls();
        let mut knots = self.knots.clone();
        let degree = self.degree;
        let start = knots[degree];
        let end = knots[controls.len()];
        let internal = knots
            .iter()
            .copied()
            .filter(|knot| *knot > start && *knot < end)
            .fold(Vec::<f64>::new(), |mut unique, knot| {
                if unique.last().is_none_or(|last| *last != knot) {
                    unique.push(knot);
                }
                unique
            });
        for knot in internal {
            while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
                insert_homogeneous_knot(degree, knot, &mut knots, &mut controls)?;
            }
        }
        let mut spans = Vec::new();
        for index in degree..controls.len() {
            if knots[index + 1] <= knots[index] {
                continue;
            }
            spans.push((
                ParameterRange::new(knots[index], knots[index + 1])?,
                controls[index - degree..=index].to_vec(),
            ));
        }
        Ok(spans)
    }

    /// Continue the terminal rational polynomial spans without changing the
    /// curve or its parameterization anywhere in the original domain.
    pub(crate) fn extended(&self, range: ParameterRange) -> KernelResult<Self> {
        let mut spans = self.bezier_spans()?;
        let last = spans.len() - 1;
        for (index, (span, controls)) in spans.iter_mut().enumerate() {
            let start = if index == 0 {
                range.start.min(span.start)
            } else {
                span.start
            };
            let end = if index == last {
                range.end.max(span.end)
            } else {
                span.end
            };
            let a = (start - span.start) / span.span();
            let b = (end - span.start) / span.span();
            if a != 0.0 || b != 1.0 {
                *controls = (0..=self.degree)
                    .map(|index| {
                        let mut work = controls.clone();
                        for step in 0..self.degree {
                            let t = if step < self.degree - index { a } else { b };
                            for j in 0..self.degree - step {
                                work[j] = work[j] * (1.0 - t) + work[j + 1] * t;
                            }
                        }
                        work[0]
                    })
                    .collect();
                *span = ParameterRange::new(start, end)?;
            }
        }
        Self::from_bezier_spans(self.degree, spans)
    }

    // Intersection guides choose a branch; their terminal polynomial is not
    // the modeled geometry. Continue its tangent rather than extrapolating
    // high-order curvature that can fold far away from the exact supports.
    fn extended_guide(&self, range: ParameterRange) -> KernelResult<Self> {
        let mut spans = self.bezier_spans()?;
        let start = spans[0].0.start;
        let end = spans.last().unwrap().0.end;
        let tangent_span = |range: ParameterRange, endpoint: f64| -> KernelResult<Vec<Vec3>> {
            let value = self.evaluate(endpoint)?;
            Ok((0..=self.degree).map(|index| {
                let parameter = range.start + range.span() * index as f64 / self.degree as f64;
                let point = value.point + value.first_derivative * (parameter - endpoint);
                Vec3::new(point.x, point.y, 1.0)
            }).collect())
        };
        if range.start < start {
            let extension = ParameterRange::new(range.start, start)?;
            spans.insert(0, (extension, tangent_span(extension, start)?));
        }
        if range.end > end {
            let extension = ParameterRange::new(end, range.end)?;
            spans.push((extension, tangent_span(extension, end)?));
        }
        Self::from_bezier_spans(self.degree, spans)
    }

    fn from_bezier_spans(degree: usize, spans: Vec<(ParameterRange, Vec<Vec3>)>) -> KernelResult<Self> {
        let mut controls = spans[0].1.clone();
        let mut knots = vec![spans[0].0.start; degree + 1];
        for (span, next) in spans.iter().skip(1) {
            let shared = controls.last() == next.first();
            knots.extend(vec![span.start; degree + usize::from(!shared)]);
            controls.extend_from_slice(&next[usize::from(shared)..]);
        }
        knots.extend(vec![spans.last().unwrap().0.end; degree + 1]);
        Self::new(
            degree,
            controls
                .iter()
                .map(|p| Point2::new(p.x / p.z, p.y / p.z))
                .collect(),
            controls.iter().map(|p| p.z).collect(),
            knots,
        )
    }

    fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        let start = self.knots[self.degree];
        let end = self.knots[self.control_points.len()];
        let endpoint_envelope = f64::EPSILON * start.abs().max(end.abs()).max(1.0) * 64.0;
        if parameter < start - endpoint_envelope || parameter > end + endpoint_envelope {
            return Err(invalid(format!(
                "2D NURBS parameter {parameter} lies outside its natural domain [{start}, {end}]",
            )));
        }
        let parameter = parameter.clamp(start, end);
        let [value, first, second] = homogeneous_curve_jet(
            self.degree,
            &self.knots,
            self.control_points.len(),
            parameter,
            |index| {
                let point = self.control_points[index];
                let weight = self.weights[index];
                Vec3::new(point.x * weight, point.y * weight, weight)
            },
        );
        if ![value, first, second].into_iter().all(finite_vector) {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "2D NURBS evaluation produced a non-finite value",
            ));
        }
        if value.z == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "2D NURBS has zero homogeneous weight",
            ));
        }
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

fn insert_homogeneous_knot(
    degree: usize,
    knot: f64,
    knots: &mut Vec<f64>,
    controls: &mut Vec<Vec3>,
) -> KernelResult<()> {
    let old_count = controls.len();
    let span = if knot >= knots[old_count] {
        old_count - 1
    } else {
        (degree..old_count)
            .find(|index| knot >= knots[*index] && knot < knots[*index + 1])
            .ok_or_else(|| invalid("NURBS knot insertion parameter is outside the domain"))?
    };
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    let mut inserted = vec![Vec3::new(0.0, 0.0, 0.0); old_count + 1];
    inserted[..=span - degree].copy_from_slice(&controls[..=span - degree]);
    inserted[(span - multiplicity + 1)..(old_count + 1)]
        .copy_from_slice(&controls[(span - multiplicity)..old_count]);
    for index in span - degree + 1..=span - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        let alpha = if denominator == 0.0 {
            0.0
        } else {
            (knot - knots[index]) / denominator
        };
        inserted[index] = controls[index] * alpha + controls[index - 1] * (1.0 - alpha);
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Ok(())
}

fn insert_homogeneous_knot4(
    degree: usize,
    knot: f64,
    knots: &mut Vec<f64>,
    controls: &mut Vec<Vec4>,
) -> KernelResult<()> {
    let old_count = controls.len();
    let span = if knot >= knots[old_count] {
        old_count - 1
    } else {
        (degree..old_count)
            .find(|index| knot >= knots[*index] && knot < knots[*index + 1])
            .ok_or_else(|| invalid("NURBS knot insertion parameter is outside the domain"))?
    };
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    let mut inserted = vec![Vec4::new(0.0, 0.0, 0.0, 0.0); old_count + 1];
    inserted[..=span - degree].copy_from_slice(&controls[..=span - degree]);
    inserted[(span - multiplicity + 1)..(old_count + 1)]
        .copy_from_slice(&controls[(span - multiplicity)..old_count]);
    for index in span - degree + 1..=span - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        let alpha = if denominator == 0.0 {
            0.0
        } else {
            (knot - knots[index]) / denominator
        };
        inserted[index] = controls[index] * alpha + controls[index - 1] * (1.0 - alpha);
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Curve2 {
    Line(Line2),
    Circle(Circle2),
    Nurbs(NurbsCurve2),
    Offset(super::OffsetCurve2),
    Projected(super::ProjectedCurve2),
    SurfaceIntersection(SurfaceIntersectionPcurve2),
    SurfaceProjection(SurfaceProjectionPcurve2),
}

impl Curve2 {
    /// A short human-readable name of the curve type, for diagnostics.
    pub(crate) fn kind_name(&self) -> &'static str {
        match self {
            Self::Line(_) => "line",
            Self::Circle(_) => "circle",
            Self::Nurbs(_) => "NURBS",
            Self::Offset(_) => "offset",
            Self::Projected(_) => "projected",
            Self::SurfaceIntersection(_) => "surface intersection",
            Self::SurfaceProjection(_) => "surface projection",
        }
    }

    pub(crate) fn extended(&self, range: ParameterRange) -> KernelResult<Self> {
        if self.domain().contains_range(range, 0.0) {
            return Ok(self.clone());
        }
        let range = match self.domain().bounds {
            DomainBounds::Bounded(previous) => ParameterRange::new(
                range.start.min(range.end).min(previous.start.min(previous.end)),
                range.start.max(range.end).max(previous.start.max(previous.end)),
            )?,
            _ => range,
        };
        match self {
            Self::Nurbs(curve) => Ok(curve.extended(range)?.into()),
            Self::SurfaceIntersection(pcurve) => {
                Ok(Self::SurfaceIntersection(SurfaceIntersectionPcurve2 {
                    curve: Box::new(pcurve.curve.extended(range)?),
                    first: pcurve.first,
                }))
            }
            Self::SurfaceProjection(pcurve) => {
                // A trimmed projection may end at a periodic chart seam even
                // though its exact source circle or line continues beyond it.
                if !matches!(pcurve.curve.as_ref(), Curve3::SurfaceIntersection(_))
                    && pcurve.curve.domain().contains_range(range, 0.0)
                {
                    let mut points = (0..=32)
                        .map(|index| {
                            let t = range.start + range.span() * index as f64 / 32.0;
                            let point = pcurve.curve.evaluate(t)?.point;
                            let uv = pcurve.surface.project(point, pcurve.tolerance)?.parameters;
                            Ok(Point2::new(uv.0, uv.1))
                        })
                        .collect::<KernelResult<Vec<_>>>()?;
                    super::intersection::unwrap_periodic_points(
                        &mut points,
                        pcurve.surface.domains(),
                    );
                    let guide = super::intersection::smooth_nurbs_guide(&points, range)?;
                    let anchor = (pcurve.range.start + pcurve.range.end) * 0.5;
                    let previous = self.evaluate(anchor)?.point;
                    let current = guide.evaluate(anchor)?.point;
                    let shift =
                        super::whole_periods(pcurve.surface.domains(), previous - current);
                    for point in &mut points {
                        *point += shift;
                    }
                    return Ok(SurfaceProjectionPcurve2::new(
                        *pcurve.curve.clone(),
                        *pcurve.surface.clone(),
                        super::intersection::smooth_nurbs_guide(&points, range)?.into(),
                        range,
                        pcurve.tolerance,
                    )?
                    .into());
                }
                let Curve3::SurfaceIntersection(curve) = pcurve.curve.as_ref() else {
                    return Err(invalid(
                        "projection source cannot cover its extension",
                    ));
                };
                let range = ParameterRange::new(
                    range.start.min(pcurve.range.start),
                    range.end.max(pcurve.range.end),
                )?;
                let extended = curve.extended(range)?;
                // Boolean trimming can wrap an intersection's own exact
                // p-curve in a projection. Reuse that retained chart when
                // extending: extrapolating the fitted projection guide can
                // wander by a full period and introduce a seam jump.
                if pcurve.surface.as_ref() == curve.first_surface() {
                    return Ok(extended.exact_pcurves()[0].clone());
                }
                if pcurve.surface.as_ref() == curve.second_surface() {
                    return Ok(extended.exact_pcurves()[1].clone());
                }
                Ok(SurfaceProjectionPcurve2::new(
                    extended.into(),
                    *pcurve.surface.clone(),
                    pcurve.guide.extended(range)?,
                    range,
                    pcurve.tolerance,
                )?
                .into())
            }
            _ => Err(invalid("curve cannot cover its extension")),
        }
    }

    /// Split quadrature at guide knots: the exact intersection is smooth in
    /// space, but its guide can give it a piecewise-smooth parameter speed.
    pub(crate) fn integration_breakpoints(&self, range: ParameterRange) -> Vec<f64> {
        let mut parameters = Vec::new();
        self.collect_parameter_breaks(&mut parameters);
        parameters.retain(|value| {
            *value > range.start.min(range.end) && *value < range.start.max(range.end)
        });
        parameters.sort_by(f64::total_cmp);
        parameters.dedup();
        parameters
    }

    fn collect_parameter_breaks(&self, parameters: &mut Vec<f64>) {
        match self {
            Self::Nurbs(curve) => parameters.extend_from_slice(curve.knots()),
            Self::Offset(curve) => curve.base().collect_parameter_breaks(parameters),
            Self::Projected(curve) => curve.curve().collect_parameter_breaks(parameters),
            Self::SurfaceIntersection(curve) => {
                curve
                    .curve
                    .first_pcurve
                    .collect_parameter_breaks(parameters);
                curve
                    .curve
                    .second_pcurve
                    .collect_parameter_breaks(parameters);
            }
            Self::SurfaceProjection(curve) => {
                curve.guide.collect_parameter_breaks(parameters);
                curve.curve.collect_parameter_breaks(parameters);
            }
            Self::Line(_) | Self::Circle(_) => {}
        }
    }

    // A trim can cross knots in a supporting blend's spine even when its own
    // guide is smooth. Map those knots into the boundary parameter for quadrature.
    pub(crate) fn support_integration_breakpoints(&self, range: ParameterRange) -> KernelResult<Vec<f64>> {
        let Self::SurfaceIntersection(pcurve) = self else { return Ok(Vec::new()); };
        let curve = &pcurve.curve;
        let mut result = Vec::new();
        for (side, surface) in [curve.first_surface(), curve.second_surface()].into_iter().enumerate() {
            for axis in 0..2 {
                let knots = surface.integration_breakpoints(axis);
                if knots.is_empty() { continue; }
                let samples = (0..=64).map(|index| {
                    let parameter = range.start + range.span() * index as f64 / 64.0;
                    Ok((parameter, curve.corrected_parameters(parameter)?[side * 2 + axis]))
                }).collect::<KernelResult<Vec<_>>>()?;
                for knot in knots {
                    for pair in samples.windows(2) {
                        if (pair[0].1 - knot) * (pair[1].1 - knot) >= 0.0 { continue; }
                        let mut low = pair[0];
                        let mut high = pair[1];
                        for _ in 0..48 {
                            let parameter = (low.0 + high.0) * 0.5;
                            let value = curve.corrected_parameters(parameter)?[side * 2 + axis];
                            if (low.1 - knot) * (value - knot) > 0.0 {
                                low = (parameter, value);
                            } else { high = (parameter, value); }
                        }
                        result.push((low.0 + high.0) * 0.5);
                    }
                }
            }
        }
        Ok(result)
    }

    pub fn domain(&self) -> ParameterDomain {
        match self {
            Self::Line(_) => ParameterDomain::unbounded(),
            Self::Circle(_) => ParameterDomain::ANGLE,
            Self::Nurbs(curve) => curve.domain(),
            Self::Offset(curve) => curve.base().domain(),
            Self::Projected(curve) => ParameterDomain { bounds: DomainBounds::Bounded(curve.range()),period:None },
            Self::SurfaceIntersection(curve) => ParameterDomain {
                bounds: DomainBounds::Bounded(curve.curve.range()),
                period: None,
            },
            Self::SurfaceProjection(curve) => curve.domain(),
        }
    }

    pub fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        match self {
            Self::Line(line) => line.evaluate(parameter),
            Self::Circle(circle) => circle.evaluate(parameter),
            Self::Nurbs(curve) => curve.evaluate(parameter),
            Self::Offset(curve) => curve.evaluate(parameter),
            Self::Projected(curve) => curve.evaluate(parameter),
            Self::SurfaceIntersection(curve) => curve.evaluate(parameter),
            Self::SurfaceProjection(curve) => curve.evaluate(parameter),
        }
    }

    pub(crate) fn first_order(&self, parameter: f64) -> KernelResult<(Point2, Vec2)> {
        match self {
            Self::SurfaceProjection(curve) => curve.first_order(parameter),
            _ => self
                .evaluate(parameter)
                .map(|value| (value.point, value.first_derivative)),
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
                    let (point, derivative) = self.first_order(parameter)?;
                    Ok(0.5 * (point.x * derivative.y - point.y * derivative.x))
                };
                let coordinate_scale = curve
                    .control_points()
                    .iter()
                    .map(|point| point.x.abs().max(point.y.abs()))
                    .fold(1.0_f64, f64::max);
                let absolute_tolerance =
                    tolerance.max(f64::EPSILON.sqrt() * coordinate_scale.powi(2) * 64.0);
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
                            + super::integration::adaptive_simpson(
                                interval[0],
                                interval[1],
                                |parameter| Ok([integrand(parameter)?]),
                                [interval_tolerance],
                                24,
                                "adaptive p-curve area integration did not converge",
                            )?[0])
                    }
                })
            }
            Self::Offset(_) | Self::Projected(_) => {
                Ok(super::integration::adaptive_gauss_kronrod_partitioned(range.start, range.end,
                    |u| { let (p,d) = self.first_order(u)?; Ok([0.5*(p.x*d.y-p.y*d.x)]) },
                    [tolerance],24,"offset area integration did not converge",&self.integration_breakpoints(range))?[0])
            }
            Self::SurfaceIntersection(_) | Self::SurfaceProjection(_) => {
                let integrand = |parameter: f64| -> KernelResult<f64> {
                    let (point, derivative) = self.first_order(parameter)?;
                    Ok(0.5 * (point.x * derivative.y - point.y * derivative.x))
                };
                let coordinate_scale = [0.25, 0.5, 0.75]
                    .into_iter()
                    .map(|fraction| range.start + range.span() * fraction)
                    .map(|parameter| self.first_order(parameter))
                    .collect::<KernelResult<Vec<_>>>()?
                    .into_iter()
                    .map(|value| value.0.x.abs().max(value.0.y.abs()))
                    .fold(1.0_f64, f64::max);
                let absolute_tolerance =
                    tolerance.max(f64::EPSILON.sqrt() * coordinate_scale.powi(2) * 64.0);
                let exact = super::integration::adaptive_gauss_kronrod(
                    range.start,
                    range.end,
                    |parameter| Ok([integrand(parameter)?]),
                    [absolute_tolerance],
                    16,
                    "adaptive exact p-curve area integration did not converge",
                )
                .map(|value| value[0]);
                exact.or_else(|error| {
                    // Signed area is used only to establish loop winding. At a
                    // tangential trim endpoint the exact inverse derivative can
                    // be singular even though the retained branch guide has an
                    // unambiguous winding; integrate that guide geometrically
                    // without substituting it for the modeled p-curve.
                    let guide = match self {
                        Self::SurfaceIntersection(pcurve) if pcurve.first => {
                            &pcurve.curve.first_pcurve
                        }
                        Self::SurfaceIntersection(pcurve) => &pcurve.curve.second_pcurve,
                        Self::SurfaceProjection(pcurve) => &pcurve.guide,
                        _ => unreachable!(),
                    };
                    let mut previous = guide
                        .evaluate(range.start)
                        .map_err(|_| error.clone())?
                        .point;
                    let mut area = 0.0;
                    for index in 1..=64 {
                        let parameter = range.start + range.span() * index as f64 / 64.0;
                        let next = guide.evaluate(parameter).map_err(|_| error.clone())?.point;
                        area += 0.5 * (previous.x * next.y - previous.y * next.x);
                        previous = next;
                    }
                    Ok(area)
                })
            }
        }
    }
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

// Damped Gauss-Newton refinement shared by globally seeded offset projections
// and projections that must stay near a caller's UV seed. Keep periodic lifts;
// only nonperiodic bounded coordinates are clamped.
fn refine_surface_projection(
    point: Point3,
    seed: Point2,
    domains: [ParameterDomain; 2],
    first_order: impl Fn(f64, f64) -> KernelResult<(Point3, Vec3, Vec3)>,
    determinant_tolerance: impl Fn(f64, f64) -> f64,
    converged: impl Fn(Vec2) -> bool,
) -> KernelResult<Projection2> {
    let constrain = |uv: Point2| Point2::from(std::array::from_fn::<_, 2, _>(|axis| {
        if domains[axis].period.is_none() {
            if let DomainBounds::Bounded(range) = domains[axis].bounds {
                return uv[axis].clamp(range.start.min(range.end), range.start.max(range.end));
            }
        }
        uv[axis]
    }));
    let mut uv = constrain(seed);
    for _ in 0..48 {
        let (projected, du, dv) = first_order(uv.x, uv.y)?;
        let residual = projected - point;
        let a = du.magnitude2();
        let b = du.dot(dv);
        let c = dv.magnitude2();
        let determinant = a * c - b * b;
        if determinant.abs() <= determinant_tolerance(a, c) { break; }
        let first = -du.dot(residual);
        let second = -dv.dot(residual);
        let step = Vec2::new((first * c - second * b) / determinant, (second * a - first * b) / determinant);
        let mut damping = 1.0;
        let mut next = uv;
        for _ in 0..24 {
            let trial = constrain(uv + step * damping);
            if first_order(trial.x, trial.y)?.0.distance2(point) < residual.magnitude2() {
                next = trial;
                break;
            }
            damping *= 0.5;
        }
        let movement = next - uv;
        uv = next;
        if converged(movement) { break; }
    }
    let projected = first_order(uv.x, uv.y)?.0;
    Ok(Projection2 { parameters: (uv.x, uv.y), point: projected, distance: projected.distance(point) })
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

    fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        let domain_start = self.knots[self.degree];
        let domain_end = self.knots[self.control_points.len()];
        let endpoint_envelope =
            f64::EPSILON * domain_start.abs().max(domain_end.abs()).max(1.0) * 64.0;
        if parameter < domain_start - endpoint_envelope
            || parameter > domain_end + endpoint_envelope
        {
            return Err(invalid(format!(
                "3D NURBS parameter {parameter} lies outside its natural domain [{domain_start}, {domain_end}]",
            )));
        }
        let parameter = parameter.clamp(domain_start, domain_end);
        let [value, first, second] = homogeneous_curve_jet(
            self.degree,
            &self.knots,
            self.control_points.len(),
            parameter,
            |index| {
                let point = self.control_points[index];
                let weight = self.weights[index];
                Vec4::new(point.x * weight, point.y * weight, point.z * weight, weight)
            },
        );
        if ![value, first, second].into_iter().all(|v| {
            v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && v.w.is_finite()
        }) {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "NURBS evaluation produced a non-finite value",
            ));
        }
        if value.w == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "NURBS has zero homogeneous weight",
            ));
        }

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
            let mut derivative = evaluation.first_derivative.magnitude2()
                + residual.dot(evaluation.second_derivative);
            // Newton's stationary-point Hessian can be negative away from
            // the nearest point. Use a descent direction in that region.
            if derivative <= 0.0 {
                derivative = evaluation.first_derivative.magnitude2();
            }
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

/// Evaluate a homogeneous curve and its derivatives on the active
/// knot span. Only degree + 1 controls can contribute at a parameter; building
/// whole derivative curves here makes every sample linear in model size.
/// The same local evaluator serves rational 2D trims and 3D geometry. Common
/// degrees use stack storage; arbitrary higher degrees remain supported.
fn homogeneous_curve_jet<V: cgmath::VectorSpace<Scalar = f64>, const N: usize>(
    degree: usize,
    knots: &[f64],
    control_count: usize,
    parameter: f64,
    mut control: impl FnMut(usize) -> V,
) -> [V; N] {
    let span = find_span(degree, knots, control_count, parameter);
    let first = span - degree;
    let mut knots = &knots[first..first + 2 * degree + 2];
    let count = degree + 1;
    let mut stack = [V::zero(); 32];
    let mut spill = Vec::new();
    let storage = if count <= 16 {
        &mut stack[..count * 2]
    } else {
        spill.resize(count * 2, V::zero());
        spill.as_mut_slice()
    };
    let (controls, values) = storage.split_at_mut(count);
    for (index, value) in controls.iter_mut().enumerate() {
        *value = control(first + index);
    }
    let mut jet = [V::zero(); N];
    for (order, result) in jet.iter_mut().enumerate().take(degree + 1) {
        let current_degree = degree - order;
        values[..=current_degree].copy_from_slice(&controls[..=current_degree]);
        for level in 1..=current_degree {
            for j in (level..=current_degree).rev() {
                let denominator = knots[j + current_degree + 1 - level] - knots[j];
                values[j] = if denominator == 0.0 {
                    values[j - 1]
                } else {
                    let alpha = (parameter - knots[j]) / denominator;
                    values[j - 1] * (1.0 - alpha) + values[j] * alpha
                };
            }
        }
        *result = values[current_degree];
        if order + 1 < N && current_degree > 0 {
            for j in 0..current_degree {
                let denominator = knots[j + current_degree + 1] - knots[j + 1];
                controls[j] = if denominator == 0.0 {
                    V::zero()
                } else {
                    (controls[j + 1] - controls[j]) * (current_degree as f64 / denominator)
                };
            }
            knots = &knots[1..knots.len() - 1];
        }
    }
    jet
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

/// Guide data fixing the parameterization of an intersection curve.
struct IntersectionGauge {
    guide: [f64; 4],
    derivative: [f64; 4],
    second: [f64; 4],
    tangent: [f64; 4],
    tangent_derivative: [f64; 4],
}

fn dot4(a: [f64; 4], b: [f64; 4]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn sub4(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    std::array::from_fn(|index| a[index] - b[index])
}

/// A general regular intersection of two parametric surfaces.
///
/// The dual parameter-space guides provide a stable global parameterization;
/// evaluation corrects their four parameters against the two retained parent
/// surfaces. Consequently the modeling curve is the implicit common zero set,
/// rather than a polyline or a sampled 3D replacement.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceIntersectionCurve {
    first_surface: Box<Surface3>,
    second_surface: Box<Surface3>,
    first_pcurve: Curve2,
    second_pcurve: Curve2,
    range: ParameterRange,
    tolerance: ModelTolerance,
    achieved_error: f64,
    evaluations: ParameterEvaluationCache<IntersectionEvaluation>,
}

/// Evaluations belong to immutable geometry and can be shared by its edge,
/// p-curves and blend supports. Keys are exact parameter bits, never rounded.
/// Keep the cache bounded and exclude its incidental contents from identity.
type IntersectionEvaluation = ([f64; 4], Option<[f64; 4]>, Option<(Point3, Vec3)>);

#[derive(Clone)]
struct ParameterEvaluationCache<T>(
    std::sync::Arc<std::sync::Mutex<ParameterCacheEntries<T>>>,
);

const PARAMETER_CACHE_SETS: usize = 128;
const PARAMETER_CACHE_WAYS: usize = 4;

/// Small set-associative cache: constant lookup cost, no per-entry allocation,
/// and local eviction instead of periodically discarding every reusable value.
/// Storage is allocated only on the first successful evaluation.
struct ParameterCacheEntries<T> {
    sets: Vec<Option<Box<ParameterCacheSet<T>>>>,
}

struct ParameterCacheSet<T> {
    slots: [Option<(u64, T)>; PARAMETER_CACHE_WAYS],
    next: usize,
}

impl<T> Default for ParameterCacheEntries<T> {
    fn default() -> Self {
        Self { sets: Vec::new() }
    }
}

fn parameter_cache_set(mut bits: u64) -> usize {
    // Avalanche both the exponent and mantissa. Neighboring floating-point
    // samples often differ only in their low bits, or share many trailing zeros.
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((bits ^ (bits >> 31)) as usize) & (PARAMETER_CACHE_SETS - 1)
}

impl<T> Default for ParameterEvaluationCache<T> {
    fn default() -> Self {
        Self(std::sync::Arc::default())
    }
}
impl<T> std::fmt::Debug for ParameterEvaluationCache<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ParameterEvaluationCache")
    }
}
impl<T> PartialEq for ParameterEvaluationCache<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl<T: Copy> ParameterEvaluationCache<T> {
    fn get(&self, parameter: f64) -> Option<T> {
        let entries = self.0.lock().ok()?;
        let bits = parameter.to_bits();
        entries.sets.get(parameter_cache_set(bits))?.as_ref()?
            .slots
            .iter()
            .flatten()
            .find(|(key, _)| *key == bits)
            .map(|(_, value)| *value)
    }
    fn insert(&self, parameter: f64, value: T) {
        if let Ok(mut entries) = self.0.lock() {
            if entries.sets.is_empty() {
                entries.sets.resize_with(PARAMETER_CACHE_SETS, || None);
            }
            let bits = parameter.to_bits();
            // Most newly created supports are sampled at only a few parameters.
            // Allocate four entries at a time instead of reserving 512 large
            // differential records for every short-lived geometry object.
            let set = entries.sets[parameter_cache_set(bits)].get_or_insert_with(|| {
                Box::new(ParameterCacheSet {
                    slots: [None; PARAMETER_CACHE_WAYS],
                    next: 0,
                })
            });
            if let Some(slot) = set.slots.iter_mut()
                .find(|slot| slot.is_some_and(|(key, _)| key == bits))
            {
                *slot = Some((bits, value));
                return;
            }
            let way = set.next;
            set.slots[way] = Some((bits, value));
            set.next = (way + 1) % PARAMETER_CACHE_WAYS;
        }
    }
}

/// Exact curve obtained by evaluating a parameter-space curve on a retained
/// surface support.  This is the natural representation for a fillet contact
/// where the parent and blend surfaces are tangent rather than transversely
/// intersecting.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceCurve3 {
    surface: Box<Surface3>,
    pcurve: Curve2,
    range: ParameterRange,
    tolerance: ModelTolerance,
}

impl SurfaceCurve3 {
    pub fn new(
        surface: Surface3,
        pcurve: Curve2,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !pcurve.domain().contains_range(range, tolerance.parameter) {
            return Err(invalid(
                "surface curve p-curve does not cover its trim range",
            ));
        }
        let curve = Self {
            surface: Box::new(surface),
            pcurve,
            range,
            tolerance,
        };
        for parameter in [range.start, (range.start + range.end) * 0.5, range.end] {
            curve.evaluate(parameter)?;
        }
        Ok(curve)
    }

    pub fn surface(&self) -> &Surface3 {
        &self.surface
    }

    pub fn pcurve(&self) -> &Curve2 {
        &self.pcurve
    }

    pub fn range(&self) -> ParameterRange {
        self.range
    }

    fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        let uv = self.pcurve.evaluate(parameter)?;
        let surface = self.surface.evaluate(uv.point.x, uv.point.y)?;
        Ok(CurveEvaluation {
            point: surface.point,
            first_derivative: surface.du * uv.first_derivative.x
                + surface.dv * uv.first_derivative.y,
            second_derivative: surface.duu * uv.first_derivative.x.powi(2)
                + surface.duv * (2.0 * uv.first_derivative.x * uv.first_derivative.y)
                + surface.dvv * uv.first_derivative.y.powi(2)
                + surface.du * uv.second_derivative.x
                + surface.dv * uv.second_derivative.y,
        })
    }

    fn first_order(&self, parameter: f64) -> KernelResult<(Point3, Vec3)> {
        let (uv, uv_tangent) = self.pcurve.first_order(parameter)?;
        let (point, du, dv) = self.surface.first_order(uv.x, uv.y)?;
        Ok((point, du * uv_tangent.x + dv * uv_tangent.y))
    }

    fn project(&self, point: Point3) -> KernelResult<Projection1> {
        let mut parameter = (0..=48)
            .map(|index| self.range.start + self.range.span() * index as f64 / 48.0)
            .map(|parameter| {
                self.evaluate(parameter)
                    .map(|value| (parameter, value.point.distance2(point)))
            })
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|candidate| candidate.0)
            .ok_or_else(|| invalid("surface curve projection has no seed"))?;
        for _ in 0..32 {
            let value = self.evaluate(parameter)?;
            let residual = value.point - point;
            let derivative =
                value.first_derivative.magnitude2() + residual.dot(value.second_derivative);
            if derivative.abs() <= f64::EPSILON * 256.0 {
                break;
            }
            let next = (parameter - residual.dot(value.first_derivative) / derivative).clamp(
                self.range.start.min(self.range.end),
                self.range.start.max(self.range.end),
            );
            if (next - parameter).abs() <= self.tolerance.parameter {
                parameter = next;
                break;
            }
            parameter = next;
        }
        let projected = self.evaluate(parameter)?.point;
        Ok(Projection1 {
            parameter,
            point: projected,
            distance: projected.distance(point),
        })
    }
}

/// Exact parameter-space representation of one retained side of a regular
/// surface/surface intersection.  It shares the same Newton-corrected implicit
/// curve as the 3D edge, so a BREP coedge never stores a fitted UV surrogate.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceIntersectionPcurve2 {
    curve: Box<SurfaceIntersectionCurve>,
    first: bool,
}

impl SurfaceIntersectionPcurve2 {
    pub(crate) fn surface(&self) -> &Surface3 {
        if self.first {
            self.curve.first_surface()
        } else {
            self.curve.second_surface()
        }
    }

    pub(crate) fn guide(&self) -> &Curve2 {
        if self.first {
            self.curve.first_pcurve()
        } else {
            self.curve.second_pcurve()
        }
    }

    pub(crate) fn spatial_curve(&self) -> Curve3 {
        Curve3::SurfaceIntersection((*self.curve).clone())
    }

    pub(crate) fn lies_on(&self, surface: &Surface3) -> bool {
        self.curve.first_surface() == surface || self.curve.second_surface() == surface
    }

    fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        let parameters = self.curve.corrected_parameters(parameter)?;
        let derivative = self.curve.parameter_derivative(parameter, parameters)?;
        let (point, first_derivative) = if self.first {
            (
                Point2::new(parameters[0], parameters[1]),
                Vec2::new(derivative[0], derivative[1]),
            )
        } else {
            (
                Point2::new(parameters[2], parameters[3]),
                Vec2::new(derivative[2], derivative[3]),
            )
        };
        Ok(Curve2Evaluation {
            point,
            first_derivative,
            second_derivative: Vec2::new(0.0, 0.0),
        })
    }
}

/// Exact parameter-space image of a retained 3D curve on a surface.
///
/// The guide is used only to select a local inverse branch. Every evaluation
/// corrects the parameters against the exact curve and surface and obtains the
/// derivatives by differentiating that identity. This covers tangential shared
/// boundaries, where a regular surface/surface intersection is singular.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceProjectionPcurve2 {
    curve: Box<Curve3>,
    surface: Box<Surface3>,
    guide: Box<Curve2>,
    range: ParameterRange,
    tolerance: ModelTolerance,
    evaluations: ParameterEvaluationCache<(Point2, Vec2)>,
}

impl SurfaceProjectionPcurve2 {
    pub fn new(
        curve: Curve3,
        surface: Surface3,
        guide: Curve2,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !curve.domain().contains_range(range, tolerance.parameter)
            || !guide.domain().contains_range(range, tolerance.parameter)
        {
            return Err(invalid(
                "surface-projection curve and guide must cover the trim range",
            ));
        }
        let value = Self {
            curve: Box::new(curve),
            surface: Box::new(surface),
            guide: Box::new(guide),
            range,
            tolerance,
            evaluations: ParameterEvaluationCache::default(),
        };
        for parameter in [range.start, (range.start + range.end) * 0.5, range.end] {
            value.evaluate(parameter)?;
        }
        Ok(value)
    }

    pub fn curve(&self) -> &Curve3 {
        &self.curve
    }

    pub fn surface(&self) -> &Surface3 {
        &self.surface
    }

    pub fn guide(&self) -> &Curve2 {
        &self.guide
    }

    pub fn range(&self) -> ParameterRange {
        self.range
    }

    fn continuation_period(&self) -> Option<f64> {
        self.curve.domain().period
            .filter(|period| self.range.span().abs() + self.tolerance.parameter >= *period)
    }

    fn domain(&self) -> ParameterDomain {
        // A closed spatial curve need not close in the surface's UV chart:
        // a circle on a cylinder advances u by one full turn. Its projection
        // continues by a UV translation rather than periodic evaluation.
        ParameterDomain {
            bounds: if self.continuation_period().is_some() {
                DomainBounds::Unbounded
            } else {
                DomainBounds::Bounded(self.range)
            },
            period: None,
        }
    }

    fn corrected_parameters(&self, parameter: f64) -> KernelResult<(f64, f64)> {
        let mut parameter = parameter;
        let mut lift = Vec2::new(0.0, 0.0);
        if !self.range.contains(parameter, self.tolerance) {
            if let Some(period) = self.continuation_period() {
                let center = (self.range.start + self.range.end) * 0.5;
                let turns = ((parameter - center) / period).round();
                parameter -= turns * period;
                let start = self.range.start.min(self.range.end);
                let first = self.guide.first_order(start)?.0;
                let last = self.guide.first_order(start + period)?.0;
                for (axis, domain) in self.surface.domains().iter().enumerate() {
                    if let Some(surface_period) = domain.period {
                        lift[axis] = ((last[axis] - first[axis]) / surface_period).round()
                            * surface_period * turns;
                    }
                }
            }
        }
        if !self.range.contains(parameter, self.tolerance) {
            return Err(invalid(
                "surface-projection parameter lies outside its certified range",
            ));
        }
        parameter = parameter.clamp(
            self.range.start.min(self.range.end),
            self.range.start.max(self.range.end),
        );
        let target = self.curve.first_order(parameter)?.0;
        let guide = self.guide.first_order(parameter)?.0;
        let domains = self.surface.domains();
        let constrain = |domain: ParameterDomain, value: f64| {
            if domain.period.is_some() {
                value
            } else if let DomainBounds::Bounded(range) = domain.bounds {
                value.clamp(range.start.min(range.end), range.start.max(range.end))
            } else {
                value
            }
        };
        let mut uv = (guide.x, guide.y);
        for _ in 0..48 {
            uv.0 = constrain(domains[0], uv.0);
            uv.1 = constrain(domains[1], uv.1);
            let (point, du, dv) = self.surface.first_order(uv.0, uv.1)?;
            let residual = point - target;
            let scale = point
                .to_vec()
                .magnitude()
                .max(target.to_vec().magnitude())
                .max(1.0);
            // Model tolerance permits a small normal offset, but is too coarse
            // for the tangential position: adjacent uses can acquire different
            // endpoints when their guides stop at different residuals.
            if residual.magnitude() <= f64::EPSILON * scale * 2048.0 {
                return Ok((uv.0 + lift.x, uv.1 + lift.y));
            }
            let a00 = du.magnitude2();
            let a01 = du.dot(dv);
            let a11 = dv.magnitude2();
            let determinant = a00 * a11 - a01 * a01;
            let b0 = -du.dot(residual);
            let b1 = -dv.dot(residual);
            let delta = if determinant.abs() <= f64::EPSILON * a00.max(a11).powi(2) * 256.0 {
                if a00 + a11 == 0.0 { break; }
                (b0 / (a00 + a11), b1 / (a00 + a11))
            } else { (
                (b0 * a11 - b1 * a01) / determinant,
                (b1 * a00 - b0 * a01) / determinant,
            ) };
            let previous = residual.magnitude2();
            let mut damping = 1.0;
            let mut accepted = false;
            for _ in 0..12 {
                let candidate = (
                    constrain(domains[0], uv.0 + delta.0 * damping),
                    constrain(domains[1], uv.1 + delta.1 * damping),
                );
                if (self.surface.first_order(candidate.0, candidate.1)?.0 - target).magnitude2()
                    < previous
                {
                    uv = candidate;
                    accepted = true;
                    break;
                }
                damping *= 0.5;
            }
            if !accepted {
                break;
            }
        }
        let point = self.surface.first_order(uv.0, uv.1)?.0;
        let scale = point
            .to_vec()
            .magnitude()
            .max(target.to_vec().magnitude())
            .max(1.0);
        // Once refinement stops improving, retain the existing certification
        // allowance for a curve that lies only within tolerance of its support.
        if point.distance(target) <= self.tolerance.linear_at_scale(scale) {
            return Ok((uv.0 + lift.x, uv.1 + lift.y));
        }
        Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "exact curve-to-surface parameter correction did not converge",
        ))
    }

    fn solve_tangent(du: Vec3, dv: Vec3, vector: Vec3, guide: impl FnOnce() -> KernelResult<Vec2>) -> KernelResult<Vec2> {
        let a00 = du.magnitude2();
        let a01 = du.dot(dv);
        let a11 = dv.magnitude2();
        let determinant = a00 * a11 - a01 * a01;
        if determinant.abs() <= f64::EPSILON * a00.max(a11).powi(2) * 256.0 {
            // At a collapsed rail the inverse has one free chart direction.
            // Retain the guide's component in that null space and solve the
            // remaining spatial derivative in the rank-one tangent plane.
            let trace = a00 + a11;
            if trace > 0.0 {
                let guide = guide()?;
                let residual = vector - du * guide.x - dv * guide.y;
                return Ok(guide + Vec2::new(du.dot(residual), dv.dot(residual)) / trace);
            }
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "surface-projection p-curve reached a singular parameterization",
            ));
        }
        let b0 = du.dot(vector);
        let b1 = dv.dot(vector);
        Ok(Vec2::new(
            (b0 * a11 - b1 * a01) / determinant,
            (b1 * a00 - b0 * a01) / determinant,
        ))
    }

    fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        let uv = self.corrected_parameters(parameter)?;
        let curve = self.curve.evaluate(parameter)?;
        let surface = self.surface.evaluate(uv.0, uv.1)?;
        let first = Self::solve_tangent(surface.du, surface.dv, curve.first_derivative,
            || Ok(self.guide.evaluate(parameter)?.first_derivative))?;
        let curvature = surface.duu * first.x.powi(2)
            + surface.duv * (2.0 * first.x * first.y)
            + surface.dvv * first.y.powi(2);
        let second =
            Self::solve_tangent(surface.du, surface.dv, curve.second_derivative - curvature,
                || Ok(self.guide.evaluate(parameter)?.second_derivative))?;
        Ok(Curve2Evaluation {
            point: Point2::new(uv.0, uv.1),
            first_derivative: first,
            second_derivative: second,
        })
    }

    fn first_order(&self, parameter: f64) -> KernelResult<(Point2, Vec2)> {
        if let Some(value) = self.evaluations.get(parameter) {
            return Ok(value);
        }
        let uv = self.corrected_parameters(parameter)?;
        let (_, curve_tangent) = self.curve.first_order(parameter)?;
        let (_, du, dv) = self.surface.first_order(uv.0, uv.1)?;
        let value = (
            Point2::new(uv.0, uv.1),
            Self::solve_tangent(du, dv, curve_tangent, || Ok(self.guide.first_order(parameter)?.1))?,
        );
        self.evaluations.insert(parameter, value);
        Ok(value)
    }
}

impl From<SurfaceProjectionPcurve2> for Curve2 {
    fn from(value: SurfaceProjectionPcurve2) -> Self {
        Self::SurfaceProjection(value)
    }
}

// Splice rational guide spans without fitting a new approximation or extending
// either certified branch beyond its trim. Full knot multiplicity retains each
// guide's own gauge at the junction; the corrected spatial curve is continuous.
fn join_intersection_guides(
    first: &Curve2, first_range: ParameterRange,
    second: &Curve2, second_range: ParameterRange,
    join: f64, offset: Vec2,
) -> KernelResult<Option<(NurbsCurve2, usize)>> {
    let spans = |curve: &Curve2, trim: ParameterRange| -> KernelResult<Option<(usize, Vec<(ParameterRange, Vec<Vec3>)>)>> {
        Ok(match curve {
            Curve2::Nurbs(curve) => Some((curve.degree, curve.bezier_spans()?)),
            Curve2::Line(line) => {
                let range = ParameterRange::new(trim.start.min(trim.end), trim.start.max(trim.end))?;
                let a = line.evaluate(range.start)?.point;
                let b = line.evaluate(range.end)?.point;
                Some((1, vec![(range, vec![Vec3::new(a.x, a.y, 1.0), Vec3::new(b.x, b.y, 1.0)])]))
            }
            _ => None,
        })
    };
    let Some((first_degree, first_spans)) = spans(first, first_range)? else { return Ok(None); };
    let Some((second_degree, second_spans)) = spans(second, second_range)? else { return Ok(None); };
    let degree = first_degree.max(second_degree);
    let mut pieces = Vec::new();
    let mut junction = 0;
    for (side, (source_degree, spans, source_range, target, shift)) in [
        (first_degree, first_spans, first_range, ParameterRange::new(0.0, join)?, Vec2::new(0.0, 0.0)),
        (second_degree, second_spans, second_range, ParameterRange::new(join, 1.0)?, offset),
    ].into_iter().enumerate() {
        let mut source_pieces = Vec::new();
        for (span, controls) in spans {
            let start = span.start.max(source_range.start.min(source_range.end));
            let end = span.end.min(source_range.start.max(source_range.end));
            if end <= start { continue; }
            let (start, end) = if source_range.span() > 0.0 { (start, end) } else { (end, start) };
            let a = (start - span.start) / span.span();
            let b = (end - span.start) / span.span();
            let mut controls = (0..=source_degree).map(|index| {
                let mut work = controls.clone();
                for step in 0..source_degree {
                    let t = if step < source_degree - index { a } else { b };
                    for j in 0..source_degree - step {
                        work[j] = work[j] * (1.0 - t) + work[j + 1] * t;
                    }
                }
                work[0]
            }).collect::<Vec<_>>();
            // Degree elevation in homogeneous coordinates is exact.
            while controls.len() <= degree {
                let n = controls.len();
                controls = (0..=n).map(|i| {
                    if i == 0 { controls[0] }
                    else if i == n { controls[n - 1] }
                    else { controls[i - 1] * (i as f64 / n as f64) + controls[i] * (1.0 - i as f64 / n as f64) }
                }).collect();
            }
            for point in &mut controls {
                point.x += shift.x * point.z;
                point.y += shift.y * point.z;
            }
            let map = |t| target.start + (t - source_range.start) / source_range.span() * target.span();
            let start = map(start);
            let end = map(end);
            if end > start { source_pieces.push((start, end, controls)); }
        }
        source_pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (Some(first), Some(last)) = (source_pieces.first(), source_pieces.last()) else {
            return Ok(None);
        };
        // Both source trims must still reach their endpoints after mapping.
        // Otherwise there is no complete branch to anchor at the splice.
        if first.0 != target.start || last.1 != target.end { return Ok(None); }
        if side == 1 { junction = pieces.len() * (degree + 1); }
        pieces.extend(source_pieces);
    }
    let mut controls = Vec::new();
    let mut knots = Vec::new();
    for (start, _, points) in &pieces {
        knots.extend(vec![*start; degree + 1]);
        controls.extend_from_slice(points);
    }
    knots.extend(vec![1.0; degree + 1]);
    let guide = NurbsCurve2::new(degree,
        controls.iter().map(|p| Point2::new(p.x / p.z, p.y / p.z)).collect(),
        controls.iter().map(|p| p.z).collect(), knots,
    )?;
    Ok(Some((guide, junction)))
}

impl SurfaceIntersectionCurve {
    /// Join adjacent trims of the same support pair. Only the branch guides
    /// are reparameterized; evaluation still solves the exact two surfaces.
    pub(crate) fn joined(
        &self,
        range: ParameterRange,
        other: &Self,
        other_range: ParameterRange,
    ) -> KernelResult<Option<Self>> {
        let swapped = self.first_surface == other.second_surface
            && self.second_surface == other.first_surface;
        if !swapped && (self.first_surface != other.first_surface
            || self.second_surface != other.second_surface) {
            return Ok(None);
        }
        let end = self.evaluate(range.end)?;
        let start = other.evaluate(other_range.start)?;
        let a = end.first_derivative * range.span().signum();
        let b = start.first_derivative * other_range.span().signum();
        if end.point.distance(start.point) > self.tolerance.linear_at_scale(end.point.to_vec().magnitude()) * 8.0
            || a.normalize().dot(b.normalize()) <= 0.0
            || a.normalize().cross(b.normalize()).magnitude() > self.tolerance.angular * 8.0 {
            return Ok(None);
        }
        // Endpoint speed is ill-conditioned at a tangential support contact.
        // Use spatial traversal lengths to allocate the joined parameter range
        // so an ordinary arc cannot be compressed into a microscopic interval.
        let length = |curve: &Self, range: ParameterRange| -> KernelResult<f64> {
            let points = (0..=16).map(|index| {
                curve.first_order(range.start + range.span() * index as f64 / 16.0)
                    .map(|value| value.0)
            }).collect::<KernelResult<Vec<_>>>()?;
            Ok(points.windows(2).map(|pair| pair[0].distance(pair[1])).sum())
        };
        let other_length = length(other, other_range)?;
        let length = length(self, range)?;
        if length.min(other_length) <= self.tolerance.linear_at_scale(end.point.to_vec().magnitude()) {
            return Ok(None);
        }
        let join = length / (length + other_length);
        let parameters = self.corrected_parameters(range.end)?;
        let other_parameters = other.corrected_parameters(other_range.start)?;
        let first_parameters = self.corrected_parameters(range.start)?;
        let last_parameters = other.corrected_parameters(other_range.end)?;
        let guides = [&self.first_pcurve, &self.second_pcurve];
        let other_guides = if swapped { [&other.second_pcurve, &other.first_pcurve] }
            else { [&other.first_pcurve, &other.second_pcurve] };
        let surfaces = [self.first_surface.as_ref(), self.second_surface.as_ref()];
        let mut joined = Vec::<Curve2>::new();
        for side in 0..2 {
            let other_side = if swapped { 1 - side } else { side };
            let offset = super::whole_periods(
                surfaces[side].domains(),
                Vec2::new(
                    parameters[side * 2] - other_parameters[other_side * 2],
                    parameters[side * 2 + 1] - other_parameters[other_side * 2 + 1],
                ),
            );
            let Some((mut guide, junction)) = join_intersection_guides(
                guides[side], range, other_guides[side], other_range, join, offset,
            )? else { return Ok(None); };
            // Retain the corrected endpoints as exact guide anchors. Near a
            // tangency, roundoff from restricting the rational spans can move
            // a freshly corrected endpoint by much more than model tolerance.
            let point = |values: [f64; 4], side: usize| Point2::new(values[side * 2], values[side * 2 + 1]);
            let last = guide.control_points.len() - 1;
            let mut anchor = |index: usize, neighbor: usize, point: Point2| {
                let delta = point - guide.control_points[index];
                guide.control_points[index] = point;
                if guide.degree > 1 { guide.control_points[neighbor] += delta; }
            };
            anchor(0, 1, point(first_parameters, side));
            anchor(last, last - 1, point(last_parameters, other_side) + offset);
            anchor(junction - 1, junction - 2, point(parameters, side));
            anchor(junction, junction + 1, point(parameters, side));
            joined.push(guide.into());
        }
        for parameter in [0.0, 1.0] {
            let first = joined[0].evaluate(parameter)?.first_derivative;
            let second = joined[1].evaluate(parameter)?.first_derivative;
            if first.magnitude2() + second.magnitude2() <= self.tolerance.parameter.powi(2) {
                return Ok(None);
            }
        }
        Ok(Some(Self::new(
            *self.first_surface.clone(), *self.second_surface.clone(),
            joined.remove(0), joined.remove(0), ParameterRange::new(0.0, 1.0)?,
            self.tolerance, self.achieved_error.max(other.achieved_error),
        )?))
    }

    pub(crate) fn extended(&self, range: ParameterRange) -> KernelResult<Self> {
        let range = ParameterRange::new(
            range.start.min(self.range.start),
            range.end.max(self.range.end),
        )?;
        let extend = |guide: &Curve2| match guide {
            Curve2::Nurbs(curve) => curve.extended_guide(range).map(Curve2::Nurbs),
            _ => guide.extended(range),
        };
        Self::new(
            *self.first_surface.clone(),
            *self.second_surface.clone(),
            extend(&self.first_pcurve)?,
            extend(&self.second_pcurve)?,
            range,
            self.tolerance,
            self.achieved_error,
        )
    }

    pub fn new(
        first_surface: Surface3,
        second_surface: Surface3,
        first_pcurve: Curve2,
        second_pcurve: Curve2,
        range: ParameterRange,
        tolerance: ModelTolerance,
        achieved_error: f64,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !achieved_error.is_finite() || achieved_error < 0.0 {
            return Err(invalid(
                "surface-intersection certification error must be finite and nonnegative",
            ));
        }
        if !first_pcurve
            .domain()
            .contains_range(range, tolerance.parameter)
            || !second_pcurve
                .domain()
                .contains_range(range, tolerance.parameter)
        {
            return Err(invalid(
                "surface-intersection p-curves must cover the certified curve range",
            ));
        }
        let curve = Self {
            first_surface: Box::new(first_surface),
            second_surface: Box::new(second_surface),
            first_pcurve,
            second_pcurve,
            range,
            tolerance,
            achieved_error,
            evaluations: ParameterEvaluationCache::default(),
        };
        for parameter in [range.start, (range.start + range.end) * 0.5, range.end] {
            curve.corrected_parameters(parameter)?;
        }
        Ok(curve)
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

    pub fn range(&self) -> ParameterRange {
        self.range
    }

    pub fn achieved_error(&self) -> f64 {
        self.achieved_error
    }

    /// Convert support residual to transverse positional uncertainty. Near
    /// tangency, a small surface residual does not establish a small distance
    /// between intersection branches or between an intersection and a trim.
    pub(crate) fn positional_uncertainty(&self, parameter: f64) -> KernelResult<f64> {
        let parameters = self.corrected_parameters(parameter)?;
        let (point, first_u, first_v) = self.first_surface.first_order(parameters[0], parameters[1])?;
        let (_, second_u, second_v) = self.second_surface.first_order(parameters[2], parameters[3])?;
        let sine = first_u.cross(first_v).normalize()
            .cross(second_u.cross(second_v).normalize()).magnitude();
        if sine <= self.tolerance.angular { return Ok(f64::INFINITY); }
        Ok(self.achieved_error.max(self.tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0))) / sine)
    }

    pub fn pcurve_for_surface(&self, surface: &Surface3) -> Option<&Curve2> {
        if self.first_surface.as_ref() == surface {
            Some(&self.first_pcurve)
        } else if self.second_surface.as_ref() == surface {
            Some(&self.second_pcurve)
        } else {
            None
        }
    }

    pub fn exact_pcurves(&self) -> [Curve2; 2] {
        [
            Curve2::SurfaceIntersection(SurfaceIntersectionPcurve2 {
                curve: Box::new(self.clone()),
                first: true,
            }),
            Curve2::SurfaceIntersection(SurfaceIntersectionPcurve2 {
                curve: Box::new(self.clone()),
                first: false,
            }),
        ]
    }

    /// The parameter gauge at a curve parameter: the guides' combined value
    /// and derivatives, with the guide's unit tangent and its derivative.
    fn gauge(&self, parameter: f64) -> KernelResult<IntersectionGauge> {
        let first_guide = self.first_pcurve.evaluate(parameter)?;
        let second_guide = self.second_pcurve.evaluate(parameter)?;
        let guide = [
            first_guide.point.x,
            first_guide.point.y,
            second_guide.point.x,
            second_guide.point.y,
        ];
        let derivative = [
            first_guide.first_derivative.x,
            first_guide.first_derivative.y,
            second_guide.first_derivative.x,
            second_guide.first_derivative.y,
        ];
        let second = [
            first_guide.second_derivative.x,
            first_guide.second_derivative.y,
            second_guide.second_derivative.x,
            second_guide.second_derivative.y,
        ];
        let speed = derivative
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if speed <= self.tolerance.parameter {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "surface-intersection p-curve guide has a singular tangent",
            ));
        }
        let tangent = derivative.map(|value| value / speed);
        let speed_derivative = derivative
            .iter()
            .zip(second)
            .map(|(first, second)| first * second)
            .sum::<f64>()
            / speed;
        let tangent_derivative = std::array::from_fn(|index| {
            second[index] / speed - derivative[index] * speed_derivative / speed.powi(2)
        });
        Ok(IntersectionGauge {
            guide,
            derivative,
            second,
            tangent,
            tangent_derivative,
        })
    }

    // Reuse the converged parameters for both spatial and parameter-space
    // tangents, without cloning the intersection or running Newton again.
    fn parameter_derivative(&self, parameter: f64, parameters: [f64; 4]) -> KernelResult<[f64; 4]> {
        let parameter = parameter.clamp(
            self.range.start.min(self.range.end),
            self.range.start.max(self.range.end),
        );
        if let Some((_, Some(derivative), _)) = self.evaluations.get(parameter) {
            return Ok(derivative);
        }
        let gauge = self.gauge(parameter)?;
        let (_, first_du, first_dv) = self
            .first_surface
            .first_order(parameters[0], parameters[1])?;
        let (_, second_du, second_dv) = self
            .second_surface
            .first_order(parameters[2], parameters[3])?;
        let gauge_rhs = dot4(gauge.tangent, gauge.derivative)
            - dot4(gauge.tangent_derivative, sub4(parameters, gauge.guide));
        let matrix = [
            [first_du.x, first_dv.x, -second_du.x, -second_dv.x],
            [first_du.y, first_dv.y, -second_du.y, -second_dv.y],
            [first_du.z, first_dv.z, -second_du.z, -second_dv.z],
            gauge.tangent,
        ];
        let derivative =
            solve_affine_least_squares_4(matrix, [0.0, 0.0, 0.0, gauge_rhs], gauge.derivative)?;
        self.evaluations
            .insert(parameter, (parameters, Some(derivative), None));
        Ok(derivative)
    }

    /// Second parameter derivatives, from differentiating the coincidence of
    /// both surfaces and the guide gauge once more. `bend` is the difference
    /// of the surfaces' second-order terms along the tangent.
    fn parameter_second_derivative(
        &self,
        parameter: f64,
        parameters: [f64; 4],
        derivative: [f64; 4],
        tangents: [Vec3; 4],
        bend: Vec3,
    ) -> KernelResult<[f64; 4]> {
        let low = self.range.start.min(self.range.end);
        let high = self.range.start.max(self.range.end);
        let parameter = parameter.clamp(low, high);
        let gauge = self.gauge(parameter)?;
        // The gauge tangent's curvature only weighs the small deviation of the
        // converged parameters from the guide, so a difference suffices.
        let step = 1.0e-6 * (high - low);
        let before = (parameter - step).max(low);
        let after = (parameter + step).min(high);
        let early = self.gauge(before)?.tangent_derivative;
        let late = self.gauge(after)?.tangent_derivative;
        let tangent_second: [f64; 4] =
            std::array::from_fn(|index| (late[index] - early[index]) / (after - before));
        let gauge_rhs = dot4(gauge.tangent, gauge.second)
            - 2.0 * dot4(gauge.tangent_derivative, sub4(derivative, gauge.derivative))
            - dot4(tangent_second, sub4(parameters, gauge.guide));
        let [first_du, first_dv, second_du, second_dv] = tangents;
        let matrix = [
            [first_du.x, first_dv.x, -second_du.x, -second_dv.x],
            [first_du.y, first_dv.y, -second_du.y, -second_dv.y],
            [first_du.z, first_dv.z, -second_du.z, -second_dv.z],
            gauge.tangent,
        ];
        // At a tangential contact both surfaces share a normal along which their
        // bending cannot agree; the least-squares value is the best estimate.
        affine_least_squares_4(matrix, [bend.x, bend.y, bend.z, gauge_rhs], gauge.second)
    }

    fn corrected_parameters(&self, parameter: f64) -> KernelResult<[f64; 4]> {
        if !self.range.contains(parameter, self.tolerance) {
            return Err(invalid(
                "surface-intersection parameter lies outside its certified range",
            ));
        }
        let parameter = parameter.clamp(
            self.range.start.min(self.range.end),
            self.range.start.max(self.range.end),
        );
        if let Some((parameters, _, _)) = self.evaluations.get(parameter) {
            return Ok(parameters);
        }
        let first_guide = self.first_pcurve.evaluate(parameter)?;
        let second_guide = self.second_pcurve.evaluate(parameter)?;
        let guide = [
            first_guide.point.x,
            first_guide.point.y,
            second_guide.point.x,
            second_guide.point.y,
        ];
        let tangent = [
            first_guide.first_derivative.x,
            first_guide.first_derivative.y,
            second_guide.first_derivative.x,
            second_guide.first_derivative.y,
        ];
        let tangent_norm = tangent
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if tangent_norm <= self.tolerance.parameter {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "surface-intersection guide has a singular tangent",
            ));
        }
        let tangent = tangent.map(|value| value / tangent_norm);
        let mut parameters = guide;
        // Newton needs no second derivatives; evaluating them on rolling
        // surfaces would repeatedly finite-difference the same contact curves.
        for _ in 0..24 {
            let (first_point, first_du, first_dv) = self
                .first_surface
                .first_order(parameters[0], parameters[1])?;
            let (second_point, second_du, second_dv) = self
                .second_surface
                .first_order(parameters[2], parameters[3])?;
            let residual = first_point - second_point;
            let gauge = parameters
                .iter()
                .zip(guide)
                .zip(tangent)
                .map(|((value, guide), tangent)| (value - guide) * tangent)
                .sum::<f64>();
            // The model tolerance certifies that the retained guide selects the
            // right intersection branch; it must not define curve evaluation.
            // Stopping at that comparatively coarse threshold switches between
            // corrected and uncorrected parameterizations along one edge and
            // creates a false derivative kink. Converge the implicit identity
            // to floating-point accuracy so position and derivatives describe
            // one continuous curve.
            let coordinate_scale = first_point
                .to_vec()
                .magnitude()
                .max(second_point.to_vec().magnitude())
                .max(1.0);
            let parameter_scale = parameters
                .iter()
                .chain(guide.iter())
                .map(|value| value.abs())
                .fold(1.0_f64, f64::max);
            let linear_target = f64::EPSILON * coordinate_scale * 2048.0;
            let parameter_target = f64::EPSILON * parameter_scale * 2048.0;
            if residual.magnitude() <= linear_target && gauge.abs() <= parameter_target {
                self.evaluations.insert(parameter, (parameters, None, None));
                return Ok(parameters);
            }
            let matrix = [
                [first_du.x, first_dv.x, -second_du.x, -second_dv.x],
                [first_du.y, first_dv.y, -second_du.y, -second_dv.y],
                [first_du.z, first_dv.z, -second_du.z, -second_dv.z],
                tangent,
            ];
            let Some(delta) =
                solve_linear(matrix, [-residual.x, -residual.y, -residual.z, -gauge])
            else {
                break;
            };
            let mut accepted = false;
            let previous = residual.magnitude2() + gauge * gauge;
            let mut damping = 1.0;
            for _ in 0..12 {
                let mut candidate =
                    std::array::from_fn(|index| parameters[index] + delta[index] * damping);
                if parameters_in_surface_domains(
                    &self.first_surface,
                    [candidate[0], candidate[1]],
                    self.tolerance,
                ) && parameters_in_surface_domains(
                    &self.second_surface,
                    [candidate[2], candidate[3]],
                    self.tolerance,
                ) {
                    // Domain acceptance includes the parameter tolerance;
                    // evaluators require coordinates inside the natural box.
                    for (axis, domain) in self
                        .first_surface
                        .domains()
                        .into_iter()
                        .chain(self.second_surface.domains())
                        .enumerate()
                    {
                        if domain.period.is_none() {
                            if let DomainBounds::Bounded(range) = domain.bounds {
                                candidate[axis] = candidate[axis]
                                    .clamp(range.start.min(range.end), range.start.max(range.end));
                            }
                        }
                    }
                    let (first_point, _, _) =
                        self.first_surface.first_order(candidate[0], candidate[1])?;
                    let (second_point, _, _) = self
                        .second_surface
                        .first_order(candidate[2], candidate[3])?;
                    let residual = first_point - second_point;
                    let gauge = candidate
                        .iter()
                        .zip(guide)
                        .zip(tangent)
                        .map(|((value, guide), tangent)| (value - guide) * tangent)
                        .sum::<f64>();
                    if residual.magnitude2() + gauge * gauge < previous {
                        parameters = candidate;
                        accepted = true;
                        break;
                    }
                }
                damping *= 0.5;
            }
            if !accepted {
                break;
            }
        }
        let (first_point, _, _) = self
            .first_surface
            .first_order(parameters[0], parameters[1])?;
        let (second_point, _, _) = self
            .second_surface
            .first_order(parameters[2], parameters[3])?;
        let scale = first_point
            .to_vec()
            .magnitude()
            .max(second_point.to_vec().magnitude())
            .max(1.0);
        if first_point.distance(second_point)
            <= self
                .achieved_error
                .max(self.tolerance.linear_at_scale(scale))
        {
            return Ok(parameters);
        }
        Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            format!(
                "surface-intersection corrector did not converge at parameter {parameter} (guide residual {}, corrected residual {})",
                self.first_surface
                    .evaluate(guide[0], guide[1])?
                    .point
                    .distance(self.second_surface.evaluate(guide[2], guide[3])?.point),
                first_point.distance(second_point),
            ),
        ))
    }

    fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        let parameters = self.corrected_parameters(parameter)?;
        let first = self.first_surface.evaluate(parameters[0], parameters[1])?;
        let second = self.second_surface.evaluate(parameters[2], parameters[3])?;
        let derivative = self.parameter_derivative(parameter, parameters)?;
        let first_derivative = first.du * derivative[0] + first.dv * derivative[1];
        // Both surfaces bend the curve; the parameters' own acceleration makes
        // up the difference between them.
        let bend = |surface: &SurfaceEvaluation, du: f64, dv: f64| {
            surface.duu * du.powi(2) + surface.duv * (2.0 * du * dv) + surface.dvv * dv.powi(2)
        };
        let first_bend = bend(&first, derivative[0], derivative[1]);
        let second_bend = bend(&second, derivative[2], derivative[3]);
        let acceleration = self.parameter_second_derivative(
            parameter,
            parameters,
            derivative,
            [first.du, first.dv, second.du, second.dv],
            second_bend - first_bend,
        )?;
        let second_derivative = first_bend + first.du * acceleration[0] + first.dv * acceleration[1];
        Ok(CurveEvaluation {
            point: Point3::from_vec((first.point.to_vec() + second.point.to_vec()) * 0.5),
            first_derivative,
            second_derivative,
        })
    }

    fn first_order(&self, parameter: f64) -> KernelResult<(Point3, Vec3)> {
        // Trim projections and boundary checks revisit the same parameters.
        // Cache the exact spatial differential with the converged chart data.
        if let Some((_, _, Some(value))) = self.evaluations.get(parameter) {
            return Ok(value);
        }
        let parameters = self.corrected_parameters(parameter)?;
        let derivative = self.parameter_derivative(parameter, parameters)?;
        let value = self.first_order_at(parameters, derivative)?;
        self.evaluations.insert(parameter, (parameters, Some(derivative), Some(value)));
        Ok(value)
    }

    fn first_order_at(
        &self,
        parameters: [f64; 4],
        derivative: [f64; 4],
    ) -> KernelResult<(Point3, Vec3)> {
        let (first_point, first_du, first_dv) = self
            .first_surface
            .first_order(parameters[0], parameters[1])?;
        let (second_point, _, _) = self
            .second_surface
            .first_order(parameters[2], parameters[3])?;
        Ok((
            Point3::from_vec((first_point.to_vec() + second_point.to_vec()) * 0.5),
            first_du * derivative[0] + first_dv * derivative[1],
        ))
    }

    fn project(&self, point: Point3) -> KernelResult<Projection1> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let mut seeds = (0..=32)
            .map(|index| self.range.start + self.range.span() * index as f64 / 32.0)
            .collect::<Vec<_>>();
        self.first_pcurve.collect_parameter_breaks(&mut seeds);
        self.second_pcurve.collect_parameter_breaks(&mut seeds);
        seeds.retain(|parameter| self.range.contains(*parameter, self.tolerance));
        seeds.sort_by(f64::total_cmp);
        seeds.dedup();
        let parameter = seeds.into_iter()
            .filter_map(|parameter| {
                self.evaluate(parameter)
                    .ok()
                    .map(|value| (parameter, value.point.distance2(point)))
            })
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|value| value.0)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "surface-intersection projection has no seed",
                )
            })?;
        let projected = self.evaluate(parameter)?.point;
        refine_curve_projection(
            point,
            self.range,
            self.tolerance,
            Projection1 {
                parameter,
                point: projected,
                distance: projected.distance(point),
            },
            |parameter| self.first_order(parameter),
        )
    }
}

// Refine a seeded projection without leaving its retained parameter interval.
// Shared by complete implicit curves and trimmed Boolean arrangement edges.
fn refine_curve_projection(
    point: Point3,
    range: ParameterRange,
    tolerance: ModelTolerance,
    mut best: Projection1,
    first_order: impl Fn(f64) -> KernelResult<(Point3, Vec3)>,
) -> KernelResult<Projection1> {
    let low = range.start.min(range.end);
    let high = range.start.max(range.end);
    // Corrected implicit intersections provide a reliable first derivative,
    // but not full curvature. Damping keeps the distance decreasing without
    // depending on an incomplete second derivative or leaving the trim.
    for _ in 0..64 {
        let (projected, tangent) = first_order(best.parameter)?;
        let residual = projected - point;
        let speed2 = tangent.magnitude2();
        if speed2 <= f64::EPSILON {
            break;
        }
        let mut step = residual.dot(tangent) / speed2;
        let mut next = best;
        for _ in 0..24 {
            let parameter = (best.parameter - step).clamp(low, high);
            let Ok((projected, _)) = first_order(parameter) else {
                step *= 0.5;
                continue;
            };
            let distance = projected.distance(point);
            if distance < best.distance {
                next = Projection1 {
                    parameter,
                    point: projected,
                    distance,
                };
                break;
            }
            step *= 0.5;
        }
        let converged = (next.parameter - best.parameter).abs() <= tolerance.parameter;
        best = next;
        if converged {
            break;
        }
    }
    Ok(best)
}

fn parameters_in_surface_domains(
    surface: &Surface3,
    parameters: [f64; 2],
    tolerance: ModelTolerance,
) -> bool {
    surface.domains()[0].contains(parameters[0], tolerance.parameter)
        && surface.domains()[1].contains(parameters[1], tolerance.parameter)
}


/// Best least-squares solution of `matrix * value = right`, choosing the one
/// closest to `reference` when the system loses rank at a tangential
/// intersection. The system need not be consistent.
///
/// The symmetric Jacobi decomposition computes the Moore-Penrose inverse of
/// the normal matrix.  Unlike substituting the guide derivative when Gaussian
/// elimination becomes singular, this preserves every independent exact
/// surface and gauge constraint and remains well-defined at rank transitions.
fn affine_least_squares_4(
    matrix: [[f64; 4]; 4],
    right: [f64; 4],
    reference: [f64; 4],
) -> KernelResult<[f64; 4]> {
    // A regular surface/surface intersection has four independent tangent
    // constraints. Solve that system directly: forming A^T A would square its
    // condition number and can manufacture a discontinuous rank transition
    // near an otherwise regular endpoint. Only use the affine pseudoinverse
    // below when the original system is genuinely rank deficient.
    if let Some(solution) = solve_linear(matrix, right) {
        let achieved = (0..4)
            .map(|row| {
                ((0..4)
                    .map(|column| matrix[row][column] * solution[column])
                    .sum::<f64>()
                    - right[row])
                    .powi(2)
            })
            .sum::<f64>()
            .sqrt();
        let scale = right
            .iter()
            .chain(solution.iter())
            .map(|value| value.abs())
            .fold(1.0_f64, f64::max);
        if achieved.is_finite() && achieved <= f64::EPSILON.sqrt() * scale * 64.0 {
            return Ok(solution);
        }
    }
    let residual = std::array::from_fn::<_, 4, _>(|row| {
        right[row]
            - (0..4)
                .map(|column| matrix[row][column] * reference[column])
                .sum::<f64>()
    });
    let mut normal: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..4)
                .map(|index| matrix[index][row] * matrix[index][column])
                .sum::<f64>()
        })
    });
    let projected = std::array::from_fn::<_, 4, _>(|column| {
        (0..4)
            .map(|row| matrix[row][column] * residual[row])
            .sum::<f64>()
    });
    let mut eigenvectors: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| if row == column { 1.0 } else { 0.0 })
    });
    for _ in 0..96 {
        let mut pivot = (0, 1);
        let mut largest = normal[0][1].abs();
        for row in 0..4 {
            for column in row + 1..4 {
                if normal[row][column].abs() > largest {
                    largest = normal[row][column].abs();
                    pivot = (row, column);
                }
            }
        }
        let diagonal_scale = (0..4)
            .map(|index| normal[index][index].abs())
            .fold(1.0_f64, f64::max);
        if largest <= f64::EPSILON * diagonal_scale * 64.0 {
            break;
        }
        let (first, second) = pivot;
        let off_diagonal = normal[first][second];
        let tau = (normal[second][second] - normal[first][first]) / (2.0 * off_diagonal);
        let tangent = tau.signum() / (tau.abs() + (1.0 + tau * tau).sqrt());
        let cosine = 1.0 / (1.0 + tangent * tangent).sqrt();
        let sine = tangent * cosine;
        let first_diagonal = normal[first][first];
        let second_diagonal = normal[second][second];
        normal[first][first] = cosine * cosine * first_diagonal
            - 2.0 * sine * cosine * off_diagonal
            + sine * sine * second_diagonal;
        normal[second][second] = sine * sine * first_diagonal
            + 2.0 * sine * cosine * off_diagonal
            + cosine * cosine * second_diagonal;
        normal[first][second] = 0.0;
        normal[second][first] = 0.0;
        for index in 0..4 {
            if index == first || index == second {
                continue;
            }
            let first_value = normal[index][first];
            let second_value = normal[index][second];
            normal[index][first] = cosine * first_value - sine * second_value;
            normal[first][index] = normal[index][first];
            normal[index][second] = sine * first_value + cosine * second_value;
            normal[second][index] = normal[index][second];
        }
        for row in &mut eigenvectors {
            let first_value = row[first];
            let second_value = row[second];
            row[first] = cosine * first_value - sine * second_value;
            row[second] = sine * first_value + cosine * second_value;
        }
    }
    let maximum_eigenvalue = (0..4)
        .map(|index| normal[index][index].abs())
        .fold(0.0_f64, f64::max);
    if !maximum_eigenvalue.is_finite() || maximum_eigenvalue <= f64::MIN_POSITIVE {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "surface-intersection tangent constraints have zero rank",
        ));
    }
    let eigenvalue_tolerance = maximum_eigenvalue * f64::EPSILON * 4096.0;
    let mut correction = [0.0; 4];
    for eigenvector in 0..4 {
        let eigenvalue = normal[eigenvector][eigenvector];
        if eigenvalue <= eigenvalue_tolerance {
            continue;
        }
        let coefficient = (0..4)
            .map(|row| eigenvectors[row][eigenvector] * projected[row])
            .sum::<f64>()
            / eigenvalue;
        for row in 0..4 {
            correction[row] += eigenvectors[row][eigenvector] * coefficient;
        }
    }
    Ok(std::array::from_fn(|index| reference[index] + correction[index]))
}

/// Solve `matrix * value = right` like `affine_least_squares_4`, but reject
/// systems whose constraints cannot all hold.
fn solve_affine_least_squares_4(
    matrix: [[f64; 4]; 4],
    right: [f64; 4],
    reference: [f64; 4],
) -> KernelResult<[f64; 4]> {
    let solution = affine_least_squares_4(matrix, right, reference)?;
    let achieved = (0..4)
        .map(|row| {
            ((0..4)
                .map(|column| matrix[row][column] * solution[column])
                .sum::<f64>()
                - right[row])
                .powi(2)
        })
        .sum::<f64>()
        .sqrt();
    let scale = right
        .iter()
        .chain(solution.iter())
        .map(|value| value.abs())
        .fold(1.0_f64, f64::max);
    let allowed = f64::EPSILON.sqrt() * scale * 256.0;
    if !achieved.is_finite() || achieved > allowed {
        return Err(KernelError::from_diagnostic(
            super::Diagnostic::new(
                DiagnosticKind::NumericalFailure,
                "surface-intersection tangent constraints are inconsistent",
            )
            .with_accuracy(allowed, achieved),
        ));
    }
    Ok(solution)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Curve3 {
    Line(Line3),
    Circle(Circle3),
    Nurbs(NurbsCurve3),
    SurfaceIntersection(SurfaceIntersectionCurve),
    SurfaceCurve(SurfaceCurve3),
}

impl Curve3 {
    fn collect_parameter_breaks(&self, parameters: &mut Vec<f64>) {
        match self {
            Self::Nurbs(curve) => parameters.extend_from_slice(curve.knots()),
            Self::SurfaceIntersection(curve) => {
                curve.first_pcurve.collect_parameter_breaks(parameters);
                curve.second_pcurve.collect_parameter_breaks(parameters);
            }
            Self::SurfaceCurve(curve) => curve.pcurve().collect_parameter_breaks(parameters),
            Self::Line(_) | Self::Circle(_) => {}
        }
    }

    pub fn domain(&self) -> ParameterDomain {
        match self {
            Self::Line(_) => ParameterDomain::unbounded(),
            Self::Circle(_) => ParameterDomain::ANGLE,
            Self::Nurbs(curve) => curve.domain(),
            Self::SurfaceIntersection(curve) => ParameterDomain {
                bounds: DomainBounds::Bounded(curve.range()),
                period: None,
            },
            Self::SurfaceCurve(curve) => ParameterDomain {
                bounds: DomainBounds::Bounded(curve.range()),
                period: match curve.pcurve() {
                    Curve2::Line(line) => curve
                        .surface()
                        .parameter_periods((line.origin().x, line.origin().y))
                        .ok()
                        .and_then(|periods| {
                            (0..2).find_map(|axis| {
                                (line.direction()[1 - axis] == 0.0 && line.direction()[axis] != 0.0)
                                    .then(|| {
                                        periods[axis]
                                            .map(|period| period / line.direction()[axis].abs())
                                    })
                                    .flatten()
                            })
                        }),
                    _ => None,
                },
            },
        }
    }

    pub fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        match self {
            Self::Line(line) => line.evaluate(parameter),
            Self::Circle(circle) => circle.evaluate(parameter),
            Self::Nurbs(curve) => curve.evaluate(parameter),
            Self::SurfaceIntersection(curve) => curve.evaluate(parameter),
            Self::SurfaceCurve(curve) => curve.evaluate(parameter),
        }
    }

    pub(crate) fn third_derivative(&self, parameter: f64) -> KernelResult<Vec3> {
        match self {
            Self::Line(_) => Ok(Vec3::new(0.0, 0.0, 0.0)),
            Self::Circle(_) => Ok(-self.evaluate(parameter)?.first_derivative),
            Self::Nurbs(curve) => {
                let value = curve.evaluate(parameter)?;
                let parameter = parameter.clamp(curve.knots[curve.degree], curve.knots[curve.control_points.len()]);
                let [weight, first, second, third] = homogeneous_curve_jet(
                    curve.degree, &curve.knots, curve.control_points.len(), parameter,
                    |index| {
                        let point = curve.control_points[index];
                        let weight = curve.weights[index];
                        Vec4::new(point.x * weight, point.y * weight, point.z * weight, weight)
                    },
                );
                Ok((third.truncate()
                    - value.point.to_vec() * third.w
                    - value.first_derivative * (3.0 * second.w)
                    - value.second_derivative * (3.0 * first.w)) / weight.w)
            }
            // Implicit curves have no closed-form jerk; difference their
            // curvature within the certified domain
            _ => {
                let step = 1.0e-5 * parameter.abs().max(1.0);
                let (mut before, mut after) = (parameter - step, parameter + step);
                if let DomainBounds::Bounded(range) = self.domain().bounds {
                    before = before.max(range.start.min(range.end));
                    after = after.min(range.start.max(range.end));
                }
                let ahead = self.evaluate(after)?.second_derivative;
                let behind = self.evaluate(before)?.second_derivative;
                Ok((ahead - behind) / (after - before))
            }
        }
    }

    /// Parameters inside `range` where the curve's smoothness may drop.
    pub(crate) fn integration_breakpoints(&self, range: ParameterRange) -> Vec<f64> {
        let mut parameters = Vec::new();
        self.collect_parameter_breaks(&mut parameters);
        parameters.retain(|value| {
            *value > range.start.min(range.end) && *value < range.start.max(range.end)
        });
        parameters.sort_by(f64::total_cmp);
        parameters.dedup();
        parameters
    }

    pub(crate) fn first_order(&self, parameter: f64) -> KernelResult<(Point3, Vec3)> {
        match self {
            Self::SurfaceIntersection(curve) => curve.first_order(parameter),
            Self::SurfaceCurve(curve) => curve.first_order(parameter),
            _ => self
                .evaluate(parameter)
                .map(|value| (value.point, value.first_derivative)),
        }
    }

    /// Closest point on a retained edge interval, including its endpoints.
    /// Searching the whole basis curve can select another branch or periodic
    /// copy and miss a nearby boundary of a thin Boolean region.
    pub(crate) fn project_on_trim(
        &self,
        point: Point3,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Projection1> {
        let low = range.start.min(range.end);
        let high = range.start.max(range.end);
        let mut seeds = vec![low, high];
        if matches!(self, Self::Line(_) | Self::Circle(_)) {
            // On a circle's axis every retained parameter minimizes distance.
            // A trim still has a well-defined closest point at either endpoint.
            let on_axis = matches!(self, Self::Circle(circle) if {
                let delta = point - circle.center();
                delta.dot(circle.x_radius()) == 0.0 && delta.dot(circle.y_radius()) == 0.0
            });
            let parameter = if on_axis { low } else { self.project(point, tolerance)?.parameter };
            let parameter = self.domain().nearest(parameter, (low + high) * 0.5);
            seeds.push(parameter.clamp(low, high));
        } else {
            seeds.extend((1..32).map(|i| low + (high - low) * i as f64 / 32.0));
            self.collect_parameter_breaks(&mut seeds);
        }
        seeds.retain(|parameter| *parameter >= low && *parameter <= high);
        // Nested intersection guides share knots. Each distinct parameter
        // needs one exact evaluation, irrespective of how many guides use it.
        seeds.sort_by(f64::total_cmp);
        seeds.dedup();
        let mut best = None;
        for parameter in seeds {
            let projected = self.first_order(parameter)?.0;
            let distance = projected.distance(point);
            if best
                .as_ref()
                .is_none_or(|value: &Projection1| distance < value.distance)
            {
                best = Some(Projection1 {
                    parameter,
                    point: projected,
                    distance,
                });
            }
        }
        let best = best.ok_or_else(|| invalid("curve trim has no projection seed"))?;
        if matches!(self, Self::Line(_) | Self::Circle(_)) {
            return Ok(best);
        }
        refine_curve_projection(point, range, tolerance, best, |parameter| {
            self.first_order(parameter)
        })
    }

    pub fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection1> {
        match self {
            Self::Line(line) => line.project(point),
            Self::Circle(circle) => circle.project(point),
            Self::Nurbs(curve) => curve.project(point, tolerance),
            Self::SurfaceIntersection(curve) => curve.project(point),
            Self::SurfaceCurve(curve) => curve.project(point),
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
impl From<SurfaceIntersectionCurve> for Curve3 {
    fn from(value: SurfaceIntersectionCurve) -> Self {
        Self::SurfaceIntersection(value)
    }
}
impl From<SurfaceCurve3> for Curve3 {
    fn from(value: SurfaceCurve3) -> Self {
        Self::SurfaceCurve(value)
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

    /// The spatial point at plane coordinates `uv`.
    pub fn point_at(self, uv: Point2) -> Point3 {
        self.origin + self.vector_at(uv.to_vec())
    }

    /// The spatial direction of the plane-coordinate vector `uv`.
    pub fn vector_at(self, uv: Vec2) -> Vec3 {
        self.u * uv.x + self.v * uv.y
    }

    /// Plane coordinates of the orthogonal projection of `point`.
    pub fn coordinates(self, point: Point3) -> KernelResult<Point2> {
        let (u, v) = self.project(point)?.parameters;
        Ok(Point2::new(u, v))
    }

    /// Lift an exact curve given in plane coordinates into space.
    pub fn lift_curve(self, curve: &Curve2, tolerance: ModelTolerance) -> KernelResult<Curve3> {
        Ok(match curve {
            Curve2::Line(line) => {
                Line3::new(self.point_at(line.origin()), self.vector_at(line.direction()))?.into()
            }
            Curve2::Circle(circle) => Circle3::new(
                self.point_at(circle.center()),
                self.u * circle.radius(),
                self.v * circle.radius(),
                tolerance,
            )?
            .into(),
            Curve2::Nurbs(curve) => NurbsCurve3::new(
                curve.degree(),
                curve.control_points().iter().map(|point| self.point_at(*point)).collect(),
                curve.weights().to_vec(),
                curve.knots().to_vec(),
            )?
            .into(),
            Curve2::SurfaceIntersection(curve) if curve.lies_on(&Surface3::Plane(self)) => {
                curve.spatial_curve()
            }
            Curve2::SurfaceProjection(curve) if curve.surface() == &Surface3::Plane(self) => {
                curve.curve().clone()
            }
            Curve2::SurfaceIntersection(_) | Curve2::SurfaceProjection(_) => {
                return Err(super::unsupported(
                    "an implicit p-curve can only be lifted onto its retained plane",
                ))
            }
            Curve2::Offset(_) | Curve2::Projected(_) => SurfaceCurve3::new(
                Surface3::Plane(self), curve.clone(),
                match curve.domain().bounds {
                    DomainBounds::Bounded(range) => range,
                    _ => return Err(super::unsupported("lifting an unbounded offset requires a trim")),
                }, tolerance,
            )?.into(),
        })
    }

    /// The exact plane-coordinate image of a line, circle or NURBS curve lying
    /// in this plane. Other curve types have no closed form here.
    pub(crate) fn flatten_curve(
        self,
        curve: &Curve3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Option<Curve2>> {
        Ok(match curve {
            Curve3::Line(line) => {
                let origin = self.coordinates(line.origin())?;
                let end = self.coordinates(line.origin() + line.direction())?;
                Some(Line2::new(origin, end - origin)?.into())
            }
            Curve3::Nurbs(curve) => Some(
                NurbsCurve2::new(
                    curve.degree(),
                    curve
                        .control_points()
                        .iter()
                        .map(|point| self.coordinates(*point))
                        .collect::<KernelResult<_>>()?,
                    curve.weights().to_vec(),
                    curve.knots().to_vec(),
                )?
                .into(),
            ),
            Curve3::Circle(circle) => {
                let center = self.coordinates(circle.center())?;
                let x = self.coordinates(circle.center() + circle.x_radius())? - center;
                let y = self.coordinates(circle.center() + circle.y_radius())? - center;
                let scale = x.magnitude().max(y.magnitude()).max(1.0);
                let allowed = tolerance.parameter * scale * 64.0;
                (x.x > 0.0
                    && y.y > 0.0
                    && x.y.abs() <= allowed
                    && y.x.abs() <= allowed
                    && (x.x - y.y).abs() <= allowed)
                    .then(|| Circle2::new(center, (x.x + y.y) * 0.5).map(Into::into))
                    .transpose()?
            }
            Curve3::SurfaceIntersection(_) | Curve3::SurfaceCurve(_) => None,
        })
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

/// An exact surface of revolution whose meridian is retained as a 2D curve.
///
/// The profile uses `(radius, axial_distance)` coordinates in `frame`; the
/// first surface parameter is the revolution angle and the second is the
/// profile-curve parameter. Canonical cylinders, cones, spheres, and tori are
/// represented by their dedicated types instead, while this type preserves
/// rotational structure for more general analytical or rational profiles.
#[derive(Debug, Clone, PartialEq)]
pub struct RevolvedSurface3 {
    frame: Frame3,
    profile: Curve2,
    profile_range: ParameterRange,
    tolerance: ModelTolerance,
}

/// Validated tensor-product rational B-spline surface.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurface3 {
    degrees: (usize, usize),
    control_points: Vec<Vec<Point3>>,
    weights: Vec<Vec<f64>>,
    knots: (Vec<f64>, Vec<f64>),
}

#[derive(Debug, Clone)]
pub(crate) struct BezierSurfacePatch {
    pub(crate) u_range: ParameterRange,
    pub(crate) v_range: ParameterRange,
    pub(crate) controls: Vec<Vec<Vec4>>,
}

fn bezier_controls_on_interval(controls: &[Vec4], a: f64, b: f64) -> Vec<Vec4> {
    let degree = controls.len() - 1;
    (0..=degree).map(|index| {
        let mut work = controls.to_vec();
        for step in 0..degree {
            let t = if step < degree - index { a } else { b };
            for j in 0..degree - step {
                work[j] = work[j] * (1.0 - t) + work[j + 1] * t;
            }
        }
        work[0]
    }).collect()
}

impl BezierSurfacePatch {
    pub(crate) fn controls_in_bounds(&self, bounds: [ParameterRange; 2]) -> Vec<Vec<Vec4>> {
        let a = (bounds[0].start - self.u_range.start) / self.u_range.span();
        let b = (bounds[0].end - self.u_range.start) / self.u_range.span();
        let mut net = self.controls.iter()
            .map(|row| bezier_controls_on_interval(row, a, b)).collect::<Vec<_>>();
        let a = (bounds[1].start - self.v_range.start) / self.v_range.span();
        let b = (bounds[1].end - self.v_range.start) / self.v_range.span();
        for column in 0..net[0].len() {
            let controls = net.iter().map(|row| row[column]).collect::<Vec<_>>();
            for (row, value) in bezier_controls_on_interval(&controls, a, b).into_iter().enumerate() {
                net[row][column] = value;
            }
        }
        net
    }

    fn coincident_in_bounds(&self, other: &Self, bounds: [ParameterRange; 2], tolerance: ModelTolerance) -> bool {
        let first = self.controls_in_bounds(bounds);
        let second = other.controls_in_bounds(bounds);
        let degrees = |net: &[Vec<Vec4>]| (net[0].len() - 1, net.len() - 1);
        let (p, q) = degrees(&first);
        let (r, s) = degrees(&second);
        let binomial = |degree: usize, index: usize| {
            (0..index.min(degree - index)).fold(1.0, |value, i| value * (degree - i) as f64 / (i + 1) as f64)
        };
        let factors = |a: usize, b: usize| (0..=a).map(|i| (0..=b).map(|j| {
            binomial(a, i) * binomial(b, j) / binomial(a + b, i + j)
        }).collect::<Vec<_>>()).collect::<Vec<_>>();
        let u_factors = factors(p, r);
        let v_factors = factors(q, s);
        let mut residual = vec![vec![Vec3::new(0.0, 0.0, 0.0); p + r + 1]; q + s + 1];
        let mut denominator = vec![vec![0.0; p + r + 1]; q + s + 1];
        for (v, row) in first.iter().enumerate() {
            for (u, a) in row.iter().enumerate() {
                for (w, row) in second.iter().enumerate() {
                    for (t, b) in row.iter().enumerate() {
                        let factor = u_factors[u][t] * v_factors[v][w];
                        residual[v + w][u + t] += (a.truncate() * b.w - b.truncate() * a.w) * factor;
                        denominator[v + w][u + t] += a.w * b.w * factor;
                    }
                }
            }
        }
        let scale = first.iter().flatten().chain(second.iter().flatten())
            .map(|point| (point.truncate() / point.w).magnitude()).fold(1.0_f64, f64::max);
        let linear = tolerance.linear_at_scale(scale);
        // Bernstein weights form a positive partition of unity. Bounding each
        // coefficient of A.xyz * B.w - B.xyz * A.w by its denominator therefore
        // bounds the distance everywhere in this patch, without point sampling.
        residual.iter().flatten().zip(denominator.iter().flatten())
            .all(|(error, weight)| *weight > 0.0 && error.magnitude() <= linear * weight)
    }
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

    /// The positive rational basis lies in the affine hull of its controls.
    /// Recognize planar supports from that hull, not from sampled positions.
    pub(crate) fn plane(&self, tolerance: ModelTolerance) -> KernelResult<Option<Plane3>> {
        let domains = self.domains();
        let midpoint = |domain: ParameterDomain| match domain.bounds {
            DomainBounds::Bounded(range) => (range.start + range.end) * 0.5,
            _ => unreachable!(),
        };
        let jet = self.evaluate(midpoint(domains[0]), midpoint(domains[1]))?;
        let plane = Plane3::from_origin_normal(jet.point, jet.du.cross(jet.dv))?;
        for point in self.control_points.iter().flatten() {
            if plane.project(*point)?.distance > tolerance.linear {
                return Ok(None);
            }
        }
        Ok(Some(plane))
    }
    pub fn weights(&self) -> &[Vec<f64>] {
        &self.weights
    }
    pub fn knots(&self) -> (&[f64], &[f64]) {
        (&self.knots.0, &self.knots.1)
    }

    pub fn domains(&self) -> [ParameterDomain; 2] {
        let mut domains = [
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
        ];
        let same = |a: Point3, b: Point3| {
            a.distance(b)
                <= f64::EPSILON
                    * 256.0
                    * a.to_vec().magnitude().max(b.to_vec().magnitude()).max(1.0)
        };
        let closed = [
            self.control_points
                .iter()
                .zip(&self.weights)
                .all(|(row, weights)| {
                    same(row[0], *row.last().unwrap()) && weights[0] == *weights.last().unwrap()
                }),
            self.control_points[0]
                .iter()
                .zip(self.control_points.last().unwrap())
                .all(|(a, b)| same(*a, *b))
                && self.weights[0] == *self.weights.last().unwrap(),
        ];
        for axis in 0..2 {
            if closed[axis] {
                if let DomainBounds::Bounded(range) = domains[axis].bounds {
                    domains[axis].period = Some(range.span());
                }
            }
        }
        domains
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

    /// Compare the complete rational control nets in a common parameter box.
    /// Continuation changes knots and controls, but retains this polynomial
    /// identity. Point samples alone cannot certify coincident spline supports.
    pub(crate) fn coincident_support(&self, other: &Self, tolerance: ModelTolerance) -> KernelResult<bool> {
        let first = self.bezier_patches()?;
        let second = other.bezier_patches()?;
        let mut overlap = false;
        for a in &first {
            for b in &second {
                let start = [a.u_range.start.max(b.u_range.start), a.v_range.start.max(b.v_range.start)];
                let end = [a.u_range.end.min(b.u_range.end), a.v_range.end.min(b.v_range.end)];
                if (0..2).any(|axis| end[axis] - start[axis] <= tolerance.parameter) {
                    continue;
                }
                overlap = true;
                let bounds = [ParameterRange::new(start[0], end[0])?, ParameterRange::new(start[1], end[1])?];
                if !a.coincident_in_bounds(b, bounds, tolerance) {
                    return Ok(false);
                }
            }
        }
        Ok(overlap)
    }

    /// Continue a single rational Bezier patch without changing its parameter
    /// chart or its values on the original domain. Shell openings can require
    /// an intersection just beyond a spline's original endpoint.
    pub(crate) fn extended_bezier(&self, padding: [f64; 2]) -> KernelResult<Self> {
        let (p, q) = self.degrees;
        let ranges = [
            ParameterRange::new(self.knots.0[p], self.knots.0[self.control_points[0].len()])?,
            ParameterRange::new(self.knots.1[q], self.knots.1[self.control_points.len()])?,
        ];
        let extend_axis = [
            self.control_points[0].len() == p + 1,
            self.control_points.len() == q + 1,
        ];
        let padding = std::array::from_fn::<_, 2, _>(|axis| {
            if extend_axis[axis] && self.domains()[axis].period.is_none() {
                padding[axis]
            } else {
                0.0
            }
        });
        // Blossom evaluation gives the controls on any affine subinterval,
        // including an interval extending beyond [0, 1], in homogeneous space.
        let extend = |controls: &[Vec4], axis: usize| -> Vec<Vec4> {
            if padding[axis] == 0.0 {
                return controls.to_vec();
            }
            let a = -padding[axis] / ranges[axis].span();
            let b = 1.0 - a;
            bezier_controls_on_interval(controls, a, b)
        };
        let mut net = self
            .homogeneous_rows()
            .iter()
            .map(|row| extend(row, 0))
            .collect::<Vec<_>>();
        for column in 0..net[0].len() {
            let controls = net.iter().map(|row| row[column]).collect::<Vec<_>>();
            for (row, value) in extend(&controls, 1).into_iter().enumerate() {
                net[row][column] = value;
            }
        }
        let knots = |axis: usize, degree: usize| {
            if padding[axis] == 0.0 {
                return if axis == 0 {
                    self.knots.0.clone()
                } else {
                    self.knots.1.clone()
                };
            }
            let mut knots = vec![ranges[axis].start - padding[axis]; degree + 1];
            knots.extend(vec![ranges[axis].end + padding[axis]; degree + 1]);
            knots
        };
        Self::new(
            self.degrees,
            net.iter()
                .map(|row| {
                    row.iter()
                        .map(|v| Point3::new(v.x / v.w, v.y / v.w, v.z / v.w))
                        .collect()
                })
                .collect(),
            net.iter()
                .map(|row| row.iter().map(|v| v.w).collect())
                .collect(),
            (knots(0, p), knots(1, q)),
        )
    }

    /// Extract exact rational Bézier patches by knot insertion in both
    /// parameter directions. This changes representation only; it is used for
    /// convex-hull intersection isolation and never tessellates the surface.
    pub(crate) fn bezier_patches(&self) -> KernelResult<Vec<BezierSurfacePatch>> {
        let mut controls = self.homogeneous_rows();
        let mut u_knots = self.knots.0.clone();
        let mut v_knots = self.knots.1.clone();
        let (u_degree, v_degree) = self.degrees;

        let u_start = u_knots[u_degree];
        let u_end = u_knots[controls[0].len()];
        let u_internal = u_knots
            .iter()
            .copied()
            .filter(|knot| *knot > u_start && *knot < u_end)
            .fold(Vec::<f64>::new(), |mut unique, knot| {
                if unique.last().is_none_or(|last| *last != knot) {
                    unique.push(knot);
                }
                unique
            });
        for knot in u_internal {
            while u_knots
                .iter()
                .filter(|candidate| **candidate == knot)
                .count()
                < u_degree
            {
                let old_knots = u_knots.clone();
                let mut next_knots = None;
                let mut next_rows = Vec::with_capacity(controls.len());
                for row in controls {
                    let mut row_knots = old_knots.clone();
                    let mut row = row;
                    insert_homogeneous_knot4(u_degree, knot, &mut row_knots, &mut row)?;
                    next_knots.get_or_insert(row_knots);
                    next_rows.push(row);
                }
                controls = next_rows;
                u_knots = next_knots.expect("a NURBS surface has at least one row");
            }
        }

        let v_start = v_knots[v_degree];
        let v_end = v_knots[controls.len()];
        let v_internal = v_knots
            .iter()
            .copied()
            .filter(|knot| *knot > v_start && *knot < v_end)
            .fold(Vec::<f64>::new(), |mut unique, knot| {
                if unique.last().is_none_or(|last| *last != knot) {
                    unique.push(knot);
                }
                unique
            });
        for knot in v_internal {
            while v_knots
                .iter()
                .filter(|candidate| **candidate == knot)
                .count()
                < v_degree
            {
                let old_knots = v_knots.clone();
                let width = controls[0].len();
                let mut columns = Vec::with_capacity(width);
                let mut next_knots = None;
                for u in 0..width {
                    let mut column = controls.iter().map(|row| row[u]).collect::<Vec<_>>();
                    let mut column_knots = old_knots.clone();
                    insert_homogeneous_knot4(v_degree, knot, &mut column_knots, &mut column)?;
                    next_knots.get_or_insert(column_knots);
                    columns.push(column);
                }
                let height = columns[0].len();
                controls = (0..height)
                    .map(|v| columns.iter().map(|column| column[v]).collect())
                    .collect();
                v_knots = next_knots.expect("a NURBS surface has at least one column");
            }
        }

        let u_spans = (u_degree..controls[0].len())
            .filter(|index| u_knots[*index + 1] > u_knots[*index])
            .collect::<Vec<_>>();
        let v_spans = (v_degree..controls.len())
            .filter(|index| v_knots[*index + 1] > v_knots[*index])
            .collect::<Vec<_>>();
        let mut patches = Vec::new();
        for v in v_spans {
            for u in &u_spans {
                patches.push(BezierSurfacePatch {
                    u_range: ParameterRange::new(u_knots[*u], u_knots[*u + 1])?,
                    v_range: ParameterRange::new(v_knots[v], v_knots[v + 1])?,
                    controls: controls[v - v_degree..=v]
                        .iter()
                        .map(|row| row[*u - u_degree..=*u].to_vec())
                        .collect(),
                });
            }
        }
        Ok(patches)
    }

    fn homogeneous_jet(&self, u: f64, v: f64) -> [[Vec4; 3]; 3] {
        std::array::from_fn(|order| {
            homogeneous_curve_jet(
                self.degrees.1,
                &self.knots.1,
                self.control_points.len(),
                v,
                |row| {
                    homogeneous_curve_jet::<_, 3>(
                        self.degrees.0,
                        &self.knots.0,
                        self.control_points[row].len(),
                        u,
                        |column| {
                            let point = self.control_points[row][column];
                            let weight = self.weights[row][column];
                            Vec4::new(point.x * weight, point.y * weight, point.z * weight, weight)
                        },
                    )[order]
                },
            )
        })
    }
    fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        let domains = self.domains();
        let in_domain = |domain: ParameterDomain, value: f64| match domain.bounds {
            super::DomainBounds::Bounded(range) => {
                if domain.period.is_some() {
                    return Some(domain.normalize(value));
                }
                let minimum = range.start.min(range.end);
                let maximum = range.start.max(range.end);
                let slack = f64::EPSILON
                    * minimum.abs().max(maximum.abs()).max(value.abs()).max(1.0)
                    * 256.0;
                (value >= minimum - slack && value <= maximum + slack)
                    .then_some(value.clamp(minimum, maximum))
            }
            _ => Some(value),
        };
        let Some(u) = u.is_finite().then(|| in_domain(domains[0], u)).flatten() else {
            return Err(invalid(
                "NURBS surface parameters lie outside its natural domain",
            ));
        };
        let Some(v) = v.is_finite().then(|| in_domain(domains[1], v)).flatten() else {
            return Err(invalid(
                "NURBS surface parameters lie outside its natural domain",
            ));
        };
        // A tensor-product NURBS only depends on the active controls in each
        // direction. Reuse the curve evaluator instead of constructing whole
        // derivative control nets for every surface sample.
        let jet = self.homogeneous_jet(u, v);
        if !jet.iter().flatten().all(|value| {
            value.x.is_finite() && value.y.is_finite() && value.z.is_finite() && value.w.is_finite()
        }) {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "NURBS surface evaluation produced a non-finite value",
            ));
        }
        let [value, dv, dvv] = jet[0];
        let [du, duv, _] = jet[1];
        let duu = jet[2][0];
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
        let constrain = |axis: usize, value: f64| {
            if domains[axis].period.is_some() {
                value
            } else if let DomainBounds::Bounded(range) = domains[axis].bounds {
                value.clamp(range.start, range.end)
            } else {
                value
            }
        };

        let mut converged = false;
        for _ in 0..80 {
            let evaluation = self.evaluate(u, v)?;
            let residual = evaluation.point - point;
            let mut gradient = [residual.dot(evaluation.du), residual.dot(evaluation.dv)];
            let mut h00 = evaluation.du.magnitude2() + residual.dot(evaluation.duu);
            let mut h01 = evaluation.du.dot(evaluation.dv) + residual.dot(evaluation.duv);
            let mut h11 = evaluation.dv.magnitude2() + residual.dot(evaluation.dvv);
            if h00 <= 0.0 || h11 <= 0.0 || h00 * h11 <= h01 * h01 {
                h00 = evaluation.du.magnitude2();
                h01 = evaluation.du.dot(evaluation.dv);
                h11 = evaluation.dv.magnitude2();
            }
            for (axis, parameter) in [u, v].into_iter().enumerate() {
                if domains[axis].period.is_none() {
                    let DomainBounds::Bounded(range) = domains[axis].bounds else {
                        continue;
                    };
                    if (parameter <= range.start && gradient[axis] >= 0.0)
                        || (parameter >= range.end && gradient[axis] <= 0.0)
                    {
                        gradient[axis] = 0.0;
                        h01 = 0.0;
                        if axis == 0 {
                            h00 = 1.0
                        } else {
                            h11 = 1.0
                        }
                    }
                }
            }
            let determinant = h00 * h11 - h01 * h01;
            if determinant.abs() <= f64::EPSILON {
                break;
            }
            let mut du = (-gradient[0] * h11 + gradient[1] * h01) / determinant;
            let mut dv = (-gradient[1] * h00 + gradient[0] * h01) / determinant;
            let previous_distance = residual.magnitude2();
            let mut next_u = constrain(0, u + du);
            let mut next_v = constrain(1, v + dv);
            for _ in 0..12 {
                let next_distance = self.evaluate(next_u, next_v)?.point.distance2(point);
                if next_distance <= previous_distance {
                    break;
                }
                du *= 0.5;
                dv *= 0.5;
                next_u = constrain(0, u + du);
                next_v = constrain(1, v + dv);
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

impl RevolvedSurface3 {
    pub fn new(
        frame: Frame3,
        profile: Curve2,
        profile_range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !profile
            .domain()
            .contains_range(profile_range, tolerance.parameter)
        {
            return Err(invalid(
                "revolved-surface profile does not cover its trim range",
            ));
        }
        let surface = Self {
            frame,
            profile,
            profile_range,
            tolerance,
        };
        for index in 0..=8 {
            let parameter = profile_range.start + profile_range.span() * index as f64 / 8.0;
            let profile = surface.profile.evaluate(parameter)?;
            let scale = profile.point.x.abs().max(profile.point.y.abs()).max(1.0);
            let linear = tolerance.linear_at_scale(scale);
            let boundary_pole = (index == 0 || index == 8)
                && profile.point.x.abs() <= linear
                && profile.first_derivative.magnitude() > linear;
            if profile.point.x < -linear
                || (profile.point.x <= linear && !boundary_pole)
                || profile.first_derivative.magnitude() <= linear
            {
                return Err(invalid(
                    "revolved-surface profile must remain regular and may meet its axis only at an isolated boundary pole",
                ));
            }
        }
        Ok(surface)
    }

    pub fn frame(&self) -> Frame3 {
        self.frame
    }

    pub fn axis(&self) -> Axis3 {
        self.frame.axis()
    }

    pub fn profile(&self) -> &Curve2 {
        &self.profile
    }

    pub fn profile_range(&self) -> ParameterRange {
        self.profile_range
    }

    fn profile_domain(&self) -> ParameterDomain {
        ParameterDomain {
            bounds: DomainBounds::Bounded(self.profile_range),
            period: self.profile.domain().period.filter(|period| {
                self.profile_range.span().abs() + self.tolerance.parameter >= *period
            }),
        }
    }

    fn evaluate(&self, angle: f64, parameter: f64) -> KernelResult<SurfaceEvaluation> {
        let parameter = self.profile_domain().normalize(parameter);
        if !angle.is_finite() || !self.profile_range.contains(parameter, self.tolerance) {
            return Err(invalid(
                "revolved-surface parameter lies outside its domain",
            ));
        }
        let parameter = parameter.clamp(
            self.profile_range.start.min(self.profile_range.end),
            self.profile_range.start.max(self.profile_range.end),
        );
        let profile = self.profile.evaluate(parameter)?;
        let (sin, cos) = angle.sin_cos();
        let radial = self.frame.x() * cos + self.frame.y() * sin;
        let tangent = -self.frame.x() * sin + self.frame.y() * cos;
        Ok(SurfaceEvaluation {
            point: self.frame.origin()
                + radial * profile.point.x
                + self.frame.z() * profile.point.y,
            du: tangent * profile.point.x,
            dv: radial * profile.first_derivative.x + self.frame.z() * profile.first_derivative.y,
            duu: -radial * profile.point.x,
            duv: tangent * profile.first_derivative.x,
            dvv: radial * profile.second_derivative.x
                + self.frame.z() * profile.second_derivative.y,
        })
    }

    fn project(&self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.frame.origin();
        let x = delta.dot(self.frame.x());
        let y = delta.dot(self.frame.y());
        let axial = delta.dot(self.frame.z());
        let angle = if x == 0.0 && y == 0.0 {
            0.0
        } else {
            y.atan2(x).rem_euclid(std::f64::consts::TAU)
        };
        let target = Point2::new((x * x + y * y).sqrt(), axial);
        let mut parameter = (0..=32)
            .map(|index| self.profile_range.start + self.profile_range.span() * index as f64 / 32.0)
            .map(|parameter| {
                self.profile
                    .evaluate(parameter)
                    .map(|value| (parameter, value.point.distance2(target)))
            })
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|candidate| candidate.0)
            .ok_or_else(|| invalid("revolved-surface projection has no seed"))?;
        for _ in 0..32 {
            let value = self.profile.evaluate(parameter)?;
            let residual = value.point - target;
            let derivative =
                value.first_derivative.magnitude2() + residual.dot(value.second_derivative);
            if derivative.abs() <= f64::EPSILON * 256.0 {
                break;
            }
            let next = (parameter - residual.dot(value.first_derivative) / derivative).clamp(
                self.profile_range.start.min(self.profile_range.end),
                self.profile_range.start.max(self.profile_range.end),
            );
            if (next - parameter).abs() <= self.tolerance.parameter {
                parameter = next;
                break;
            }
            parameter = next;
        }
        let projected = self.evaluate(angle, parameter)?.point;
        Ok(Projection2 {
            parameters: (angle, parameter),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

/// Exact normal-offset definition for a regular parent surface. General NURBS
/// offsets are not NURBS in closed form, so retaining the parent and signed
/// distance is the non-approximating representation.
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetSurface3 {
    base: Box<Surface3>,
    distance: f64,
    v_slope: f64,
    tolerance: ModelTolerance,
}

impl OffsetSurface3 {
    pub fn new(base: Surface3, distance: f64, tolerance: ModelTolerance) -> KernelResult<Self> {
        tolerance.validate()?;
        if !distance.is_finite() || distance.abs() <= tolerance.linear {
            return Err(invalid(
                "surface offset distance must be finite and exceed model tolerance",
            ));
        }
        Ok(Self {
            base: Box::new(base),
            distance,
            v_slope: 0.0,
            tolerance,
        })
    }

    pub fn base(&self) -> &Surface3 {
        &self.base
    }

    pub fn distance(&self) -> f64 {
        self.distance
    }

    pub fn v_slope(&self) -> f64 {
        self.v_slope
    }

    /// An affine offset law in the parent's second parameter. A vertical
    /// extrusion with this law is an exact constant-angle draft surface.
    pub fn with_linear_distance(
        base: Surface3,
        distance: f64,
        v_slope: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !distance.is_finite() || !v_slope.is_finite()
            || !matches!(base.domains()[1].bounds, DomainBounds::Bounded(_))
        {
            return Err(invalid("a linear offset law needs finite coefficients and a bounded second parameter"));
        }
        Ok(Self { base: Box::new(base), distance, v_slope, tolerance })
    }

    pub(crate) fn maximum_distance(&self) -> f64 {
        if self.v_slope == 0.0 {
            return self.distance.abs();
        }
        let DomainBounds::Bounded(range) = self.base.domains()[1].bounds else {
            unreachable!()
        };
        (self.distance + self.v_slope * range.start).abs()
            .max((self.distance + self.v_slope * range.end).abs())
    }

    fn first_order(&self, u: f64, v: f64) -> KernelResult<(Point3, Vec3, Vec3)> {
        if let Surface3::RollingBall(blend) = self.base.as_ref() {
            // The ball's radial normal and its derivatives are exact. Using
            // finite-differenced surface curvature here magnifies spine knots.
            let (point, du, dv) = blend.first_order(u, v)?;
            let (center, tangent) = blend.center_curve.first_order(blend.center_parameter(u))?;
            let radial = (point - center) / blend.radius;
            let sign = du.cross(dv).dot(radial).signum();
            let distance = (self.distance + self.v_slope * v) * sign;
            return Ok((point + radial * distance,
                du + (du - tangent) * (distance / blend.radius),
                dv * (1.0 + distance / blend.radius) + radial * self.v_slope * sign));
        }
        let value = self.base.evaluate(u, v)?;
        let cross = value.du.cross(value.dv);
        let magnitude = cross.magnitude();
        if magnitude
            <= self
                .tolerance
                .linear_at_scale(value.du.magnitude().max(value.dv.magnitude()).max(1.0))
        {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "offset surface parent has a singular parameterization",
            ));
        }
        let normal = cross / magnitude;
        let project_tangent = |derivative: Vec3| derivative - normal * normal.dot(derivative);
        let cross_u = value.duu.cross(value.dv) + value.du.cross(value.duv);
        let cross_v = value.duv.cross(value.dv) + value.du.cross(value.dvv);
        let normal_u = project_tangent(cross_u) / magnitude;
        let normal_v = project_tangent(cross_v) / magnitude;
        let distance = self.distance + self.v_slope * v;
        Ok((
            value.point + normal * distance,
            value.du + normal_u * distance,
            value.dv + normal_v * distance + normal * self.v_slope,
        ))
    }

    fn shifted_parameter(&self, direction: usize, parameter: f64, delta: f64) -> f64 {
        let domain = self.base.domains()[direction];
        let shifted = parameter + delta;
        if domain.period.is_some() {
            shifted
        } else if let DomainBounds::Bounded(range) = domain.bounds {
            shifted.clamp(range.start.min(range.end), range.start.max(range.end))
        } else {
            shifted
        }
    }

    fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        let (point, du, dv) = self.first_order(u, v)?;
        let domains = self.base.domains();
        let parameter_scale = domains
            .iter()
            .filter_map(|domain| match domain.bounds {
                DomainBounds::Bounded(range) => Some(range.span().abs()),
                _ => None,
            })
            .fold(1.0_f64, f64::max);
        let step = (self.tolerance.parameter.sqrt() * parameter_scale)
            .max(f64::EPSILON.sqrt() * parameter_scale);
        let u_minus = self.shifted_parameter(0, u, -step);
        let u_plus = self.shifted_parameter(0, u, step);
        let v_minus = self.shifted_parameter(1, v, -step);
        let v_plus = self.shifted_parameter(1, v, step);
        let (_, du_minus, dv_u_minus) = self.first_order(u_minus, v)?;
        let (_, du_plus, dv_u_plus) = self.first_order(u_plus, v)?;
        let (_, du_v_minus, dv_minus) = self.first_order(u, v_minus)?;
        let (_, du_v_plus, dv_plus) = self.first_order(u, v_plus)?;
        let u_span = u_plus - u_minus;
        let v_span = v_plus - v_minus;
        if u_span.abs() <= self.tolerance.parameter || v_span.abs() <= self.tolerance.parameter {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "offset surface derivative stencil collapsed at a parameter boundary",
            ));
        }
        let duu = (du_plus - du_minus) / u_span;
        let dvv = (dv_plus - dv_minus) / v_span;
        let duv = ((dv_u_plus - dv_u_minus) / u_span + (du_v_plus - du_v_minus) / v_span) * 0.5;
        Ok(SurfaceEvaluation {
            point,
            du,
            dv,
            duu,
            duv,
            dvv,
        })
    }

    fn project(&self, point: Point3) -> KernelResult<Projection2> {
        let domains = self.base.domains();
        let bounded = match (domains[0].bounds, domains[1].bounds) {
            (DomainBounds::Bounded(u), DomainBounds::Bounded(v)) => Some([u, v]),
            _ => None,
        };
        // An unbounded blend has no finite grid to search. Its parent provides
        // a local seed in the same UV chart; bounded parents keep the grid path.
        let parent_seed = if bounded.is_none() {
            self.base
                .project(point, self.tolerance)
                .ok()
                .map(|projection| projection.parameters)
        } else {
            None
        };
        let parameters = bounded
            .into_iter()
            .flat_map(|[u_range, v_range]| {
                (0..=8).flat_map(move |u_index| {
                    (0..=8).map(move |v_index| {
                        (
                            u_range.start + u_range.span() * u_index as f64 / 8.0,
                            v_range.start + v_range.span() * v_index as f64 / 8.0,
                        )
                    })
                })
            })
            .chain(parent_seed)
            .filter_map(|parameters| {
                self.evaluate(parameters.0, parameters.1)
                    .ok()
                    .map(|value| (parameters, value.point.distance2(point)))
            })
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|candidate| candidate.0)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "offset surface projection has no seed",
                )
            })?;
        refine_surface_projection(
            point, Point2::new(parameters.0, parameters.1), domains,
            |u, v| self.first_order(u, v),
            |_, _| f64::EPSILON * 256.0,
            |step| step.x.abs().max(step.y.abs()) <= self.tolerance.parameter,
        )
    }
}

/// Exact circular or ruled cross sections between two retained contact rails.
///
/// The center curve is the intersection of the two inward offsets. Its two
/// p-curves retain the corresponding parameters on the un-offset supports.
/// Consequently this is an analytical/implicit surface definition rather than
/// a fitted patch: every evaluation is constructed from the exact center and
/// contact points on the parent surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeBlendSurface3 {
    ruled: bool,
    shared_intersection: bool,
    center_curve: Box<Curve3>,
    first_surface: Box<Surface3>,
    second_surface: Box<Surface3>,
    first_pcurve: Curve2,
    second_pcurve: Curve2,
    center_range: ParameterRange,
    center_period: Option<f64>,
    radius: f64,
    tolerance: ModelTolerance,
    sections: ParameterEvaluationCache<BlendSection>,
}

#[derive(Clone, Copy)]
struct BlendSection {
    first: (Point3, Vec3),
    second: (Point3, Vec3),
    circular: Option<CircularBlendSection>,
}

/// Prepared differential data shared by every transverse sample of a section.
/// Geometry validation and the circular frame depend on the rail parameter,
/// not on the position across the fillet.
#[derive(Clone, Copy)]
struct CircularBlendSection {
    center_point: Point3,
    center_tangent: Vec3,
    first_unit: Vec3,
    second_unit: Vec3,
    first_derivative: Vec3,
    second_derivative: Vec3,
    angle: f64,
    sine: f64,
    angle_cosine: f64,
    angle_derivative: f64,
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

    pub(crate) fn is_ruled(&self) -> bool {
        self.ruled
    }

    pub fn center_curve(&self) -> &Curve3 {
        &self.center_curve
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

    fn contact(
        surface: &Surface3,
        pcurve: &Curve2,
        parameter: f64,
    ) -> KernelResult<(Point3, Vec3)> {
        let (parameters, parameter_tangent) = pcurve.first_order(parameter)?;
        Self::contact_at(surface, parameters, parameter_tangent)
    }

    fn contact_at(
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
    fn section(&self, parameter: f64) -> KernelResult<BlendSection> {
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

    fn circular_section(
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
        let angle = cosine.acos();
        if angle <= self.tolerance.angular || std::f64::consts::PI - angle <= self.tolerance.angular
        {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "rolling-ball contact normals do not define a regular circular span",
            ));
        }
        let sine = angle.sin();
        let first_derivative = (first.1 - center_tangent) / self.radius;
        let second_derivative = (second.1 - center_tangent) / self.radius;
        let cosine_derivative =
            first_derivative.dot(second_unit) + first_unit.dot(second_derivative);
        let angle_derivative = -cosine_derivative / sine;
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

    fn center_parameter(&self, parameter: f64) -> f64 {
        self.center_period.map_or(parameter, |period| {
            self.center_range.start.min(self.center_range.end)
                + (parameter - self.center_range.start.min(self.center_range.end))
                    .rem_euclid(period)
        })
    }

    fn blend_period(&self, parameter: f64) -> KernelResult<Option<f64>> {
        if self.ruled {
            return Ok(None);
        }
        let section = self
            .section(self.center_parameter(parameter))?
            .circular
            .unwrap();
        Ok(Some(std::f64::consts::TAU / section.angle))
    }

    fn first_order(&self, parameter: f64, blend: f64) -> KernelResult<(Point3, Vec3, Vec3)> {
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

    fn point(&self, parameter: f64, blend: f64) -> KernelResult<Point3> {
        self.first_order(parameter, blend).map(|value| value.0)
    }

    fn shifted(&self, parameter: f64, delta: f64) -> f64 {
        (parameter + delta).clamp(
            self.center_range.start.min(self.center_range.end),
            self.center_range.start.max(self.center_range.end),
        )
    }

    fn evaluate(&self, parameter: f64, blend: f64) -> KernelResult<SurfaceEvaluation> {
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

    fn project(&self, point: Point3) -> KernelResult<Projection2> {
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

#[derive(Debug, Clone, PartialEq)]
pub enum Surface3 {
    Plane(Plane3),
    Cylinder(Cylinder3),
    Cone(Cone3),
    Sphere(Sphere3),
    Torus(Torus3),
    Revolved(RevolvedSurface3),
    Swept(super::SweptSurface3),
    Nurbs(NurbsSurface3),
    Offset(OffsetSurface3),
    RollingBall(RollingBallSurface3),
    Ruled(EdgeBlendSurface3),
}

impl Surface3 {
    /// A short human-readable name of the support type, for diagnostics.
    pub(crate) fn kind_name(&self) -> &'static str {
        match self {
            Self::Plane(_) => "plane",
            Self::Cylinder(_) => "cylinder",
            Self::Cone(_) => "cone",
            Self::Sphere(_) => "sphere",
            Self::Torus(_) => "torus",
            Self::Revolved(_) => "revolved",
            Self::Swept(_) => "swept",
            Self::Nurbs(_) => "NURBS",
            Self::Offset(_) => "offset",
            Self::RollingBall(_) => "rolling-ball",
            Self::Ruled(_) => "ruled",
        }
    }

    pub(crate) fn integration_breakpoints(&self, axis: usize) -> Vec<f64> {
        match self {
            Self::Nurbs(surface) => {
                let (u, v) = surface.knots();
                if axis == 0 { u.to_vec() } else { v.to_vec() }
            }
            Self::RollingBall(surface) if axis == 0 => {
                let mut knots = Vec::new();
                surface.center_curve.collect_parameter_breaks(&mut knots);
                surface.first_pcurve.collect_parameter_breaks(&mut knots);
                surface.second_pcurve.collect_parameter_breaks(&mut knots);
                knots.sort_by(f64::total_cmp);
                knots.dedup();
                knots
            }
            Self::Offset(surface) => surface.base().integration_breakpoints(axis),
            _ => Vec::new(),
        }
    }
    fn parameter_periods(&self, uv: (f64, f64)) -> KernelResult<[Option<f64>; 2]> {
        let mut periods = self.domains().map(|domain| domain.period);
        match self {
            Self::RollingBall(surface) => periods[1] = surface.blend_period(uv.0)?,
            Self::Offset(surface) => return surface.base.parameter_periods(uv),
            _ => {}
        }
        Ok(periods)
    }

    /// Construct a spatial boundary in the p-curve's parameterization. Use
    /// analytic geometry only when it follows from the defining surface and
    /// p-curve; no sampled recognition or geometric fitting is performed.
    pub(crate) fn boundary_curve(
        &self,
        pcurve: Curve2,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Curve3> {
        // A projected latitude circle is still an exact circle after a torus
        // offset. Preserve its periodic support so a seam endpoint can move past
        // its old trim when the neighboring face is offset.
        if let (Surface3::Torus(torus), Curve2::SurfaceProjection(projection)) = (self, &pcurve) {
            if let (Surface3::Torus(parent), Curve3::Circle(circle)) =
                (projection.surface(), projection.curve())
            {
                let axis = parent.normal();
                let delta = circle.center() - parent.center();
                if torus.center() == parent.center()
                    && torus.x_major() == parent.x_major()
                    && torus.y_major() == parent.y_major()
                    && (delta - axis * delta.dot(axis)).magnitude() <= tolerance.linear
                    && circle.normal().cross(axis).magnitude() <= tolerance.angular
                {
                    let v = pcurve.evaluate((range.start + range.end) * 0.5)?.point.y;
                    let radius = torus.major_radius() + torus.minor_radius() * v.cos();
                    return super::Circle3::new(
                        torus.center() + axis * (torus.minor_radius() * v.sin()),
                        circle.x_radius().normalize() * radius,
                        circle.y_radius().normalize() * radius,
                        tolerance,
                    )
                    .map(Into::into);
                }
            }
        }
        if let (Self::Plane(plane), Curve2::Circle(circle)) = (self, &pcurve) {
            let u = plane.u();
            let v = plane.v();
            if u.dot(v).abs() <= tolerance.angular * u.magnitude() * v.magnitude()
                && (u.magnitude() - v.magnitude()).abs() <= tolerance.linear {
                return Circle3::new(
                    plane.origin() + u * circle.center().x + v * circle.center().y,
                    u * circle.radius(), v * circle.radius(), tolerance,
                ).map(Curve3::from);
            }
        }
        if let Curve2::SurfaceProjection(projected) = &pcurve {
            if projected.surface() == self {
                return Ok(projected.curve().clone());
            }
            // Parallel offset planes retain their UV basis. Their boundary is
            // the source analytic curve translated between the two origins.
            if let (Self::Plane(from), Self::Plane(to)) = (projected.surface(), self) {
                if from.u() == to.u() && from.v() == to.v() {
                    let translation = to.origin() - from.origin();
                    match projected.curve() {
                        Curve3::Line(line) => {
                            return Line3::new(line.origin() + translation, line.direction())
                                .map(Curve3::from);
                        }
                        Curve3::Circle(circle) => {
                            return Circle3::new(
                                circle.center() + translation,
                                circle.x_radius(),
                                circle.y_radius(),
                                tolerance,
                            )
                            .map(Curve3::from);
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Curve2::Line(line) = &pcurve {
            let origin = line.origin();
            let direction = line.direction();
            match self {
                Self::Plane(plane) => {
                    return Line3::new(
                        plane.origin() + plane.u() * origin.x + plane.v() * origin.y,
                        plane.u() * direction.x + plane.v() * direction.y,
                    )
                    .map(Curve3::from);
                }
                Self::Cylinder(_) | Self::Cone(_) if direction.x == 0.0 => {
                    let value = self.evaluate(origin.x, origin.y)?;
                    return Line3::new(value.point, value.dv * direction.y).map(Curve3::from);
                }
                _ => {}
            }
            // Circle3 uses radians. Unit angular rates preserve the edge and
            // coedge parameters, including reversed traversal and seam crossings.
            let circular_frame = if direction.y == 0.0 && direction.x.abs() == 1.0 {
                match self {
                    Self::Cylinder(cylinder) => Some((
                        cylinder.origin() + cylinder.axis() * origin.y,
                        cylinder.x_radius(),
                        cylinder.y_radius(),
                    )),
                    Self::Cone(cone) if origin.y != 0.0 => Some((
                        cone.apex() + cone.axis() * origin.y,
                        cone.x_radius() * origin.y,
                        cone.y_radius() * origin.y,
                    )),
                    Self::Sphere(sphere) => Some((
                        sphere.center() + sphere.z_radius() * origin.y.sin(),
                        sphere.x_radius() * origin.y.cos(),
                        sphere.y_radius() * origin.y.cos(),
                    )),
                    Self::Revolved(surface) => {
                        let profile = surface.profile().evaluate(origin.y)?.point;
                        let frame = surface.frame();
                        Some((
                            frame.origin() + frame.z() * profile.y,
                            frame.x() * profile.x,
                            frame.y() * profile.x,
                        ))
                    }
                    Self::Torus(torus) => {
                        let scale =
                            1.0 + torus.minor_radius() * origin.y.cos() / torus.major_radius();
                        Some((
                            torus.center() + torus.normal() * (torus.minor_radius() * origin.y.sin()),
                            torus.x_major() * scale,
                            torus.y_major() * scale,
                        ))
                    }
                    _ => None,
                }
                .map(|(center, x, y)| (center, x, y, origin.x, direction.x))
            } else if direction.x == 0.0 && direction.y.abs() == 1.0 {
                match self {
                    Self::Sphere(sphere) => Some((
                        sphere.center(),
                        sphere.x_radius() * origin.x.cos() + sphere.y_radius() * origin.x.sin(),
                        sphere.z_radius(),
                    )),
                    Self::Torus(torus) => {
                        let radial =
                            torus.x_major() * origin.x.cos() + torus.y_major() * origin.x.sin();
                        Some((
                            torus.center() + radial,
                            radial * (torus.minor_radius() / torus.major_radius()),
                            torus.normal() * torus.minor_radius(),
                        ))
                    }
                    _ => None,
                }
                .map(|(center, x, y)| (center, x, y, origin.y, direction.y))
            } else {
                None
            };
            if let Some((center, x, y, angle, sense)) = circular_frame {
                return Circle3::new(
                    center,
                    x * angle.cos() + y * angle.sin(),
                    (-x * angle.sin() + y * angle.cos()) * sense,
                    tolerance,
                )
                .map(Curve3::from);
            }
        }
        SurfaceCurve3::new(self.clone(), pcurve, range, tolerance).map(Curve3::from)
    }

    pub(crate) fn first_order(&self, u: f64, v: f64) -> KernelResult<(Point3, Vec3, Vec3)> {
        match self {
            Self::Swept(surface) => surface.first_order(u, v),
            Self::Offset(surface) => surface.first_order(u, v),
            Self::RollingBall(surface) | Self::Ruled(surface) => surface.first_order(u, v),
            _ => self
                .evaluate(u, v)
                .map(|value| (value.point, value.du, value.dv)),
        }
    }

    pub fn domains(&self) -> [ParameterDomain; 2] {
        match self {
            Self::Plane(_) => [ParameterDomain::unbounded(), ParameterDomain::unbounded()],
            Self::Cylinder(_) | Self::Cone(_) => {
                [ParameterDomain::ANGLE, ParameterDomain::unbounded()]
            }
            Self::Sphere(_) => [
                ParameterDomain::ANGLE,
                ParameterDomain {
                    bounds: DomainBounds::Bounded(ParameterRange {
                        start: -std::f64::consts::FRAC_PI_2,
                        end: std::f64::consts::FRAC_PI_2,
                    }),
                    period: None,
                },
            ],
            Self::Torus(_) => [ParameterDomain::ANGLE, ParameterDomain::ANGLE],
            Self::Revolved(surface) => [ParameterDomain::ANGLE, surface.profile_domain()],
            Self::Swept(surface) => surface.domains(),
            Self::Nurbs(surface) => surface.domains(),
            Self::Offset(surface) => surface.base.domains(),
            Self::RollingBall(surface) | Self::Ruled(surface) => [
                ParameterDomain {
                    bounds: DomainBounds::Bounded(surface.center_range()),
                    period: surface.center_period,
                },
                ParameterDomain::unbounded(),
            ],
        }
    }

    pub fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        match self {
            Self::Plane(plane) => plane.evaluate(u, v),
            Self::Cylinder(cylinder) => cylinder.evaluate(u, v),
            Self::Cone(cone) => cone.evaluate(u, v),
            Self::Sphere(sphere) => sphere.evaluate(u, v),
            Self::Torus(torus) => torus.evaluate(u, v),
            Self::Revolved(surface) => surface.evaluate(u, v),
            Self::Swept(surface) => surface.evaluate(u, v),
            Self::Nurbs(surface) => surface.evaluate(u, v),
            Self::Offset(surface) => surface.evaluate(u, v),
            Self::RollingBall(surface) | Self::Ruled(surface) => surface.evaluate(u, v),
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
            _ => {
                let (_, du, dv) = self.first_order(u, v)?;
                let normal = du.cross(dv);
                if normal.magnitude2() == 0.0 || !finite_vector(normal) {
                    return Err(KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "surface derivatives are singular at the requested parameter",
                    ));
                }
                Ok(normal.normalize())
            }
        }
    }

    pub fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection2> {
        match self {
            Self::Plane(plane) => plane.project(point),
            Self::Cylinder(cylinder) => cylinder.project(point),
            Self::Cone(cone) => cone.project(point),
            Self::Sphere(sphere) => sphere.project(point),
            Self::Torus(torus) => torus.project(point),
            Self::Revolved(surface) => surface.project(point),
            Self::Swept(surface) => surface.project(point),
            Self::Nurbs(surface) => surface.project(point, tolerance),
            Self::Offset(surface) => surface.project(point),
            Self::RollingBall(surface) | Self::Ruled(surface) => surface.project(point),
        }
    }

    pub(crate) fn project_near(
        &self, point: Point3, seed: Point2, tolerance: ModelTolerance,
    ) -> KernelResult<Projection2> {
        refine_surface_projection(
            point, seed, self.domains(),
            |u, v| self.first_order(u, v),
            |a, c| f64::EPSILON * a.max(c).powi(2) * 256.0,
            |step| step.magnitude() <= tolerance.parameter,
        )
    }

    pub fn offset(&self, distance: f64, tolerance: ModelTolerance) -> KernelResult<Self> {
        match self {
            Self::Plane(plane) => Ok(Plane3::new(
                plane.origin() + plane.normal() * distance,
                plane.u(),
                plane.v(),
                tolerance,
            )?
            .into()),
            Self::Cylinder(cylinder) => {
                // `Cylinder3` permits its axial direction to be parallel or
                // antiparallel to the radial frame normal.  The parameter-space
                // surface normal (`du × dv`) points radially outward in the
                // first case and inward in the second, so an offset along that
                // normal changes the geometric radius with the same sign.
                let normal_sign = if cylinder
                    .axis()
                    .dot(cylinder.x_radius().cross(cylinder.y_radius()))
                    >= 0.0
                {
                    1.0
                } else {
                    -1.0
                };
                let radius = cylinder.radius() + distance * normal_sign;
                if radius <= tolerance.linear_at_scale(cylinder.radius()) {
                    return Err(invalid("cylinder offset collapses its radius"));
                }
                let scale = radius / cylinder.radius();
                Ok(Cylinder3::new(
                    cylinder.origin(),
                    cylinder.axis(),
                    cylinder.x_radius() * scale,
                    cylinder.y_radius() * scale,
                    tolerance,
                )?
                .into())
            }
            Self::Sphere(sphere) => {
                let source_radius = sphere.radius();
                let radius = source_radius + distance;
                if radius <= tolerance.linear_at_scale(source_radius) {
                    return Err(invalid("sphere offset collapses its radius"));
                }
                let scale = radius / source_radius;
                Ok(Sphere3::new(
                    sphere.center(),
                    sphere.x_radius() * scale,
                    sphere.y_radius() * scale,
                    sphere.z_radius() * scale,
                    tolerance,
                )?
                .into())
            }
            Self::Torus(torus) => {
                let minor_radius = torus.minor_radius() + distance;
                Ok(Torus3::new(
                    torus.center(),
                    torus.x_major(),
                    torus.y_major(),
                    minor_radius,
                    tolerance,
                )?
                .into())
            }
            _ => OffsetSurface3::new(self.clone(), distance, tolerance).map(Self::Offset),
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
impl From<RevolvedSurface3> for Surface3 {
    fn from(value: RevolvedSurface3) -> Self {
        Self::Revolved(value)
    }
}
impl From<NurbsSurface3> for Surface3 {
    fn from(value: NurbsSurface3) -> Self {
        Self::Nurbs(value)
    }
}
impl From<OffsetSurface3> for Surface3 {
    fn from(value: OffsetSurface3) -> Self {
        Self::Offset(value)
    }
}
impl From<RollingBallSurface3> for Surface3 {
    fn from(value: RollingBallSurface3) -> Self {
        if value.is_ruled() {
            Self::Ruled(value)
        } else {
            Self::RollingBall(value)
        }
    }
}
