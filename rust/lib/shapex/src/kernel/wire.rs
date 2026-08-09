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
        let same_circle = segments.iter().all(
            |segment| matches!(segment.curve(), Curve2::Circle(circle) if circle == reference),
        );
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
    let polylines = segments
        .iter()
        .map(|segment| segment.polyline(tolerance))
        .collect::<KernelResult<Vec<_>>>()?;
    for first_segment in 0..polylines.len() {
        for second_segment in first_segment..polylines.len() {
            let adjacent = second_segment == first_segment
                || second_segment == first_segment + 1
                || (first_segment == 0 && second_segment + 1 == polylines.len());
            for first in polylines[first_segment].windows(2) {
                for second in polylines[second_segment].windows(2) {
                    if adjacent
                        && (first[0].distance(second[0]) <= tolerance.linear
                            || first[0].distance(second[1]) <= tolerance.linear
                            || first[1].distance(second[0]) <= tolerance.linear
                            || first[1].distance(second[1]) <= tolerance.linear)
                    {
                        continue;
                    }
                    if segments_intersect(
                        first[0],
                        first[1],
                        second[0],
                        second[1],
                        tolerance.linear,
                    ) {
                        return Err(invalid("wire must not self-intersect"));
                    }
                }
            }
        }
    }
    Ok(())
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
