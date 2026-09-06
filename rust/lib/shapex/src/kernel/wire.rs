use cgmath::prelude::*;

use crate::{Point2, Vec2, Vec3};

use super::{
    Circle2, Curve2, DiagnosticKind, KernelError, KernelResult, Line2, ModelTolerance,
    ParameterRange,
};

/// A finite, oriented use of an exact two-dimensional basis curve.
///
#[derive(Debug, Clone, PartialEq)]
pub struct TrimmedCurve2 {
    curve: Curve2,
    range: ParameterRange,
}

impl TrimmedCurve2 {
    pub fn new(curve: impl Into<Curve2>, range: ParameterRange) -> KernelResult<Self> {
        let curve = curve.into();
        if !curve.domain().contains_range(range, f64::EPSILON * 64.0) {
            return Err(invalid("trim range lies outside the curve domain"));
        }
        let start = curve.evaluate(range.start)?.point;
        let end = curve.evaluate(range.end)?.point;
        let closed_period = curve
            .domain()
            .period
            .map(|period| (range.span().abs() - period).abs() <= f64::EPSILON * 64.0)
            .unwrap_or(false);
        if !closed_period && start == end {
            // A clamped NURBS can describe a closed loop without a periodic
            // basis. Distinguish that valid trim from a collapsed curve.
            let closed_nurbs = if matches!(curve, Curve2::Nurbs(_)) {
                let mut distinct = false;
                for fraction in [0.25, 0.5, 0.75] {
                    distinct |=
                        curve.evaluate(range.start + range.span() * fraction)?.point != start;
                }
                distinct
            } else {
                false
            };
            if !closed_nurbs {
                return Err(invalid("an open trimmed curve needs distinct endpoints"));
            }
        }
        Ok(Self { curve, range })
    }

    pub fn line(start: Point2, end: Point2) -> KernelResult<Self> {
        Self::new(
            Line2::new(start, end - start)?,
            ParameterRange::new(0.0, 1.0)?,
        )
    }

    pub fn circle(center: Point2, radius: f64) -> KernelResult<Self> {
        Self::new(
            Circle2::new(center, radius)?,
            ParameterRange::new(0.0, std::f64::consts::TAU)?,
        )
    }

    /// Construct the unique circular arc from `start` through `interior` to `end`.
    pub fn arc_through(start: Point2, interior: Point2, end: Point2) -> KernelResult<Self> {
        let twice_area =
            (interior.x - start.x) * (end.y - start.y) - (interior.y - start.y) * (end.x - start.x);
        let scale = start
            .to_vec()
            .magnitude()
            .max(interior.to_vec().magnitude())
            .max(end.to_vec().magnitude())
            .max(1.0);
        if twice_area.abs() <= f64::EPSILON * scale * scale * 64.0 {
            return Err(invalid("three arc points must not be collinear"));
        }
        let a2 = start.to_vec().magnitude2();
        let b2 = interior.to_vec().magnitude2();
        let c2 = end.to_vec().magnitude2();
        let denominator = 2.0
            * (start.x * (interior.y - end.y)
                + interior.x * (end.y - start.y)
                + end.x * (start.y - interior.y));
        let center = Point2::new(
            (a2 * (interior.y - end.y) + b2 * (end.y - start.y) + c2 * (start.y - interior.y))
                / denominator,
            (a2 * (end.x - interior.x) + b2 * (start.x - end.x) + c2 * (interior.x - start.x))
                / denominator,
        );
        let radius = center.distance(start);
        let parameter = |point: Point2| (point.y - center.y).atan2(point.x - center.x);
        let start_parameter = parameter(start);
        let middle_delta =
            (parameter(interior) - start_parameter).rem_euclid(std::f64::consts::TAU);
        let end_delta = (parameter(end) - start_parameter).rem_euclid(std::f64::consts::TAU);
        let end_parameter = if middle_delta < end_delta {
            start_parameter + end_delta
        } else {
            start_parameter - (std::f64::consts::TAU - end_delta)
        };
        Self::new(
            Circle2::new(center, radius)?,
            ParameterRange::new(start_parameter, end_parameter)?,
        )
    }

    pub fn curve(&self) -> &Curve2 {
        &self.curve
    }
    pub fn range(&self) -> ParameterRange {
        self.range
    }
    pub fn start(&self) -> KernelResult<Point2> {
        Ok(self.curve.evaluate(self.range.start)?.point)
    }
    pub fn end(&self) -> KernelResult<Point2> {
        Ok(self.curve.evaluate(self.range.end)?.point)
    }
    fn is_closed(&self, tolerance: ModelTolerance) -> KernelResult<bool> {
        Ok(self.start()?.distance(self.end()?) <= tolerance.linear)
    }
    pub fn reversed(&self) -> Self {
        Self {
            curve: self.curve.clone(),
            range: self.range.reversed(),
        }
    }

    fn signed_area(&self, tolerance: ModelTolerance) -> KernelResult<f64> {
        self.curve
            .signed_area_contribution(self.range, tolerance.parameter)
    }
}

/// A validated closed sequence of oriented 2D curve uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Wire2 {
    segments: Vec<TrimmedCurve2>,
    signed_area: f64,
}

impl Wire2 {
    /// Convenience constructor for a closed wire made entirely from lines.
    pub fn polygon(
        vertices: impl IntoIterator<Item = Point2>,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let mut vertices = vertices.into_iter().collect::<Vec<_>>();
        if vertices.len() > 1
            && vertices[0].distance(*vertices.last().unwrap_or(&vertices[0])) <= tolerance.linear
        {
            vertices.pop();
        }
        if vertices.len() < 3 {
            return Err(invalid("a polygon wire needs at least three vertices"));
        }
        let segments = (0..vertices.len())
            .map(|index| {
                TrimmedCurve2::line(vertices[index], vertices[(index + 1) % vertices.len()])
            })
            .collect::<KernelResult<Vec<_>>>()?;
        Self::new(segments, tolerance)
    }

    pub fn new(
        segments: impl IntoIterator<Item = TrimmedCurve2>,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        let segments: Vec<_> = segments.into_iter().collect();
        if segments.is_empty() {
            return Err(invalid("a wire needs at least one curve segment"));
        }
        for index in 0..segments.len() {
            let end = segments[index].end()?;
            let next = segments[(index + 1) % segments.len()].start()?;
            if end.distance(next)
                > tolerance.linear_at_scale(end.to_vec().magnitude().max(next.to_vec().magnitude()))
            {
                return Err(invalid("wire curve segments must connect end-to-start"));
            }
        }
        if segments.len() == 1 && !segments[0].is_closed(tolerance)? {
            return Err(invalid(
                "a one-segment wire must be a closed periodic curve",
            ));
        }
        validate_no_self_intersection(&segments, tolerance)?;
        let signed_area = segments
            .iter()
            .map(|segment| segment.signed_area(tolerance))
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .sum::<f64>();
        if signed_area.abs() <= tolerance.linear.powi(2) {
            return Err(invalid("wire must enclose an area larger than tolerance"));
        }
        Ok(Self {
            segments,
            signed_area,
        })
    }
    pub fn segments(&self) -> &[TrimmedCurve2] {
        &self.segments
    }
    pub fn signed_area(&self) -> f64 {
        self.signed_area
    }
    pub fn area(&self) -> f64 {
        self.signed_area.abs()
    }
    pub fn is_counter_clockwise(&self) -> bool {
        self.signed_area > 0.0
    }
    pub fn reversed(&self) -> Self {
        Self {
            segments: self
                .segments
                .iter()
                .rev()
                .map(TrimmedCurve2::reversed)
                .collect(),
            signed_area: -self.signed_area,
        }
    }
}

fn validate_no_self_intersection(
    segments: &[TrimmedCurve2],
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    if let Curve2::Circle(reference) = segments[0].curve() {
        let same_circle = segments.iter().all(|segment| {
            matches!(segment.curve(), Curve2::Circle(circle)
                if circle.center().distance(reference.center()) <= tolerance.linear
                    && (circle.radius() - reference.radius()).abs() <= tolerance.linear)
        });
        let total_turn = segments
            .iter()
            .map(|segment| segment.range().span())
            .sum::<f64>();
        let consistent_direction = segments
            .iter()
            .all(|segment| segment.range().span().signum() == total_turn.signum());
        if same_circle
            && consistent_direction
            && (total_turn.abs() - std::f64::consts::TAU).abs() <= tolerance.parameter
        {
            return Ok(());
        }
    }
    let rational_circle_arcs = segments
        .iter()
        .map(|segment| {
            let Curve2::Nurbs(curve) = segment.curve() else {
                return Ok(None);
            };
            if curve.degree() != 2
                || curve.control_points().len() != 3
                || curve.weights().len() != 3
            {
                return Ok(None);
            }
            let range = segment.range();
            let start = segment.curve().evaluate(range.start)?.point;
            let middle = segment
                .curve()
                .evaluate(range.start + range.span() * 0.5)?
                .point;
            let end = segment.curve().evaluate(range.end)?.point;
            let Ok(arc) = TrimmedCurve2::arc_through(start, middle, end) else {
                // A degree-elevated line is a valid rational quadratic, but it
                // does not participate in the circular-wire fast path.
                return Ok(None);
            };
            let Curve2::Circle(circle) = arc.curve() else {
                unreachable!()
            };
            let weight_ratio =
                curve.weights()[1] / (curve.weights()[0] * curve.weights()[2]).sqrt();
            let expected_weight = (arc.range().span().abs() * 0.5).cos();
            if (weight_ratio - expected_weight).abs() > tolerance.parameter * 32.0 {
                return Ok(None);
            }
            for unit in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let evaluation = segment
                    .curve()
                    .evaluate(range.start + range.span() * unit)?;
                let radial = evaluation.point - circle.center();
                if (radial.magnitude() - circle.radius()).abs()
                    > tolerance.linear_at_scale(circle.radius()) * 8.0
                {
                    return Ok(None);
                }
            }
            Ok(Some((*circle, arc.range().span())))
        })
        .collect::<KernelResult<Option<Vec<_>>>>()?;
    if let Some(arcs) = rational_circle_arcs {
        if let Some((reference, _)) = arcs.first() {
            let total_turn = arcs.iter().map(|(_, turn)| *turn).sum::<f64>();
            let same_circle = arcs.iter().all(|(circle, turn)| {
                circle.center().distance(reference.center()) <= tolerance.linear
                    && (circle.radius() - reference.radius()).abs() <= tolerance.linear
                    && turn.signum() == total_turn.signum()
            });
            if same_circle
                && (total_turn.abs() - std::f64::consts::TAU).abs() <= tolerance.parameter * 32.0
            {
                return Ok(());
            }
        }
    }
    for segment in segments {
        validate_segment_is_simple(segment, tolerance)?;
    }
    for first in 0..segments.len() {
        for second in first + 1..segments.len() {
            let adjacent = second == first + 1 || (first == 0 && second + 1 == segments.len());
            let first_endpoints = [segments[first].start()?, segments[first].end()?];
            let second_endpoints = [segments[second].start()?, segments[second].end()?];
            let shared = first_endpoints
                .into_iter()
                .flat_map(|first| {
                    second_endpoints
                        .into_iter()
                        .filter(move |second| first.distance(*second) <= tolerance.linear)
                        .map(move |_| first)
                })
                .collect::<Vec<_>>();
            let intersection =
                curve_pair_intersection(&segments[first], &segments[second], tolerance)?;
            if intersection.coincident
                || intersection.hits.iter().any(|hit| {
                    !adjacent
                        || shared
                            .iter()
                            // The generic solver accepts roots within this
                            // geometric band, including roots at tangencies.
                            .all(|shared| shared.distance(hit.point) > tolerance.linear * 8.0)
                })
            {
                return Err(invalid("wire must not self-intersect"));
            }
        }
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct CurvePairIntersection {
    pub hits: Vec<CurvePairHit>,
    pub coincident: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CurvePairHit {
    pub point: Point2,
    pub first_parameter: f64,
    pub second_parameter: f64,
}

fn validate_segment_is_simple(
    segment: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let Curve2::Nurbs(curve) = segment.curve() else {
        return Ok(());
    };
    if segment.start()?.distance(segment.end()?) <= tolerance.linear {
        return Err(invalid("a NURBS wire segment needs distinct endpoints"));
    }

    // Exact rational Bézier subdivision turns an arbitrary regular spline into
    // projection-monotone pieces. Each such piece is simple; testing every
    // non-adjacent pair then proves that the complete segment is simple too.
    // The subdivision changes representation only and never approximates the
    // curve.
    let mut pending = trimmed_bezier_spans(curve, segment.range())?
        .into_iter()
        .map(|(range, controls)| (range, controls, 0_u8))
        .collect::<Vec<_>>();
    let mut pieces = Vec::new();
    while let Some((range, controls, depth)) = pending.pop() {
        if bezier_controls_projection_monotone(&controls, tolerance.linear) {
            pieces.push((range, controls));
            continue;
        }
        if depth >= 32 || range.span().abs() <= tolerance.parameter * 16.0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "could not certify NURBS segment simplicity at the model tolerance",
            ));
        }
        let (left, right) = split_values(&controls);
        let middle = (range.start + range.end) * 0.5;
        pending.push((
            ParameterRange {
                start: middle,
                end: range.end,
            },
            right,
            depth + 1,
        ));
        pending.push((
            ParameterRange {
                start: range.start,
                end: middle,
            },
            left,
            depth + 1,
        ));
    }
    pieces.sort_by(|first, second| first.0.start.total_cmp(&second.0.start));
    for first in 0..pieces.len() {
        for second in first + 1..pieces.len() {
            let first_curve = TrimmedCurve2::new(curve.clone(), pieces[first].0)?;
            let second_curve = TrimmedCurve2::new(curve.clone(), pieces[second].0)?;
            let intersection = nurbs_nurbs_intersection(&first_curve, &second_curve, tolerance)?;
            let adjacent = second == first + 1;
            let shared = first_curve.end()?;
            if intersection.coincident
                || intersection
                    .hits
                    .iter()
                    .any(|hit| !adjacent || hit.point.distance(shared) > tolerance.linear)
            {
                return Err(invalid("wire must not self-intersect"));
            }
        }
    }
    Ok(())
}

fn bezier_controls_projection_monotone(controls: &[Vec3], tolerance: f64) -> bool {
    let points = controls
        .iter()
        .map(|control| Point2::new(control.x / control.z, control.y / control.z))
        .collect::<Vec<_>>();
    let direction = points.last().unwrap() - points[0];
    let length = direction.magnitude();
    if length <= tolerance {
        return false;
    }
    let unit = direction / length;
    points
        .windows(2)
        .all(|pair| (pair[1] - pair[0]).dot(unit) >= -tolerance)
}

pub(crate) fn wires_intersect(
    first: &Wire2,
    second: &Wire2,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    for first_segment in first.segments() {
        for second_segment in second.segments() {
            let intersection = curve_pair_intersection(first_segment, second_segment, tolerance)?;
            if intersection.coincident || !intersection.hits.is_empty() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(crate) fn wire_contains_point(
    wire: &Wire2,
    point: Point2,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    curve_loop_contains_point(wire.segments(), point, tolerance)
}

/// Classify a point against an existing closed topology loop without treating
/// that loop as new sketch input. Published BREP loops have already passed the
/// topology and geometry validators and may contain periodic or post-operation
/// p-curves that `Wire2::new` deliberately refuses as user-authored wires.
pub(crate) fn curve_loop_contains_point(
    segments: &[TrimmedCurve2],
    point: Point2,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let mut winding = 0_i32;
    // Move the classification ray away from shared vertices. The original
    // point is still used for the exact boundary-distance checks below.
    let ray_point = Point2::new(
        point.x,
        point.y + tolerance.linear * 0.271_828_182_845_904_5,
    );
    let ray_end_x = segments
        .iter()
        .map(|segment| match segment.curve() {
            Curve2::Line(_) => segment
                .start()
                .and_then(|start| segment.end().map(|end| start.x.max(end.x))),
            Curve2::Circle(circle) => Ok(circle.center().x + circle.radius()),
            Curve2::Nurbs(curve) => Ok(curve
                .control_points()
                .iter()
                .map(|control| control.x)
                .fold(f64::NEG_INFINITY, f64::max)),
            Curve2::SurfaceIntersection(_) | Curve2::SurfaceProjection(_) => (0..=32)
                .map(|index| {
                    let parameter =
                        segment.range().start + segment.range().span() * index as f64 / 32.0;
                    segment
                        .curve()
                        .evaluate(parameter)
                        .map(|value| value.point.x)
                })
                .try_fold(f64::NEG_INFINITY, |maximum, value| {
                    value.map(|value| maximum.max(value))
                }),
        })
        .collect::<KernelResult<Vec<_>>>()?
        .into_iter()
        .fold(point.x + 1.0, f64::max)
        + point.x.abs().max(1.0) * 2.0;
    for segment in segments {
        match segment.curve() {
            Curve2::Line(_) => {
                let start = segment.start()?;
                let end = segment.end()?;
                if point_on_segment(point, start, end, tolerance.linear) {
                    return Ok(false);
                }
                let cross = (end.x - start.x) * (ray_point.y - start.y)
                    - (end.y - start.y) * (ray_point.x - start.x);
                if start.y <= ray_point.y && end.y > ray_point.y && cross > 0.0 {
                    winding += 1;
                } else if start.y > ray_point.y && end.y <= ray_point.y && cross < 0.0 {
                    winding -= 1;
                }
            }
            Curve2::Circle(circle) => {
                let radial = point - circle.center();
                let point_parameter = radial.y.atan2(radial.x);
                if (radial.magnitude() - circle.radius()).abs() <= tolerance.linear
                    && periodic_range_contains(
                        segment.range(),
                        point_parameter,
                        tolerance.parameter,
                    )
                {
                    return Ok(false);
                }
                if (segment.range().span().abs() - std::f64::consts::TAU).abs()
                    <= tolerance.parameter
                {
                    if radial.magnitude() < circle.radius() - tolerance.linear {
                        winding += segment.range().span().signum() as i32;
                    }
                    continue;
                }
                let sine = (ray_point.y - circle.center().y) / circle.radius();
                if sine.abs() > 1.0 + tolerance.linear / circle.radius() {
                    continue;
                }
                let first = sine.clamp(-1.0, 1.0).asin();
                let candidates = [first, std::f64::consts::PI - first];
                for base in candidates {
                    for parameter in periodic_parameters_in_range(segment.range(), base) {
                        let x = circle.center().x + circle.radius() * parameter.cos();
                        if x <= ray_point.x + tolerance.linear {
                            continue;
                        }
                        let direction = segment.range().span().signum();
                        let vertical = circle.radius() * parameter.cos() * direction;
                        if vertical.abs() <= tolerance.linear {
                            continue;
                        }
                        let progress = if direction > 0.0 {
                            parameter - segment.range().start
                        } else {
                            segment.range().start - parameter
                        };
                        let length = segment.range().span().abs();
                        let at_start = progress.abs() <= tolerance.parameter;
                        let at_end = (progress - length).abs() <= tolerance.parameter;
                        let include = (!at_start && !at_end)
                            || (at_start && vertical > 0.0)
                            || (at_end && vertical < 0.0);
                        if include {
                            winding += if vertical > 0.0 { 1 } else { -1 };
                        }
                    }
                }
            }
            Curve2::Nurbs(_) | Curve2::SurfaceIntersection(_) | Curve2::SurfaceProjection(_) => {
                let boundary_ray = TrimmedCurve2::line(
                    Point2::new(point.x - tolerance.linear * 2.0, point.y),
                    Point2::new(point.x + tolerance.linear * 2.0, point.y),
                )?;
                if curve_pair_intersection(segment, &boundary_ray, tolerance)?
                    .hits
                    .iter()
                    .any(|hit| hit.point.distance(point) <= tolerance.linear)
                {
                    return Ok(false);
                }
                let ray = TrimmedCurve2::line(ray_point, Point2::new(ray_end_x, ray_point.y))?;
                for hit in curve_pair_intersection(segment, &ray, tolerance)?.hits {
                    if hit.point.x <= ray_point.x + tolerance.linear {
                        continue;
                    }
                    let derivative = segment
                        .curve()
                        .evaluate(hit.first_parameter)?
                        .first_derivative
                        .y
                        * segment.range().span().signum();
                    if derivative.abs() <= tolerance.linear {
                        continue;
                    }
                    let progress = (hit.first_parameter - segment.range().start)
                        * segment.range().span().signum();
                    let length = segment.range().span().abs();
                    let at_start = progress.abs() <= tolerance.parameter;
                    let at_end = (progress - length).abs() <= tolerance.parameter;
                    let include = (!at_start && !at_end)
                        || (at_start && derivative > 0.0)
                        || (at_end && derivative < 0.0);
                    if include {
                        winding += if derivative > 0.0 { 1 } else { -1 };
                    }
                }
            }
        }
    }
    Ok(winding != 0)
}

fn point_on_segment(point: Point2, start: Point2, end: Point2, tolerance: f64) -> bool {
    let direction = end - start;
    let length = direction.magnitude();
    if length == 0.0 {
        return point.distance(start) <= tolerance;
    }
    let cross = direction.x * (point.y - start.y) - direction.y * (point.x - start.x);
    cross.abs() <= tolerance * length
        && point.x >= start.x.min(end.x) - tolerance
        && point.x <= start.x.max(end.x) + tolerance
        && point.y >= start.y.min(end.y) - tolerance
        && point.y <= start.y.max(end.y) + tolerance
}

fn periodic_parameters_in_range(range: ParameterRange, base: f64) -> Vec<f64> {
    let low = range.start.min(range.end);
    let high = range.start.max(range.end);
    let period = std::f64::consts::TAU;
    let first = base + ((low - base) / period).ceil() * period;
    let mut result = Vec::new();
    let mut parameter = first;
    while parameter <= high + f64::EPSILON * 64.0 && result.len() < 3 {
        result.push(parameter);
        parameter += period;
    }
    result
}

pub(crate) fn curve_pair_intersection(
    first: &TrimmedCurve2,
    second: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<CurvePairIntersection> {
    match (first.curve(), second.curve()) {
        (Curve2::SurfaceIntersection(_) | Curve2::SurfaceProjection(_), _)
        | (_, Curve2::SurfaceIntersection(_) | Curve2::SurfaceProjection(_)) => {
            general_parametric_pair(first, second, tolerance)
        }
        (Curve2::Line(_), Curve2::Line(_)) => line_line_intersection(first, second, tolerance),
        (Curve2::Line(_), Curve2::Circle(_)) => line_circle_intersection(first, second, tolerance),
        (Curve2::Circle(_), Curve2::Line(_)) => {
            let mut intersection = line_circle_intersection(second, first, tolerance)?;
            for hit in &mut intersection.hits {
                std::mem::swap(&mut hit.first_parameter, &mut hit.second_parameter);
            }
            Ok(intersection)
        }
        (Curve2::Circle(_), Curve2::Circle(_)) => {
            circle_circle_intersection(first, second, tolerance)
        }
        (Curve2::Nurbs(_), _) => certified_nurbs_pair(first, second, tolerance),
        (_, Curve2::Nurbs(_)) => {
            let mut intersection = certified_nurbs_pair(second, first, tolerance)?;
            for hit in &mut intersection.hits {
                std::mem::swap(&mut hit.first_parameter, &mut hit.second_parameter);
            }
            Ok(intersection)
        }
    }
}

fn general_parametric_pair(
    first: &TrimmedCurve2,
    second: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<CurvePairIntersection> {
    let mut seeds = Vec::new();
    let divisions = 32;
    for first_index in 0..=divisions {
        let first_parameter =
            first.range().start + first.range().span() * first_index as f64 / divisions as f64;
        let first_value = first.curve().evaluate(first_parameter)?;
        for second_index in 0..=divisions {
            let second_parameter = second.range().start
                + second.range().span() * second_index as f64 / divisions as f64;
            let second_value = second.curve().evaluate(second_parameter)?;
            let local_scale = first_value
                .point
                .to_vec()
                .magnitude()
                .max(second_value.point.to_vec().magnitude())
                .max(1.0);
            // Convert parameter-cell widths to geometric lengths. Comparing a
            // spatial distance with raw parameter spans misses intersections
            // whenever either curve has a non-unit parameterization.
            let cell_size = first_value.first_derivative.magnitude() * first.range().span().abs()
                / divisions as f64
                + second_value.first_derivative.magnitude() * second.range().span().abs()
                    / divisions as f64;
            if first_value.point.distance(second_value.point)
                <= cell_size.max(tolerance.linear_at_scale(local_scale) * 32.0)
            {
                seeds.push((first_parameter, second_parameter));
            }
        }
    }
    let mut hits = Vec::new();
    for (mut first_parameter, mut second_parameter) in seeds {
        for _ in 0..32 {
            let first_value = first.curve().evaluate(first_parameter)?;
            let second_value = second.curve().evaluate(second_parameter)?;
            let residual = first_value.point - second_value.point;
            let determinant = first_value.first_derivative.x * -second_value.first_derivative.y
                - first_value.first_derivative.y * -second_value.first_derivative.x;
            if determinant.abs() <= f64::EPSILON * 256.0 {
                break;
            }
            let first_delta = (-residual.x * -second_value.first_derivative.y
                + residual.y * -second_value.first_derivative.x)
                / determinant;
            let second_delta = (first_value.first_derivative.x * -residual.y
                + first_value.first_derivative.y * residual.x)
                / determinant;
            first_parameter = (first_parameter + first_delta).clamp(
                first.range().start.min(first.range().end),
                first.range().start.max(first.range().end),
            );
            second_parameter = (second_parameter + second_delta).clamp(
                second.range().start.min(second.range().end),
                second.range().start.max(second.range().end),
            );
            if first_delta.abs().max(second_delta.abs()) <= tolerance.parameter {
                break;
            }
        }
        let first_point = first.curve().evaluate(first_parameter)?.point;
        let second_point = second.curve().evaluate(second_parameter)?.point;
        if first_point.distance(second_point) <= tolerance.linear * 8.0
            && !hits.iter().any(|hit: &CurvePairHit| {
                (hit.first_parameter - first_parameter).abs() <= tolerance.parameter * 16.0
                    && (hit.second_parameter - second_parameter).abs() <= tolerance.parameter * 16.0
            })
        {
            hits.push(CurvePairHit {
                point: Point2::from_vec((first_point.to_vec() + second_point.to_vec()) * 0.5),
                first_parameter,
                second_parameter,
            });
        }
    }
    Ok(CurvePairIntersection {
        hits,
        coincident: false,
    })
}

fn line_line_intersection(
    first: &TrimmedCurve2,
    second: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<CurvePairIntersection> {
    let a = first.start()?;
    let b = first.end()?;
    let c = second.start()?;
    let d = second.end()?;
    if !segments_intersect(a, b, c, d, tolerance.linear) {
        return Ok(CurvePairIntersection::default());
    }
    let cross = |left: Vec2, right: Vec2| left.x * right.y - left.y * right.x;
    let denominator = cross(b - a, d - c);
    if denominator.abs() <= tolerance.linear.powi(2) {
        return Ok(CurvePairIntersection {
            coincident: true,
            ..Default::default()
        });
    }
    let parameter = cross(c - a, d - c) / denominator;
    let second_parameter = cross(c - a, b - a) / denominator;
    Ok(CurvePairIntersection {
        hits: vec![CurvePairHit {
            point: a + (b - a) * parameter,
            first_parameter: first.range().start + first.range().span() * parameter,
            second_parameter: second.range().start + second.range().span() * second_parameter,
        }],
        coincident: false,
    })
}

fn line_circle_intersection(
    line_segment: &TrimmedCurve2,
    circle_segment: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<CurvePairIntersection> {
    let Curve2::Line(line) = line_segment.curve() else {
        unreachable!()
    };
    let Curve2::Circle(circle) = circle_segment.curve() else {
        unreachable!()
    };
    let offset = line.origin() - circle.center();
    let direction = line.direction();
    let a = direction.magnitude2();
    let b = 2.0 * offset.dot(direction);
    let c = offset.magnitude2() - circle.radius().powi(2);
    let discriminant = b * b - 4.0 * a * c;
    let threshold = tolerance.linear.powi(2) * a.max(1.0);
    if discriminant < -threshold {
        return Ok(CurvePairIntersection::default());
    }
    let root = discriminant.max(0.0).sqrt();
    let parameters = if discriminant.abs() <= threshold {
        vec![-b / (2.0 * a)]
    } else {
        vec![(-b - root) / (2.0 * a), (-b + root) / (2.0 * a)]
    };
    let mut hits = Vec::new();
    for parameter in parameters {
        let point = line.origin() + direction * parameter;
        let circle_parameter = (point.y - circle.center().y).atan2(point.x - circle.center().x);
        if line_segment.range().contains(parameter, tolerance)
            && periodic_range_contains(
                circle_segment.range(),
                circle_parameter,
                tolerance.parameter,
            )
        {
            hits.push(CurvePairHit {
                point,
                first_parameter: parameter,
                second_parameter: circle_parameter,
            });
        }
    }
    Ok(CurvePairIntersection {
        hits,
        coincident: false,
    })
}

fn circle_circle_intersection(
    first_segment: &TrimmedCurve2,
    second_segment: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<CurvePairIntersection> {
    let Curve2::Circle(first) = first_segment.curve() else {
        unreachable!()
    };
    let Curve2::Circle(second) = second_segment.curve() else {
        unreachable!()
    };
    let delta = second.center() - first.center();
    let distance = delta.magnitude();
    let linear = tolerance.linear_at_scale(first.radius().max(second.radius()));
    if distance <= linear && (first.radius() - second.radius()).abs() <= linear {
        return Ok(CurvePairIntersection {
            coincident: circular_ranges_overlap(
                first_segment.range(),
                second_segment.range(),
                tolerance.parameter,
            ),
            ..Default::default()
        });
    }
    if distance > first.radius() + second.radius() + linear
        || distance + first.radius().min(second.radius())
            < first.radius().max(second.radius()) - linear
        || distance <= linear
    {
        return Ok(CurvePairIntersection::default());
    }
    let along =
        (first.radius().powi(2) - second.radius().powi(2) + distance.powi(2)) / (2.0 * distance);
    let height_squared = first.radius().powi(2) - along.powi(2);
    if height_squared < -linear.powi(2) {
        return Ok(CurvePairIntersection::default());
    }
    let direction = delta / distance;
    let perpendicular = Vec2::new(-direction.y, direction.x);
    let midpoint = first.center() + direction * along;
    let height = height_squared.max(0.0).sqrt();
    let candidates = if height <= linear {
        vec![midpoint]
    } else {
        vec![
            midpoint + perpendicular * height,
            midpoint - perpendicular * height,
        ]
    };
    let hits = candidates
        .into_iter()
        .filter_map(|point| {
            let first_parameter = (point.y - first.center().y).atan2(point.x - first.center().x);
            let second_parameter = (point.y - second.center().y).atan2(point.x - second.center().x);
            (periodic_range_contains(first_segment.range(), first_parameter, tolerance.parameter)
                && periodic_range_contains(
                    second_segment.range(),
                    second_parameter,
                    tolerance.parameter,
                ))
            .then_some(CurvePairHit {
                point,
                first_parameter,
                second_parameter,
            })
        })
        .collect();
    Ok(CurvePairIntersection {
        hits,
        coincident: false,
    })
}

fn periodic_range_contains(range: ParameterRange, parameter: f64, tolerance: f64) -> bool {
    let low = range.start.min(range.end);
    let high = range.start.max(range.end);
    let period = std::f64::consts::TAU;
    let normalized = low + (parameter - low).rem_euclid(period);
    normalized <= high + tolerance || normalized - period >= low - tolerance
}

fn circular_ranges_overlap(first: ParameterRange, second: ParameterRange, tolerance: f64) -> bool {
    let interior = |range: ParameterRange| {
        [
            range.start + range.span() * 0.25,
            range.start + range.span() * 0.5,
            range.start + range.span() * 0.75,
        ]
    };
    interior(first)
        .into_iter()
        .any(|parameter| periodic_range_contains(second, parameter, tolerance))
        || interior(second)
            .into_iter()
            .any(|parameter| periodic_range_contains(first, parameter, tolerance))
}

fn certified_nurbs_pair(
    nurbs_segment: &TrimmedCurve2,
    other: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<CurvePairIntersection> {
    let Curve2::Nurbs(nurbs) = nurbs_segment.curve() else {
        unreachable!()
    };
    if let Curve2::Nurbs(_) = other.curve() {
        return nurbs_nurbs_intersection(nurbs_segment, other, tolerance);
    }
    let mut parameters = Vec::new();
    for (span, controls) in nurbs.bezier_spans()? {
        let coefficients: Vec<f64> = match other.curve() {
            Curve2::Line(line) => controls
                .iter()
                .map(|control| {
                    line.direction().x * (control.y - line.origin().y * control.z)
                        - line.direction().y * (control.x - line.origin().x * control.z)
                })
                .collect(),
            Curve2::Circle(circle) => {
                let x = controls
                    .iter()
                    .map(|control| control.x - circle.center().x * control.z)
                    .collect::<Vec<_>>();
                let y = controls
                    .iter()
                    .map(|control| control.y - circle.center().y * control.z)
                    .collect::<Vec<_>>();
                let w = controls.iter().map(|control| control.z).collect::<Vec<_>>();
                let xx = bernstein_product(&x, &x);
                let yy = bernstein_product(&y, &y);
                let ww = bernstein_product(&w, &w);
                xx.iter()
                    .zip(yy)
                    .zip(ww)
                    .map(|((x, y), w)| x + y - circle.radius().powi(2) * w)
                    .collect()
            }
            Curve2::Nurbs(_) => unreachable!(),
            Curve2::SurfaceIntersection(_) => unreachable!(),
            Curve2::SurfaceProjection(_) => unreachable!(),
        };
        isolate_bernstein_roots(&coefficients, span, 0, tolerance.parameter, &mut parameters);
    }
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|first, second| (*first - *second).abs() <= tolerance.parameter * 32.0);
    let mut hits = Vec::new();
    for seed in parameters {
        let parameter = refine_implicit_root(nurbs_segment, other, seed)?;
        if !nurbs_segment.range().contains(parameter, tolerance) {
            continue;
        }
        let point = nurbs_segment.curve().evaluate(parameter)?.point;
        let projection = match other.curve() {
            Curve2::Line(line) => {
                let value =
                    (point - line.origin()).dot(line.direction()) / line.direction().magnitude2();
                (value, line.origin() + line.direction() * value)
            }
            Curve2::Circle(circle) => {
                let value = (point.y - circle.center().y).atan2(point.x - circle.center().x);
                (
                    value,
                    circle.center() + Vec2::new(value.cos(), value.sin()) * circle.radius(),
                )
            }
            Curve2::Nurbs(_) => unreachable!(),
            Curve2::SurfaceIntersection(_) => unreachable!(),
            Curve2::SurfaceProjection(_) => unreachable!(),
        };
        let in_other = match other.curve() {
            Curve2::Circle(_) => {
                periodic_range_contains(other.range(), projection.0, tolerance.parameter)
            }
            _ => other.range().contains(projection.0, tolerance),
        };
        if in_other && point.distance(projection.1) <= tolerance.linear * 4.0 {
            hits.push(CurvePairHit {
                point,
                first_parameter: parameter,
                second_parameter: projection.0,
            });
        }
    }
    deduplicate_hits(&mut hits, tolerance.linear);
    Ok(CurvePairIntersection {
        hits,
        coincident: false,
    })
}

fn binomial(n: usize, k: usize) -> f64 {
    let k = k.min(n - k);
    (0..k).fold(1.0, |value, index| {
        value * (n - index) as f64 / (index + 1) as f64
    })
}

fn bernstein_product(first: &[f64], second: &[f64]) -> Vec<f64> {
    let p = first.len() - 1;
    let q = second.len() - 1;
    let mut result = vec![0.0; p + q + 1];
    for (i, left) in first.iter().enumerate() {
        for (j, right) in second.iter().enumerate() {
            result[i + j] +=
                left * right * binomial(p, i) * binomial(q, j) / binomial(p + q, i + j);
        }
    }
    result
}

fn split_values<T>(values: &[T]) -> (Vec<T>, Vec<T>)
where
    T: Copy + std::ops::Add<Output = T> + std::ops::Mul<f64, Output = T>,
{
    split_values_at(values, 0.5)
}

fn split_values_at<T>(values: &[T], parameter: f64) -> (Vec<T>, Vec<T>)
where
    T: Copy + std::ops::Add<Output = T> + std::ops::Mul<f64, Output = T>,
{
    let mut levels = vec![values.to_vec()];
    while levels.last().is_some_and(|level| level.len() > 1) {
        let previous = levels.last().unwrap();
        levels.push(
            previous
                .windows(2)
                .map(|pair| pair[0] * (1.0 - parameter) + pair[1] * parameter)
                .collect(),
        );
    }
    let left = levels.iter().map(|level| level[0]).collect();
    let right = levels
        .iter()
        .rev()
        .map(|level| *level.last().unwrap())
        .collect();
    (left, right)
}

fn trimmed_bezier_spans(
    curve: &super::NurbsCurve2,
    trim: ParameterRange,
) -> KernelResult<Vec<(ParameterRange, Vec<Vec3>)>> {
    let trim_start = trim.start.min(trim.end);
    let trim_end = trim.start.max(trim.end);
    let mut result = Vec::new();
    for (span, controls) in curve.bezier_spans()? {
        let start = span.start.max(trim_start);
        let end = span.end.min(trim_end);
        if end <= start {
            continue;
        }
        let span_length = span.end - span.start;
        let end_fraction = (end - span.start) / span_length;
        let mut clipped = if end_fraction < 1.0 {
            split_values_at(&controls, end_fraction).0
        } else {
            controls
        };
        if start > span.start {
            let start_fraction = (start - span.start) / (end - span.start);
            clipped = split_values_at(&clipped, start_fraction).1;
        }
        result.push((ParameterRange { start, end }, clipped));
    }
    Ok(result)
}

fn isolate_bernstein_roots(
    coefficients: &[f64],
    range: ParameterRange,
    depth: u8,
    parameter_tolerance: f64,
    output: &mut Vec<f64>,
) {
    let scale = coefficients
        .iter()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let rounding = scale * f64::EPSILON * 128.0;
    let nonnegative = coefficients.iter().all(|value| *value >= -rounding);
    let nonpositive = coefficients.iter().all(|value| *value <= rounding);
    if nonnegative || nonpositive {
        if coefficients[0].abs() <= rounding {
            output.push(range.start);
        }
        if coefficients
            .last()
            .is_some_and(|value| value.abs() <= rounding)
        {
            output.push(range.end);
        }
        return;
    }
    if depth >= 56 || range.span().abs() <= parameter_tolerance * 16.0 {
        output.push((range.start + range.end) * 0.5);
        return;
    }
    let (left, right) = split_values(coefficients);
    let middle = (range.start + range.end) * 0.5;
    isolate_bernstein_roots(
        &left,
        ParameterRange {
            start: range.start,
            end: middle,
        },
        depth + 1,
        parameter_tolerance,
        output,
    );
    isolate_bernstein_roots(
        &right,
        ParameterRange {
            start: middle,
            end: range.end,
        },
        depth + 1,
        parameter_tolerance,
        output,
    );
}

fn refine_implicit_root(
    nurbs: &TrimmedCurve2,
    other: &TrimmedCurve2,
    mut parameter: f64,
) -> KernelResult<f64> {
    for _ in 0..24 {
        let evaluation = nurbs.curve().evaluate(parameter)?;
        let (value, derivative) = match other.curve() {
            Curve2::Line(line) => {
                let delta = evaluation.point - line.origin();
                (
                    line.direction().x * delta.y - line.direction().y * delta.x,
                    line.direction().x * evaluation.first_derivative.y
                        - line.direction().y * evaluation.first_derivative.x,
                )
            }
            Curve2::Circle(circle) => {
                let delta = evaluation.point - circle.center();
                (
                    delta.magnitude2() - circle.radius().powi(2),
                    2.0 * delta.dot(evaluation.first_derivative),
                )
            }
            Curve2::Nurbs(_) => unreachable!(),
            Curve2::SurfaceIntersection(_) => unreachable!(),
            Curve2::SurfaceProjection(_) => unreachable!(),
        };
        if derivative.abs() <= f64::EPSILON {
            break;
        }
        let next = (parameter - value / derivative).clamp(
            nurbs.range().start.min(nurbs.range().end),
            nurbs.range().start.max(nurbs.range().end),
        );
        if (next - parameter).abs() <= f64::EPSILON * 64.0 {
            break;
        }
        parameter = next;
    }
    Ok(parameter)
}

fn deduplicate_hits(hits: &mut Vec<CurvePairHit>, tolerance: f64) {
    let mut unique = Vec::<CurvePairHit>::new();
    for hit in hits.drain(..) {
        if unique
            .iter()
            .all(|candidate| candidate.point.distance(hit.point) > tolerance)
        {
            unique.push(hit);
        }
    }
    *hits = unique;
}

fn nurbs_nurbs_intersection(
    first: &TrimmedCurve2,
    second: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<CurvePairIntersection> {
    if first.curve() == second.curve() {
        let overlap_start = first
            .range()
            .start
            .min(first.range().end)
            .max(second.range().start.min(second.range().end));
        let overlap_end = first
            .range()
            .start
            .max(first.range().end)
            .min(second.range().start.max(second.range().end));
        if overlap_end > overlap_start + tolerance.parameter {
            return Ok(CurvePairIntersection {
                coincident: true,
                ..Default::default()
            });
        }
    }
    let (Curve2::Nurbs(first_curve), Curve2::Nurbs(second_curve)) = (first.curve(), second.curve())
    else {
        unreachable!()
    };
    let mut stack = Vec::new();
    for first_span in trimmed_bezier_spans(first_curve, first.range())? {
        for second_span in trimmed_bezier_spans(second_curve, second.range())? {
            stack.push((
                first_span.0,
                first_span.1.clone(),
                second_span.0,
                second_span.1.clone(),
                0_u8,
            ));
        }
    }
    let mut seeds = Vec::new();
    let mut visited = 0_usize;
    while let Some((first_range, first_controls, second_range, second_controls, depth)) =
        stack.pop()
    {
        visited += 1;
        if visited > 250_000 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "certified NURBS intersection exceeded its subdivision budget",
            ));
        }
        let first_bounds = homogeneous_bounds(&first_controls);
        let second_bounds = homogeneous_bounds(&second_controls);
        if bounds_separated(first_bounds, second_bounds, tolerance.linear) {
            continue;
        }
        let first_size = bounds_size(first_bounds);
        let second_size = bounds_size(second_bounds);
        if depth >= 48 || first_size.max(second_size) <= tolerance.linear * 2.0 {
            seeds.push((
                (first_range.start + first_range.end) * 0.5,
                (second_range.start + second_range.end) * 0.5,
            ));
            continue;
        }
        if first_size * first_range.span().abs() >= second_size * second_range.span().abs() {
            let (left, right) = split_values(&first_controls);
            let middle = (first_range.start + first_range.end) * 0.5;
            stack.push((
                ParameterRange {
                    start: middle,
                    end: first_range.end,
                },
                right,
                second_range,
                second_controls.clone(),
                depth + 1,
            ));
            stack.push((
                ParameterRange {
                    start: first_range.start,
                    end: middle,
                },
                left,
                second_range,
                second_controls,
                depth + 1,
            ));
        } else {
            let (left, right) = split_values(&second_controls);
            let middle = (second_range.start + second_range.end) * 0.5;
            stack.push((
                first_range,
                first_controls.clone(),
                ParameterRange {
                    start: middle,
                    end: second_range.end,
                },
                right,
                depth + 1,
            ));
            stack.push((
                first_range,
                first_controls,
                ParameterRange {
                    start: second_range.start,
                    end: middle,
                },
                left,
                depth + 1,
            ));
        }
    }
    let mut hits = Vec::new();
    for (mut first_parameter, mut second_parameter) in seeds {
        for _ in 0..24 {
            let first_value = first.curve().evaluate(first_parameter)?;
            let second_value = second.curve().evaluate(second_parameter)?;
            let delta = first_value.point - second_value.point;
            let determinant = first_value.first_derivative.y * second_value.first_derivative.x
                - first_value.first_derivative.x * second_value.first_derivative.y;
            if determinant.abs() <= f64::EPSILON * 128.0 {
                break;
            }
            let first_step = (delta.x * second_value.first_derivative.y
                - delta.y * second_value.first_derivative.x)
                / determinant;
            let second_step = (first_value.first_derivative.x * delta.y
                - first_value.first_derivative.y * delta.x)
                / determinant;
            first_parameter = (first_parameter + first_step).clamp(
                first.range().start.min(first.range().end),
                first.range().start.max(first.range().end),
            );
            second_parameter = (second_parameter - second_step).clamp(
                second.range().start.min(second.range().end),
                second.range().start.max(second.range().end),
            );
            if first_step.abs().max(second_step.abs()) <= tolerance.parameter {
                break;
            }
        }
        if !first.range().contains(first_parameter, tolerance)
            || !second.range().contains(second_parameter, tolerance)
        {
            continue;
        }
        let first_point = first.curve().evaluate(first_parameter)?.point;
        let second_point = second.curve().evaluate(second_parameter)?.point;
        if first_point.distance(second_point) <= tolerance.linear * 4.0 {
            hits.push(CurvePairHit {
                point: Point2::from_vec((first_point.to_vec() + second_point.to_vec()) * 0.5),
                first_parameter,
                second_parameter,
            });
        }
    }
    deduplicate_hits(&mut hits, tolerance.linear);
    Ok(CurvePairIntersection {
        hits,
        coincident: false,
    })
}

fn homogeneous_bounds(controls: &[cgmath::Vector3<f64>]) -> (f64, f64, f64, f64) {
    controls.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), control| {
            let x = control.x / control.z;
            let y = control.y / control.z;
            (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
        },
    )
}

fn bounds_separated(
    first: (f64, f64, f64, f64),
    second: (f64, f64, f64, f64),
    tolerance: f64,
) -> bool {
    first.1 < second.0 - tolerance
        || second.1 < first.0 - tolerance
        || first.3 < second.2 - tolerance
        || second.3 < first.2 - tolerance
}

fn bounds_size(bounds: (f64, f64, f64, f64)) -> f64 {
    (bounds.1 - bounds.0).hypot(bounds.3 - bounds.2)
}

pub(crate) fn segments_intersect(
    a: Point2,
    b: Point2,
    c: Point2,
    d: Point2,
    tolerance: f64,
) -> bool {
    let cross = |first: Vec2, second: Vec2| first.x * second.y - first.y * second.x;
    let ab = b - a;
    let cd = d - c;
    let denominator = cross(ab, cd);
    if denominator.abs() <= tolerance * tolerance {
        let collinear = cross(c - a, ab).abs() <= tolerance * ab.magnitude().max(1.0)
            && cross(d - a, ab).abs() <= tolerance * ab.magnitude().max(1.0);
        if !collinear {
            return false;
        }
        let use_x = ab.x.abs() >= ab.y.abs();
        let projection = |point: Point2| if use_x { point.x } else { point.y };
        let first = [projection(a), projection(b)];
        let second = [projection(c), projection(d)];
        return first[0].min(first[1]) <= second[0].max(second[1]) + tolerance
            && second[0].min(second[1]) <= first[0].max(first[1]) + tolerance;
    }
    let ac = c - a;
    let first = cross(ac, cd) / denominator;
    let second = cross(ac, ab) / denominator;
    -tolerance <= first
        && first <= 1.0 + tolerance
        && -tolerance <= second
        && second <= 1.0 + tolerance
}

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}
