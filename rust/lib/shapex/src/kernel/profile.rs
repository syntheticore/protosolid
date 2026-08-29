use cgmath::prelude::*;

use super::wire::curve_pair_intersection;
use super::wire::{wire_contains_point, wires_intersect};
use super::{
    BooleanKind, Curve2, DiagnosticKind, KernelError, KernelResult, ModelTolerance, ParameterRange,
    TrimmedCurve2, Wire2,
};

/// A validated planar region bounded by exact 2D curve wires.
///
/// The outer wire is normalized counter-clockwise and holes clockwise.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    outer: Wire2,
    holes: Vec<Wire2>,
    area: f64,
    tolerance: ModelTolerance,
}

impl Profile {
    pub fn new(
        outer: Wire2,
        holes: impl IntoIterator<Item = Wire2>,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        let outer = if outer.is_counter_clockwise() {
            outer
        } else {
            outer.reversed()
        };
        let holes = holes
            .into_iter()
            .map(|hole| {
                if hole.is_counter_clockwise() {
                    hole.reversed()
                } else {
                    hole
                }
            })
            .collect::<Vec<_>>();
        for hole in &holes {
            if wires_intersect(&outer, hole, tolerance)?
                || !wire_contains_point(&outer, hole.segments()[0].start()?, tolerance)?
            {
                return Err(invalid(
                    "profile holes must lie strictly inside the outer wire",
                ));
            }
        }
        for first in 0..holes.len() {
            for second in holes.iter().skip(first + 1) {
                if wires_intersect(&holes[first], second, tolerance)?
                    || wire_contains_point(second, holes[first].segments()[0].start()?, tolerance)?
                    || wire_contains_point(&holes[first], second.segments()[0].start()?, tolerance)?
                {
                    return Err(invalid("profile holes may not touch, overlap, or nest"));
                }
            }
        }
        let area = outer.area() - holes.iter().map(Wire2::area).sum::<f64>();
        if area <= tolerance.linear.powi(2) {
            return Err(invalid("profile area does not exceed model tolerance"));
        }
        Ok(Self {
            outer,
            holes,
            area,
            tolerance,
        })
    }

    pub fn outer(&self) -> &Wire2 {
        &self.outer
    }
    pub fn holes(&self) -> &[Wire2] {
        &self.holes
    }
    pub fn area(&self) -> f64 {
        self.area
    }
    pub fn tolerance(&self) -> ModelTolerance {
        self.tolerance
    }

    pub(crate) fn boolean_planar(
        &self,
        other: &Self,
        kind: BooleanKind,
    ) -> KernelResult<Vec<Self>> {
        if self.tolerance != other.tolerance {
            return Err(KernelError::new(
                DiagnosticKind::InvalidTolerance,
                "planar Boolean profiles require the same model tolerance",
            ));
        }
        let mut first_boundaries = vec![self.outer.clone()];
        first_boundaries.extend(self.holes.iter().cloned());
        let mut second_boundaries = vec![other.outer.clone()];
        second_boundaries.extend(other.holes.iter().cloned());
        let mut first_splits = first_boundaries
            .iter()
            .map(|wire| vec![Vec::<f64>::new(); wire.segments().len()])
            .collect::<Vec<_>>();
        let mut second_splits = second_boundaries
            .iter()
            .map(|wire| vec![Vec::<f64>::new(); wire.segments().len()])
            .collect::<Vec<_>>();
        for (first_wire_index, first_wire) in first_boundaries.iter().enumerate() {
            for (second_wire_index, second_wire) in second_boundaries.iter().enumerate() {
                for (first_segment_index, first_segment) in first_wire.segments().iter().enumerate()
                {
                    for (second_segment_index, second_segment) in
                        second_wire.segments().iter().enumerate()
                    {
                        let intersection =
                            curve_pair_intersection(first_segment, second_segment, self.tolerance)?;
                        if intersection.coincident {
                            split_coincident_segments(
                                first_segment,
                                second_segment,
                                &mut first_splits[first_wire_index][first_segment_index],
                                &mut second_splits[second_wire_index][second_segment_index],
                                self.tolerance,
                            )?;
                            continue;
                        }
                        for hit in intersection.hits {
                            first_splits[first_wire_index][first_segment_index]
                                .push(hit.first_parameter);
                            second_splits[second_wire_index][second_segment_index]
                                .push(hit.second_parameter);
                        }
                    }
                }
            }
        }

        let first_fragments = split_boundaries(&first_boundaries, first_splits, self.tolerance)?;
        let second_fragments = split_boundaries(&second_boundaries, second_splits, self.tolerance)?;
        let mut selected = Vec::new();
        for fragment in first_fragments.into_iter().chain(second_fragments) {
            let evaluation = fragment
                .curve()
                .evaluate(fragment.range().start + fragment.range().span() * 0.5)?;
            let tangent = evaluation.first_derivative * fragment.range().span().signum();
            if tangent.magnitude() <= self.tolerance.linear {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "analytical planar Boolean encountered a singular boundary fragment",
                ));
            }
            let left = crate::Vec2::new(-tangent.y, tangent.x).normalize();
            let scale = evaluation.point.to_vec().magnitude().max(1.0);
            let offset = self.tolerance.linear_at_scale(scale) * 16.0;
            let left_point = evaluation.point + left * offset;
            let right_point = evaluation.point - left * offset;
            let result_contains = |point| -> KernelResult<bool> {
                let first = profile_contains(self, point)?;
                let second = profile_contains(other, point)?;
                Ok(match kind {
                    BooleanKind::Union => first || second,
                    BooleanKind::Intersection => first && second,
                    BooleanKind::Difference => first && !second,
                })
            };
            let left_inside = result_contains(left_point)?;
            let right_inside = result_contains(right_point)?;
            let fragment = match (left_inside, right_inside) {
                (true, false) => fragment,
                (false, true) => fragment.reversed(),
                _ => continue,
            };
            if !selected.iter().try_fold(false, |same, existing| {
                Ok::<_, KernelError>(same || fragments_match(existing, &fragment, self.tolerance)?)
            })? {
                selected.push(fragment);
            }
        }
        profiles_from_fragments(selected, self.tolerance)
    }
}

fn split_coincident_segments(
    first: &TrimmedCurve2,
    second: &TrimmedCurve2,
    first_splits: &mut Vec<f64>,
    second_splits: &mut Vec<f64>,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    for parameter in [second.range().start, second.range().end] {
        let point = second.curve().evaluate(parameter)?.point;
        if let Some(parameter) = parameter_on_segment(first, point, tolerance)? {
            first_splits.push(parameter);
        }
    }
    for parameter in [first.range().start, first.range().end] {
        let point = first.curve().evaluate(parameter)?.point;
        if let Some(parameter) = parameter_on_segment(second, point, tolerance)? {
            second_splits.push(parameter);
        }
    }
    Ok(())
}

fn parameter_on_segment(
    segment: &TrimmedCurve2,
    point: crate::Point2,
    tolerance: ModelTolerance,
) -> KernelResult<Option<f64>> {
    let parameter = match segment.curve() {
        Curve2::Line(line) => {
            let direction = line.direction();
            (point - line.origin()).dot(direction) / direction.magnitude2()
        }
        Curve2::Circle(circle) => {
            let radial = point - circle.center();
            if (radial.magnitude() - circle.radius()).abs() > tolerance.linear {
                return Ok(None);
            }
            trim_parameter(segment, radial.y.atan2(radial.x))
        }
        Curve2::Nurbs(_) => {
            return Ok(None);
        }
    };
    let low = segment.range().start.min(segment.range().end) - tolerance.parameter;
    let high = segment.range().start.max(segment.range().end) + tolerance.parameter;
    if parameter < low || parameter > high {
        return Ok(None);
    }
    let evaluated = segment.curve().evaluate(parameter)?.point;
    Ok((evaluated.distance(point) <= tolerance.linear).then_some(parameter))
}

fn fragments_match(
    first: &TrimmedCurve2,
    second: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    let first_start = first.start()?;
    let first_end = first.end()?;
    let second_start = second.start()?;
    let second_end = second.end()?;
    if !((first_start.distance(second_start) <= tolerance.linear
        && first_end.distance(second_end) <= tolerance.linear)
        || (first_start.distance(second_end) <= tolerance.linear
            && first_end.distance(second_start) <= tolerance.linear))
    {
        return Ok(false);
    }
    Ok(fragment_midpoint(first)?.distance(fragment_midpoint(second)?) <= tolerance.linear)
}

fn split_boundaries(
    boundaries: &[Wire2],
    split_parameters: Vec<Vec<Vec<f64>>>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<TrimmedCurve2>> {
    let mut output = Vec::new();
    for (wire_index, wire) in boundaries.iter().enumerate() {
        for (segment_index, segment) in wire.segments().iter().enumerate() {
            let mut parameters = vec![segment.range().start, segment.range().end];
            parameters.extend(
                split_parameters[wire_index][segment_index]
                    .iter()
                    .map(|parameter| trim_parameter(segment, *parameter)),
            );
            let start = segment.range().start;
            let span = segment.range().span();
            parameters.sort_by(|first, second| {
                ((*first - start) / span).total_cmp(&((*second - start) / span))
            });
            parameters.dedup_by(|first, second| (*first - *second).abs() <= tolerance.parameter);
            for interval in parameters.windows(2) {
                if (interval[1] - interval[0]).abs() <= tolerance.parameter {
                    continue;
                }
                output.push(TrimmedCurve2::new(
                    segment.curve().clone(),
                    ParameterRange::new(interval[0], interval[1])?,
                )?);
            }
        }
    }
    Ok(output)
}

fn trim_parameter(segment: &TrimmedCurve2, parameter: f64) -> f64 {
    if !matches!(segment.curve(), Curve2::Circle(_)) {
        return parameter;
    }
    let low = segment.range().start.min(segment.range().end);
    let high = segment.range().start.max(segment.range().end);
    let period = std::f64::consts::TAU;
    let candidate = parameter + ((low - parameter) / period).ceil() * period;
    if candidate <= high + f64::EPSILON * 64.0 {
        candidate
    } else {
        candidate - period
    }
}

fn fragment_midpoint(fragment: &TrimmedCurve2) -> KernelResult<crate::Point2> {
    Ok(fragment
        .curve()
        .evaluate((fragment.range().start + fragment.range().end) * 0.5)?
        .point)
}

fn profile_contains(profile: &Profile, point: crate::Point2) -> KernelResult<bool> {
    Ok(
        wire_contains_point(profile.outer(), point, profile.tolerance())?
            && !profile
                .holes()
                .iter()
                .map(|hole| wire_contains_point(hole, point, profile.tolerance()))
                .collect::<KernelResult<Vec<_>>>()?
                .into_iter()
                .any(|inside| inside),
    )
}

fn profiles_from_fragments(
    mut fragments: Vec<TrimmedCurve2>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<Profile>> {
    let mut wires = Vec::new();
    while let Some(first) = fragments.pop() {
        let start = first.start()?;
        let mut end = first.end()?;
        let mut loop_segments = vec![first];
        while end.distance(start) > tolerance.linear {
            let Some(next_index) = fragments.iter().position(|candidate| {
                candidate
                    .start()
                    .is_ok_and(|point| point.distance(end) <= tolerance.linear)
            }) else {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "analytical planar Boolean fragments do not form closed wires",
                ));
            };
            let next = fragments.swap_remove(next_index);
            end = next.end()?;
            loop_segments.push(next);
        }
        wires.push(Wire2::new(
            merge_adjacent_fragments(loop_segments, tolerance)?,
            tolerance,
        )?);
    }
    let mut outers = wires
        .iter()
        .filter(|wire| wire.is_counter_clockwise())
        .cloned()
        .map(|wire| (wire, Vec::new()))
        .collect::<Vec<(Wire2, Vec<Wire2>)>>();
    for hole in wires
        .into_iter()
        .filter(|wire| !wire.is_counter_clockwise())
    {
        let point = hole.segments()[0].start()?;
        let mut owner: Option<usize> = None;
        for (index, (outer, _)) in outers.iter().enumerate() {
            if wire_contains_point(outer, point, tolerance)?
                && owner.is_none_or(|current| outer.area() < outers[current].0.area())
            {
                owner = Some(index);
            }
        }
        let Some(owner) = owner else {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "analytical planar Boolean produced a hole without an outer boundary",
            ));
        };
        outers[owner].1.push(hole);
    }
    outers
        .into_iter()
        .map(|(outer, holes)| Profile::new(outer, holes, tolerance))
        .collect()
}

fn merge_adjacent_fragments(
    mut segments: Vec<TrimmedCurve2>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<TrimmedCurve2>> {
    let mut changed = true;
    while changed && segments.len() > 1 {
        changed = false;
        for index in 0..segments.len() {
            let next = (index + 1) % segments.len();
            let (Curve2::Line(_), Curve2::Line(_)) =
                (segments[index].curve(), segments[next].curve())
            else {
                continue;
            };
            let first_direction = segments[index]
                .curve()
                .evaluate(segments[index].range().start + segments[index].range().span() * 0.5)?
                .first_derivative
                * segments[index].range().span().signum();
            let second_direction = segments[next]
                .curve()
                .evaluate(segments[next].range().start + segments[next].range().span() * 0.5)?
                .first_derivative
                * segments[next].range().span().signum();
            let first_direction = first_direction.normalize();
            let second_direction = second_direction.normalize();
            if (first_direction.x * second_direction.y - first_direction.y * second_direction.x)
                .abs()
                > tolerance.angular
                || first_direction.dot(second_direction) < 1.0 - tolerance.angular
            {
                continue;
            }
            let start = segments[index].start()?;
            let join = segments[index].end()?;
            let next_start = segments[next].start()?;
            let end = segments[next].end()?;
            if join.distance(next_start) > tolerance.linear
                || ((next_start - start).x * first_direction.y
                    - (next_start - start).y * first_direction.x)
                    .abs()
                    > tolerance.linear
            {
                continue;
            }
            let merged = TrimmedCurve2::line(start, end)?;
            if next == 0 {
                segments[index] = merged;
                segments.remove(0);
            } else {
                segments[index] = merged;
                segments.remove(next);
            }
            changed = true;
            break;
        }
    }
    Ok(segments)
}

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}
