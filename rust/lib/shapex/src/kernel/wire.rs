use cgmath::prelude::*;

use crate::{Point2, Vec2};

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
            return Err(invalid("an open trimmed curve needs distinct endpoints"));
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

    fn polyline(&self, tolerance: ModelTolerance) -> KernelResult<Vec<Point2>> {
        let start = self.curve.evaluate(self.range.start)?.point;
        let end = self.curve.evaluate(self.range.end)?.point;
        let mut output = vec![start];
        subdivide_curve(
            &self.curve,
            self.range.start,
            self.range.end,
            start,
            end,
            tolerance.linear,
            0,
            &mut output,
        )?;
        Ok(output)
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

    pub(crate) fn polyline(&self, tolerance: ModelTolerance) -> KernelResult<Vec<Point2>> {
        let mut output = Vec::new();
        for segment in &self.segments {
            let mut points = segment.polyline(tolerance)?;
            if !output.is_empty() {
                points.remove(0);
            }
            output.extend(points);
        }
        if output.len() > 1
            && output[0].distance(*output.last().unwrap_or(&output[0])) <= tolerance.linear
        {
            output.pop();
        }
        Ok(output)
    }
}

#[allow(clippy::too_many_arguments)]
fn subdivide_curve(
    curve: &Curve2,
    start_parameter: f64,
    end_parameter: f64,
    start: Point2,
    end: Point2,
    tolerance: f64,
    depth: u8,
    output: &mut Vec<Point2>,
) -> KernelResult<()> {
    let middle_parameter = (start_parameter + end_parameter) * 0.5;
    let middle = curve.evaluate(middle_parameter)?.point;
    let chord = end - start;
    let distance = if chord.magnitude2() == 0.0 {
        middle.distance(start)
    } else {
        let parameter = (middle - start).dot(chord) / chord.magnitude2();
        middle.distance(start + chord * parameter.clamp(0.0, 1.0))
    };
    if distance > tolerance * 0.25 && depth < 24 {
        subdivide_curve(
            curve,
            start_parameter,
            middle_parameter,
            start,
            middle,
            tolerance,
            depth + 1,
            output,
        )?;
        subdivide_curve(
            curve,
            middle_parameter,
            end_parameter,
            middle,
            end,
            tolerance,
            depth + 1,
            output,
        )?;
    } else {
        output.push(end);
    }
    Ok(())
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
                            .all(|shared| shared.distance(hit.point) > tolerance.linear)
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
    let controls = curve.control_points();
    let direction = segment.end()? - segment.start()?;
    let length = direction.magnitude();
    if length <= tolerance.linear {
        return Err(invalid("a NURBS wire segment needs distinct endpoints"));
    }
    let unit = direction / length;
    let ordered = if segment.range().span() >= 0.0 {
        controls.iter().collect::<Vec<_>>()
    } else {
        controls.iter().rev().collect::<Vec<_>>()
    };
    let mut previous = ordered[0].to_vec().dot(unit);
    for control in ordered.iter().skip(1) {
        let projection = control.to_vec().dot(unit);
        if projection + tolerance.linear < previous {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "certifying a non-monotone NURBS wire segment is not implemented",
            ));
        }
        previous = projection;
    }
    Ok(())
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
    let mut winding = 0_i32;
    // Move the classification ray away from shared vertices. The original
    // point is still used for the exact boundary-distance checks below.
    let ray_point = Point2::new(
        point.x,
        point.y + tolerance.linear * 0.271_828_182_845_904_5,
    );
    for segment in wire.segments() {
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
            Curve2::Nurbs(_) => {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "certified point containment for a NURBS boundary is not implemented",
                ));
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
        (Curve2::Nurbs(curve), _) => certified_nurbs_pair(curve, second, tolerance),
        (_, Curve2::Nurbs(curve)) => certified_nurbs_pair(curve, first, tolerance),
    }
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
    nurbs: &super::NurbsCurve2,
    other: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<CurvePairIntersection> {
    let mut controls = nurbs.control_points().to_vec();
    let other_points = match other.curve() {
        Curve2::Line(_) => vec![other.start()?, other.end()?],
        Curve2::Nurbs(curve) => curve.control_points().to_vec(),
        Curve2::Circle(circle) => {
            let bounds = control_bounds(&controls);
            let closest_x = circle.center().x.clamp(bounds.0, bounds.1);
            let closest_y = circle.center().y.clamp(bounds.2, bounds.3);
            if circle.center().distance(Point2::new(closest_x, closest_y))
                > circle.radius() + tolerance.linear
            {
                return Ok(CurvePairIntersection::default());
            }
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "certifying a possible NURBS/circle boundary intersection is not implemented",
            ));
        }
    };
    let first_bounds = control_bounds(&controls);
    let second_bounds = control_bounds(&other_points);
    if first_bounds.1 + tolerance.linear < second_bounds.0
        || second_bounds.1 + tolerance.linear < first_bounds.0
        || first_bounds.3 + tolerance.linear < second_bounds.2
        || second_bounds.3 + tolerance.linear < first_bounds.2
    {
        return Ok(CurvePairIntersection::default());
    }
    if let Curve2::Line(_) = other.curve() {
        let start = other_points[0];
        let end = other_points[1];
        let direction = other_points[1] - start;
        let cross =
            |point: Point2| direction.x * (point.y - start.y) - direction.y * (point.x - start.x);
        controls.retain(|point| {
            point.distance(start) > tolerance.linear && point.distance(end) > tolerance.linear
        });
        let values = controls
            .iter()
            .map(|point| cross(*point))
            .collect::<Vec<_>>();
        let threshold = tolerance.linear * direction.magnitude().max(1.0);
        if !values.is_empty()
            && (values.iter().all(|value| *value > threshold)
                || values.iter().all(|value| *value < -threshold))
        {
            return Ok(CurvePairIntersection::default());
        }
    }
    Err(KernelError::new(
        DiagnosticKind::UnsupportedGeometry,
        "certifying this possible NURBS boundary intersection is not implemented",
    ))
}

fn control_bounds(points: &[Point2]) -> (f64, f64, f64, f64) {
    points.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), point| {
            (
                min_x.min(point.x),
                max_x.max(point.x),
                min_y.min(point.y),
                max_y.max(point.y),
            )
        },
    )
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
