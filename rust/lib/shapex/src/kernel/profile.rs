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
                            return Err(KernelError::new(
                                DiagnosticKind::UnsupportedGeometry,
                                "analytical planar Booleans do not yet support coincident curve intervals",
                            ));
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
        for fragment in first_fragments {
            let inside = profile_contains(other, fragment_midpoint(&fragment)?)?;
            let keep = match kind {
                BooleanKind::Union | BooleanKind::Difference => !inside,
                BooleanKind::Intersection => inside,
            };
            if keep {
                selected.push(fragment);
            }
        }
        for fragment in second_fragments {
            let inside = profile_contains(self, fragment_midpoint(&fragment)?)?;
            let keep = match kind {
                BooleanKind::Union => !inside,
                BooleanKind::Difference | BooleanKind::Intersection => inside,
            };
            if keep {
                selected.push(if kind == BooleanKind::Difference {
                    fragment.reversed()
                } else {
                    fragment
                });
            }
        }
        profiles_from_fragments(selected, self.tolerance)
    }
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
        wires.push(Wire2::new(loop_segments, tolerance)?);
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

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}
