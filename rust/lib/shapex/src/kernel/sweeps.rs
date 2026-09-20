//! Exact section-based feature construction. Display triangles never enter BREP.
use super::euler::{BrepPatch, BrepPatchLoop, BrepPatchSegment};
use super::*;
use crate::{Point2, Point3, Vec2, Vec3};
use cgmath::prelude::*;

fn invalid(message: &str) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}
fn unsupported(message: &str) -> KernelError {
    KernelError::new(DiagnosticKind::UnsupportedGeometry, message)
}
fn range(a: f64, b: f64) -> KernelResult<ParameterRange> {
    ParameterRange::new(a, b)
}
fn lift(plane: Plane3, point: Point2) -> Point3 {
    plane.origin() + plane.u() * point.x + plane.v() * point.y
}
pub(super) fn lift_curve(plane: Plane3, curve: &Curve2, tolerance: ModelTolerance) -> KernelResult<Curve3> {
    Ok(match curve {
        Curve2::Line(line) => Line3::new(
            lift(plane, line.origin()),
            plane.u() * line.direction().x + plane.v() * line.direction().y,
        )?
        .into(),
        Curve2::Circle(circle) => Circle3::new(
            lift(plane, circle.center()),
            plane.u() * circle.radius(),
            plane.v() * circle.radius(),
            tolerance,
        )?
        .into(),
        Curve2::Nurbs(curve) => NurbsCurve3::new(
            curve.degree(),
            curve
                .control_points()
                .iter()
                .map(|p| lift(plane, *p))
                .collect(),
            curve.weights().to_vec(),
            curve.knots().to_vec(),
        )?
        .into(),
        _ => {
            return Err(unsupported(
                "section curves must be lines, circles, or NURBS",
            ))
        }
    })
}
fn reverse(patch: &mut BrepPatch) {
    patch.orientation = if patch.orientation == FaceOrientation::Forward {
        FaceOrientation::Reversed
    } else {
        FaceOrientation::Forward
    };
    for loop_ in &mut patch.loops {
        loop_.segments.reverse();
        for edge in &mut loop_.segments {
            edge.range = edge.range.reversed();
        }
    }
}
pub(super) fn cap(profile: &Profile, plane: Plane3, reversed: bool) -> KernelResult<BrepPatch> {
    let mut loops = Vec::new();
    for (index, wire) in std::iter::once(profile.outer())
        .chain(profile.holes())
        .enumerate()
    {
        let segments = wire
            .segments()
            .iter()
            .map(|segment| {
                Ok(BrepPatchSegment {
                    curve: lift_curve(plane, segment.curve(), profile.tolerance())?,
                    pcurve: segment.curve().clone(),
                    range: segment.range(),
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        loops.push(BrepPatchLoop {
            kind: if index == 0 {
                LoopKind::Outer
            } else {
                LoopKind::Inner
            },
            segments,
        });
    }
    let mut patch = BrepPatch {
        surface: plane.into(),
        orientation: FaceOrientation::Forward,
        loops,
    };
    if reversed {
        reverse(&mut patch);
    }
    Ok(patch)
}
pub(super) fn publish(patches: &[BrepPatch], tolerance: ModelTolerance) -> KernelResult<Compound> {
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    transaction.brep_shell(patches)?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [])
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
            let p = lift(
                plane,
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
        let p = lift(plane, point) - axis.origin();
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
                _ => return Err(unsupported("unsupported revolution profile curve")),
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
                _ => unreachable!(),
            };
            if minimum < -tolerance.linear {
                return Err(invalid("revolution profile crosses the axis"));
            }
            segments.push(mapped);
        }
        wires.push(Wire2::new(segments, tolerance)?);
    }
    let outer = wires.remove(0);
    Ok((Profile::new(outer, wires, tolerance)?, frame))
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
    if !full {
        // Radial/axial profile normal points opposite the positive sweep.
        patches.push(cap(&profile, section_plane(low)?, false)?);
        patches.push(cap(&profile, section_plane(high)?, true)?);
    }
    for wire in std::iter::once(profile.outer()).chain(profile.holes()) {
        for segment in wire.segments() {
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
                            curve: lift_curve(section_plane(a)?, segment.curve(), tolerance)?,
                            pcurve: Line2::new(Point2::new(a, 0.0), Vec2::unit_y())?.into(),
                            range: range(start, end)?,
                        },
                    ));
                }
                edges.sort_by_key(|(index, _)| *index);
                let mut patch = BrepPatch {
                    surface: surface.clone(),
                    orientation: FaceOrientation::Forward,
                    loops: vec![BrepPatchLoop {
                        kind: LoopKind::Outer,
                        segments: edges.into_iter().map(|(_, s)| s).collect(),
                    }],
                };
                if segment.range().span() < 0.0 {
                    reverse(&mut patch);
                }
                promote_revolution(&mut patch, frame, segment, tolerance)?;
                patches.push(patch);
            }
        }
    }
    publish(&patches, tolerance)
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
        patch.orientation = if patch.orientation == FaceOrientation::Forward {
            FaceOrientation::Reversed
        } else {
            FaceOrientation::Forward
        };
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
    degree: usize,
    points: Vec<Point2>,
    weights: Vec<f64>,
    knots: Vec<f64>,
}
impl SectionCurve {
    fn curve2(&self) -> KernelResult<Curve2> {
        Ok(NurbsCurve2::new(
            self.degree,
            self.points.clone(),
            self.weights.clone(),
            self.knots.clone(),
        )?
        .into())
    }
    fn curve3(&self, plane: Plane3) -> KernelResult<Curve3> {
        lift_curve(plane, &self.curve2()?, ModelTolerance::default())
    }
    fn reversed(&self) -> Self {
        Self {
            degree: self.degree,
            points: self.points.iter().rev().copied().collect(),
            weights: self.weights.iter().rev().copied().collect(),
            knots: self.knots.iter().rev().map(|t| 1.0 - t).collect(),
        }
    }
}
fn split_controls(controls: &[Vec3], t: f64) -> (Vec<Vec3>, Vec<Vec3>) {
    let mut row = controls.to_vec();
    let mut left = vec![row[0]];
    let mut right = vec![*row.last().unwrap()];
    while row.len() > 1 {
        row = row
            .windows(2)
            .map(|p| p[0] * (1.0 - t) + p[1] * t)
            .collect();
        left.push(row[0]);
        right.push(*row.last().unwrap());
    }
    right.reverse();
    (left, right)
}
fn section_from_controls(controls: Vec<Vec3>) -> SectionCurve {
    let degree = controls.len() - 1;
    SectionCurve {
        degree,
        points: controls
            .iter()
            .map(|p| Point2::new(p.x / p.z, p.y / p.z))
            .collect(),
        weights: controls.iter().map(|p| p.z).collect(),
        knots: std::iter::repeat(0.0)
            .take(degree + 1)
            .chain(std::iter::repeat(1.0).take(degree + 1))
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
                    result.push(section_from_controls(controls.clone()));
                } else {
                    let (first, rest) = split_controls(&controls, 1.0 / remaining as f64);
                    result.push(section_from_controls(first));
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
    for segment in profile.outer().segments() {
        match segment.curve() {
            Curve2::Line(_) => {
                let a = segment.start()?;
                let b = segment.end()?;
                result.push(SectionCurve {
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
                        controls = split_controls(&controls, end_fraction).0;
                    }
                    let start_fraction = (start - span.start) / (end - span.start);
                    if start_fraction > 0.0 {
                        controls = split_controls(&controls, start_fraction).1;
                    }
                    spans.push(section_from_controls(controls));
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
                curve: lift_curve(plane, &c.curve2()?, tolerance)?,
                pcurve: c.curve2()?,
                range: range(0.0, 1.0)?,
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let mut patch = BrepPatch {
        surface: plane.into(),
        orientation: FaceOrientation::Forward,
        loops: vec![BrepPatchLoop {
            kind: LoopKind::Outer,
            segments,
        }],
    };
    if reversed {
        reverse(&mut patch);
    }
    Ok(patch)
}
fn basis(index: usize, degree: usize, t: f64, knots: &[f64], count: usize) -> f64 {
    if t == 1.0 {
        return if index == count - 1 { 1.0 } else { 0.0 };
    }
    if degree == 0 {
        return if knots[index] <= t && t < knots[index + 1] {
            1.0
        } else {
            0.0
        };
    }
    let mut value = 0.0;
    let first = knots[index + degree] - knots[index];
    let second = knots[index + degree + 1] - knots[index + 1];
    if first > 0.0 {
        value += (t - knots[index]) / first * basis(index, degree - 1, t, knots, count);
    }
    if second > 0.0 {
        value += (knots[index + degree + 1] - t) / second
            * basis(index + 1, degree - 1, t, knots, count);
    }
    value
}
/// Interpolate homogeneous section controls with one common cubic B-spline
/// basis; the resulting surface passes through every supplied section exactly.
fn interpolation(count: usize) -> KernelResult<(usize, Vec<f64>, Vec<Vec<f64>>)> {
    let degree = 3.min(count - 1);
    let parameters = (0..count)
        .map(|i| i as f64 / (count - 1) as f64)
        .collect::<Vec<_>>();
    let mut knots = vec![0.0; degree + 1];
    for j in 1..count - degree {
        knots.push(parameters[j..j + degree].iter().sum::<f64>() / degree as f64);
    }
    knots.extend(vec![1.0; degree + 1]);
    let mut matrix = parameters
        .iter()
        .map(|t| {
            (0..count)
                .map(|i| basis(i, degree, *t, &knots, count))
                .chain(std::iter::repeat(0.0).take(count))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for i in 0..count {
        matrix[i][count + i] = 1.0;
    }
    for i in 0..count {
        let pivot = (i..count)
            .max_by(|a, b| matrix[*a][i].abs().total_cmp(&matrix[*b][i].abs()))
            .unwrap();
        matrix.swap(i, pivot);
        let divisor = matrix[i][i];
        if divisor.abs() < 1.0e-12 {
            return Err(invalid("loft section interpolation is singular"));
        }
        for j in 0..2 * count {
            matrix[i][j] /= divisor;
        }
        for k in 0..count {
            if k != i {
                let factor = matrix[k][i];
                for j in 0..2 * count {
                    matrix[k][j] -= factor * matrix[i][j];
                }
            }
        }
    }
    Ok((
        degree,
        knots,
        matrix
            .into_iter()
            .map(|row| row[count..].to_vec())
            .collect(),
    ))
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
                    .map(|c| lift(plane, c.points[0]).to_vec())
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
                            let a =
                                lift(sections[i - 1].0, previous[j].points[0]).to_vec() - a_center;
                            let b = lift(sections[i].0, candidate[(j + shift) % count].points[0])
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
    let (v_degree, v_knots, inverse) = interpolation(sections.len())?;
    let forward =
        (sections[1].0.origin() - sections[0].0.origin()).dot(sections[0].0.normal()) > 0.0;
    let mut patches = vec![section_cap(&curves[0], sections[0].0, forward, tolerance)?];
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
        end_cap.orientation = if end_cap.orientation == FaceOrientation::Forward {
            FaceOrientation::Reversed
        } else {
            FaceOrientation::Forward
        };
    }
    patches.push(end_cap);
    for index in 0..count {
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
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for row in &inverse {
            let mut point_row = Vec::new();
            let mut weight_row = Vec::new();
            for u in 0..first.points.len() {
                let mut point = Vec3::new(0.0, 0.0, 0.0);
                let mut weight = 0.0;
                for (i, factor) in row.iter().enumerate() {
                    let c = &curves[i][index];
                    let w = c.weights[u] * factor;
                    point += lift(sections[i].0, c.points[u]).to_vec() * w;
                    weight += w;
                }
                if weight <= 0.0 {
                    return Err(invalid(
                        "loft interpolation produced nonpositive rational weights",
                    ));
                }
                point_row.push(Point3::from_vec(point / weight));
                weight_row.push(weight);
            }
            points.push(point_row);
            weights.push(weight_row);
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
                    curve: curves[i][index].curve3(sections[i].0)?,
                    pcurve: Line2::new(Point2::new(0.0, v), Vec2::unit_x())?.into(),
                    range: range(start, end)?,
                },
            ));
        }
        for (order, u, start, end) in [(1, first.points.len() - 1, 0.0, 1.0), (3, 0, 1.0, 0.0)] {
            let curve = NurbsCurve3::new(
                v_degree,
                points.iter().map(|r| r[u]).collect(),
                weights.iter().map(|r| r[u]).collect(),
                v_knots.clone(),
            )?
            .into();
            edges.push((
                order,
                BrepPatchSegment {
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
            surface,
            orientation: FaceOrientation::Forward,
            loops: vec![BrepPatchLoop {
                kind: LoopKind::Outer,
                segments: edges.into_iter().map(|(_, s)| s).collect(),
            }],
        };
        if !forward {
            reverse(&mut patch);
        }
        patches.push(patch);
    }
    publish(&patches, tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle(x: f64, y: f64, w: f64, h: f64) -> Profile {
        let t = ModelTolerance::default();
        Profile::new(
            Wire2::polygon(
                [
                    Point2::new(x, y),
                    Point2::new(x + w, y),
                    Point2::new(x + w, y + h),
                    Point2::new(x, y + h),
                ],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap()
    }
    fn circle(r: f64) -> Profile {
        let t = ModelTolerance::default();
        Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(0.0, 0.0), r).unwrap()],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap()
    }
    fn upper(z: f64) -> Plane3 {
        Plane3::new(
            Point3::new(0.0, 0.0, z),
            Vec3::unit_x(),
            Vec3::unit_y(),
            ModelTolerance::default(),
        )
        .unwrap()
    }
    fn mesh(body: &Compound) {
        assert!(!body
            .tessellate(TessellationOptions::new(0.05, 0.15, 24).unwrap())
            .unwrap()
            .is_empty());
    }
    #[test]
    fn feature_revolution_crosses_half_turn() {
        let profile = rectangle(2.0, 0.0, 2.0, 3.0);
        let axis = Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).unwrap();
        for degrees in [
            179.0_f64,
            179.999999,
            180.0,
            180.000001,
            181.0,
            270.0,
            359.0,
            -179.0,
            -179.999999,
            -180.0,
            -180.000001,
            -181.0,
            -270.0,
            -359.0,
        ] {
            let angle = degrees.to_radians();
            let body = profile.revolve(Plane3::xy(), axis, angle).unwrap();
            assert!(
                (body.volume() - 18.0 * angle.abs()).abs() < 1e-5,
                "{degrees}: volume {}",
                body.volume()
            );
            let display = body
                .tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap())
                .unwrap();
            let volume = display
                .triangles
                .iter()
                .map(|t| {
                    display.positions[t[0]].to_vec().dot(
                        display.positions[t[1]]
                            .to_vec()
                            .cross(display.positions[t[2]].to_vec()),
                    ) / 6.0
                })
                .sum::<f64>();
            assert!(
                (volume - body.volume()).abs() < body.volume() * 0.01,
                "{degrees}: display volume {volume}, body volume {}",
                body.volume()
            );
            assert!(body.centroid().z * angle < 0.0);
            let round = Profile::new(
                Wire2::new(
                    [TrimmedCurve2::circle(Point2::new(5.0, 0.0), 1.0).unwrap()],
                    ModelTolerance::default(),
                )
                .unwrap(),
                [],
                ModelTolerance::default(),
            )
            .unwrap();
            let torus = round.revolve(Plane3::xy(), axis, angle).unwrap();
            assert!(
                torus.centroid().z * angle < 0.0,
                "{degrees}: torus centroid {:?}",
                torus.centroid()
            );
            let display = torus
                .tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap())
                .unwrap();
            let z_moment = display
                .triangles
                .iter()
                .map(|t| {
                    let [a, b, c] = t.map(|i| display.positions[i]);
                    a.to_vec().dot(b.to_vec().cross(c.to_vec())) / 6.0 * (a.z + b.z + c.z) / 4.0
                })
                .sum::<f64>();
            assert!(
                z_moment * angle < 0.0,
                "{degrees}: torus display z moment {z_moment}"
            );
        }
    }
    #[test]
    fn feature_sewing_reconciles_boundary_subdivisions() {
        let bodies = [
            rectangle(0.0, 0.0, 10.0, 8.0)
                .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))
                .unwrap(),
            circle(3.0)
                .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))
                .unwrap(),
            circle(3.0)
                .loft(Plane3::xy(), [(upper(5.0), &circle(2.0))])
                .unwrap(),
        ];
        for body in bodies {
            let topology = body.topology();
            let mut patches = body
                .faces()
                .map(|handle| {
                    let face = topology.face(handle.topology_id()).unwrap();
                    BrepPatch {
                        surface: face.surface().clone(),
                        orientation: face.orientation(),
                        loops: face
                            .loops()
                            .iter()
                            .map(|id| BrepPatchLoop {
                                kind: topology.loop_(*id).unwrap().kind(),
                                segments: topology
                                    .loop_coedges(*id)
                                    .unwrap()
                                    .iter()
                                    .map(|id| {
                                        let c = topology.coedge(*id).unwrap();
                                        BrepPatchSegment {
                                            curve: topology.edge(c.edge()).unwrap().curve().clone(),
                                            pcurve: c.pcurve().clone(),
                                            range: c.range(),
                                        }
                                    })
                                    .collect(),
                            })
                            .collect(),
                    }
                })
                .collect::<Vec<_>>();
            let patch = patches
                .iter_mut()
                .find(|p| matches!(p.surface, Surface3::Plane(_)))
                .unwrap();
            let segment = patch.loops[0].segments.remove(0);
            for (index, pair) in [0.0, 0.2, 0.65, 1.0].windows(2).enumerate() {
                patch.loops[0].segments.insert(
                    index,
                    BrepPatchSegment {
                        range: range(
                            segment.range.start + segment.range.span() * pair[0],
                            segment.range.start + segment.range.span() * pair[1],
                        )
                        .unwrap(),
                        ..segment.clone()
                    },
                );
            }
            let result = publish(&patches, body.tolerance()).unwrap();
            assert!((result.volume() - body.volume()).abs() < 1e-5);
            assert_eq!(result.edges().count(), body.edges().count() + 2);
            mesh(&result);
        }
    }
    #[test]
    fn feature_chamfer_after_two_fillets() {
        let body = rectangle(30.0, 17.0, 100.0, 60.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 21.0))
            .unwrap();
        let edge = body
            .edges()
            .find(|e| {
                e.endpoints().is_ok_and(|p| {
                    p.iter()
                        .all(|p| (p.x - 30.0).abs() < 1e-7 && (p.y - 17.0).abs() < 1e-7)
                })
            })
            .unwrap()
            .clone();
        let body = body.fillet(&[edge], 20.0).unwrap();
        let edge = body
            .edges()
            .find(|e| {
                matches!(e.curve(), Curve3::Circle(_))
                    && e.endpoints()
                        .is_ok_and(|p| p.iter().all(|p| p.z.abs() < 1e-7))
            })
            .unwrap()
            .clone();
        for radius in [2.0, 5.0] {
            let filleted = body.fillet(&[edge.clone()], radius).unwrap();
            for rotation in [
                crate::Matrix3::identity(),
                crate::Matrix3::from_angle_x(cgmath::Deg(90.0)),
            ] {
                let transform = RigidTransform3::new(
                    rotation,
                    Vec3::new(12.430018992481585, 0.0, -24.348125762908836),
                    body.tolerance(),
                )
                .unwrap();
                let body = filleted.transformed(transform).unwrap();
                let edges = body
                    .edges()
                    .filter(|e| matches!(e.curve(), Curve3::Line(_)))
                    .filter(|e| {
                        e.endpoints().is_ok_and(|p| {
                            let p = p.map(|p| transform.inverse().transform_point(p));
                            p.iter().all(|p| p.z.abs() < 1e-7)
                                && (p.iter().all(|p| (p.y - 77.0).abs() < 1e-7)
                                    || p.iter().all(|p| (p.x - 130.0).abs() < 1e-7))
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                assert_eq!(edges.len(), 2);
                for edge in edges {
                    for follow in [false, true] {
                        let result = body
                            .chamfer_with_tangent_propagation(&[edge.clone()], 1.0, follow)
                            .unwrap_or_else(|e| panic!("radius {radius}, follow {follow}: {e}"));
                        assert!(result.volume() < body.volume());
                        assert_eq!(result.solids().len(), 1);
                        mesh(&result);
                    }
                }
            }
        }
    }
    #[test]
    fn feature_chamfer_multiple_tangent_edges() {
        let body = rectangle(30.0, 17.0, 100.0, 60.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 21.0))
            .unwrap();
        let edge = body
            .edges()
            .find(|e| {
                e.endpoints().is_ok_and(|p| {
                    p.iter()
                        .all(|p| (p.x - 30.0).abs() < 1e-7 && (p.y - 17.0).abs() < 1e-7)
                })
            })
            .unwrap()
            .clone();
        let body = body.fillet(&[edge], 20.0).unwrap();
        let edges = body
            .edges()
            .filter(|e| {
                e.endpoints().is_ok_and(|p| {
                    p.iter().all(|p| (p.z - 21.0).abs() < 1e-7)
                        && (matches!(e.curve(), Curve3::Circle(_))
                            || p.iter().all(|p| (p.x - 30.0).abs() < 1e-7)
                            || p.iter().all(|p| (p.y - 17.0).abs() < 1e-7))
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(edges.len(), 3);
        for mode in [
            ChamferMode::TwoDistances(2.0),
            ChamferMode::DistanceAngle(0.6),
        ] {
            for flip in [false, true] {
                let references = edges.iter().map(|edge| edge.connected_faces().unwrap().into_iter()
                    .find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.normal().z.abs() > 0.9) != flip)
                    .unwrap()).collect::<Vec<_>>();
                let all = body
                    .chamfer_with_options(&edges, &references, 1.0, mode, true)
                    .unwrap();
                for (edge, face) in edges.iter().zip(&references) {
                    let following = body
                        .chamfer_with_options(&[edge.clone()], &[face.clone()], 1.0, mode, true)
                        .unwrap();
                    assert!((following.volume() - all.volume()).abs() < 1e-5);
                    mesh(&following);
                    let selected = body
                        .chamfer_with_options(&[edge.clone()], &[face.clone()], 1.0, mode, false)
                        .unwrap();
                    assert!(selected.volume() > following.volume());
                }
            }
        }
        for distance in [1.0, 11.0] {
            for selection in [
                vec![edges[0].clone(), edges[1].clone()],
                vec![edges[0].clone(), edges[2].clone()],
                vec![edges[1].clone(), edges[2].clone()],
                edges.clone(),
            ] {
                let result = body.chamfer(&selection, distance).unwrap_or_else(|e| {
                    panic!("{} edges, distance {distance}: {e}", selection.len())
                });
                assert!(result.volume() < body.volume());
                mesh(&result);
            }
        }
        let all = body.chamfer(&edges, 11.0).unwrap();
        for edge in &edges {
            let following = body
                .chamfer_with_tangent_propagation(&[edge.clone()], 11.0, true)
                .unwrap();
            assert!((following.volume() - all.volume()).abs() < 1e-5);
            let selected = body
                .chamfer_with_tangent_propagation(&[edge.clone()], 11.0, false)
                .unwrap();
            assert!(selected.volume() > following.volume());
            mesh(&selected);
        }
        let reversed = body
            .chamfer(&edges.into_iter().rev().collect::<Vec<_>>(), 11.0)
            .unwrap();
        assert!((reversed.volume() - all.volume()).abs() < 1e-5);
    }
    #[test]
    fn feature_partial_revolution() {
        let p = rectangle(2.0, 0.0, 2.0, 3.0);
        let axis = Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).unwrap();
        for angle in [std::f64::consts::PI, -std::f64::consts::PI] {
            let body = p.revolve(Plane3::xy(), axis, angle).unwrap();
            assert!((body.volume() - 18.0 * std::f64::consts::PI).abs() < 1.0e-5);
            mesh(&body);
        }
        let body = rectangle(0.0, 0.0, 2.0, 3.0)
            .revolve(Plane3::xy(), axis, std::f64::consts::PI)
            .unwrap();
        assert!((body.volume() - 6.0 * std::f64::consts::PI).abs() < 1.0e-5);
        mesh(&body);
    }
    #[test]
    fn feature_conical_revolution() {
        let t = ModelTolerance::default();
        let p = Profile::new(
            Wire2::polygon(
                [
                    Point2::new(2.0, 0.0),
                    Point2::new(4.0, 0.0),
                    Point2::new(2.0, 2.0),
                ],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap();
        let body = p
            .revolve(
                Plane3::xy(),
                Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).unwrap(),
                std::f64::consts::TAU,
            )
            .unwrap();
        assert!((body.volume() - 32.0 / 3.0 * std::f64::consts::PI).abs() < 1.0e-5);
        mesh(&body);
    }
    #[test]
    fn feature_loft_sections() {
        let a = rectangle(-2.0, -2.0, 4.0, 4.0);
        let b = rectangle(-1.0, -1.0, 2.0, 2.0);
        let body = a.loft(Plane3::xy(), [(upper(3.0), &b)]).unwrap();
        assert!((body.volume() - 28.0).abs() < 1.0e-5);
        mesh(&body);
        let a = circle(2.0);
        let b = circle(1.0);
        let body = a.loft(Plane3::xy(), [(upper(3.0), &b)]).unwrap();
        assert!((body.volume() - 7.0 * std::f64::consts::PI).abs() < 1.0e-5);
        mesh(&body);
        let body = a
            .loft(Plane3::xy(), [(upper(3.0), &b), (upper(6.0), &a)])
            .unwrap();
        assert!(body.volume() > 0.0);
        mesh(&body);
    }
    #[test]
    fn feature_loft_compatibility() {
        let t = ModelTolerance::default();
        let a = rectangle(-2.0, -2.0, 4.0, 4.0);
        let b = Profile::new(
            Wire2::polygon(
                [
                    Point2::new(-1.0, -1.0),
                    Point2::new(1.0, -1.0),
                    Point2::new(0.0, 1.0),
                ],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap();
        let body = a.loft(Plane3::xy(), [(upper(3.0), &b)]).unwrap();
        assert!(body.volume() > 0.0);
        mesh(&body);
        let round = circle(1.0);
        let body = a.loft(Plane3::xy(), [(upper(3.0), &round)]).unwrap();
        assert!(body.volume() > 0.0);
        mesh(&body);
        let inverted = Plane3::new(
            Point3::new(0.0, 0.0, 3.0),
            Vec3::unit_x(),
            -Vec3::unit_y(),
            t,
        )
        .unwrap();
        let body = a.loft(Plane3::xy(), [(inverted, &a)]).unwrap();
        assert!((body.volume() - 48.0).abs() < 1.0e-5);
        mesh(&body);
    }
    #[test]
    fn feature_split() {
        let body = rectangle(-2.0, -2.0, 4.0, 4.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 4.0))
            .unwrap();
        assert!((body.split(upper(1.0), "left").unwrap().volume() - 16.0).abs() < 1.0e-6);
        assert!((body.split(upper(1.0), "right").unwrap().volume() - 48.0).abs() < 1.0e-6);
        let both = body.split(upper(1.0), "both").unwrap();
        assert_eq!(both.solids().len(), 2);
        assert!((both.volume() - 64.0).abs() < 1.0e-6);
        let cylinder = circle(2.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 4.0))
            .unwrap();
        assert!(
            (cylinder.split(upper(1.0), "left").unwrap().volume() - 4.0 * std::f64::consts::PI)
                .abs()
                < 1.0e-6
        );
    }
    #[test]
    fn feature_chamfer() {
        let body = rectangle(0.0, 0.0, 10.0, 10.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        let edge = body.edges().next().unwrap().clone();
        let result = body.chamfer(&[edge], 1.0).unwrap();
        assert!((result.volume() - 995.0).abs() < 1.0e-5);
        mesh(&result);
        let cylinder = circle(5.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        let edge = cylinder
            .edges()
            .find(|e| matches!(e.curve(), Curve3::Circle(_)))
            .unwrap()
            .clone();
        let result = cylinder.chamfer(&[edge], 1.0).unwrap();
        assert!(result.volume() < cylinder.volume());
        mesh(&result);
    }
    fn filleted_asymmetric_chamfer() -> Compound {
        let body = rectangle(
            -37.04944282031129,
            -21.764692712619542,
            78.94647629000909,
            47.585532703500046,
        )
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 34.0))
        .unwrap();
        let edge = body
            .edges()
            .find(|e| {
                e.endpoints()
                    .unwrap()
                    .iter()
                    .all(|p| p.x > 41.0 && p.y < -21.0)
            })
            .unwrap()
            .clone();
        let reference = edge
            .connected_faces()
            .unwrap()
            .into_iter()
            .find(|f| {
                let p = f
                    .surface()
                    .project(Point3::new(0.0, -21.764692712619542, 0.0), body.tolerance())
                    .unwrap();
                p.distance < 1e-6
            })
            .unwrap();
        let body = body
            .chamfer_with_options(
                &[edge],
                &[reference],
                11.0,
                ChamferMode::DistanceAngle(55.0_f64.to_radians()),
                true,
            )
            .unwrap();
        let edges = body
            .edges()
            .filter(|e| {
                let p = e.endpoints().unwrap();
                let top = p.iter().all(|p| (p.z - 34.0).abs() < 1e-6);
                (top && p.iter().all(|p| p.x > 25.0 || p.y < -20.0))
                    || (p[0].z - p[1].z).abs() > 33.0
                        && (p[0].x > 25.0 && p[0].y < -5.0 || p[0].x < -37.0 && p[0].y > 25.0)
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(edges.len(), 6);
        body.fillet(&edges, 6.0).unwrap()
    }

    #[test]
    fn shell_filleted_asymmetric_chamfer() {
        let body = filleted_asymmetric_chamfer();
        for face in body
            .faces()
            .filter(|f| matches!(f.surface(), Surface3::Sphere(_)))
        {
            assert_eq!(
                body.topology()
                    .face(face.topology_id())
                    .unwrap()
                    .orientation(),
                super::super::FaceOrientation::Forward
            );
        }
        let opening = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.normal().z.abs() > 0.99 && p.origin().z.abs() < 1e-6)).unwrap().clone();
        for openings in [vec![], vec![opening]] {
            for distance in [-2.0, -1.0, -0.5, 0.5, 1.0, 2.0] {
                for join in ["sharp", "rounded"] {
                    let result = body
                        .shell_with_join(&openings, distance, join)
                        .unwrap_or_else(|error| {
                            panic!(
                                "openings {:?}, distance {distance}, join {join}: {error}",
                                openings.iter().map(|f| f.topology_id()).collect::<Vec<_>>()
                            )
                        });
                    mesh(&result);
                }
            }
        }
    }
    #[test]
    fn cut_shelled_asymmetric_chamfer() {
        let body = filleted_asymmetric_chamfer();
        let opening = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.normal().z.abs() > 0.99 && p.origin().z.abs() < 1e-6)).unwrap().clone();
        let body = body.shell(&[opening], 5.0).unwrap();
        let tol = body.tolerance();
        let profile = Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(
                    Point2::new(-21.08780917145651, 5.736491377054718),
                    32.892404807198155,
                )
                .unwrap()],
                tol,
            )
            .unwrap(),
            [],
            tol,
        )
        .unwrap();
        let mut previous_volume = body.volume();
        for distance in [5.0, 10.0, 10.99, 11.0, 11.01, 12.0, 15.0, 20.0, 30.0, 40.0] {
            let tool = profile
                .extrude(upper(39.0), Vec3::new(0.0, 0.0, -distance))
                .unwrap();
            let cut = body
                .difference(&tool)
                .unwrap_or_else(|e| panic!("distance {distance}: {e}"));
            assert_eq!(cut.solids().len(), if distance < 39.0 { 1 } else { 2 });
            assert!(cut.volume() > 0.0 && cut.volume() < body.volume());
            assert!(cut.volume() <= previous_volume + 1e-5);
            previous_volume = cut.volume();
            mesh(&cut);
        }
    }
    #[test]
    fn feature_asymmetric_chamfer() {
        let body = rectangle(0.0, 0.0, 20.0, 20.0)
            .extrude(Plane3::xy(), Vec3::new(3.0, 2.0, 10.0))
            .unwrap();
        for edge in body.edges() {
            for reference in edge.connected_faces().unwrap() {
                for mode in [
                    ChamferMode::TwoDistances(2.0),
                    ChamferMode::DistanceAngle(0.6),
                ] {
                    let result = body
                        .chamfer_with_options(
                            &[edge.clone()],
                            &[reference.clone()],
                            1.0,
                            mode,
                            false,
                        )
                        .unwrap();
                    let bevel = result
                        .faces()
                        .find(|face| {
                            body.faces()
                                .all(|old| old.persistent_id() != face.persistent_id())
                        })
                        .unwrap();
                    let mut setbacks = Vec::new();
                    for boundary in bevel.boundary_edges().unwrap() {
                        let points = boundary.endpoints().unwrap();
                        let midpoint =
                            Point3::from_vec((points[0].to_vec() + points[1].to_vec()) * 0.5);
                        if (points[1] - points[0])
                            .normalize()
                            .cross(edge.endpoints().unwrap()[1] - edge.endpoints().unwrap()[0])
                            .magnitude()
                            < 1e-6
                        {
                            let distance = edge
                                .curve()
                                .project(midpoint, body.tolerance())
                                .unwrap()
                                .distance;
                            if reference
                                .surface()
                                .project(midpoint, body.tolerance())
                                .unwrap()
                                .distance
                                < 1e-6
                            {
                                assert!((distance - 1.0).abs() < 1e-6);
                            }
                            setbacks.push(distance);
                        }
                    }
                    assert_eq!(setbacks.len(), 2);
                    if let ChamferMode::TwoDistances(_) = mode {
                        setbacks.sort_by(f64::total_cmp);
                        assert!(
                            (setbacks[0] - 1.0).abs() < 1e-6 && (setbacks[1] - 2.0).abs() < 1e-6
                        );
                    } else {
                        let n0 = reference.surface().normal(0.0, 0.0).unwrap();
                        let n1 = bevel.surface().normal(0.0, 0.0).unwrap();
                        assert!((n0.dot(n1).abs().clamp(-1.0, 1.0).acos() - 0.6).abs() < 1e-6);
                    }
                    mesh(&result);
                }
            }
        }
        let cylinder = circle(10.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
            .unwrap();
        let edge = cylinder
            .edges()
            .find(|edge| matches!(edge.curve(), Curve3::Circle(_)))
            .unwrap();
        for reference in edge.connected_faces().unwrap() {
            for mode in [
                ChamferMode::TwoDistances(2.0),
                ChamferMode::DistanceAngle(0.6),
            ] {
                let second = match mode {
                    ChamferMode::TwoDistances(d) => d,
                    _ => 0.6_f64.tan(),
                };
                let (radial, axial) = if matches!(reference.surface(), Surface3::Plane(_)) {
                    (1.0, second)
                } else {
                    (second, 1.0)
                };
                let result = cylinder
                    .chamfer_with_options(&[edge.clone()], &[reference.clone()], 1.0, mode, true)
                    .unwrap();
                let removed =
                    std::f64::consts::PI * axial * (10.0 * radial - radial * radial / 3.0);
                assert!((cylinder.volume() - result.volume() - removed).abs() < 1e-5);
                mesh(&result);
            }
        }
    }

    #[test]
    fn feature_concave_chamfer() {
        let tolerance = ModelTolerance::default();
        let outer = rectangle(0.0, 0.0, 10.0, 10.0);
        let hole = rectangle(3.0, 3.0, 4.0, 4.0);
        let body = Profile::new(outer.outer().clone(), [hole.outer().clone()], tolerance)
            .unwrap()
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        let edge = body
            .edges()
            .find(|edge| {
                edge.endpoints().is_ok_and(|points| {
                    points
                        .iter()
                        .all(|p| (p.x - 3.0).abs() < 1.0e-7 && (p.y - 3.0).abs() < 1.0e-7)
                })
            })
            .unwrap()
            .clone();
        for reference in edge.connected_faces().unwrap() {
            for mode in [ChamferMode::TwoDistances(2.0), ChamferMode::DistanceAngle(0.6)] {
                let second = match mode { ChamferMode::TwoDistances(d) => d, _ => 0.6_f64.tan() };
                let result = body.chamfer_with_options(&[edge.clone()], &[reference.clone()], 1.0, mode, false).unwrap();
                assert!((result.volume() - body.volume() - 5.0 * second).abs() < 1e-5);
                mesh(&result);
            }
        }
        let result = body.chamfer(&[edge], 1.0).unwrap();
        assert!((result.volume() - 845.0).abs() < 1.0e-5);
        mesh(&result);
    }
    #[test]
    fn feature_edge_cases() {
        let t = ModelTolerance::default();
        let axis = Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).unwrap();
        let cone = Profile::new(
            Wire2::polygon(
                [
                    Point2::new(0.0, 0.0),
                    Point2::new(3.0, 0.0),
                    Point2::new(0.0, 4.0),
                ],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap()
        .revolve(Plane3::xy(), axis, std::f64::consts::TAU)
        .unwrap();
        assert!((cone.volume() - 12.0 * std::f64::consts::PI).abs() < 1.0e-5);
        mesh(&cone);
        let p = Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(5.0, 0.0), 1.0).unwrap()],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap();
        let torus = p.revolve(Plane3::xy(), axis, std::f64::consts::PI).unwrap();
        assert!((torus.volume() - 5.0 * std::f64::consts::PI.powi(2)).abs() < 1.0e-5);
        mesh(&torus);
        let body = rectangle(0.0, 0.0, 10.0, 10.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        let result = body
            .chamfer(&body.edges().cloned().collect::<Vec<_>>(), 1.0)
            .unwrap();
        assert!(result.volume() < 1000.0);
        mesh(&result);
        let cylinder = circle(5.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        let rims = cylinder
            .edges()
            .filter(|e| matches!(e.curve(), Curve3::Circle(_)))
            .cloned()
            .collect::<Vec<_>>();
        let result = cylinder.chamfer(&rims, 1.0).unwrap();
        assert!(result.volume() < cylinder.volume());
        mesh(&result);
        let slanted =
            Plane3::from_origin_normal(Point3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 1.0, 1.0))
                .unwrap();
        let both = cylinder.split(slanted, "both").unwrap();
        assert_eq!(both.solids().len(), 2);
        assert!((both.volume() - cylinder.volume()).abs() < 1.0e-5);
        assert!((body.split(upper(20.0), "left").unwrap().volume() - body.volume()).abs() < 1.0e-7);
        assert_eq!(body.split(upper(20.0), "right").unwrap().solids().len(), 0);
        let face = cylinder
            .faces()
            .find(|f| matches!(f.surface(), Surface3::Cylinder(_)))
            .unwrap()
            .clone();
        let wider = cylinder
            .draft(&[face], Plane3::xy(), -0.1_f64.atan())
            .unwrap();
        assert!(wider.volume() > cylinder.volume());
        mesh(&wider);
    }
    #[test]
    fn feature_rounded_corner_draft() {
        let body = rounded_corner_block();
        let faces = body
            .faces()
            .filter(|f| match f.surface() {
                Surface3::Cylinder(_) => true,
                Surface3::Plane(p) => {
                    (p.normal().x.abs() > 0.9 && (p.origin().x - 30.0).abs() < 1e-7)
                        || (p.normal().y.abs() > 0.9 && (p.origin().y - 17.0).abs() < 1e-7)
                }
                _ => false,
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(faces.len(), 3);
        for angle in [5_f64.to_radians(), -5_f64.to_radians()] {
            let result = body.draft(&faces, upper(10.0), angle).unwrap();
            assert_eq!(result.faces().count(), body.faces().count());
            let corner_loss = 1.0 - std::f64::consts::FRAC_PI_4;
            let expected = body.volume()
                + (160.0 - 20.0 * corner_loss) * angle.tan() * 50.0
                + (1.0 - corner_loss) * angle.tan().powi(2) * 1000.0 / 3.0;
            // The general BREP mass integrator uses dimensional quadrature
            // targets; check its volume separately from the exact section radii.
            assert!(
                (result.volume() - expected).abs() < expected * 1e-6,
                "draft angle {angle}: volume {}, expected {expected}",
                result.volume()
            );
            let cone = result
                .faces()
                .find_map(|face| match face.surface() {
                    Surface3::Cone(cone) => Some(*cone),
                    _ => None,
                })
                .unwrap();
            for z in [0.0, 10.0] {
                let v = (z - cone.apex().z) / cone.axis().z;
                let radius = cone.reference_radius() * v;
                assert!((radius - (10.0 + (10.0 - z) * angle.tan())).abs() < 1e-7);
            }
            mesh(&result);
        }
    }
    #[test]
    fn feature_chamfer_application_coordinates() {
        let plane = Plane3::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::unit_x(),
            -Vec3::unit_z(),
            ModelTolerance::default(),
        )
        .unwrap();
        let body = rectangle(-72.2352286216668, -32.617235, 144.4704572433336, 65.23447)
            .extrude(plane, Vec3::new(0.0, 21.0, 0.0))
            .unwrap();
        let edge = body
            .edges()
            .find(|e| {
                e.endpoints().is_ok_and(|p| {
                    p.iter().all(|p| {
                        (p.x + 72.2352286216668).abs() < 1e-7 && (p.z - 32.617235).abs() < 1e-7
                    })
                })
            })
            .unwrap()
            .clone();
        let body = body.fillet(&[edge], 20.0).unwrap();
        let edge = body
            .edges()
            .find(|e| {
                matches!(e.curve(), Curve3::Line(_))
                    && e.endpoints().is_ok_and(|p| {
                        p.iter()
                            .all(|p| (p.y - 21.0).abs() < 1e-7 && (p.z - 32.617235).abs() < 1e-7)
                    })
            })
            .unwrap()
            .clone();
        let result = body.chamfer(&[edge], 1.0).unwrap();
        assert_eq!(result.solids().len(), 1);
        assert!(result.volume() < body.volume());
        mesh(&result);
    }
    #[test]
    fn feature_rounded_corner_chamfer() {
        for corner in [(30.0, 17.0), (130.0, 17.0), (130.0, 77.0), (30.0, 77.0)] {
            for rotation in [
                crate::Matrix3::identity(),
                crate::Matrix3::from_angle_x(cgmath::Deg(-90.0)),
                crate::Matrix3::from_angle_y(cgmath::Deg(37.0)),
            ] {
                let transform = super::super::RigidTransform3::new(
                    rotation,
                    Vec3::new(2.2352286216668, 11.0, 32.617235),
                    ModelTolerance::default(),
                )
                .unwrap();
                let body = rounded_corner_block_at(corner)
                    .transformed(transform)
                    .unwrap();
                let edges = body
                    .edges()
                    .filter(|e| matches!(e.curve(), Curve3::Line(_)))
                    .filter(|e| {
                        e.endpoints().is_ok_and(|p| {
                            p.iter().all(|p| {
                                (transform.inverse().transform_point(*p).z - 10.0).abs() < 1e-7
                            })
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                for edge in edges {
                    let endpoints = edge.endpoints().unwrap();
                    let result = body
                        .chamfer(&[edge], 1.0)
                        .unwrap_or_else(|e| panic!("chamfer at {endpoints:?}: {e:?}"));
                    assert!(result.volume() < body.volume());
                    mesh(&result);
                }
            }
        }
    }
    fn rounded_corner_block() -> Compound {
        rounded_corner_block_at((30.0, 17.0))
    }
    fn rounded_corner_block_at(corner: (f64, f64)) -> Compound {
        let body = rectangle(30.0, 17.0, 100.0, 60.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        let edge = body
            .edges()
            .find(|e| {
                e.endpoints().is_ok_and(|p| {
                    p.iter()
                        .all(|p| (p.x - corner.0).abs() < 1e-7 && (p.y - corner.1).abs() < 1e-7)
                })
            })
            .unwrap()
            .clone();
        body.fillet(&[edge], 10.0).unwrap()
    }
    #[test]
    fn feature_draft_reversed_axis_and_cone() {
        let cylinder = circle(5.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, -10.0))
            .unwrap();
        let face = cylinder
            .faces()
            .find(|f| matches!(f.surface(), Surface3::Cylinder(_)))
            .unwrap()
            .clone();
        let result = cylinder
            .draft(&[face], Plane3::xy(), 0.1_f64.atan())
            .unwrap();
        assert!((result.volume() - 910.0 / 3.0 * std::f64::consts::PI).abs() < 1.0e-5);
        mesh(&result);
        let face = result
            .faces()
            .find(|f| matches!(f.surface(), Surface3::Cone(_)))
            .unwrap()
            .clone();
        let changed = result.draft(&[face], Plane3::xy(), 0.2_f64.atan()).unwrap();
        assert!(changed.volume() > result.volume());
        mesh(&changed);
    }
    #[test]
    fn feature_draft() {
        let body = rectangle(-5.0, -5.0, 10.0, 10.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        let faces = body
            .faces()
            .filter(|f| matches!(f.surface(),Surface3::Plane(p) if p.normal().z.abs()<0.1))
            .cloned()
            .collect::<Vec<_>>();
        let angle = 0.1_f64.atan();
        let result = body.draft(&faces, Plane3::xy(), angle).unwrap();
        assert!((result.volume() - 2440.0 / 3.0).abs() < 1.0e-5);
        mesh(&result);
        let cylinder = circle(5.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        let face = cylinder
            .faces()
            .find(|f| matches!(f.surface(), Surface3::Cylinder(_)))
            .unwrap()
            .clone();
        let result = cylinder.draft(&[face], Plane3::xy(), angle).unwrap();
        assert!((result.volume() - 610.0 / 3.0 * std::f64::consts::PI).abs() < 1.0e-5);
        mesh(&result);
    }
}
