//! Trimmed-face geometry shared by classification, intersections, and operations.
use std::borrow::Cow;
use cgmath::prelude::*;
use crate::{Point2, Point3, Vec2};
use super::polygon::point_segment_distance;
use super::{box_center, nearest_uv, whole_periods, Curve2, Curve3, IntersectionSet, KernelResult,
    LoopKind, ModelTolerance, ParameterRange, Surface3};
use crate::geometry_error;

/// A directed trim with caller-owned metadata. Geometry utilities never inspect
/// that metadata; Boolean construction can retain ancestry without imposing it
/// on classification or topology validation.
#[derive(Debug, Clone)]
pub(super) struct FaceCurve<P = ()> {
    pub(super) provenance: P,
    pub(super) curve: Curve3,
    pub(super) pcurve: Curve2,
    pub(super) range: ParameterRange,
    pub(super) loop_end: bool,
    pub(super) loop_kind: LoopKind,
    pub(super) parameter_seam: bool,
}

impl<P> FaceCurve<P> {
    pub(super) fn with_provenance<Q>(self, provenance: Q) -> FaceCurve<Q> {
        FaceCurve { provenance, curve: self.curve, pcurve: self.pcurve,
            range: self.range, loop_end: self.loop_end, loop_kind: self.loop_kind,
            parameter_seam: self.parameter_seam }
    }
}

/// Translate periodic UV copies of a bounded intersection while retaining its
/// exact spatial curve and parameterization.
pub(super) fn intersection_chart_copies(
    domain: super::DomainBounds,
    pcurve: &Curve2,
    surface: &Surface3,
    bounds: [ParameterRange; 2],
    tolerance: ModelTolerance,
) -> KernelResult<Vec<Curve2>> {
    if !surface.domains().iter().any(|d| d.period.is_some()) {
        return Ok(vec![pcurve.clone()]);
    }
    let super::DomainBounds::Bounded(range) = domain else {
        return Ok(vec![pcurve.clone()]);
    };
    let mut minimum = Point2::new(f64::INFINITY, f64::INFINITY);
    let mut maximum = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for i in 0..=32 {
        let p = pcurve
            .evaluate(range.start + range.span() * i as f64 / 32.0)?
            .point;
        minimum.x = minimum.x.min(p.x);
        minimum.y = minimum.y.min(p.y);
        maximum.x = maximum.x.max(p.x);
        maximum.y = maximum.y.max(p.y);
    }
    let offsets = |axis: usize| {
        let Some(period) = surface.domains()[axis].period else { return vec![0.0]; };
        [-period, 0.0, period]
            .into_iter()
            .filter(|offset| {
                minimum[axis] + offset < bounds[axis].end + tolerance.parameter
                    && maximum[axis] + offset > bounds[axis].start - tolerance.parameter
            })
            .collect::<Vec<_>>()
    };
    let mut result = Vec::new();
    for u in offsets(0) {
        for v in offsets(1) {
            result.push(if u == 0.0 && v == 0.0 {
                pcurve.clone()
            } else {
                translate_pcurve(pcurve, Vec2::new(u, v), tolerance)?
            });
        }
    }
    Ok(result)
}

pub(super) fn align_pcurve_to_face_chart<P>(
    pcurve: &Curve2,
    trim: Option<ParameterRange>,
    surface: &Surface3,
    boundary: &[FaceCurve<P>],
    tolerance: ModelTolerance,
) -> KernelResult<Curve2> {
    let mut bounds = [
        Point2::new(f64::INFINITY, f64::INFINITY),
        Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    ];
    for segment in boundary {
        for (_, point) in sample_pcurve(segment, 8)? {
            bounds[0].x = bounds[0].x.min(point.x);
            bounds[0].y = bounds[0].y.min(point.y);
            bounds[1].x = bounds[1].x.max(point.x);
            bounds[1].y = bounds[1].y.max(point.y);
        }
    }
    let parameter = match trim
        .map(super::DomainBounds::Bounded)
        .unwrap_or(pcurve.domain().bounds)
    {
        super::DomainBounds::Bounded(range) => range.middle(),
        _ => 0.0,
    };
    let point = pcurve.evaluate(parameter)?.point;
    let center = bounds[0].midpoint(bounds[1]);
    let offset = whole_periods(surface.domains(), center - point);
    if offset.magnitude2() <= tolerance.parameter.powi(2) {
        return Ok(pcurve.clone());
    }
    translate_pcurve(pcurve, offset, tolerance)
}

pub(super) fn translate_pcurve(
    pcurve: &Curve2,
    offset: Vec2,
    tolerance: ModelTolerance,
) -> KernelResult<Curve2> {
    match pcurve {
        Curve2::Projected(_) => Err(super::unsupported("translating a projected curve requires a planar transform")),
        Curve2::Offset(curve) => Ok(super::OffsetCurve2::new(translate_pcurve(curve.base(), offset, tolerance)?, curve.distance())?.into()),
        Curve2::Line(line) => {
            Ok(super::Line2::new(line.origin() + offset, line.direction())?.into())
        }
        Curve2::Circle(circle) => {
            Ok(super::Circle2::new(circle.center() + offset, circle.radius())?.into())
        }
        Curve2::Nurbs(curve) => Ok(super::NurbsCurve2::new(
            curve.degree(),
            curve
                .control_points()
                .iter()
                .map(|point| *point + offset)
                .collect(),
            curve.weights().to_vec(),
            curve.knots().to_vec(),
        )?
        .into()),
        Curve2::SurfaceProjection(projection) => {
            let guide = translate_pcurve(projection.guide(), offset, tolerance)?;
            Ok(super::SurfaceProjectionPcurve2::new(
                projection.curve().clone(),
                projection.surface().clone(),
                guide,
                projection.range(),
                tolerance,
            )?
            .into())
        }
        Curve2::SurfaceIntersection(intersection) => {
            let spatial = intersection.spatial_curve();
            let Curve3::SurfaceIntersection(curve) = &spatial else {
                unreachable!()
            };
            // A projection can retain an intersection on a different support as
            // its guide. Translate that guide on its own selected side, not on
            // the outer projection's destination surface.
            let guide = translate_pcurve(intersection.guide(), offset, tolerance)?;
            let range = curve.range();
            Ok(super::SurfaceProjectionPcurve2::new(
                spatial,
                intersection.surface().clone(),
                guide,
                range,
                tolerance,
            )?
            .into())
        }
    }
}

pub(super) fn face_parameter_bounds<P>(
    boundary: &[FaceCurve<P>],
    tolerance: ModelTolerance,
) -> KernelResult<[ParameterRange; 2]> {
    let mut minimum = Point2::new(f64::INFINITY, f64::INFINITY);
    let mut maximum = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for curve in boundary {
        let samples = sample_pcurve(curve, 16)?;
        let mut points = samples.iter().map(|sample| sample.1).collect::<Vec<_>>();
        // A merged guide need not reach a chart extremum at a uniform sample.
        // Tracing against an underestimated box terminates an intersection
        // inside the face and leaves an open arrangement boundary.
        for pair in samples.windows(2) {
            let first = curve.pcurve.first_order(pair[0].0)?.1;
            let second = curve.pcurve.first_order(pair[1].0)?.1;
            for axis in 0..2 {
                if first[axis] * second[axis] >= 0.0 { continue; }
                let mut low = pair[0].0;
                let mut high = pair[1].0;
                for _ in 0..48 {
                    let middle = (low + high) * 0.5;
                    let derivative = curve.pcurve.first_order(middle)?.1[axis];
                    if first[axis] * derivative > 0.0 { low = middle; }
                    else { high = middle; }
                }
                points.push(curve.pcurve.first_order((low + high) * 0.5)?.0);
            }
        }
        for point in points {
            minimum.x = minimum.x.min(point.x);
            minimum.y = minimum.y.min(point.y);
            maximum.x = maximum.x.max(point.x);
            maximum.y = maximum.y.max(point.y);
        }
    }
    let expand = |minimum: f64, maximum: f64| {
        let padding = tolerance.parameter * 64.0;
        if maximum - minimum <= padding {
            (minimum - padding, maximum + padding)
        } else {
            (minimum, maximum)
        }
    };
    let u = expand(minimum.x, maximum.x);
    let v = expand(minimum.y, maximum.y);
    Ok([
        ParameterRange::new(u.0, u.1)?,
        ParameterRange::new(v.0, v.1)?,
    ])
}

pub(super) fn face_boundary_curves(
    topology: &super::Topology,
    face: super::FaceId,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<FaceCurve<super::CoedgeId>>> {
    let surface = topology.face(face)?.surface();
    let domains = surface.domains();
    let mut curves = Vec::new();
    for loop_id in topology.face(face)?.loops() {
        let loop_value = topology.loop_(*loop_id)?;
        let coedges = topology.loop_coedges(*loop_id)?;
        let last = coedges.len().saturating_sub(1);
        // Keep the complete loop in one continuous periodic chart. Individual
        // coedges may use different principal copies of the same seam.
        let mut previous = None::<Point2>;
        for (index, coedge_id) in coedges.into_iter().enumerate() {
            let coedge = topology.coedge(coedge_id)?;
            let edge = topology.edge(coedge.edge())?;
            let mut pcurve = coedge.pcurve().clone();
            if let Some(previous) = previous {
                let start = pcurve.evaluate(coedge.range().start)?.point;
                let delta = previous - start;
                let rates = if domains.iter().any(|domain| domain.period.is_some()) {
                    let (_, du, dv) = surface.first_order(previous.x, previous.y)?;
                    [du.magnitude(), dv.magnitude()]
                } else { [f64::INFINITY; 2] };
                // At a pole the angular coordinate is unconstrained. Keep
                // each incident meridian's own chart rather than rounding a
                // half-turn bridge onto the opposite side of the face.
                let shift = |axis: usize| if rates[axis] > tolerance.linear {
                    domains[axis].whole_periods(delta[axis])
                } else {
                    0.0
                };
                let offset = Vec2::new(shift(0), shift(1));
                if offset.magnitude2() > tolerance.parameter.powi(2) {
                    pcurve = translate_pcurve(&pcurve, offset, tolerance)?;
                }
            }
            previous = Some(pcurve.evaluate(coedge.range().end)?.point);
            curves.push(FaceCurve {
                provenance: coedge_id,
                curve: edge.curve().clone(),
                pcurve,
                range: coedge.range(),
                loop_end: index == last,
                loop_kind: loop_value.kind(),
                parameter_seam: topology.edge_faces(coedge.edge())?.len() == 1,
            });
        }
    }
    Ok(curves)
}

/// Chords of each boundary p-curve, in boundary order. Implicit p-curves solve
/// a projection per evaluation, so repeated point queries reuse these.
#[derive(Clone)]
pub(super) struct TrimSamples {
    curves: Vec<Vec<(f64, Point2)>>,
    /// Only complete contours can be skipped by enclosure: an open collection
    /// of imprinted curves can still cross a ray from outside its bounds.
    loops: Vec<(std::ops::Range<usize>, Option<super::bounds::Bounds>)>,
}

impl std::ops::Deref for TrimSamples {
    type Target = [Vec<(f64, Point2)>];

    fn deref(&self) -> &Self::Target { &self.curves }
}

pub(super) fn trim_samples<P>(boundary: &[FaceCurve<P>]) -> KernelResult<TrimSamples> {
    let curves = boundary.iter().map(|curve| sample_pcurve(curve, 32))
        .collect::<KernelResult<Vec<_>>>()?;
    let mut loops = Vec::new();
    let mut start = 0;
    let mut bounds: Option<super::bounds::Bounds> = None;
    let mut bounded = true;
    for (index, curve) in boundary.iter().enumerate() {
        if bounded {
            if let Ok(next) = super::bounds::curve2(&curve.pcurve, curve.range) {
                bounds = Some(bounds.map_or(next, |current| current.union(next)));
            } else {
                bounded = false;
                bounds = None;
            }
        }
        if curve.loop_end {
            loops.push((start..index + 1, bounds));
            start = index + 1;
            bounds = None;
            bounded = true;
        }
    }
    if start < boundary.len() { loops.push((start..boundary.len(), None)); }
    Ok(TrimSamples { curves, loops })
}

pub(super) fn point_in_trim<P>(point: Point2, boundary: &[FaceCurve<P>]) -> KernelResult<bool> {
    point_in_sampled_trim(point, boundary, &trim_samples(boundary)?)
}

pub(super) fn point_in_sampled_trim<P>(
    point: Point2,
    boundary: &[FaceCurve<P>],
    samples: &TrimSamples,
) -> KernelResult<bool> {
    let mut crossings = 0_u32;
    let query = super::bounds::Bounds(
        Point3::new(point.x, point.y, 0.0), Point3::new(point.x, point.y, 0.0),
    );
    for (range, bounds) in &samples.loops {
        if bounds.is_some_and(|bounds| bounds.separated(query, 1e-9)) { continue; }
        for (curve, samples) in boundary[range.clone()].iter().zip(&samples.curves[range.clone()]) {
            for pair in samples.windows(2) {
                let mut first = pair[0].1;
                let mut second = pair[1].1;
                // Adjacent exact p-curves can round their common endpoint to
                // opposite sides of the ray. Give that endpoint one half-open
                // crossing decision, particularly at a tangent line/arc junction.
                for endpoint in [&mut first, &mut second] {
                    if (endpoint.y - point.y).abs() <= 1.0e-12 {
                        endpoint.y = point.y;
                    }
                }
                if point_segment_distance(point, first, second) <= 1.0e-10 {
                    return Ok(true);
                }
                if (first.y > point.y) != (second.y > point.y) {
                    // The chords only bracket a ray crossing. Classifying against
                    // their x coordinate can exclude an exact intersection lying
                    // on a curved trim, leaving its neighboring face open.
                    let mut low = pair[0].0;
                    let mut high = pair[1].0;
                    let mut crossing = first;
                    for _ in 0..48 {
                        if first.y == point.y {
                            crossing = first;
                            break;
                        }
                        if second.y == point.y {
                            crossing = second;
                            break;
                        }
                        let middle = (low + high) * 0.5;
                        crossing = curve.pcurve.evaluate(middle)?.point;
                        if (crossing.y - point.y).abs() <= 1.0e-12 {
                            break;
                        }
                        if (crossing.y > point.y) == (first.y > point.y) {
                            low = middle;
                        } else {
                            high = middle;
                        }
                    }
                    let x = crossing.x;
                    if (x - point.x).abs() <= 1.0e-9 {
                        return Ok(true);
                    }
                    if x > point.x {
                        crossings += 1;
                    }
                }
            }
        }
        // Close the UV contour at shared vertices too. Independently corrected
        // coedges can round to opposite sides of a ray at their common endpoint;
        // omitting that tiny chart bridge changes the crossing parity everywhere
        // to its left. At a pole the same bridge spans the angular chart gap.
        if !boundary[range.end - 1].loop_end { continue; }
        for index in range.clone() {
            let next = if index + 1 == range.end { range.start } else { index + 1 };
            let mut first = samples[index].last().unwrap().1;
            let mut second = samples[next][0].1;
            for endpoint in [&mut first, &mut second] {
                if (endpoint.y - point.y).abs() <= 1e-12 { endpoint.y = point.y; }
            }
            if (first.y > point.y) != (second.y > point.y) {
                let x = first.x + (second.x - first.x) * (point.y - first.y) / (second.y - first.y);
                if (x - point.x).abs() <= 1e-9 { return Ok(true); }
                if x > point.x { crossings += 1; }
            }
        }
    }
    Ok(crossings % 2 == 1)
}

/// Test every periodic representative that lies within the trim's chart bounds.
/// A relocated seam can make that chart wider than one period, so the copy
/// nearest its bounding-box center is not sufficient to classify a point.
pub(super) fn point_in_sampled_periodic_trim<P>(
    point: Point2,
    boundary: &[FaceCurve<P>],
    samples: &TrimSamples,
    domains: [super::ParameterDomain; 2],
    bounds: [ParameterRange; 2],
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    if point_in_sampled_trim(point, boundary, samples)? { return Ok(true); }
    let copies: [Vec<f64>; 2] = std::array::from_fn(|axis| {
        if let Some(period) = domains[axis].period {
            let first = ((bounds[axis].start - point[axis] - tolerance.parameter) / period).ceil() as i64;
            let last = ((bounds[axis].end - point[axis] + tolerance.parameter) / period).floor() as i64;
            (first..=last).map(|turn| point[axis] + turn as f64 * period).collect()
        } else { vec![point[axis]] }
    });
    for u in &copies[0] {
        for v in &copies[1] {
            let copy = Point2::new(*u, *v);
            if copy != point && point_in_sampled_trim(copy, boundary, samples)? { return Ok(true); }
        }
    }
    Ok(false)
}

pub(super) fn sample_pcurve<P>(curve: &FaceCurve<P>, count: usize) -> KernelResult<Vec<(f64, Point2)>> {
    let mut parameters = (0..=count)
        .map(|index| curve.range.start + curve.range.span() * index as f64 / count.max(1) as f64)
        .collect::<Vec<_>>();
    parameters.extend(curve.pcurve.integration_breakpoints(curve.range));
    parameters.sort_by(f64::total_cmp);
    parameters.dedup();
    if curve.range.span() < 0.0 { parameters.reverse(); }
    parameters.into_iter()
        .map(|parameter| Ok((parameter, curve.pcurve.evaluate(parameter)?.point)))
        .collect()
}


pub(super) fn trimmed_faces_intersect(
    topology: &super::Topology,
    first: super::FaceId,
    second: super::FaceId,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    // Only certified enclosures may skip the exact test.
    if let (Ok(first_bounds), Ok(second_bounds)) =
        (super::bounds::face(topology, first), super::bounds::face(topology, second)) {
        if first_bounds.separated(second_bounds, tolerance.linear) {
            return Ok(false);
        }
    }
    let first_surface = topology.face(first)?.surface();
    let second_surface = topology.face(second)?.surface();
    let first_boundary = face_boundary_curves(topology, first, tolerance)?;
    let second_boundary = face_boundary_curves(topology, second, tolerance)?;
    let first_bounds = face_parameter_bounds(&first_boundary, tolerance)?;
    let second_bounds = face_parameter_bounds(&second_boundary, tolerance)?;
    let intersections = first_surface.intersect_surface_in_bounds(second_surface,
        first_bounds, second_bounds, tolerance)?;
    match intersections {
        IntersectionSet::Empty => Ok(false),
        IntersectionSet::Coincident => {
            // Spline intersections compare p-curves, so both trims must use
            // the same support coordinates even when their faces do not.
            let mut projected_boundary = second_boundary.clone();
            for curve in &mut projected_boundary {
                if first_surface != second_surface {
                    curve.pcurve = super::SurfaceProjectionPcurve2::new(
                        curve.curve.clone(), first_surface.clone(), curve.pcurve.clone(),
                        curve.range, tolerance,
                    )?.into();
                }
                curve.pcurve = align_pcurve_to_face_chart(&curve.pcurve, Some(curve.range),
                    first_surface, &first_boundary, tolerance)?;
            }
            // Crossings between coplanar trims can enclose a narrow overlap
            // without containing any endpoint or midpoint of either boundary.
            for first in &first_boundary {
                for second in &projected_boundary {
                    if !curve_intersections(first, second, tolerance)?.is_empty() {
                        return Ok(true);
                    }
                }
            }
            for (boundary, target, target_boundary, bounds) in [
                (&first_boundary, second_surface, &second_boundary, second_bounds),
                (&second_boundary, first_surface, &first_boundary, first_bounds),
            ] {
                for edge in boundary {
                    for fraction in [0.0, 0.5, 1.0] {
                        let point = edge.curve.evaluate(edge.range.start + edge.range.span() * fraction)?.point;
                        let projection = target.project(point, tolerance)?;
                        let uv = nearest_uv(
                            target.domains(),
                            Point2::new(projection.parameters.0, projection.parameters.1),
                            box_center(bounds),
                        );
                        if point_in_trim(uv, target_boundary)? { return Ok(true); }
                    }
                }
            }
            Ok(false)
        }
        IntersectionSet::Isolated(intersections) => {
            for intersection in intersections {
                let first_pcurve = align_pcurve_to_face_chart(&intersection.first_pcurve,
                    None, first_surface, &first_boundary, tolerance)?;
                let second_pcurve = align_pcurve_to_face_chart(&intersection.second_pcurve,
                    None, second_surface, &second_boundary, tolerance)?;
                let first_copies = intersection_chart_copies(intersection.curve.domain().bounds, &first_pcurve,
                    first_surface, first_bounds, tolerance)?;
                let second_copies = intersection_chart_copies(intersection.curve.domain().bounds, &second_pcurve,
                    second_surface, second_bounds, tolerance)?;
                for first in &first_copies {
                    for second in &second_copies {
                        if !trim_to_faces(&intersection.curve, first, &first_boundary,
                            second, &second_boundary, tolerance)?.is_empty() {
                            return Ok(true);
                        }
                    }
                }
            }
            Ok(false)
        }
    }
}

pub(super) fn trim_to_faces<P, Q>(
    curve: &Curve3,
    first: &Curve2,
    first_boundary: &[FaceCurve<P>],
    second: &Curve2,
    second_boundary: &[FaceCurve<Q>],
    tolerance: ModelTolerance,
) -> KernelResult<Vec<ParameterRange>> {
    trim_to_faces_sampled(curve, first, first_boundary, second, second_boundary, None, tolerance)
}

pub(super) fn trim_to_faces_sampled<P, Q>(
    curve: &Curve3,
    first: &Curve2,
    first_boundary: &[FaceCurve<P>],
    second: &Curve2,
    second_boundary: &[FaceCurve<Q>],
    samples: Option<[&TrimSamples; 2]>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<ParameterRange>> {
    let ranges = partition_to_faces_sampled(curve, first, first_boundary, second, second_boundary, samples, tolerance)?;
    let mut merged = Vec::<ParameterRange>::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut() {
            if (previous.end - range.start).abs() <= tolerance.parameter * 64.0 {
                previous.end = range.end;
                continue;
            }
        }
        merged.push(range);
    }
    Ok(merged)
}

/// Retain boundary crossings as separate intervals, including where an
/// intersection changes from an existing boundary into a face interior.
pub(super) fn partition_to_faces<P, Q>(
    curve: &Curve3,
    first: &Curve2,
    first_boundary: &[FaceCurve<P>],
    second: &Curve2,
    second_boundary: &[FaceCurve<Q>],
    tolerance: ModelTolerance,
) -> KernelResult<Vec<ParameterRange>> {
    partition_to_faces_sampled(curve, first, first_boundary, second, second_boundary, None, tolerance)
}

fn partition_to_faces_sampled<P, Q>(
    curve: &Curve3,
    first: &Curve2,
    first_boundary: &[FaceCurve<P>],
    second: &Curve2,
    second_boundary: &[FaceCurve<Q>],
    samples: Option<[&TrimSamples; 2]>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<ParameterRange>> {
    fn finite_domain<P>(curve: &Curve2, boundary: &[FaceCurve<P>], tolerance: ModelTolerance) -> KernelResult<ParameterRange> {
        if let super::DomainBounds::Bounded(range) = curve.domain().bounds {
            return Ok(range);
        }
        if let Curve2::SurfaceProjection(projection) = curve {
            return Ok(projection.range());
        }
        let Curve2::Line(line) = curve else {
            return Err(geometry_error("an unbounded surface-intersection p-curve must be linear"));
        };
        let denominator = line.direction().magnitude2();
        let mut parameters = Vec::new();
        for segment in boundary {
            parameters.extend(
                sample_pcurve(segment, 8)?
                    .into_iter()
                    .map(|(_, point)| (point - line.origin()).dot(line.direction()) / denominator),
            );
        }
        let minimum = parameters.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = parameters.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let padding = (maximum - minimum).abs() * 0.05 + tolerance.parameter * 64.0;
        ParameterRange::new(minimum - padding, maximum + padding)
    }
    let first_natural = match first.domain().bounds {
        super::DomainBounds::Bounded(range) => Some(range),
        _ => None,
    };
    let second_natural = match second.domain().bounds {
        super::DomainBounds::Bounded(range) => Some(range),
        _ => None,
    };
    let curve_natural = match (curve.domain().bounds, curve.domain().period) {
        (super::DomainBounds::Bounded(range), None) => Some(range),
        _ => None,
    };
    // The two p-curves share the spatial curve parameter.  An unbounded
    // p-curve (most commonly `u = t` on a periodic cylinder) must not shorten
    // the bounded domain supplied by its mate merely because the face's UV
    // chart has a different seam.
    let domain = match (first_natural, second_natural) {
        (Some(first), Some(second)) => {
            let start = first.start.min(first.end).max(second.start.min(second.end));
            let end = first.start.max(first.end).min(second.start.max(second.end));
            if end - start <= tolerance.parameter * 16.0 {
                return Ok(Vec::new());
            }
            ParameterRange::new(start, end)?
        }
        (Some(range), None) | (None, Some(range)) => range,
        (None, None) if curve_natural.is_some() => curve_natural.unwrap(),
        (None, None) => {
            let first_domain = finite_domain(first, first_boundary, tolerance)?;
            let second_domain = finite_domain(second, second_boundary, tolerance)?;
            let start = first_domain
                .start
                .min(first_domain.end)
                .max(second_domain.start.min(second_domain.end));
            let end = first_domain
                .start
                .max(first_domain.end)
                .min(second_domain.start.max(second_domain.end));
            if end - start <= tolerance.parameter * 16.0 {
                return Ok(Vec::new());
            }
            ParameterRange::new(start, end)?
        }
    };
    // An analytic p-curve can be unbounded even when its spatial curve wraps
    // a projection certified only over a retained source interval.
    let domain = if let Some(range) = curve_natural {
        let start = domain.start.min(domain.end).max(range.start.min(range.end));
        let end = domain.start.max(domain.end).min(range.start.max(range.end));
        if end - start <= tolerance.parameter * 16.0 { return Ok(Vec::new()); }
        ParameterRange::new(start, end)?
    } else { domain };
    let (first_boundary_samples, second_boundary_samples) = match samples {
        Some([first, second]) => (Cow::Borrowed(first), Cow::Borrowed(second)),
        None => (Cow::Owned(trim_samples(first_boundary)?), Cow::Owned(trim_samples(second_boundary)?)),
    };
    let inside = |parameter: f64| -> KernelResult<bool> {
        Ok(
            point_in_sampled_trim(first.evaluate(parameter)?.point, first_boundary, &first_boundary_samples)?
                && point_in_sampled_trim(second.evaluate(parameter)?.point, second_boundary, &second_boundary_samples)?,
        )
    };
    let first_curve: FaceCurve = FaceCurve {
        provenance: Default::default(),
        curve: curve.clone(),
        pcurve: first.clone(),
        range: domain,
        loop_end: false,
        loop_kind: LoopKind::Outer,
        parameter_seam: false,
    };
    let second_curve: FaceCurve = FaceCurve {
        provenance: Default::default(),
        curve: curve.clone(),
        pcurve: second.clone(),
        range: domain,
        loop_end: false,
        loop_kind: LoopKind::Outer,
        parameter_seam: false,
    };
    let first_samples = sample_pcurve(&first_curve, 32)?;
    let second_samples = sample_pcurve(&second_curve, 32)?;
    let mut parameters = vec![domain.start, domain.end];
    for (boundary, samples) in first_boundary.iter().zip(first_boundary_samples.iter()) {
        parameters.extend(
            curve_intersections_sampled(&first_curve, boundary, Some([&first_samples, samples]), tolerance)?
                .into_iter()
                .map(|hit| hit.0),
        );
    }
    for (boundary, samples) in second_boundary.iter().zip(second_boundary_samples.iter()) {
        parameters.extend(
            curve_intersections_sampled(&second_curve, boundary, Some([&second_samples, samples]), tolerance)?
                .into_iter()
                .map(|hit| hit.0),
        );
    }
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|first, second| (*first - *second).abs() <= tolerance.parameter * 64.0);
    let mut ranges = Vec::<ParameterRange>::new();
    for pair in parameters.windows(2) {
        let middle = (pair[0] + pair[1]) * 0.5;
        if pair[1] - pair[0] <= tolerance.parameter * 16.0 {
            continue;
        }
        let shifts: Vec<f64> = match curve.domain().period {
            Some(period) => vec![0.0, -period, period, -2.0 * period, 2.0 * period],
            None => vec![0.0],
        };
        // Select the equivalent parameter branch that lies directly in both
        // faces' UV charts.  This keeps later arrangement endpoints on the
        // same side of a periodic seam instead of relying on a wrapped
        // point-in-polygon test that cannot preserve edge connectivity.
        for shift in shifts {
            // Spatial periodicity does not make a bounded UV chart periodic.
            // Other surface sheets are supplied by intersection_chart_copies;
            // only shift parameters when both p-curves support that branch.
            if !first.domain().contains(middle + shift, tolerance.parameter)
                || !second.domain().contains(middle + shift, tolerance.parameter)
            {
                continue;
            }
            if inside(middle + shift)? {
                ranges.push(ParameterRange::new(pair[0] + shift, pair[1] + shift)?);
                break;
            }
        }
    }
    ranges.sort_by(|first, second| first.start.total_cmp(&second.start));
    Ok(ranges)
}

pub(super) fn curve_intersections<P, Q>(
    first: &FaceCurve<P>,
    second: &FaceCurve<Q>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<(f64, f64)>> {
    curve_intersections_sampled(first, second, None, tolerance)
}

/// `samples` are both curves' `trim_samples`, reused when one curve meets many.
pub(super) fn curve_intersections_sampled<P, Q>(
    first: &FaceCurve<P>,
    second: &FaceCurve<Q>,
    samples: Option<[&[(f64, Point2)]; 2]>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<(f64, f64)>> {
    let endpoint_hits = || -> KernelResult<Vec<(f64, f64)>> {
        let fit = |curve: &Curve3, range: ParameterRange, point: Point3| -> Option<f64> {
            let projection = curve.project_on_trim(point, range, tolerance).ok()?;
            let parameter = curve.domain().nearest(projection.parameter, range.middle());
            let allowed = tolerance.linear_at_scale(point.to_vec().magnitude()) * 16.0;
            (projection.distance <= allowed && range.contains(parameter, tolerance))
                .then_some(parameter.clamp(range.start.min(range.end), range.start.max(range.end)))
        };
        let mut hits = Vec::<(f64, f64)>::new();
        for first_parameter in [first.range.start, first.range.end] {
            let point = first.curve.evaluate(first_parameter)?.point;
            if let Some(second_parameter) = fit(&second.curve, second.range, point) {
                hits.push((first_parameter, second_parameter));
            }
        }
        for second_parameter in [second.range.start, second.range.end] {
            let point = second.curve.evaluate(second_parameter)?.point;
            if let Some(first_parameter) = fit(&first.curve, first.range, point) {
                hits.push((first_parameter, second_parameter));
            }
        }
        Ok(hits)
    };
    let deduplicate = |mut hits: Vec<(f64, f64)>| {
        hits.sort_by(|first, second| {
            first
                .0
                .total_cmp(&second.0)
                .then_with(|| first.1.total_cmp(&second.1))
        });
        hits.dedup_by(|first, second| {
            (first.0 - second.0).abs() <= tolerance.parameter * 64.0
                && (first.1 - second.1).abs() <= tolerance.parameter * 64.0
        });
        hits
    };
    if let (Curve3::SurfaceIntersection(a), Curve3::SurfaceIntersection(b)) =
        (&first.curve, &second.curve)
    {
        if (coincident_supports(a.first_surface(), b.first_surface(), tolerance)?
            && coincident_supports(a.second_surface(), b.second_surface(), tolerance)?)
            || (coincident_supports(a.first_surface(), b.second_surface(), tolerance)?
                && coincident_supports(a.second_surface(), b.first_surface(), tolerance)?)
        {
            // Two guides of the same regular support intersection can overlap.
            // Intersecting their approximation splines invents crossings along
            // that overlap; only trim endpoints subdivide the common curve.
            return Ok(deduplicate(endpoint_hits()?));
        }
    }
    if matches!(first.curve, Curve3::Line(_) | Curve3::Circle(_))
        && matches!(second.curve, Curve3::Line(_) | Curve3::Circle(_))
    {
        match first.curve.intersect_curve(&second.curve, tolerance)? {
            IntersectionSet::Isolated(hits) => {
                let mut hits = hits
                    .into_iter()
                    .filter_map(|hit| {
                        let fit = |parameter: f64, curve: &Curve3, range: ParameterRange| {
                            let parameter = curve.domain().nearest(parameter, range.middle());
                            range.contains(parameter, tolerance).then_some(parameter)
                        };
                        fit(hit.first_parameter, &first.curve, first.range).zip(fit(
                            hit.second_parameter,
                            &second.curve,
                            second.range,
                        ))
                    })
                    .collect::<Vec<_>>();
                hits.extend(endpoint_hits()?);
                return Ok(deduplicate(hits));
            }
            IntersectionSet::Empty | IntersectionSet::Coincident => {
                // Coincident trims can cover different portions of the same
                // basis curve. Their overlap endpoints are arrangement
                // vertices just as much as transverse intersections are. If
                // they are not transferred to both parameter ranges, a long
                // edge and a shorter duplicate survive with different node
                // pairs and leave the selected boundary graph non-manifold.
                return Ok(deduplicate(endpoint_hits()?));
            }
        }
    }
    let (first_samples, second_samples) = match samples {
        Some([first, second]) => (Cow::Borrowed(first), Cow::Borrowed(second)),
        None => (Cow::Owned(sample_pcurve(first, 32)?), Cow::Owned(sample_pcurve(second, 32)?)),
    };
    let mut hits = Vec::<(f64, f64)>::new();
    for first_pair in first_samples.windows(2) {
        for second_pair in second_samples.windows(2) {
            let Some((first_fraction, second_fraction)) = segment_intersection_parameters(
                first_pair[0].1,
                first_pair[1].1,
                second_pair[0].1,
                second_pair[1].1,
            ) else {
                continue;
            };
            let mut parameters = [
                first_pair[0].0
                    + (first_pair[1].0 - first_pair[0].0) * first_fraction.clamp(0.0, 1.0),
                second_pair[0].0
                    + (second_pair[1].0 - second_pair[0].0) * second_fraction.clamp(0.0, 1.0),
            ];
            for _ in 0..24 {
                let first_value = first.pcurve.first_order(parameters[0])?;
                let second_value = second.pcurve.first_order(parameters[1])?;
                let residual = first_value.0 - second_value.0;
                if residual.magnitude() <= tolerance.parameter * 16.0 {
                    break;
                }
                let determinant =
                    first_value.1.x * -second_value.1.y - first_value.1.y * -second_value.1.x;
                if determinant.abs() <= f64::EPSILON * 256.0 {
                    break;
                }
                let first_delta =
                    (-residual.x * -second_value.1.y - second_value.1.x * residual.y) / determinant;
                let second_delta =
                    (first_value.1.x * -residual.y + residual.x * first_value.1.y) / determinant;
                parameters[0] = (parameters[0] + first_delta).clamp(
                    first.range.start.min(first.range.end),
                    first.range.start.max(first.range.end),
                );
                parameters[1] = (parameters[1] + second_delta).clamp(
                    second.range.start.min(second.range.end),
                    second.range.start.max(second.range.end),
                );
            }
            let first_point = first.pcurve.evaluate(parameters[0])?.point;
            let second_point = second.pcurve.evaluate(parameters[1])?.point;
            let uv_error = first_point.distance(second_point);
            let within_tolerance = if uv_error <= tolerance.parameter * 64.0 { true } else {
                // Implicit supports converge in model units. Their rounded
                // inverse charts need not attain the absolute UV tolerance.
                let (a, da) = first.curve.first_order(parameters[0])?;
                let (b, db) = second.curve.first_order(parameters[1])?;
                let linear = tolerance.linear_at_scale(a.to_vec().magnitude().max(b.to_vec().magnitude()));
                let first_rate = first.pcurve.first_order(parameters[0])?.1.magnitude() / da.magnitude();
                let second_rate = second.pcurve.first_order(parameters[1])?.1.magnitude() / db.magnitude();
                a.distance(b) <= linear && uv_error <= linear * first_rate.max(second_rate)
            };
            if within_tolerance
                && !hits.iter().any(|(a, b)| {
                    (parameters[0] - *a).abs() <= tolerance.parameter * 64.0
                        && (parameters[1] - *b).abs() <= tolerance.parameter * 64.0
                })
            {
                hits.push((parameters[0], parameters[1]));
            }
        }
    }
    hits.extend(endpoint_hits()?);
    Ok(deduplicate(hits))
}

pub(super) fn segment_intersection_parameters(
    a: Point2,
    b: Point2,
    c: Point2,
    d: Point2,
) -> Option<(f64, f64)> {
    let first = b - a;
    let second = d - c;
    let denominator = first.x * second.y - first.y * second.x;
    if denominator.abs() <= f64::EPSILON * 256.0 {
        return None;
    }
    let delta = c - a;
    let first_parameter = (delta.x * second.y - delta.y * second.x) / denominator;
    let second_parameter = (delta.x * first.y - delta.y * first.x) / denominator;
    ((-1.0e-9..=1.0 + 1.0e-9).contains(&first_parameter)
        && (-1.0e-9..=1.0 + 1.0e-9).contains(&second_parameter))
    .then_some((first_parameter, second_parameter))
}

pub(super) fn coincident_supports(
    first: &Surface3,
    second: &Surface3,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    if first == second {
        return Ok(true);
    }
    match (first, second) {
        (Surface3::Nurbs(first), Surface3::Nurbs(second)) => first.coincident_support(second, tolerance),
        (Surface3::Torus(_), Surface3::Torus(_)) => Ok(matches!(
            first.intersect_surface(second, tolerance),
            Ok(IntersectionSet::Coincident)
        )),
        (Surface3::Plane(first), Surface3::Plane(second)) => {
            Ok(
                first.normal().dot(second.normal()).abs() >= 1.0 - tolerance.angular * 8.0
                    && first.project(second.origin())?.distance
                        <= tolerance.linear_at_scale(second.origin().to_vec().magnitude()) * 16.0,
            )
        }
        (Surface3::Cylinder(first), Surface3::Cylinder(second)) => {
            let axes_parallel = first
                .axis()
                .normalize()
                .dot(second.axis().normalize())
                .abs()
                >= 1.0 - tolerance.angular * 8.0;
            let radial_offset = (second.origin() - first.origin())
                .cross(first.axis().normalize())
                .magnitude();
            Ok(axes_parallel
                && radial_offset <= tolerance.linear * 16.0
                && (first.radius() - second.radius()).abs() <= tolerance.linear * 16.0)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Compound, NurbsCurve2, Plane3, Profile, TrimmedCurve2, Wire2};
    use crate::Vec3;

    #[test]
    fn open_trim_curves_keep_crossings_outside_their_bounds() {
        let tolerance = ModelTolerance::default();
        let curve: Curve2 = super::super::Line2::new(Point2::new(0.0, -1.0), Vec2::unit_y()).unwrap().into();
        let boundary = vec![FaceCurve {
            provenance: (),
            curve: Plane3::xy().lift_curve(&curve, tolerance).unwrap(),
            pcurve: curve,
            range: ParameterRange::new(0.0, 2.0).unwrap(),
            loop_end: false,
            loop_kind: LoopKind::Outer,
            parameter_seam: false,
        }];
        let samples = trim_samples(&boundary).unwrap();
        assert!(point_in_sampled_trim(Point2::new(-2.0, 0.0), &boundary, &samples).unwrap());
        assert!(!point_in_sampled_trim(Point2::new(2.0, 0.0), &boundary, &samples).unwrap());
    }

    #[test]
    fn closed_trim_chart_gaps_preserve_ray_crossing_parity() {
        for gap in [1e-11, 1e-15] {
            let points = [Point2::new(1.0, gap), Point2::new(0.0, 1.0),
                Point2::new(-1.0, 0.0), Point2::new(0.0, -1.0), Point2::new(1.0, -gap)];
            let boundary = points.windows(2).enumerate().map(|(index, pair)| {
                let delta = pair[1] - pair[0];
                FaceCurve { provenance: (),
                    curve: super::super::Line3::new(Plane3::xy().point_at(pair[0]), Vec3::new(delta.x, delta.y, 0.0)).unwrap().into(),
                    pcurve: super::super::Line2::new(pair[0], delta).unwrap().into(),
                    range: ParameterRange::new(0.0, 1.0).unwrap(), loop_end: index == 3,
                    loop_kind: LoopKind::Outer, parameter_seam: false }
            }).collect::<Vec<_>>();
            let samples = trim_samples(&boundary).unwrap();
            for y in [-1e-10, 0.0, 1e-10] {
                for (x, inside) in [(-2.0, false), (0.0, true), (2.0, false)] {
                    assert_eq!(point_in_sampled_trim(Point2::new(x, y), &boundary, &samples).unwrap(), inside,
                        "gap {gap}, query ({x}, {y})");
                }
            }
        }
    }

    fn spline_disk(center: Point3, chart_center: Point2, angle: f64) -> Compound {
        let tolerance = ModelTolerance::default();
        let controls = [
            (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0),
            (-1.0, 0.0), (-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (1.0, 0.0),
        ];
        let segments = (0..4).map(|index| {
            let points = controls[index * 2..index * 2 + 3].iter()
                .map(|(x, y)| chart_center + Vec2::new(*x, *y)).collect();
            TrimmedCurve2::new(
                NurbsCurve2::new(2, points, vec![1.0, 0.5_f64.sqrt(), 1.0],
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap(),
                ParameterRange::new(0.0, 1.0).unwrap(),
            ).unwrap()
        });
        let (sin, cos) = angle.sin_cos();
        let u = Vec3::new(cos, sin, 0.0);
        let v = Vec3::new(-sin, cos, 0.0);
        let plane = Plane3::new(center - u * chart_center.x - v * chart_center.y,
            u, v, tolerance).unwrap();
        Profile::new(Wire2::new(segments, tolerance).unwrap(), [], tolerance).unwrap()
            .extrude(plane, Vec3::unit_z()).unwrap()
    }

    #[test]
    fn coincident_spline_trims_use_a_common_chart() {
        let first = spline_disk(Point3::origin(), Point2::origin(), 0.0);
        let angle = std::f64::consts::FRAC_PI_8;
        for (center, chart_center, rotation, expected) in [
            (Point3::new(1.5, 1.5, 0.0), Point2::new(0.5, 0.0), 0.0, false),
            (Point3::new(1.5, 1.5, 0.0), Point2::new(0.5, 0.0), 0.3, false),
            // This narrow overlap misses both disks' trim endpoints and
            // midpoints, so it must be detected by boundary intersections.
            (Point3::new(1.99 * angle.cos(), 1.99 * angle.sin(), 0.0),
                Point2::new(5.0, -3.0), 0.0, true),
            (Point3::new(1.99 * angle.cos(), 1.99 * angle.sin(), 0.0),
                Point2::new(5.0, -3.0), std::f64::consts::FRAC_PI_2, true),
        ] {
            let second = spline_disk(center, chart_center, rotation);
            let pair = first.combined(&[second]).unwrap();
            let caps = pair.faces().filter(|face| matches!(face.surface(),
                Surface3::Plane(plane) if plane.origin().z.abs() < 1.0e-8))
                .map(|face| face.topology_id()).collect::<Vec<_>>();
            assert_eq!(caps.len(), 2);
            for (a, b) in [(caps[0], caps[1]), (caps[1], caps[0])] {
                assert_eq!(trimmed_faces_intersect(pair.topology(), a, b, pair.tolerance()).unwrap(),
                    expected, "center {center:?}, chart center {chart_center:?}, rotation {rotation}");
            }
        }
    }
}
