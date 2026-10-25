//! Analytic and spline parameter-space curves.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line2 {
    pub(super) origin: Point2,
    pub(super) direction: Vec2,
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

    pub(super) fn evaluate(self, parameter: f64) -> KernelResult<Curve2Evaluation> {
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
    pub(super) center: Point2,
    pub(super) radius: f64,
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

    pub(super) fn evaluate(self, parameter: f64) -> KernelResult<Curve2Evaluation> {
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
    pub(super) degree: usize,
    pub(super) control_points: Vec<Point2>,
    pub(super) weights: Vec<f64>,
    pub(super) knots: Vec<f64>,
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

    pub(super) fn homogeneous_controls(&self) -> Vec<Vec3> {
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
        let mut knots = self.knots.to_vec();
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
    pub(super) fn extended_guide(&self, range: ParameterRange) -> KernelResult<Self> {
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

    pub(super) fn from_bezier_spans(degree: usize, spans: Vec<(ParameterRange, Vec<Vec3>)>) -> KernelResult<Self> {
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

    pub(super) fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
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
            return Err(geometry_error("2D NURBS has zero homogeneous weight"));
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

pub(super) fn insert_homogeneous_knot(
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

pub(super) fn insert_homogeneous_knot4(
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
