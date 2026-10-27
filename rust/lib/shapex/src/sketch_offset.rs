//! Sketch offsetting for application tooling: open and closed wires with
//! join styles, and region offsets of profiles. Nothing in the kernel depends
//! on it; it is exported for the WASM transport.
use super::planar::*;
use super::{invalid, Curve2, KernelResult, ModelTolerance, ParameterRange, Profile, TrimmedCurve2, Wire2};
use crate::{Point2, Vec2};
use cgmath::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinStyle {
    Round,
    Miter,
    Bevel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OffsetOptions {
    pub join: JoinStyle,
    /// Optional per-corner styles in traversal order (including the closing
    /// corner of a closed wire). An empty vector uses `join` everywhere.
    pub corner_joins: Vec<JoinStyle>,
    /// Maximum miter length divided by offset distance; infinity is uncapped.
    pub miter_limit: f64,
}
impl Default for OffsetOptions {
    fn default() -> Self {
        Self {
            join: JoinStyle::Round,
            corner_joins: Vec::new(),
            miter_limit: 4.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WireOffset2 {
    pub wire: Wire2,
    /// Source segment for each output segment. Joins have no source segment.
    pub sources: Vec<Option<usize>>,
}

impl Wire2 {
    /// Require a simple wire: no self-intersections, overlaps, or contacts
    /// except the shared endpoint of consecutive segments.
    pub fn validate_simple(&self, tolerance: ModelTolerance) -> KernelResult<()> {
        tolerance.validate()?;
        let segments = self.segments();
        for (i, curve) in segments.iter().enumerate() {
            if !curve.self_intersections(tolerance)?.is_empty() {
                return Err(invalid("wire intersects itself"));
            }
            for (j, other) in segments[..i].iter().enumerate() {
                for hit in curve.intersections(other, tolerance)? {
                    let allowed = match hit {
                        CurveIntersection2::Point { point, .. } => {
                            (j + 1 == i && point.distance(curve.start()?) <= tolerance.linear)
                                || (self.is_closed()
                                    && j == 0
                                    && i + 1 == segments.len()
                                    && point.distance(curve.end()?) <= tolerance.linear)
                        }
                        CurveIntersection2::Overlap { .. } => false,
                    };
                    if !allowed {
                        return Err(invalid("wire boundaries intersect or overlap"));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn offset(
        &self,
        distance: f64,
        options: OffsetOptions,
        tolerance: ModelTolerance,
    ) -> KernelResult<WireOffset2> {
        if !distance.is_finite() || options.miter_limit.is_nan() || options.miter_limit < 1.0 {
            return Err(invalid(
                "offset requires finite distance and a miter limit of at least one (or infinity)",
            ));
        }
        let closed = self.is_closed();
        let corner_count = self.segments().len() - usize::from(!closed);
        if !options.corner_joins.is_empty() && options.corner_joins.len() != corner_count {
            return Err(invalid("offset needs one join style per corner"));
        }
        let mut curves = self
            .segments()
            .iter()
            .map(|curve| curve.offset(distance))
            .collect::<KernelResult<Vec<_>>>()?;
        let mut joins = vec![None; curves.len()];
        let original_ranges: Vec<_> = curves.iter().map(TrimmedCurve2::range).collect();
        for i in 0..curves.len() {
            if i + 1 == curves.len() && !closed {
                break;
            }
            let next = (i + 1) % curves.len();
            let a = curves[i].end()?;
            let b = curves[next].start()?;
            if a.distance(b) <= tolerance.linear {
                continue;
            }
            let vertex = self.segments()[i].end()?;
            let tangent = |curve: &TrimmedCurve2, end: bool| -> KernelResult<Vec2> {
                let range = curve.range();
                Ok(curve
                    .curve()
                    .evaluate(if end { range.end } else { range.start })?
                    .first_derivative
                    .normalize()
                    * range.span().signum())
            };
            let u = tangent(&self.segments()[i], true)?;
            let v = tangent(&self.segments()[next], false)?;
            let turn = u.x * v.y - u.y * v.x;
            let outside = turn * distance < 0.0;
            let join = options.corner_joins.get(i).copied().unwrap_or(options.join);
            if outside && join == JoinStyle::Round {
                let radial = (a - vertex).normalize() + (b - vertex).normalize();
                if radial.magnitude2() == 0.0 {
                    return Err(invalid("a reversing corner has no unique round join"));
                }
                joins[i] = Some(TrimmedCurve2::arc_through(
                    a,
                    vertex + radial.normalize() * distance.abs(),
                    b,
                )?);
                continue;
            }
            if outside && join == JoinStyle::Bevel {
                joins[i] = Some(TrimmedCurve2::line(a, b)?);
                continue;
            }
            let mut hit = None;
            if let (Curve2::Line(left), Curve2::Line(right)) =
                (curves[i].curve(), curves[next].curve())
            {
                let denominator = left.direction().x * right.direction().y
                    - left.direction().y * right.direction().x;
                if denominator.abs()
                    > tolerance.angular
                        * left.direction().magnitude()
                        * right.direction().magnitude()
                {
                    let delta = right.origin() - left.origin();
                    let t = (delta.x * right.direction().y - delta.y * right.direction().x)
                        / denominator;
                    let p = left.origin() + left.direction() * t;
                    let s = (p - right.origin()).dot(right.direction())
                        / right.direction().magnitude2();
                    hit = Some((t, s, p));
                }
            } else {
                let hits = curves[i].intersections(&curves[next], tolerance)?;
                for intersection in hits {
                    if let CurveIntersection2::Point {
                        point,
                        first_parameter,
                        second_parameter,
                        ..
                    } = intersection
                    {
                        if hit
                            .as_ref()
                            .is_none_or(|(_, _, previous): &(f64, f64, Point2)| {
                                point.distance(vertex) < previous.distance(vertex)
                            })
                        {
                            hit = Some((first_parameter, second_parameter, point));
                        }
                    }
                }
            }
            if let Some((t, s, p)) = hit {
                if outside && p.distance(vertex) > distance.abs() * options.miter_limit {
                    joins[i] = Some(TrimmedCurve2::line(a, b)?);
                    continue;
                }
                curves[i] = TrimmedCurve2::new(
                    curves[i].curve().clone(),
                    ParameterRange::new(curves[i].range().start, t)?,
                )?;
                curves[next] = TrimmedCurve2::new(
                    curves[next].curve().clone(),
                    ParameterRange::new(s, curves[next].range().end)?,
                )?;
            } else {
                return Err(invalid("offset corner has no retained intersection"));
            }
        }
        let mut segments = Vec::new();
        let mut sources = Vec::new();
        for (i, curve) in curves.into_iter().enumerate() {
            if curve.range().span() * original_ranges[i].span() <= 0.0
                || curve.length(tolerance)? <= tolerance.linear
            {
                return Err(invalid("offset collapses or reverses an edge"));
            }
            segments.push(curve);
            sources.push(Some(i));
            if let Some(join) = joins[i].take() {
                segments.push(join);
                sources.push(None);
            }
        }
        Ok(WireOffset2 {
            wire: Self::new(segments, tolerance)?,
            sources,
        })
    }
}

impl Profile {
    /// Positive distance expands the filled region; negative distance erodes it.
    /// Resolve intersecting offsets by classifying fragments against signed
    /// distance to the original boundary, then assemble all surviving regions.
    pub fn offset(&self, distance: f64) -> KernelResult<Vec<Self>> {
        if !distance.is_finite() {
            return Err(invalid("offset distance must be finite"));
        }
        if distance == 0.0 {
            return Ok(vec![self.clone()]);
        }
        let tolerance = self.tolerance();
        let mut curves: Vec<TrimmedCurve2> = Vec::new();

        // Region offsets must survive disappearing edges. Join a path only
        // after the arrangement is classified: pre-trimming a short edge
        // would discard the support needed by its surviving neighbors.
        let mut bounds = None;
        for wire in std::iter::once(self.outer()).chain(self.holes()) {
            for segment in wire.segments() {
                let next = super::bounds::curve2(segment.curve(), segment.range())?;
                bounds = Some(
                    bounds.map_or(next, |previous: super::bounds::Bounds| previous.union(next)),
                );
            }
        }
        let bounds = bounds.ok_or_else(|| invalid("profile has no boundary"))?;
        let padding = (bounds.1 - bounds.0).magnitude() + distance.abs() * 4.0;
        let mut insert = |curve: TrimmedCurve2| {
            let duplicate = curves
                .iter()
                .any(|existing| match (existing.curve(), curve.curve()) {
                    (Curve2::Line(a), Curve2::Line(b)) => {
                        let u = a.direction().normalize();
                        let v = b.direction().normalize();
                        let delta = b.origin() - a.origin();
                        (u.x * v.y - u.y * v.x).abs() <= tolerance.angular
                            && (delta.x * u.y - delta.y * u.x).abs() <= tolerance.linear
                    }
                    (Curve2::Circle(a), Curve2::Circle(b)) => {
                        a.center().distance(b.center()) <= tolerance.linear
                            && (a.radius() - b.radius()).abs() <= tolerance.linear
                    }
                    _ => false,
                });
            if !duplicate {
                curves.push(curve);
            }
        };
        for wire in std::iter::once(self.outer()).chain(self.holes()) {
            for (i, segment) in wire.segments().iter().enumerate() {
                let collapsed = matches!(segment.curve(),Curve2::Circle(circle) if circle.radius()+distance*segment.range().span().signum()<=0.0);
                if !collapsed {
                    let offset = segment.offset(-distance)?;
                    let curve = match offset.curve() {
                        Curve2::Line(line) => {
                            let range = offset.range();
                            let delta = padding / line.direction().magnitude();
                            TrimmedCurve2::new(
                                *line,
                                ParameterRange::new(
                                    range.start.min(range.end) - delta,
                                    range.start.max(range.end) + delta,
                                )?,
                            )?
                        }
                        Curve2::Circle(circle) => {
                            TrimmedCurve2::circle(circle.center(), circle.radius())?
                        }
                        _ => offset,
                    };
                    insert(curve);
                }
                let next = &wire.segments()[(i + 1) % wire.segments().len()];
                let a = segment
                    .curve()
                    .evaluate(segment.range().end)?
                    .first_derivative
                    .normalize()
                    * segment.range().span().signum();
                let b = next
                    .curve()
                    .evaluate(next.range().start)?
                    .first_derivative
                    .normalize()
                    * next.range().span().signum();
                if a.dot(b) < 1.0 - tolerance.angular
                    || (a.x * b.y - a.y * b.x).abs() > tolerance.angular
                {
                    insert(TrimmedCurve2::circle(segment.end()?, distance.abs())?);
                }
            }
        }
        let mut cuts = vec![Vec::new(); curves.len()];
        for (i, curve) in curves.iter().enumerate() {
            for hit in curve.self_intersections(tolerance)? {
                match hit {
                    CurveIntersection2::Point {
                        first_parameter,
                        second_parameter,
                        ..
                    } => cuts[i].extend([first_parameter, second_parameter]),
                    CurveIntersection2::Overlap { first, second } => {
                        cuts[i].extend([first.start, first.end, second.start, second.end])
                    }
                }
            }
        }
        for i in 0..curves.len() {
            for j in 0..i {
                for hit in curves[i].intersections(&curves[j], tolerance)? {
                    match hit {
                        CurveIntersection2::Point {
                            first_parameter,
                            second_parameter,
                            ..
                        } => {
                            cuts[i].push(first_parameter);
                            cuts[j].push(second_parameter);
                        }
                        CurveIntersection2::Overlap { first, second } => {
                            cuts[i].extend([first.start, first.end]);
                            cuts[j].extend([second.start, second.end]);
                        }
                    }
                }
            }
        }
        let signed_distance = |point: Point2| -> KernelResult<f64> {
            let mut nearest = f64::INFINITY;
            for wire in std::iter::once(self.outer()).chain(self.holes()) {
                for segment in wire.segments() {
                    nearest = nearest.min(segment.closest_point(point, tolerance)?.distance);
                }
            }
            Ok(
                if self.classify_point(point)? == PointClassification::Inside {
                    -nearest
                } else {
                    nearest
                },
            )
        };
        let mut fragments = Vec::new();
        for (curve, parameters) in curves.iter().zip(cuts) {
            for fragment in curve.split_at(parameters, tolerance)? {
                let value = fragment.curve().evaluate(fragment.range().middle())?;
                let tangent = value.first_derivative.normalize() * fragment.range().span().signum();
                let normal = Vec2::new(-tangent.y, tangent.x) * tolerance.linear * 32.0;
                let left = signed_distance(value.point + normal)? <= distance;
                let right = signed_distance(value.point - normal)? <= distance;
                if left != right {
                    fragments.push(if left { fragment } else { fragment.reversed() });
                }
            }
        }
        super::profile::profiles_from_fragments(fragments, tolerance)
    }
}
