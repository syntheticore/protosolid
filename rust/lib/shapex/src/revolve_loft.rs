//! Revolutions and lofts built from exact profile sections. Display triangles
//! never enter BREP.
use super::geometry::de_casteljau_split;
use super::sew::{cap, publish_with_roles, BrepPatch, BrepPatchLoop, BrepPatchSegment};
use super::*;
use crate::{Point2, Point3, Vec2, Vec3};
use cgmath::prelude::*;

fn range(a: f64, b: f64) -> KernelResult<ParameterRange> {
    ParameterRange::new(a, b)
}
fn radial_profile(
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
            let t0 = segment.range().start.min(segment.range().end);
            let t1 = segment.range().start.max(segment.range().end);
            let surface: Surface3 =
                RevolvedSurface3::new(frame, segment.curve().clone(), range(t0, t1)?, tolerance)?
                    .into();
            // Split a complete turn to disambiguate closed-edge welding.
            // Partial turns retain their directed parameter interval.
            let spans = if full { 2 } else { 1 };
            let angles = (0..=spans)
                .map(|i| low + (high - low) * i as f64 / spans as f64)
                .collect::<Vec<_>>();
            for span in angles.windows(2) {
                let a = span[0];
                let b = span[1];
                let mut edges = Vec::new();
                for (t, start, end) in [(t0, a, b), (t1, b, a)] {
                    let rz = segment.curve().evaluate(t)?.point;
                    if rz.x <= tolerance.linear {
                        continue;
                    }
                    let curve = Circle3::new(
                        frame.origin() + frame.z() * rz.y,
                        frame.x() * rz.x,
                        frame.y() * rz.x,
                        tolerance,
                    )?
                    .into();
                    edges.push((
                        if t == t0 { 0 } else { 2 },
                        BrepPatchSegment {
                            provenance: Default::default(),
                            curve,
                            pcurve: Line2::new(Point2::new(0.0, t), Vec2::unit_x())?.into(),
                            range: range(start, end)?,
                        },
                    ));
                }
                for (index, a, start, end) in [(1, b, t0, t1), (3, a, t1, t0)] {
                    edges.push((
                        index,
                        BrepPatchSegment {
                            provenance: Default::default(),
                            curve: section_plane(a)?.lift_trimmed_curve(segment, tolerance)?,
                            pcurve: Line2::new(Point2::new(a, 0.0), Vec2::unit_y())?.into(),
                            range: range(start, end)?,
                        },
                    ));
                }
                edges.sort_by_key(|(index, _)| *index);
                let mut patch = BrepPatch {
                    preserved: Vec::new(),
                    sources: Vec::new(),
                    surface: surface.clone(),
                    orientation: FaceOrientation::Forward,
                    loops: vec![BrepPatchLoop {
                        kind: LoopKind::Outer,
                        segments: edges.into_iter().map(|(_, s)| s).collect(),
                    }],
                };
                if segment.range().span() < 0.0 {
                    patch.reverse();
                }
                promote_revolution(&mut patch, frame, segment, tolerance)?;
                patches.push(patch);
                roles.push(CreationRole::ProfileSide { generators: vec![[0, ring, profile.generator(ring, segment_index)]],
                    chart: if full { Some(if (a == low) == (angle > 0.0) { SurfaceChart::TurnStart } else { SurfaceChart::TurnEnd }) } else { None } });
            }
        }
    }
    publish_with_roles(patches, roles, tolerance)
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
                (
                    Cone3::new(
                        frame.origin() + frame.z() * apex_z,
                        frame.z() * height,
                        frame.x() * reference.x,
                        frame.y() * reference.x,
                        tolerance,
                    )?
                    .into(),
                    Some(((origin.y - apex_z) / height, direction.y / height)),
                )
            }
        }
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
            let mut controls = curve
                .points
                .iter()
                .zip(&curve.weights)
                .map(|(p, w)| Vec3::new(p.x * w, p.y * w, *w))
                .collect::<Vec<_>>();
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
pub(super) fn loft(sections: &[(Plane3, &Profile)], align: bool) -> KernelResult<Compound> {
    if sections.len() < 2 {
        return Err(invalid("a loft needs at least two profiles"));
    }
    let tolerance = sections[0].1.tolerance();
    let mut curves = sections
        .iter()
        .map(|(_, profile)| section_curves(profile))
        .collect::<KernelResult<Vec<_>>>()?;
    compatible_sections(&mut curves)?;
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
        for section in &mut curves {
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
    // One common cubic basis through every section, so the surface passes
    // through each section exactly.
    let interpolation = super::linalg::Interpolation::new(
        &(0..sections.len())
            .map(|i| i as f64 / (sections.len() - 1) as f64)
            .collect::<Vec<_>>(),
    )?;
    let (v_degree, v_knots) = (interpolation.degree, &interpolation.knots);
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
    .is_counter_clockwise();
    if !signed_area {
        end_cap.orientation = end_cap.orientation.reversed();
    }
    patches.push(end_cap);
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
        let mut points = vec![Vec::new(); sections.len()];
        let mut weights = vec![Vec::new(); sections.len()];
        for u in 0..first.points.len() {
            let column = curves
                .iter()
                .zip(sections)
                .map(|(section, (plane, _))| {
                    let c = &section[index];
                    (plane.point_at(c.points[u]).to_vec() * c.weights[u]).extend(c.weights[u])
                })
                .collect::<Vec<_>>();
            for (row, control) in interpolation.solve(&column).into_iter().enumerate() {
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
