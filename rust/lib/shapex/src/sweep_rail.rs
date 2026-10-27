//! Sweep rails chained from several connected, tangent-continuous curves.
use super::*;
use crate::{Point3, Vec3};
use cgmath::prelude::*;

/// One trimmed curve of a sweep rail.
#[derive(Debug, Clone, PartialEq)]
pub struct RailSegment {
    pub curve: Curve3,
    pub range: ParameterRange,
}
impl RailSegment {
    pub fn new(curve: Curve3, range: ParameterRange) -> Self {
        Self { curve, range }
    }
    fn reversed(&self) -> Self {
        Self::new(self.curve.clone(), self.range.reversed())
    }
    /// Point and unit travel direction at the start or the end.
    fn end(&self, last: bool) -> KernelResult<(Point3, Vec3)> {
        let value = self.curve.evaluate(if last { self.range.end } else { self.range.start })?;
        Ok((value.point, value.first_derivative.normalize() * self.range.span().signum()))
    }
}

/// Connected segments traversed as one rail, ordered and oriented in travel
/// order. Consecutive segments join with a continuous tangent.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepRail {
    segments: Vec<RailSegment>,
    tolerance: ModelTolerance,
}
impl SweepRail {
    /// Chain segments given in any order and direction.
    pub fn new(
        segments: impl IntoIterator<Item = RailSegment>,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let mut remaining = segments.into_iter().collect::<Vec<_>>();
        if remaining.is_empty() {
            return Err(invalid("a sweep rail needs at least one segment"));
        }
        let mut rail = Self {
            segments: vec![remaining.remove(0)],
            tolerance,
        };
        while !remaining.is_empty() {
            if rail.is_closed()? {
                return Err(invalid("sweep rail segments must not branch"));
            }
            let mut attached = false;
            for back in [true, false] {
                let mut found = rail.continuations(&remaining, back, false)?;
                if found.len() > 1 {
                    return Err(invalid("sweep rail segments must not branch"));
                }
                if let Some((index, segment)) = found.pop() {
                    if !rail.joins(&segment, back)? {
                        return Err(invalid("sweep rail segments must join tangentially"));
                    }
                    remaining.swap_remove(index);
                    rail.attach(segment, back);
                    attached = true;
                    break;
                }
            }
            if !attached {
                return Err(invalid("sweep rail segments must form one connected chain"));
            }
        }
        if rail.segments.len() > 1 && rail.is_closed()? && !rail.joins(&rail.segments[0], true)? {
            return Err(invalid(
                "closed sweep rail must have a continuous tangent at its seam",
            ));
        }
        Ok(rail)
    }
    /// Continue both open ends through candidates that join tangentially,
    /// stopping where the continuation is missing or ambiguous.
    pub fn extended_tangentially(mut self, candidates: &[RailSegment]) -> KernelResult<Self> {
        let mut remaining = candidates.to_vec();
        for back in [true, false] {
            while !self.is_closed()? {
                let mut found = self.continuations(&remaining, back, true)?;
                if found.len() != 1 {
                    break;
                }
                let (index, segment) = found.pop().unwrap();
                remaining.swap_remove(index);
                self.attach(segment, back);
            }
        }
        Ok(self)
    }
    pub fn segments(&self) -> &[RailSegment] {
        &self.segments
    }
    /// Point and unit travel direction where the rail starts.
    pub fn start(&self) -> KernelResult<(Point3, Vec3)> {
        self.segments[0].end(false)
    }
    /// Point and unit travel direction where the rail ends.
    pub fn end(&self) -> KernelResult<(Point3, Vec3)> {
        self.segments[self.segments.len() - 1].end(true)
    }
    pub fn is_closed(&self) -> KernelResult<bool> {
        Ok(self.start()?.0.distance(self.end()?.0) <= self.tolerance.linear)
    }
    /// A closed chain reopened at its point nearest to `target`, splitting
    /// the segment there unless the point is a joint.
    pub(crate) fn opened_near(&self, target: Point3) -> KernelResult<Self> {
        let distance = |segment: &RailSegment, parameter: f64| {
            segment.curve.evaluate(parameter).map(|value| value.point.distance(target))
        };
        let mut best = (0, self.segments[0].range.start, f64::INFINITY);
        for (index, segment) in self.segments.iter().enumerate() {
            let at = |i: usize| segment.range.start + segment.range.span() * i as f64 / 32.0;
            for i in 0..=32 {
                let value = distance(segment, at(i))?;
                if value < best.2 {
                    best = (index, at(i), value);
                }
            }
            // Refine around the nearest sample of this segment
            if best.0 == index {
                let step = segment.range.span() / 32.0;
                let clamp = |parameter: f64| {
                    parameter.clamp(segment.range.start.min(segment.range.end), segment.range.start.max(segment.range.end))
                };
                let (mut low, mut high) = (clamp(best.1 - step), clamp(best.1 + step));
                for _ in 0..64 {
                    let third = (high - low) / 3.0;
                    if distance(segment, low + third)? < distance(segment, high - third)? {
                        high -= third;
                    } else {
                        low += third;
                    }
                }
                let parameter = (low + high) * 0.5;
                best = (index, parameter, distance(segment, parameter)?);
            }
        }
        let (index, parameter, _) = best;
        let segment = &self.segments[index];
        let near = |end: f64| {
            Ok::<_, KernelError>(
                segment.curve.evaluate(end)?.point.distance(segment.curve.evaluate(parameter)?.point)
                    <= self.tolerance.linear,
            )
        };
        let mut segments = self.segments.clone();
        if near(segment.range.start)? {
            segments.rotate_left(index);
        } else if near(segment.range.end)? {
            segments.rotate_left(index + 1);
        } else {
            let curve = segment.curve.clone();
            segments[index] = RailSegment::new(curve.clone(), ParameterRange::new(parameter, segment.range.end)?);
            segments.insert(index, RailSegment::new(curve, ParameterRange::new(segment.range.start, parameter)?));
            segments.rotate_left(index + 1);
        }
        Ok(Self {
            segments,
            tolerance: self.tolerance,
        })
    }
    pub fn reversed(&self) -> Self {
        Self {
            segments: self.segments.iter().rev().map(RailSegment::reversed).collect(),
            tolerance: self.tolerance,
        }
    }
    /// Candidates touching the rail's end (`back`) or start, oriented to
    /// continue it, with their index.
    fn continuations(
        &self,
        candidates: &[RailSegment],
        back: bool,
        tangent_only: bool,
    ) -> KernelResult<Vec<(usize, RailSegment)>> {
        let point = if back { self.end()?.0 } else { self.start()?.0 };
        let mut found = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            // A continuation at the end starts there; one at the start ends there.
            let segment = if candidate.end(!back)?.0.distance(point) <= self.tolerance.linear {
                candidate.clone()
            } else if candidate.end(back)?.0.distance(point) <= self.tolerance.linear {
                candidate.reversed()
            } else {
                continue;
            };
            if !tangent_only || self.joins(&segment, back)? {
                found.push((index, segment));
            }
        }
        Ok(found)
    }
    /// Whether an oriented continuation keeps the rail's tangent.
    fn joins(&self, segment: &RailSegment, back: bool) -> KernelResult<bool> {
        let (before, after) = if back {
            (self.end()?.1, segment.end(false)?.1)
        } else {
            (segment.end(true)?.1, self.start()?.1)
        };
        Ok(before.dot(after) >= 1.0 - self.tolerance.angular * 16.0)
    }
    fn attach(&mut self, segment: RailSegment, back: bool) {
        if back {
            self.segments.push(segment);
        } else {
            self.segments.insert(0, segment);
        }
    }
}
