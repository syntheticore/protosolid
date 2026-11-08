//! Revolutions and lofts built from exact profile sections. Display triangles
//! never enter BREP.
use super::geometry::de_casteljau_split;
use super::history::EntitySource;
use super::sew::{cap, publish_shells_with_roles, publish_with_roles, BrepPatch, BrepPatchLoop, BrepPatchSegment, SegmentProvenance};
use super::*;
use crate::{Point2, Point3, Vec2, Vec3};
use cgmath::prelude::*;
use cgmath::Vector4;
use std::collections::BTreeMap;

fn range(a: f64, b: f64) -> KernelResult<ParameterRange> {
    ParameterRange::new(a, b)
}
pub(super) fn radial_profile(
    profile: &Profile,
    plane: Plane3,
    axis: Axis3,
) -> KernelResult<(Profile, Frame3)> {
    let tolerance = profile.tolerance();
    if axis.direction().dot(plane.normal()).abs() > tolerance.angular
        || plane.project(axis.origin())?.distance > tolerance.linear
    {
        return Err(invalid("revolution axis must lie in the sketch plane"));
    }
    let mut radial = Vec3::new(0.0, 0.0, 0.0);
    for segment in profile.outer().segments() {
        for fraction in [0.0, 0.25, 0.5, 0.75] {
            let p = plane.point_at(
                segment
                    .curve()
                    .evaluate(segment.range().start + segment.range().span() * fraction)?
                    .point,
            ) - axis.origin();
            let r = p - axis.direction() * p.dot(axis.direction());
            if r.magnitude2() > radial.magnitude2() {
                radial = r;
            }
        }
    }
    if radial.magnitude() <= tolerance.linear {
        return Err(invalid("revolution profile lies on the axis"));
    }
    radial = radial.normalize();
    let frame = Frame3::new(
        axis.origin(),
        radial,
        axis.direction().cross(radial),
        tolerance,
    )?;
    let map = |point: Point2| {
        let p = plane.point_at(point) - axis.origin();
        Point2::new(p.dot(radial), p.dot(axis.direction()))
    };
    let mut wires = Vec::new();
    for wire in std::iter::once(profile.outer()).chain(profile.holes()) {
        let mut segments = Vec::new();
        for segment in wire.segments() {
            let mapped = match segment.curve() {
                Curve2::Line(_) => TrimmedCurve2::line(map(segment.start()?), map(segment.end()?))?,
                Curve2::Circle(circle) => {
                    let center = map(circle.center());
                    let first = map(segment.start()?);
                    let start = (first.y - center.y).atan2(first.x - center.x);
                    let sign = plane.u().dot(radial) * plane.v().dot(axis.direction())
                        - plane.v().dot(radial) * plane.u().dot(axis.direction());
                    TrimmedCurve2::new(
                        Circle2::new(center, circle.radius())?,
                        range(start, start + segment.range().span() * sign.signum())?,
                    )?
                }
                Curve2::Nurbs(curve) => TrimmedCurve2::new(
                    NurbsCurve2::new(
                        curve.degree(),
                        curve.control_points().iter().map(|p| map(*p)).collect(),
                        curve.weights().to_vec(),
                        curve.knots().to_vec(),
                    )?,
                    segment.range(),
                )?,
                _ => Plane3::new(frame.origin(),frame.x(),frame.z(),tolerance)?
                    .express_curve(&plane.lift_trimmed_curve(segment,tolerance)?,segment.range(),tolerance)?,
            };
            // Exact conservative radial bound for every supported curve family.
            let minimum = match mapped.curve() {
                Curve2::Line(_) => mapped.start()?.x.min(mapped.end()?.x),
                Curve2::Circle(c) => {
                    let mut m = mapped.start()?.x.min(mapped.end()?.x);
                    for k in -4..=4 {
                        if mapped.range().contains(
                            std::f64::consts::PI + k as f64 * std::f64::consts::TAU,
                            tolerance,
                        ) {
                            m = m.min(c.center().x - c.radius());
                        }
                    }
                    m
                }
                Curve2::Nurbs(c) => c
                    .control_points()
                    .iter()
                    .map(|p| p.x)
                    .fold(f64::INFINITY, f64::min),
                _ => super::bounds::curve2(mapped.curve(),mapped.range())?.0.x,
            };
            if minimum < -tolerance.linear {
                return Err(invalid("revolution profile crosses the axis"));
            }
            segments.push(mapped);
        }
        wires.push(Wire2::new(segments, tolerance)?);
    }
    let outer = wires.remove(0);
    Ok((Profile::new(outer, wires, tolerance)?.with_generators_from(profile), frame))
}

pub(super) fn revolve(
    profile: &Profile,
    plane: Plane3,
    axis: Axis3,
    angle: f64,
) -> KernelResult<Compound> {
    let (profile, frame) = radial_profile(profile, plane, axis)?;
    let tolerance = profile.tolerance();
    let full = (angle.abs() - std::f64::consts::TAU).abs() <= tolerance.parameter;
    let angle = if full { std::f64::consts::TAU.copysign(angle) } else { angle };
    let low = angle.min(0.0);
    let high = angle.max(0.0);
    let section_plane = |a: f64| {
        Plane3::new(
            frame.origin(),
            frame.x() * a.cos() + frame.y() * a.sin(),
            frame.z(),
            tolerance,
        )
    };
    let mut patches = Vec::new();
    let mut roles = Vec::new();
    let mut boundary_roles = BTreeMap::new();
    let boundary_namespace = super::identity::fresh_uuid();
    if !full {
        // Radial/axial profile normal points opposite the positive sweep.
        patches.push(cap(&profile, section_plane(low)?, false)?);
        patches.push(cap(&profile, section_plane(high)?, true)?);
        roles.push(if angle > 0.0 { CreationRole::ProfileStart } else { CreationRole::ProfileEnd });
        roles.push(if angle > 0.0 { CreationRole::ProfileEnd } else { CreationRole::ProfileStart });
    }
    for (ring, wire) in std::iter::once(profile.outer()).chain(profile.holes()).enumerate() {
        for (segment_index, segment) in wire.segments().iter().enumerate() {
            let first = segment.start()?;
            let last = segment.end()?;
            if matches!(segment.curve(), Curve2::Line(_))
                && first.x.abs() <= tolerance.linear
                && last.x.abs() <= tolerance.linear
            {
                continue;
            }
            let generator = [0, ring, profile.generator(ring, segment_index)];
            let role = |chart| CreationRole::ProfileSide { generators: vec![generator], chart };
            if full && matches!(segment.curve(), Curve2::Line(line)
                if line.direction().y.abs() <= tolerance.linear)
            {
                patches.push(revolve_radial_line(frame, first, last, low, high, tolerance)?);
                roles.push(role(Some(SurfaceChart::FullTurn)));
                continue;
            }
            let t0 = segment.range().start.min(segment.range().end);
            let t1 = segment.range().start.max(segment.range().end);
            let surface: Surface3 =
                RevolvedSurface3::new(frame, segment.curve().clone(), range(t0, t1)?, tolerance)?
                    .into();
            // With two poles there is no material rim to bound the chart.
            // Section and Boolean arrangements use two angular charts in that case.
            let two_poles = full && first.x <= tolerance.linear && last.x <= tolerance.linear;
            let middle = (low + high) * 0.5;
            let halves = [(low, middle), (middle, high)];
            let whole = [(low, high)];
            let spans = if two_poles { halves.as_slice() } else { whole.as_slice() };
            for &(a, b) in spans {
                let mut edges = Vec::new();
                for (t, next_t, start, end) in [(t0, t1, a, b), (t1, t0, b, a)] {
                    let rz = segment.curve().evaluate(t)?.point;
                    if rz.x > tolerance.linear {
                        edges.push(BrepPatchSegment {
                            provenance: Default::default(),
                            curve: Circle3::new(
                                frame.origin() + frame.z() * rz.y,
                                frame.x() * rz.x,
                                frame.y() * rz.x,
                                tolerance,
                            )?.into(),
                            pcurve: Line2::new(Point2::new(0.0, t), Vec2::unit_x())?.into(),
                            range: range(start, end)?,
                        });
                    }
                    // Both uses of a full turn's seam share one spatial curve.
                    // Their p-curves retain the two sides of the periodic chart.
                    let meridian = if full && !two_poles { 0.0 } else { end };
                    let mut provenance = SegmentProvenance::default();
                    if two_poles {
                        let mut landmark = |endpoint| {
                            let name = PersistentId { namespace: boundary_namespace, local: boundary_roles.len() as u64 };
                            boundary_roles.insert(name, CreationRole::RevolutionMeridian {
                                generator, half_turn: end == middle, endpoint,
                            });
                            EntitySource { name, unchanged: false }
                        };
                        provenance.edges.push(landmark(None));
                        provenance.vertices[0].push(landmark(Some(usize::from(t != segment.range().start))));
                        provenance.vertices[1].push(landmark(Some(usize::from(next_t != segment.range().start))));
                    }
                    edges.push(BrepPatchSegment {
                        provenance,
                        curve: section_plane(meridian)?.lift_trimmed_curve(segment, tolerance)?,
                        pcurve: Line2::new(Point2::new(end, 0.0), Vec2::unit_y())?.into(),
                        range: range(t, next_t)?,
                    });
                }
                let mut patch = BrepPatch {
                    preserved: Vec::new(),
                    sources: Vec::new(),
                    surface: surface.clone(),
                    orientation: FaceOrientation::Forward,
                    loops: vec![BrepPatchLoop {
                        kind: LoopKind::Outer,
                        segments: edges,
                    }],
                };
                if segment.range().span() < 0.0 {
                    patch.reverse();
                }
                promote_revolution(&mut patch, frame, segment, tolerance)?;
                patches.push(patch);
                roles.push(role(if two_poles {
                    Some(if (a == low) == (angle > 0.0) { SurfaceChart::TurnStart } else { SurfaceChart::TurnEnd })
                } else {
                    full.then_some(SurfaceChart::FullTurn)
                }));
            }
        }
    }
    // A complete turn seals each hole into a cavity.
    let compound = if full && !profile.holes().is_empty() {
        publish_shells_with_roles(patches, roles, tolerance)?
    } else {
        publish_with_roles(patches, roles, tolerance)?
    };
    Ok(compound.with_patch_roles(&boundary_roles))
}

/// A radial line sweeps a disk or annulus bounded directly by closed circles.
fn revolve_radial_line(
    frame: Frame3,
    first: Point2,
    last: Point2,
    low: f64,
    high: f64,
    tolerance: ModelTolerance,
) -> KernelResult<BrepPatch> {
    let (outer, inner, orientation, outer_range) = if first.x > last.x {
        (first, last, FaceOrientation::Forward, range(low, high)?)
    } else {
        (last, first, FaceOrientation::Reversed, range(high, low)?)
    };
    let center = frame.origin() + frame.z() * outer.y;
    let plane = Plane3::new(center, frame.x(), frame.y(), tolerance)?;
    let mut loops = Vec::new();
    for (radius, kind, range) in [
        (outer.x, LoopKind::Outer, outer_range),
        (inner.x, LoopKind::Inner, outer_range.reversed()),
    ] {
        if radius > tolerance.linear {
            loops.push(BrepPatchLoop {
                kind,
                segments: vec![BrepPatchSegment {
                    provenance: Default::default(),
                    curve: Circle3::new(center, frame.x() * radius, frame.y() * radius, tolerance)?
                        .into(),
                    pcurve: Circle2::new(Point2::origin(), radius)?.into(),
                    range,
                }],
            });
        }
    }
    Ok(BrepPatch {
        preserved: Vec::new(),
        sources: Vec::new(),
        surface: plane.into(),
        orientation,
        loops,
    })
}

fn promote_revolution(
    patch: &mut BrepPatch,
    frame: Frame3,
    segment: &TrimmedCurve2,
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let (surface, axial_map): (Surface3, Option<(f64, f64)>) = match segment.curve() {
        Curve2::Line(line) => {
            let origin = line.origin();
            let direction = line.direction();
            if direction.y.abs() <= tolerance.linear {
                (
                    Plane3::new(
                        frame.origin() + frame.z() * origin.y,
                        frame.x(),
                        frame.y(),
                        tolerance,
                    )?
                    .into(),
                    None,
                )
            } else if direction.x.abs() <= tolerance.linear {
                (
                    Cylinder3::new(
                        frame.origin(),
                        frame.z(),
                        frame.x() * origin.x,
                        frame.y() * origin.x,
                        tolerance,
                    )?
                    .into(),
                    Some((origin.y, direction.y)),
                )
            } else {
                let apex_t = -origin.x / direction.x;
                let apex_z = origin.y + direction.y * apex_t;
                let reference = segment
                    .curve()
                    .evaluate((segment.range().start + segment.range().end) * 0.5)?
                    .point;
                let height = reference.y - apex_z;
                // The cone's radius grows linearly from its apex on the axis.
                // Mapping by radius stays exact for almost flat cones, whose
                // tiny height would amplify rounding in an axial mapping.
                (
                    Cone3::new(
                        frame.origin() + frame.z() * apex_z,
                        frame.z() * height,
                        frame.x() * reference.x,
                        frame.y() * reference.x,
                        tolerance,
                    )?
                    .into(),
                    Some((origin.x / reference.x, direction.x / reference.x)),
                )
            }
        }
        // A circular meridian centered on the axis already uses the sphere's
        // longitude/latitude chart, including at its poles.
        Curve2::Circle(circle) if circle.center().x.abs() <= tolerance.linear => (
            crate::Sphere3::new(
                frame.origin() + frame.z() * circle.center().y,
                frame.x() * circle.radius(),
                frame.y() * circle.radius(),
                frame.z() * circle.radius(),
                tolerance,
            )?
            .into(),
            Some((
                -(segment.range().middle() / std::f64::consts::TAU).round() * std::f64::consts::TAU,
                1.0,
            )),
        ),
        Curve2::Circle(circle) if circle.center().x > circle.radius() + tolerance.linear => (
            Torus3::new(
                frame.origin() + frame.z() * circle.center().y,
                frame.x() * circle.center().x,
                frame.y() * circle.center().x,
                circle.radius(),
                tolerance,
            )?
            .into(),
            Some((0.0, 1.0)),
        ),
        _ => return Ok(()),
    };
    let mid = (segment.range().start + segment.range().end) * 0.5;
    let old = patch.surface.evaluate(0.0, mid)?;
    let uv = surface.project(old.point, tolerance)?.parameters;
    if old.normal()?.dot(surface.normal(uv.0, uv.1)?) < 0.0 {
        patch.orientation = patch.orientation.reversed();
    }
    for edge in &mut patch.loops[0].segments {
        edge.pcurve = if let Some((offset, scale)) = axial_map {
            let Curve2::Line(line) = &edge.pcurve else {
                unreachable!()
            };
            Line2::new(
                Point2::new(line.origin().x, offset + scale * line.origin().y),
                Vec2::new(line.direction().x, scale * line.direction().y),
            )?
            .into()
        } else {
            let range = range(
                edge.range.start.min(edge.range.end),
                edge.range.start.max(edge.range.end),
            )?;
            let points = (0..=16)
                .map(|i| {
                    let point = edge
                        .curve
                        .evaluate(range.start + range.span() * i as f64 / 16.0)?
                        .point;
                    let uv = surface.project(point, tolerance)?.parameters;
                    Ok(Point2::new(uv.0, uv.1))
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let guide = super::intersection::smooth_nurbs_guide(&points, range)?.into();
            SurfaceProjectionPcurve2::new(
                edge.curve.clone(),
                surface.clone(),
                guide,
                range,
                tolerance,
            )?
            .into()
        };
    }
    patch.surface = surface;
    Ok(())
}

#[derive(Clone)]
struct SectionCurve {
    source: usize,
    degree: usize,
    points: Vec<Point2>,
    weights: Vec<f64>,
    knots: Vec<f64>,
}
impl SectionCurve {
    fn curve2(&self) -> KernelResult<Curve2> {
        let start = self.points[0];
        let direction = *self.points.last().unwrap() - start;
        if self.points.len() == self.degree + 1
            && self.weights.iter().all(|weight| *weight == self.weights[0])
            && self.points.iter().enumerate().all(|(i, point)| {
                point.distance(start + direction * (i as f64 / self.degree as f64))
                    <= f64::EPSILON * direction.magnitude().max(1.0) * 32.0
            })
        {
            return Line2::new(start, direction).map(Into::into);
        }
        Ok(NurbsCurve2::new(
            self.degree,
            self.points.clone(),
            self.weights.clone(),
            self.knots.clone(),
        )?
        .into())
    }
    fn curve3(&self, plane: Plane3) -> KernelResult<Curve3> {
        plane.lift_curve(&self.curve2()?, ModelTolerance::default())
    }
    fn reversed(&self) -> Self {
        Self {
            source: self.source,
            degree: self.degree,
            points: self.points.iter().rev().copied().collect(),
            weights: self.weights.iter().rev().copied().collect(),
            knots: self.knots.iter().rev().map(|t| 1.0 - t).collect(),
        }
    }
}
fn section_from_controls(controls: Vec<Vec3>, source: usize) -> SectionCurve {
    let degree = controls.len() - 1;
    SectionCurve {
        source,
        degree,
        points: controls
            .iter()
            .map(|p| Point2::new(p.x / p.z, p.y / p.z))
            .collect(),
        weights: controls.iter().map(|p| p.z).collect(),
        knots: std::iter::repeat_n(0.0, degree + 1)
            .chain(std::iter::repeat_n(1.0, degree + 1))
            .collect(),
    }
}
fn compatible_sections(sections: &mut [Vec<SectionCurve>]) -> KernelResult<()> {
    fn gcd(a: usize, b: usize) -> usize {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
    let mut count = 1;
    let degree = sections
        .iter()
        .flatten()
        .map(|c| c.degree)
        .max()
        .unwrap_or(1);
    for section in sections.iter() {
        count = count / gcd(count, section.len()) * section.len();
        if count > 256 {
            return Err(unsupported(
                "loft section correspondence exceeds the segment limit",
            ));
        }
    }
    for section in sections {
        let subdivisions = count / section.len();
        let mut result = Vec::new();
        for curve in section.iter() {
            let mut controls = homogeneous(curve);
            while controls.len() <= degree {
                let n = controls.len();
                let mut elevated = vec![controls[0]];
                for i in 1..n {
                    let alpha = i as f64 / n as f64;
                    elevated.push(controls[i - 1] * alpha + controls[i] * (1.0 - alpha));
                }
                elevated.push(*controls.last().unwrap());
                controls = elevated;
            }
            for remaining in (1..=subdivisions).rev() {
                if remaining == 1 {
                    result.push(section_from_controls(controls.clone(), curve.source));
                } else {
                    let (first, rest) = de_casteljau_split(&controls, 1.0 / remaining as f64);
                    result.push(section_from_controls(first, curve.source));
                    controls = rest;
                }
            }
        }
        *section = result;
    }
    Ok(())
}

fn section_curves(profile: &Profile) -> KernelResult<Vec<SectionCurve>> {
    if !profile.holes().is_empty() {
        return Err(invalid("loft profiles with holes are not supported"));
    }
    let mut result = Vec::new();
    for (source, segment) in profile.outer().segments().iter().enumerate() {
        let source = profile.generator(0, source);
        match segment.curve() {
            Curve2::Line(_) => {
                let a = segment.start()?;
                let b = segment.end()?;
                result.push(SectionCurve {
                    source,
                    degree: 2,
                    points: vec![a, a + (b - a) * 0.5, b],
                    weights: vec![1.0; 3],
                    knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                });
            }
            Curve2::Circle(circle) => {
                let count =
                    (segment.range().span().abs() / std::f64::consts::FRAC_PI_2).ceil() as usize;
                for index in 0..count {
                    let a = segment.range().start
                        + segment.range().span() * index as f64 / count as f64;
                    let b = segment.range().start
                        + segment.range().span() * (index + 1) as f64 / count as f64;
                    let mid = (a + b) * 0.5;
                    let weight = ((b - a) * 0.5).cos();
                    let point = |angle: f64, scale: f64| {
                        circle.center()
                            + Vec2::new(angle.cos(), angle.sin()) * (circle.radius() * scale)
                    };
                    result.push(SectionCurve {
                        source,
                        degree: 2,
                        points: vec![point(a, 1.0), point(mid, 1.0 / weight), point(b, 1.0)],
                        weights: vec![1.0, weight, 1.0],
                        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    });
                }
            }
            Curve2::Nurbs(curve) => {
                let mut spans = Vec::new();
                let minimum = segment.range().start.min(segment.range().end);
                let maximum = segment.range().start.max(segment.range().end);
                for (span, mut controls) in curve.bezier_spans()? {
                    let start = minimum.max(span.start);
                    let end = maximum.min(span.end);
                    if end - start <= profile.tolerance().parameter {
                        continue;
                    }
                    let end_fraction = (end - span.start) / span.span();
                    if end_fraction < 1.0 {
                        controls = de_casteljau_split(&controls, end_fraction).0;
                    }
                    let start_fraction = (start - span.start) / (end - span.start);
                    if start_fraction > 0.0 {
                        controls = de_casteljau_split(&controls, start_fraction).1;
                    }
                    spans.push(section_from_controls(controls, source));
                }
                if segment.range().span() < 0.0 {
                    spans = spans.iter().rev().map(SectionCurve::reversed).collect();
                }
                result.extend(spans);
            }
            _ => return Err(unsupported("unsupported loft section curve")),
        }
    }
    Ok(result)
}
fn section_cap(
    curves: &[SectionCurve],
    plane: Plane3,
    reversed: bool,
    tolerance: ModelTolerance,
) -> KernelResult<BrepPatch> {
    let segments = curves
        .iter()
        .map(|c| {
            Ok(BrepPatchSegment {
                provenance: Default::default(),
                curve: plane.lift_curve(&c.curve2()?, tolerance)?,
                pcurve: c.curve2()?,
                range: range(0.0, 1.0)?,
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let mut patch = BrepPatch {
        preserved: Vec::new(),
        sources: Vec::new(),
        surface: plane.into(),
        orientation: FaceOrientation::Forward,
        loops: vec![BrepPatchLoop {
            kind: LoopKind::Outer,
            segments,
        }],
    };
    if reversed {
        patch.reverse();
    }
    Ok(patch)
}
fn homogeneous(curve: &SectionCurve) -> Vec<Vec3> {
    curve
        .points
        .iter()
        .zip(&curve.weights)
        .map(|(p, w)| Vec3::new(p.x * w, p.y * w, *w))
        .collect()
}

/// A loft guide with its wire parameter at every section.
struct LoftGuide<'a> {
    wire: &'a Wire3,
    parameters: Vec<f64>,
}

/// Where `guide` crosses a section boundary: the guide's wire parameter and
/// the boundary piece with its local parameter.
fn guide_crossing(
    guide: &Wire3,
    plane: Plane3,
    pieces: &[SectionCurve],
    tolerance: ModelTolerance,
) -> KernelResult<(f64, usize, f64)> {
    let boundary = Wire2::new(
        pieces
            .iter()
            .map(|c| TrimmedCurve2::new(c.curve2()?, range(0.0, 1.0)?))
            .collect::<KernelResult<Vec<_>>>()?,
        tolerance,
    )?;
    let height = |t: f64| {
        guide
            .point(t)
            .map(|p| (p - plane.origin()).dot(plane.normal()))
    };
    let end = guide.parameter_range().end;
    let samples = 64 * guide.segments().len();
    let mut best: Option<(f64, f64, f64)> = None;
    let mut previous = (0.0, height(0.0)?);
    for i in 1..=samples {
        let t = end * i as f64 / samples as f64;
        let current = (t, height(t)?);
        if previous.1 * current.1 <= 0.0 {
            let (mut low, mut high) = (previous, current);
            for _ in 0..64 {
                let t = (low.0 + high.0) * 0.5;
                let middle = (t, height(t)?);
                if (middle.1 <= 0.0) == (low.1 <= 0.0) {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            let root = (low.0 + high.0) * 0.5;
            let (position, _, distance) =
                boundary.closest_point(plane.coordinates(guide.point(root)?)?)?;
            if best.is_none_or(|(_, _, d)| distance < d) {
                best = Some((root, position, distance));
            }
        }
        previous = current;
    }
    match best {
        Some((root, position, distance)) if distance <= tolerance.linear => {
            let (index, local) = boundary.locate(position);
            Ok((root, index, local))
        }
        _ => Err(invalid("a loft guide must cross every section on its boundary")),
    }
}

/// Split section pieces at `cuts` (piece index, local parameter) and return
/// the index of the piece starting at each cut.
fn split_section(
    pieces: &[SectionCurve],
    cuts: &[(usize, f64)],
    tolerance: ModelTolerance,
) -> (Vec<SectionCurve>, Vec<usize>) {
    let cuts = cuts
        .iter()
        .map(|&(index, t)| {
            if t >= 1.0 - tolerance.parameter {
                ((index + 1) % pieces.len(), 0.0)
            } else if t <= tolerance.parameter {
                (index, 0.0)
            } else {
                (index, t)
            }
        })
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut splits = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        let mut ts = cuts
            .iter()
            .filter(|(i, t)| *i == index && *t > 0.0)
            .map(|(_, t)| *t)
            .collect::<Vec<_>>();
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| *a - *b <= tolerance.parameter);
        let mut controls = homogeneous(piece);
        let mut previous = 0.0;
        splits.push((result.len(), ts.clone()));
        for t in ts {
            let (first, rest) = de_casteljau_split(&controls, (t - previous) / (1.0 - previous));
            result.push(section_from_controls(first, piece.source));
            controls = rest;
            previous = t;
        }
        result.push(section_from_controls(controls, piece.source));
    }
    let starts = cuts
        .iter()
        .map(|&(index, t)| {
            let (base, ts) = &splits[index];
            if t == 0.0 {
                *base
            } else {
                base + 1 + ts.iter().position(|s| (s - t).abs() <= tolerance.parameter).unwrap()
            }
        })
        .collect();
    (result, starts)
}

/// Make sections correspond through their guide crossings. Every section is
/// split where the guides cross it and starts at the first guide; the pieces
/// between consecutive guides are made compatible region by region. Returns
/// the guides in boundary order and the piece count of every region.
fn guided_sections<'a>(
    curves: &mut [Vec<SectionCurve>],
    sections: &[(Plane3, &Profile)],
    guides: &'a [Wire3],
    tolerance: ModelTolerance,
) -> KernelResult<(Vec<LoftGuide<'a>>, Vec<usize>)> {
    let mut reversed = false;
    let mut order = Vec::new();
    let mut parameters = vec![Vec::new(); guides.len()];
    let mut bounds = Vec::new();
    for (i, (plane, _)) in sections.iter().enumerate() {
        // Outer wires run counter-clockwise about their own plane normal.
        if i > 0 && plane.normal().dot(sections[i - 1].0.normal()) < 0.0 {
            reversed = !reversed;
        }
        if reversed {
            curves[i] = curves[i].iter().rev().map(SectionCurve::reversed).collect();
        }
        let crossings = guides
            .iter()
            .map(|guide| guide_crossing(guide, *plane, &curves[i], tolerance))
            .collect::<KernelResult<Vec<_>>>()?;
        let cuts = crossings.iter().map(|c| (c.1, c.2)).collect::<Vec<_>>();
        let (pieces, starts) = split_section(&curves[i], &cuts, tolerance);
        let mut along = (0..guides.len()).collect::<Vec<_>>();
        along.sort_by_key(|g| starts[*g]);
        if along.windows(2).any(|w| starts[w[0]] == starts[w[1]]) {
            return Err(invalid("loft guides must cross each section at distinct points"));
        }
        if i == 0 {
            order = along;
        } else {
            let first = along.iter().position(|g| *g == order[0]).unwrap();
            along.rotate_left(first);
            if along != order {
                return Err(invalid("loft guides must cross all sections in the same order"));
            }
        }
        let count = pieces.len();
        let first = starts[order[0]];
        curves[i] = pieces[first..].iter().chain(&pieces[..first]).cloned().collect();
        bounds.push(
            order
                .iter()
                .map(|g| (starts[*g] + count - first) % count)
                .chain([count])
                .collect::<Vec<_>>(),
        );
        for (guide, crossing) in crossings.iter().enumerate() {
            parameters[guide].push(crossing.0);
        }
    }
    if parameters.iter().any(|p| {
        let rising = p[1] > p[0];
        p.windows(2).any(|w| (w[1] > w[0]) != rising || w[1] == w[0])
    }) {
        return Err(invalid("a loft guide must pass the sections in order"));
    }
    let mut regions = Vec::new();
    let mut joined = vec![Vec::new(); curves.len()];
    for region in 0..order.len() {
        let mut pieces = curves
            .iter()
            .zip(&bounds)
            .map(|(section, bounds)| section[bounds[region]..bounds[region + 1]].to_vec())
            .collect::<Vec<_>>();
        compatible_sections(&mut pieces)?;
        regions.push(pieces[0].len());
        for (section, pieces) in joined.iter_mut().zip(pieces) {
            section.extend(pieces);
        }
    }
    for (section, joined) in curves.iter_mut().zip(joined) {
        *section = joined;
    }
    let guides = order
        .iter()
        .map(|g| LoftGuide {
            wire: &guides[*g],
            parameters: std::mem::take(&mut parameters[*g]),
        })
        .collect();
    Ok((guides, regions))
}

/// Monotone cubic Hermite (Fritsch–Butland) through strictly monotone data.
fn monotone_cubic(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len();
    let secant = |k: usize| (ys[k + 1] - ys[k]) / (xs[k + 1] - xs[k]);
    let slope = |k: usize| {
        if k == 0 {
            secant(0)
        } else if k == n - 1 {
            secant(n - 2)
        } else {
            let (a, b) = (secant(k - 1), secant(k));
            2.0 * a * b / (a + b)
        }
    };
    let k = xs[1..n - 1].iter().take_while(|xk| x > **xk).count();
    let h = xs[k + 1] - xs[k];
    let s = (x - xs[k]) / h;
    let (s2, s3) = (s * s, s * s * s);
    ys[k] * (2.0 * s3 - 3.0 * s2 + 1.0)
        + slope(k) * h * (s3 - 2.0 * s2 + s)
        + ys[k + 1] * (3.0 * s2 - 2.0 * s3)
        + slope(k + 1) * h * (s3 - s2)
}

fn evaluate_column(degree: usize, knots: &[f64], column: &[Vector4<f64>], v: f64) -> Vector4<f64> {
    let (first, values) = super::geometry::basis_values(degree, knots, column.len(), v);
    values
        .iter()
        .enumerate()
        .fold(Vector4::zero(), |sum, (i, b)| sum + column[first + i] * *b)
}

/// Bend the side patches onto the guides. Each control column moves by the
/// guide deviations of its region, blended linearly between the region's
/// bounding guides, and is refit on a denser common basis in v that still
/// passes through every section exactly.
fn fit_guides(
    columns: &mut [Vec<Vec<Vector4<f64>>>],
    basis: &mut (usize, Vec<f64>),
    guides: &[LoftGuide],
    regions: &[usize],
    tolerance: ModelTolerance,
) -> KernelResult<()> {
    let spans = guides[0].parameters.len() - 1;
    let section_parameters = (0..=spans).map(|i| i as f64 / spans as f64).collect::<Vec<_>>();
    let (degree, knots) = basis.clone();
    let cartesian = |h: Vector4<f64>| h.truncate() / h.w;
    let starts = regions
        .iter()
        .scan(0, |start, size| {
            let current = *start;
            *start += size;
            Some(current)
        })
        .collect::<Vec<_>>();
    let deviation = |guide: usize, v: f64| -> KernelResult<Vec3> {
        let target = guides[guide].wire.point(monotone_cubic(
            &section_parameters,
            &guides[guide].parameters,
            v,
        ))?;
        Ok(target.to_vec() - cartesian(evaluate_column(degree, &knots, &columns[starts[guide]][0], v)))
    };
    let mut per_span = 8;
    let (parameters, deviations) = loop {
        let parameters = (0..=spans * per_span)
            .map(|k| k as f64 / (spans * per_span) as f64)
            .collect::<Vec<_>>();
        let deviations = (0..guides.len())
            .map(|guide| {
                parameters
                    .iter()
                    .enumerate()
                    .map(|(k, v)| {
                        if k % per_span == 0 {
                            Ok(Vec3::zero())
                        } else {
                            deviation(guide, *v)
                        }
                    })
                    .collect::<KernelResult<Vec<_>>>()
            })
            .collect::<KernelResult<Vec<_>>>()?;
        if per_span >= 128 {
            break (parameters, deviations);
        }
        let interpolation = super::linalg::Interpolation::new(&parameters)?;
        let mut error: f64 = 0.0;
        for (guide, deviations) in deviations.iter().enumerate() {
            let column = &columns[starts[guide]][0];
            let samples = parameters
                .iter()
                .zip(deviations)
                .map(|(v, d)| {
                    let h = evaluate_column(degree, &knots, column, *v);
                    ((cartesian(h) + d) * h.w).extend(h.w)
                })
                .collect::<Vec<_>>();
            let fitted = interpolation.solve(&samples);
            for pair in parameters.windows(2) {
                let v = (pair[0] + pair[1]) * 0.5;
                let fit = cartesian(evaluate_column(
                    interpolation.degree,
                    &interpolation.knots,
                    &fitted,
                    v,
                ));
                let original = cartesian(evaluate_column(degree, &knots, column, v));
                error = error.max((fit - original - deviation(guide, v)?).magnitude());
            }
        }
        if error <= tolerance.linear {
            break (parameters, deviations);
        }
        per_span *= 2;
    };
    let interpolation = super::linalg::Interpolation::new(&parameters)?;
    let mut patch = 0;
    for (region, size) in regions.iter().enumerate() {
        let (left, right) = (&deviations[region], &deviations[(region + 1) % guides.len()]);
        for offset in 0..*size {
            let order = columns[patch].len() - 1;
            for (j, column) in columns[patch].iter_mut().enumerate() {
                let s = (offset as f64 + j as f64 / order as f64) / *size as f64;
                let samples = parameters
                    .iter()
                    .enumerate()
                    .map(|(k, v)| {
                        let h = evaluate_column(degree, &knots, column, *v);
                        let moved = cartesian(h) + left[k] * (1.0 - s) + right[k] * s;
                        (moved * h.w).extend(h.w)
                    })
                    .collect::<Vec<_>>();
                *column = interpolation.solve(&samples);
            }
            patch += 1;
        }
    }
    *basis = (interpolation.degree, interpolation.knots);
    Ok(())
}

/// Unguided correspondence: compatible pieces, aligned by vertex position
/// unless `align` is off.
fn unguided_sections(
    curves: &mut [Vec<SectionCurve>],
    sections: &[(Plane3, &Profile)],
    align: bool,
) -> KernelResult<()> {
    compatible_sections(curves)?;
    let count = curves[0].len();
    for i in 1..curves.len() {
        if align {
            let previous = &curves[i - 1];
            let center = |section: &[SectionCurve], plane: Plane3| {
                section
                    .iter()
                    .map(|c| plane.point_at(c.points[0]).to_vec())
                    .fold(Vec3::new(0.0, 0.0, 0.0), |a, b| a + b)
                    / count as f64
            };
            let a_center = center(previous, sections[i - 1].0);
            let b_center = center(&curves[i], sections[i].0);
            let mut best = None;
            for reverse_order in [false, true] {
                let candidate = if reverse_order {
                    curves[i]
                        .iter()
                        .rev()
                        .map(SectionCurve::reversed)
                        .collect::<Vec<_>>()
                } else {
                    curves[i].clone()
                };
                for shift in 0..count {
                    let score = (0..count)
                        .map(|j| {
                            let a = sections[i - 1].0.point_at(previous[j].points[0]).to_vec()
                                - a_center;
                            let b = sections[i]
                                .0
                                .point_at(candidate[(j + shift) % count].points[0])
                                .to_vec()
                                - b_center;
                            (a - b).magnitude2()
                        })
                        .sum::<f64>();
                    if best.as_ref().is_none_or(|(cost, _)| score < *cost) {
                        best = Some((
                            score,
                            (0..count)
                                .map(|j| candidate[(j + shift) % count].clone())
                                .collect::<Vec<_>>(),
                        ));
                    }
                }
            }
            curves[i] = best.unwrap().1;
        }
    }
    // Bezier spans are an interpolation detail, not face boundaries. A
    // single section curve keeps one chart through all of its knot spans.
    if sections
        .iter()
        .all(|(_, profile)| profile.outer().segments().len() == 1)
    {
        for section in curves.iter_mut() {
            let degree = section[0].degree;
            let count = section.len();
            let mut joined = SectionCurve {
                source: section[0].source,
                degree,
                points: section[0].points.clone(),
                weights: section[0].weights.clone(),
                knots: vec![0.0; degree + 1],
            };
            for (index, curve) in section.iter().enumerate().skip(1) {
                joined.points.extend_from_slice(&curve.points[1..]);
                joined.weights.extend_from_slice(&curve.weights[1..]);
                joined
                    .knots
                    .extend(std::iter::repeat_n(index as f64 / count as f64, degree));
            }
            joined.knots.extend(vec![1.0; degree + 1]);
            *section = vec![joined];
        }
    }
    Ok(())
}

pub(super) fn loft(
    sections: &[(Plane3, &Profile)],
    align: bool,
    guides: &[Wire3],
) -> KernelResult<Compound> {
    if sections.len() < 2 {
        return Err(invalid("a loft needs at least two profiles"));
    }
    let tolerance = sections[0].1.tolerance();
    let mut curves = sections
        .iter()
        .map(|(_, profile)| section_curves(profile))
        .collect::<KernelResult<Vec<_>>>()?;
    let guided = if guides.is_empty() {
        unguided_sections(&mut curves, sections, align)?;
        None
    } else {
        Some(guided_sections(&mut curves, sections, guides, tolerance)?)
    };
    // One common cubic basis through every section, so the surface passes
    // through each section exactly.
    let interpolation = super::linalg::Interpolation::new(
        &(0..sections.len())
            .map(|i| i as f64 / (sections.len() - 1) as f64)
            .collect::<Vec<_>>(),
    )?;
    let mut columns = Vec::new();
    for index in 0..curves[0].len() {
        let first = &curves[0][index];
        if curves.iter().any(|section| {
            section[index].degree != first.degree
                || section[index].knots != first.knots
                || section[index].points.len() != first.points.len()
        }) {
            return Err(unsupported(
                "loft section curves require compatible spline degrees and knots",
            ));
        }
        columns.push(
            (0..first.points.len())
                .map(|u| {
                    interpolation.solve(
                        &curves
                            .iter()
                            .zip(sections)
                            .map(|(section, (plane, _))| {
                                let c = &section[index];
                                (plane.point_at(c.points[u]).to_vec() * c.weights[u])
                                    .extend(c.weights[u])
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
        );
    }
    let mut basis = (interpolation.degree, interpolation.knots.clone());
    if let Some((guides, regions)) = &guided {
        fit_guides(&mut columns, &mut basis, guides, regions, tolerance)?;
    }
    let (v_degree, v_knots) = (basis.0, &basis.1);
    let forward =
        (sections[1].0.origin() - sections[0].0.origin()).dot(sections[0].0.normal()) > 0.0;
    let mut patches = vec![section_cap(&curves[0], sections[0].0, forward, tolerance)?];
    let mut roles = vec![CreationRole::ProfileStart, CreationRole::ProfileEnd];
    let last = sections.len() - 1;
    // Alignment can reverse a section's winding; cap orientation follows the
    // actual boundary, while the side patches determine the shell orientation.
    let mut end_cap = section_cap(&curves[last], sections[last].0, !forward, tolerance)?;
    let signed_area = Wire2::new(
        curves[last]
            .iter()
            .map(|c| TrimmedCurve2::new(c.curve2()?, range(0.0, 1.0)?))
            .collect::<KernelResult<Vec<_>>>()?,
        tolerance,
    )?
    .is_counter_clockwise()?;
    if !signed_area {
        end_cap.orientation = end_cap.orientation.reversed();
    }
    patches.push(end_cap);
    for (index, column) in columns.iter().enumerate() {
        let first = &curves[0][index];
        let mut points = vec![Vec::new(); column[0].len()];
        let mut weights = vec![Vec::new(); column[0].len()];
        for controls in column {
            for (row, control) in controls.iter().enumerate() {
                if control.w <= 0.0 {
                    return Err(invalid(
                        "loft interpolation produced nonpositive rational weights",
                    ));
                }
                points[row].push(Point3::from_vec(control.truncate() / control.w));
                weights[row].push(control.w);
            }
        }
        let surface: Surface3 = NurbsSurface3::new(
            (first.degree, v_degree),
            points.clone(),
            weights.clone(),
            (first.knots.clone(), v_knots.clone()),
        )?
        .into();
        let mut edges = Vec::new();
        for (i, v, start, end) in [(0, 0.0, 0.0, 1.0), (last, 1.0, 1.0, 0.0)] {
            edges.push((
                if i == 0 { 0 } else { 2 },
                BrepPatchSegment {
                    provenance: Default::default(),
                    curve: curves[i][index].curve3(sections[i].0)?,
                    pcurve: Line2::new(Point2::new(0.0, v), Vec2::unit_x())?.into(),
                    range: range(start, end)?,
                },
            ));
        }
        for (order, u, start, end) in [(1, first.points.len() - 1, 0.0, 1.0), (3, 0, 1.0, 0.0)] {
            let curve = if points.len() == 2 && weights[0][u] == weights[1][u] {
                Line3::new(points[0][u], points[1][u] - points[0][u])?.into()
            } else {
                NurbsCurve3::new(
                    v_degree,
                    points.iter().map(|r| r[u]).collect(),
                    weights.iter().map(|r| r[u]).collect(),
                    v_knots.clone(),
                )?.into()
            };
            edges.push((
                order,
                BrepPatchSegment {
                    provenance: Default::default(),
                    curve,
                    pcurve: Line2::new(
                        Point2::new(if u == 0 { 0.0 } else { 1.0 }, 0.0),
                        Vec2::unit_y(),
                    )?
                    .into(),
                    range: range(start, end)?,
                },
            ));
        }
        edges.sort_by_key(|(i, _)| *i);
        let mut patch = BrepPatch {
            preserved: Vec::new(),
            sources: Vec::new(),
            surface,
            orientation: FaceOrientation::Forward,
            loops: vec![BrepPatchLoop {
                kind: LoopKind::Outer,
                segments: edges.into_iter().map(|(_, s)| s).collect(),
            }],
        };
        if let Surface3::Nurbs(surface) = &patch.surface {
            if let Some(plane) = surface.plane(tolerance)? {
                patch.surface = plane.into();
                for segment in &mut patch.loops[0].segments {
                    segment.pcurve = super::features::exact_pcurve(
                        &segment.curve, &patch.surface, &segment.pcurve, tolerance,
                    )?;
                }
            }
        }
        if !forward {
            patch.reverse();
        }
        patches.push(patch);
        roles.push(CreationRole::ProfileSide { chart: None, generators: curves.iter().enumerate()
            .map(|(section, curves)| [section, 0, curves[index].source]).collect() });
    }
    publish_with_roles(patches, roles, tolerance)
}

#[cfg(test)]
#[path = "tests/revolve_loft.rs"]
mod tests;
