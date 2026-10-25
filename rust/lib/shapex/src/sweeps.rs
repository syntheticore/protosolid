//! Exact section-based feature construction. Display triangles never enter BREP.
use super::geometry::{basis_values, de_casteljau_split};
use super::sew::{BrepPatch, BrepPatchLoop, BrepPatchSegment};
use super::*;
use crate::{Point2, Point3, Vec2, Vec3};
use cgmath::prelude::*;

fn range(a: f64, b: f64) -> KernelResult<ParameterRange> {
    ParameterRange::new(a, b)
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
                    provenance: Default::default(),
                    curve: plane.lift_trimmed_curve(segment, profile.tolerance())?,
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
        preserved: Vec::new(),
        sources: Vec::new(),
        surface: plane.into(),
        orientation: FaceOrientation::Forward,
        loops,
    };
    if reversed {
        patch.reverse();
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

// Attach roles at construction, before welding/unification can merge patches.
// The tokens identify generators, not positions in the published face array.
pub(super) fn publish_with_roles(mut patches: Vec<BrepPatch>, roles: Vec<CreationRole>, tolerance: ModelTolerance) -> KernelResult<Compound> {
    let namespace = super::identity::fresh_uuid();
    let roots = roles.into_iter().enumerate().map(|(index, role)|
        (PersistentId { namespace, local: index as u64 }, role)).collect::<std::collections::BTreeMap<_, _>>();
    for (patch, (name, _)) in patches.iter_mut().zip(&roots) {
        patch.sources.push(*name);
    }
    Ok(publish(&patches, tolerance)?.with_patch_roles(&roots))
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
            let mut row = vec![0.0; 2 * count];
            let (first, values) = basis_values(degree, &knots, count, *t);
            row[first..first + values.len()].copy_from_slice(&values);
            row
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
                    .extend(std::iter::repeat(index as f64 / count as f64).take(degree));
            }
            joined.knots.extend(vec![1.0; degree + 1]);
            *section = vec![joined];
        }
    }
    let (v_degree, v_knots, inverse) = interpolation(sections.len())?;
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
                    point += sections[i].0.point_at(c.points[u]).to_vec() * w;
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
mod tests {
    use super::*;
    #[test]
    fn revolved_sector_blend_followups() {
        let body = rectangle(12.0, -4.0, 8.0, 8.0)
            .revolve(
                Plane3::xz(),
                Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_z()).unwrap(),
                std::f64::consts::PI * 1.5,
            )
            .unwrap();
        let cap = body
            .faces()
            .find(|face| {
                matches!(face.surface(), Surface3::Plane(p)
            if p.normal().x.abs() > 0.99)
            })
            .unwrap();
        let edges = cap.boundary_edges().unwrap();
        let chamfered = body.chamfer(&edges, 1.0).unwrap();
        for edge in chamfered.edges() {
            assert!(
                chamfered.is_semantic_edge(edge),
                "hidden edge {:?}",
                edge.persistent_id()
            );
        }
        let filleted = body.fillet(&edges, 2.0).unwrap();
        for face in body.faces() {
            let resolved = match filleted.resolve(face) {
                super::super::Resolution::Preserved(face)
                | super::super::Resolution::Modified(face) => face,
                other => panic!("sector support lost during fillet: {other:?}"),
            };
            assert_eq!(
                std::mem::discriminant(face.surface()),
                std::mem::discriminant(resolved.surface())
            );
        }
        for distance in [-1.0, 1.0] {
            filleted.shell_with_join(&[], distance, ShellJoin::Sharp).unwrap();
        }
        let shell = filleted.shell(&[], -1.0).unwrap();
        assert!(shell.mass_properties().volume > 0.0);
        assert!(shell.mass_properties().volume < filleted.mass_properties().volume);
        shell
            .tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap())
            .unwrap();
        let opening = body
            .faces()
            .find(|face| {
                matches!(face.surface(), Surface3::Plane(p)
            if p.normal().y.abs() > 0.99)
            })
            .unwrap();
        let opening = match filleted.resolve(opening) {
            super::super::Resolution::Preserved(face)
            | super::super::Resolution::Modified(face) => face,
            other => panic!("opening lost: {other:?}"),
        };
        for openings in [vec![], vec![opening]] {
            let shell = filleted.shell(&openings, -2.0).unwrap();
            assert!(shell.volume() > 0.0 && shell.volume() < filleted.volume());
            shell
                .tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap())
                .unwrap();
        }
        let mut expanded = edges;
        expanded.extend(
            body.edges()
                .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
                .cloned(),
        );
        assert_eq!(expanded.len(), 8);
        let fully_rounded = body.fillet(&expanded, 2.0).unwrap();
        let opening = fully_rounded
            .faces()
            .find(|face| {
                matches!(face.surface(), Surface3::Plane(p)
            if p.normal().y.abs() > 0.99)
            })
            .unwrap()
            .clone();
        for thickness in [1.0, 2.0] {
            fully_rounded.shell(&[opening.clone()], -thickness).unwrap();
        }
        let shell = fully_rounded.shell(&[opening], -1.0).unwrap();
        let rim_edges = [shell.edges().find(|edge| {
            edge.endpoints().unwrap().iter().all(|p| p.y.abs() < 1e-7 && (p.z + 4.0).abs() < 1e-7)
        }).unwrap().clone()];
        let rounded = shell.fillet_with_options(&rim_edges,
            FilletOptions::new(0.3).with_tangent_propagation(true)).unwrap();
        let rim_edges = [rounded.edges().find(|edge| {
            edge.endpoints().unwrap().iter().all(|p| p.y.abs() < 1e-7 && (p.x - 13.0).abs() < 1e-7)
        }).unwrap().clone()];
        let chamfered = rounded.chamfer_with_tangent_propagation(&rim_edges, 0.2, true).unwrap();
        let section = Plane3::from_origin_normal(Point3::new(0.0, 0.0, -3.0), Vec3::unit_z()).unwrap();
        chamfered.split(section, SplitKeep::Right).unwrap();
    }
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
                        preserved: Vec::new(),
                        sources: Vec::new(),
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
                                            provenance: Default::default(),
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
    fn tilted_round_ended_profile_revolution() {
        let tolerance = ModelTolerance::default();
        let center = Point2::new(-52.88068565071396, -86.18951841790492);
        let arc = TrimmedCurve2::new(
            Circle2::new(center, 15.0).unwrap(),
            range(-0.4014257279586958, -3.543018381548489).unwrap(),
        )
        .unwrap();
        let upper = Point2::new(-27.61514560357321, 11.721933854678209);
        let origin = Point2::new(0.0, 0.0);
        let profile = Profile::new(
            Wire2::new(
                [
                    TrimmedCurve2::line(arc.end().unwrap(), upper).unwrap(),
                    TrimmedCurve2::line(upper, origin).unwrap(),
                    TrimmedCurve2::line(origin, arc.start().unwrap()).unwrap(),
                    arc,
                ],
                tolerance,
            ).unwrap(),
            [],
            tolerance,
        ).unwrap();
        // The long-edge axis makes the arc tangent to the axis. Its mapped
        // circle center rounds just below its radius, reversing du at the pole.
        for direction in [
            Vec3::unit_z(),
            Vec3::new(-39.07311284892735, 0.0, -92.05048534524403),
            Vec3::new(upper.x, 0.0, upper.y),
        ] {
            for angle in [std::f64::consts::PI, -std::f64::consts::PI] {
                let body = profile.revolve(
                    Plane3::xz(),
                    Axis3::new(Point3::new(0.0, 0.0, 0.0), direction).unwrap(),
                    angle,
                ).unwrap();
                assert!(!body
                    .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                    .unwrap()
                    .is_empty());
            }
        }
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
    fn draft_preserves_neutral_section_and_sets_angle() {
        let a = circle(10.0);
        let b = circle(6.0);
        let plane = Plane3::new(Point3::new(4.0, 0.0, 20.0), Vec3::unit_x(), Vec3::unit_y(), ModelTolerance::default()).unwrap();
        let body = a.loft(Plane3::xy(), [(plane, &b)]).unwrap();
        let side = body.faces().find(|face| matches!(face.surface(), Surface3::Nurbs(_))).unwrap().clone();
        for angle in [-3.0_f64, 3.0].map(f64::to_radians) {
            let drafted = body.draft(&[side.clone()], Plane3::xy(), angle).unwrap();
            let side = drafted.faces().find(|face| matches!(face.surface(), Surface3::Offset(_))).unwrap();
            for u in [0.0, 0.17, 0.4, 0.8, 1.0] {
                let foot = side.surface().evaluate(u, 0.0).unwrap().point;
                assert!((foot.to_vec().magnitude() - 10.0).abs() < 1e-7);
                for v in [0.0, 0.3, 0.7, 1.0] {
                    assert!((side.surface().normal(u, v).unwrap().z - angle.sin()).abs() < 1e-7);
                }
            }
            let r = 10.0 - 20.0 * angle.tan();
            let volume = std::f64::consts::PI * 20.0 / 3.0 * (100.0 + 10.0 * r + r * r);
            assert!((drafted.volume() - volume).abs() < volume * 1e-8);
            mesh(&drafted);
        }
    }
    #[test]
    fn spline_sector_end_fillet() {
        let t = ModelTolerance::default();
        let spline = NurbsCurve2::new(3, vec![Point2::new(20.0, -4.0), Point2::new(22.0, -1.0), Point2::new(22.0, 1.0), Point2::new(20.0, 4.0)], vec![1.0;4], vec![0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0]).unwrap();
        let profile = Profile::new(Wire2::new([
            TrimmedCurve2::line(Point2::new(12.0,-4.0), Point2::new(20.0,-4.0)).unwrap(),
            TrimmedCurve2::new(spline, range(0.0,1.0).unwrap()).unwrap(),
            TrimmedCurve2::line(Point2::new(20.0,4.0), Point2::new(12.0,4.0)).unwrap(),
            TrimmedCurve2::line(Point2::new(12.0,4.0), Point2::new(12.0,-4.0)).unwrap(),
        ],t).unwrap(),[],t).unwrap();
        let body = profile.revolve(Plane3::xz(), Axis3::new(Point3::new(0.0,0.0,0.0),Vec3::unit_z()).unwrap(),std::f64::consts::PI*1.5).unwrap();
        let cap = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(p) if p.normal().y.abs()>0.99)).unwrap();
        let result = body.fillet(&cap.boundary_edges().unwrap(),0.5).unwrap();
        mesh(&result);
    }
    #[test]
    fn intersecting_bores_rim_fillet() {
        let t = ModelTolerance::default();
        let body = rectangle(-15.0,-15.0,30.0,30.0).extrude(Plane3::xy(), Vec3::unit_z()*20.0).unwrap();
        let first = circle(5.0).extrude(Plane3::xy(),Vec3::unit_z()*20.0).unwrap();
        let second = first.transformed(RigidTransform3::new(crate::Matrix3::identity(),Vec3::unit_x()*6.0,t).unwrap()).unwrap();
        let body = body.difference(&first).unwrap().difference(&second).unwrap();
        let cap = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(p) if p.origin().z>19.0)).unwrap();
        let result = body.fillet(&cap.boundary_edges().unwrap(),0.5).unwrap();
        mesh(&result);
    }
    #[test]
    fn cross_bore_move() {
        let t = ModelTolerance::default();
        let body = circle(10.0).extrude(Plane3::xy(), Vec3::unit_z() * 20.0).unwrap();
        let plane = Plane3::new(Point3::new(0.0, -15.0, 10.0), Vec3::unit_x(), -Vec3::unit_z(), t).unwrap();
        let tool = circle(3.0).extrude(plane, Vec3::unit_y() * 30.0).unwrap();
        let original = body.clone();
        let body = body.difference(&tool).unwrap();
        let bore = body.faces().find(|face| matches!(face.surface(), Surface3::Cylinder(c) if c.radius() < 4.0)).unwrap().clone();
        let transform = RigidTransform3::new(crate::Matrix3::identity(), Vec3::unit_x(), t).unwrap();
        let moved = body.move_faces(&[bore], transform).unwrap();
        let expected = original.difference(&tool.transformed(transform).unwrap()).unwrap();
        assert!((moved.volume() - expected.volume()).abs() < expected.volume() * 1e-7);
        let bore = moved.faces().find(|face| matches!(face.surface(), Surface3::Cylinder(c) if c.radius() < 4.0)).unwrap();
        let Surface3::Cylinder(cylinder) = bore.surface() else { unreachable!() };
        assert!((cylinder.origin().x - 1.0).abs() < t.linear);
        assert!((cylinder.radius() - 3.0).abs() < t.linear);
        mesh(&moved);
    }
    #[test]
    fn tapered_planar_loft_followups() {
        let a = rectangle(-12.0, -12.0, 24.0, 24.0);
        let b = rectangle(-6.0, -8.0, 16.0, 16.0);
        let body = a.loft(Plane3::xy(), [(upper(20.0), &b)]).unwrap();
        assert!(body.faces().all(|face| matches!(face.surface(), Surface3::Plane(_))));
        for face in body.faces() {
            body.offset_faces(&[face.clone()], 0.5).unwrap();
        }
        body.fillet(&body.edges().cloned().collect::<Vec<_>>(), 0.5).unwrap();
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
    fn bulging_square_loft_rim() {
        let a = rectangle(-10.0, -10.0, 20.0, 20.0);
        let b = rectangle(-10.0, -12.0, 24.0, 24.0);
        let c = rectangle(-7.0, -8.0, 16.0, 16.0);
        let body = a.loft(Plane3::xy(), [(upper(10.0), &b), (upper(20.0), &c)]).unwrap();
        let edges = body.edges().filter(|edge| {
            edge.endpoints().unwrap().into_iter().all(|p| (p.z - 20.0).abs() < 1e-6)
        }).cloned().collect::<Vec<_>>();
        assert_eq!(edges.len(), 4);
        let result = body.fillet(&edges, 0.5).unwrap();
        assert!(result.volume() < body.volume());
        mesh(&result);
    }
    #[test]
    fn equal_orthogonal_bores() {
        let block = rectangle(-15.0, -15.0, 30.0, 30.0).extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0)).unwrap();
        let bore = circle(5.0).extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0)).unwrap();
        let plane = Plane3::new(Point3::new(0.0, -15.0, 10.0), Vec3::unit_x(), Vec3::unit_z(), block.tolerance()).unwrap();
        let cross = circle(5.0).extrude(plane, Vec3::new(0.0, 30.0, 0.0)).unwrap();
        let result = block.difference(&bore).unwrap().difference(&cross).unwrap();
        let expected = 18000.0 - 50.0 * std::f64::consts::PI * 25.0 + 16.0 * 125.0 / 3.0;
        assert!((result.volume() - expected).abs() < 1e-3);
        mesh(&result);
    }
    #[test]
    fn torus_face_limited_pocket() {
        let t = ModelTolerance::default();
        let section = Profile::new(Wire2::new([TrimmedCurve2::circle(Point2::new(12.0, 0.0), 3.0).unwrap()], t).unwrap(), [], t).unwrap();
        let body = section.revolve(Plane3::xz(), Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_z()).unwrap(), std::f64::consts::TAU).unwrap();
        let pocket = Profile::new(Wire2::new([TrimmedCurve2::circle(Point2::new(12.0, 0.0), 1.0).unwrap()], t).unwrap(), [], t).unwrap();
        let limit = body.faces().find(|face| matches!(face.surface(), Surface3::Torus(_))).unwrap();
        let tool = pocket.extrude_up_to_face(Plane3::xy(), &body, limit, 0.0, 0.0).unwrap();
        let result = body.difference(&tool).unwrap();
        assert!((result.volume() + tool.volume() - body.volume()).abs() < body.volume() * 1e-6,
            "volumes {} + {} != {}", result.volume(), tool.volume(), body.volume());
        mesh(&result);
    }
    #[test]
    fn feature_split() {
        let body = rectangle(-2.0, -2.0, 4.0, 4.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 4.0))
            .unwrap();
        assert!((body.split(upper(1.0), SplitKeep::Left).unwrap().volume() - 16.0).abs() < 1.0e-6);
        assert!((body.split(upper(1.0), SplitKeep::Right).unwrap().volume() - 48.0).abs() < 1.0e-6);
        let both = body.split(upper(1.0), SplitKeep::Both).unwrap();
        assert_eq!(both.solids().len(), 2);
        assert!((both.volume() - 64.0).abs() < 1.0e-6);
        let cylinder = circle(2.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 4.0))
            .unwrap();
        assert!(
            (cylinder.split(upper(1.0), SplitKeep::Left).unwrap().volume() - 4.0 * std::f64::consts::PI)
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

    fn shell_rim_document_23() -> (Compound, super::super::EdgeHandle) {
        // untitled(23).cad: 70 x 50 x 34, an 11 mm / 55 degree chamfer,
        // six 6 mm fillets, a rounded 6 mm outward shell, then its bottom rim.
        let x0 = -28.352029411716757;
        let y0 = -22.437515961363786;
        let x1 = x0 + 70.0;
        let y1 = y0 + 50.0;
        let body = rectangle(x0, y0, 70.0, 50.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 34.0))
            .unwrap();
        let edge = body
            .edges()
            .find(|e| {
                e.endpoints()
                    .unwrap()
                    .iter()
                    .all(|p| (p.x - x1).abs() < 1e-6 && (p.y - y0).abs() < 1e-6)
            })
            .unwrap()
            .clone();
        let reference = edge
            .connected_faces()
            .unwrap()
            .into_iter()
            .find(|f| {
                f.surface()
                    .project(Point3::new(0.0, y0, 0.0), body.tolerance())
                    .unwrap()
                    .distance
                    < 1e-6
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
                (top && p.iter().all(|p| p.x > x1 - 17.0 || p.y < y0 + 1.0))
                    || (p[0].z - p[1].z).abs() > 33.0
                        && (p[0].x > x1 - 17.0 && p[0].y < y0 + 17.0
                            || p[0].x < x0 + 1e-6 && p[0].y > y1 - 1e-6)
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(edges.len(), 6);
        let body = body.fillet(&edges, 6.0).unwrap();
        let opening = body
            .faces()
            .find(|f| {
                matches!(f.surface(), Surface3::Plane(p)
            if p.normal().z.abs() > 0.99 && p.origin().z.abs() < 1e-6)
            })
            .unwrap()
            .clone();
        let body = body.shell_with_join(&[opening], 6.0, ShellJoin::Rounded).unwrap();
        let edge = body
            .edges()
            .find(|e| {
                let p = e.endpoints().unwrap();
                p.iter()
                    .all(|p| p.z.abs() < 1e-6 && p.x > x1 - 17.0 && p.y < y0 + 17.0)
                    && matches!(e.curve(), Curve3::Line(_))
                    && (p[0].x - p[1].x).abs() > 1.0
                    && (p[0].y - p[1].y).abs() > 1.0
            })
            .unwrap()
            .clone();
        (body, edge)
    }

    #[test]
    fn second_shell_rim_fillet_document_23() {
        let (body, edge) = shell_rim_document_23();
        let body = body.fillet(&[edge], 3.0).unwrap();
        // The remaining inner rim is a 6 mm corner arc and two straight
        // edges. At either end it meets a concave edge of the first fillet.
        let edges = body.edges().filter(|edge| {
            let points = edge.endpoints().unwrap();
            points.iter().all(|p| p.z.abs() < 1e-6)
                && (points.iter().all(|p| (p.x + 28.352029411716757).abs() < 1e-6)
                    || points.iter().all(|p| (p.y - 27.562484038636214).abs() < 1e-6)
                    || matches!(edge.curve(), Curve3::Circle(c)
                        if c.center().distance(Point3::new(-22.352029411716757, 21.562484038636214, 0.0)) < 1e-6
                            && (c.radius() - 6.0).abs() < 1e-6))
        }).cloned().collect::<Vec<_>>();
        assert_eq!(edges.len(), 3);
        // At 3 mm the two terminating arcs are consumed completely.
        for radius in [0.25, 0.5, 1.0, 2.0, 2.999, 3.0] {
            let mut expected_volume: Option<f64> = None;
            for edge in &edges {
                let result = body
                    .fillet(&[edge.clone()], radius)
                    .unwrap_or_else(|error| panic!("second rim fillet {radius}: {error}"));
                assert!(result.volume() < body.volume());
                if let Some(volume) = expected_volume {
                    assert!((result.volume() - volume).abs() < 1e-6);
                }
                expected_volume = Some(result.volume());
                mesh(&result);
            }
        }
    }

    #[test]
    fn oversized_shell_rim_fillet_document_23() {
        let (body, edge) = shell_rim_document_23();
        let original_volume = body.volume();
        for radius in [1.0, 4.0, 5.99, 6.0, 6.01, 7.0, 11.0, 12.0] {
            let result = body.fillet(&[edge.clone()], radius);
            if radius < 6.0 {
                mesh(&result.unwrap());
            } else {
                let error =
                    result.expect_err(&format!("radius {radius} published invalid geometry"));
                assert!(error
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| diagnostic.kind
                        == super::super::DiagnosticKind::GeometryInvariant));
            }
            assert_eq!(body.volume(), original_volume);
        }
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
                for join in [ShellJoin::Sharp, ShellJoin::Rounded] {
                    let result = body
                        .shell_with_join(&openings, distance, join)
                        .unwrap_or_else(|error| {
                            panic!(
                                "openings {:?}, distance {distance}, join {join:?}: {error}",
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
        for thickness in [5.0, 6.0] {
            let body = filleted_asymmetric_chamfer();
            let opening = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.normal().z.abs() > 0.99 && p.origin().z.abs() < 1e-6)).unwrap().clone();
            let body = body.shell(&[opening], thickness).unwrap();
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
            let distances: &[f64] = if thickness == 5.0 {
                &[5.0, 10.0, 10.99, 11.0, 11.01, 12.0, 15.0, 20.0, 30.0, 40.0]
            } else {
                &[14.0]
            };
            for &distance in distances {
                let tool = profile
                    .extrude(upper(34.0 + thickness), Vec3::new(0.0, 0.0, -distance))
                    .unwrap();
                let cut = body
                    .difference(&tool)
                    .unwrap_or_else(|e| panic!("distance {distance}: {e}"));
                assert_eq!(
                    cut.solids().len(),
                    if distance < (34.0 + thickness) { 1 } else { 2 }
                );
                assert!(cut.volume() > 0.0 && cut.volume() < body.volume());
                assert!(cut.volume() <= previous_volume + 1e-5);
                previous_volume = cut.volume();
                mesh(&cut);
                if thickness == 5.0 && [10.0, 12.0, 20.0].contains(&distance)
                    || thickness == 6.0 && distance == 14.0
                {
                    for radius in if thickness == 5.0 {
                        vec![1.0]
                    } else {
                        vec![0.05, 0.1, 0.25]
                    } {
                        let edge = cut
                            .edges()
                            .find(|e| {
                                matches!(e.curve(), Curve3::Circle(c)
                        if (c.center().z - (34.0 + thickness)).abs() < 1e-6
                            && (c.radius() - 32.892404807198155).abs() < 1e-6)
                            })
                            .unwrap()
                            .clone();
                        let rounded = cut
                            .fillet(&[edge], radius)
                            .unwrap_or_else(|e| panic!("shell {thickness}, cut {distance}, fillet {radius}: {e}"));
                        assert_eq!(rounded.solids().len(), 1);
                        assert!(rounded.volume() > 0.0 && rounded.volume() < cut.volume());
                        mesh(&rounded);
                    }
                }
            }
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
        // Contact with the end rims requires a topology change; the retained
        // cylinder trim must not publish an unrepresented pinch point.
        let touching = Plane3::from_origin_normal(
            Point3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 1.0, 1.0),
        ).unwrap();
        assert!(cylinder.split(touching, SplitKeep::Both).is_err());
        let slanted =
            Plane3::from_origin_normal(Point3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.8, 1.0))
                .unwrap();
        let both = cylinder.split(slanted, SplitKeep::Both).unwrap();
        assert_eq!(both.solids().len(), 2);
        assert!((both.volume() - cylinder.volume()).abs() < 1.0e-5);
        assert!((body.split(upper(20.0), SplitKeep::Left).unwrap().volume() - body.volume()).abs() < 1.0e-7);
        assert_eq!(body.split(upper(20.0), SplitKeep::Right).unwrap().solids().len(), 0);
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
