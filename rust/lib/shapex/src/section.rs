//! Plane clipping of exact BREP faces. Caps reuse the exposed retained edges;
//! no finite solid cutter or material classifier is needed.
use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2};
use super::boolean::{arrange_section_regions,
    patch_complexity_key, patch_components, same_unoriented_edge, unchanged_face_patch};
use super::sew::{BrepPatch, BrepPatchLoop, BrepPatchSegment, SegmentProvenance};
use super::trim::{align_pcurve_to_face_chart, face_boundary_curves, face_parameter_bounds,
    intersection_chart_copies, partition_to_faces, point_in_sampled_trim, trim_samples};
use super::{BooleanKind, Compound, FaceOrientation, IntersectionSet, Kernel, KernelResult,
    Curve2, Curve3, Line2, Line3, LoopKind, ModelTolerance, ParameterRange, Plane3,
    PlaneCurveProjection, SplitKeep, Surface3};

type FaceCurve = super::trim::FaceCurve<SegmentProvenance>;

pub(super) fn split(source: &Compound, plane: Plane3, keep: SplitKeep) -> KernelResult<Compound> {
    if source.solids().len() == 0 { return source.unchanged(); }
    let mut tolerance = source.tolerance();
    let mut plane = Plane3::from_origin_normal(plane.origin(), plane.normal())?;
    let nearest = source.vertices().map(|vertex| (vertex.point() - plane.origin()).dot(plane.normal()))
        .min_by(|a, b| a.abs().total_cmp(&b.abs())).unwrap();
    if nearest.abs() <= tolerance.linear {
        plane = Plane3::from_origin_normal(plane.origin() + plane.normal() * nearest, plane.normal())?;
    }
    let bounds = super::bounds::compound(source)?;
    let signed = |point: Point3| (point - plane.origin()).dot(plane.normal());
    // Keep the arrangement's accumulated weld allowances below a resolved
    // gap between the plane and an existing vertex.
    let clearance = source.vertices().map(|vertex| signed(vertex.point()).abs())
        .filter(|distance| *distance > tolerance.linear).fold(f64::INFINITY, f64::min);
    tolerance.linear = tolerance.linear.min(clearance / 64.0);
    let low = bounds.corners().map(signed).fold(f64::INFINITY, f64::min);
    let high = bounds.corners().map(signed).fold(f64::NEG_INFINITY, f64::max);
    if high < -tolerance.linear || low > tolerance.linear {
        let retain = keep == SplitKeep::Both || (high < 0.0) == (keep == SplitKeep::Left);
        return if retain { source.unchanged() } else { publish(source, Vec::new(), keep, tolerance) };
    }
    let mut minimum = Point2::new(f64::INFINITY, f64::INFINITY);
    let mut maximum = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for corner in bounds.corners() {
        let point = plane.coordinates(corner)?;
        for axis in 0..2 {
            minimum[axis] = minimum[axis].min(point[axis]);
            maximum[axis] = maximum[axis].max(point[axis]);
        }
    }
    let margin = (maximum - minimum).magnitude() * 0.01 + tolerance.linear * 64.0;
    minimum -= Vec2::new(margin, margin);
    maximum += Vec2::new(margin, margin);
    let plane_bounds = [ParameterRange::new(minimum.x, maximum.x)?,
        ParameterRange::new(minimum.y, maximum.y)?];
    let rectangle = plane_boundary(plane, minimum, maximum)?;
    let scale = source.vertices().map(|vertex| vertex.point().to_vec().magnitude())
        .fold(1.0_f64, f64::max);
    let probe = (tolerance.linear_at_scale(scale) * 128.0).max(scale * 1e-4);
    // Short enough to stay inside thin but genuine section bands, long enough
    // for a tangential contact to leave the plane far beyond rounding noise.
    let contact = tolerance.linear_at_scale(scale) * 128.0;
    let mut prepared = Vec::new();
    for face in source.faces() {
        let boundary = face_boundary_curves(source.topology(), face.topology_id(), tolerance)?
            .into_iter().map(|curve| {
                let provenance = SegmentProvenance::from_coedge(source.topology(), curve.provenance)?;
                Ok(curve.with_provenance(provenance))
            }).collect::<KernelResult<Vec<_>>>()?;
        let parameters = face_parameter_bounds(&boundary, tolerance)?;
        let mut curves = boundary.clone();
        let context = |stage: &str, error: super::KernelError| {
            super::KernelError::from_diagnostics(error.diagnostics().iter().cloned().map(|diagnostic| {
                diagnostic.with_entity(face.persistent_id()).with_operation(format!("section {stage} on {}", face.surface().kind_name()))
            }).collect())
        };
        let intersections = face.surface().intersect_section_plane(
            plane, parameters, plane_bounds, tolerance)
            .map_err(|error| context("intersection", error))?;
        let coincident = matches!(intersections, IntersectionSet::Coincident);
        if let IntersectionSet::Isolated(intersections) = intersections {
            for intersection in intersections {
                let aligned = align_pcurve_to_face_chart(&intersection.first_pcurve, None,
                    face.surface(), &boundary, tolerance)?;
                for pcurve in intersection_chart_copies(intersection.curve.domain().bounds,
                    &aligned, face.surface(), parameters, tolerance)? {
                    for range in partition_to_faces(&intersection.curve, &pcurve, &boundary,
                        &intersection.second_pcurve, &rectangle, tolerance)
                        .map_err(|error| context("trim", error))? {
                        if !lies_on_boundary(&intersection.curve, range, &boundary, tolerance)?
                            && crosses_plane(face.surface(), &pcurve, range, contact, signed)? {
                            curves.push(FaceCurve { provenance: Default::default(),
                                curve: intersection.curve.clone(), pcurve: pcurve.clone(), range,
                                loop_end: false, loop_kind: LoopKind::Outer, parameter_seam: false });
                        }
                    }
                }
            }
        }
        prepared.push((face, boundary, parameters, curves, coincident));
    }
    let sides: &[SplitKeep] = match keep {
        SplitKeep::Both => &[SplitKeep::Left, SplitKeep::Right],
        SplitKeep::Left => &[SplitKeep::Left],
        SplitKeep::Right => &[SplitKeep::Right],
    };
    let mut results = Vec::new();
    for side in sides {
        let mut patches = Vec::new();
        for (face, boundary, parameters, curves, coincident) in &prepared {
            let orientation = source.topology().face(face.topology_id())?.orientation();
            let surface = face.surface();
            let inside = |point: Point3| signed(point) * if *side == SplitKeep::Left { 1.0 } else { -1.0 } < 0.0;
            let limit = parameters[0].span().abs().min(parameters[1].span().abs()) * 0.05;
            let mut clipped = if *coincident {
                let outward = surface.normal(parameters[0].middle(), parameters[1].middle())? * orientation.sign();
                let retain = (outward.dot(plane.normal()) > 0.0) == (*side == SplitKeep::Left);
                if retain { vec![unchanged_face_patch(surface, boundary, orientation, orientation, tolerance)?] }
                else { Vec::new() }
            } else if curves.len() == boundary.len() {
                let mut retained = false;
                for edge in boundary.iter().filter(|edge| !edge.parameter_seam) {
                    let (uv, tangent) = edge.pcurve.first_order(edge.range.middle())?;
                    let tangent = tangent * edge.range.span().signum() * orientation.sign();
                    if tangent.magnitude2() <= tolerance.parameter.powi(2) { continue; }
                    let inward = Vec2::new(-tangent.y, tangent.x).normalize();
                    let value = surface.evaluate(uv.x, uv.y)?;
                    let direction = value.du * inward.x + value.dv * inward.y;
                    if direction.magnitude2() <= tolerance.linear.powi(2) { continue; }
                    let candidate = uv + inward * (probe / direction.magnitude()).min(limit);
                    retained = inside(surface.evaluate(candidate.x, candidate.y)?.point);
                    break;
                }
                if retained { vec![unchanged_face_patch(surface, boundary, orientation, orientation, tolerance)?] }
                else { Vec::new() }
            } else {
                let samples = trim_samples(boundary)?;
                arrange_section_regions(surface, curves, probe, limit, tolerance, plane, *side, orientation, |uv| {
                    Ok(point_in_sampled_trim(uv, boundary, &samples)?.then_some(orientation))
                }).map_err(|error| super::KernelError::from_diagnostics(error.diagnostics().iter().cloned()
                    .map(|diagnostic| diagnostic.with_entity(face.persistent_id())
                        .with_operation(format!("section arrangement on {}", surface.kind_name()))).collect()))?
            };
            for patch in &mut clipped {
                patch.sources.push(face.persistent_id());
                if patch.retains_boundary(source.topology(), face.topology_id())? {
                    patch.preserved.push(face.persistent_id());
                }
            }
            patches.extend(clipped);
        }
        if patches.len() == prepared.len() && patches.iter().all(|patch| !patch.preserved.is_empty()) {
            results.push(source.unchanged()?);
            continue;
        }
        super::sew::conform_brep_boundaries(&mut patches, tolerance)?;
        let caps = cap_boundaries(&patches, plane, tolerance).map_err(|error|
            super::KernelError::from_diagnostics(error.diagnostics().iter().cloned()
                .map(|diagnostic| diagnostic.with_operation("section cap boundaries")).collect()))?;
        if !caps.is_empty() {
            let orientation = if *side == SplitKeep::Left { FaceOrientation::Forward } else { FaceOrientation::Reversed };
            patches.extend(cap_patches(caps, plane, orientation, tolerance)?);
        }
        results.push(publish(source, patches, *side, tolerance).map_err(|error|
            super::KernelError::from_diagnostics(error.diagnostics().iter().cloned()
                .map(|diagnostic| diagnostic.with_operation("section sewing")).collect()))?);
    }
    let first = results.remove(0);
    if results.is_empty() { Ok(first) } else { first.combined(&results) }
}

fn lies_on_boundary(curve: &Curve3, range: ParameterRange, boundary: &[FaceCurve],
    tolerance: ModelTolerance) -> KernelResult<bool> {
    for edge in boundary {
        let mut coincident = true;
        for index in 0..=8 {
            let point = curve.evaluate(range.start + range.span() * index as f64 / 8.0)?.point;
            if edge.curve.project_on_trim(point, edge.range, tolerance)?.distance > tolerance.linear * 0.5 {
                coincident = false;
                break;
            }
        }
        if coincident { return Ok(true); }
    }
    Ok(false)
}

/// Where the plane only touches a face tangentially, intersection can still
/// report short branches inside that zero band. Such a branch separates no
/// material: the face must lie on opposite sides of a real section curve.
fn crosses_plane(surface: &Surface3, pcurve: &Curve2, range: ParameterRange, distance: f64,
    signed: impl Fn(Point3) -> f64) -> KernelResult<bool> {
    let (uv, tangent) = pcurve.first_order(range.middle())?;
    let across = Vec2::new(-tangent.y, tangent.x).normalize();
    let value = surface.evaluate(uv.x, uv.y)?;
    let step = across * distance / (value.du * across.x + value.dv * across.y).magnitude();
    let first = signed(surface.evaluate(uv.x + step.x, uv.y + step.y)?.point);
    let second = signed(surface.evaluate(uv.x - step.x, uv.y - step.y)?.point);
    Ok((first > 0.0) != (second > 0.0))
}

fn plane_boundary(plane: Plane3, low: Point2, high: Point2) -> KernelResult<Vec<FaceCurve>> {
    let corners = [low, Point2::new(high.x, low.y), high, Point2::new(low.x, high.y)];
    (0..4).map(|index| {
        let point = corners[index];
        let delta = corners[(index + 1) % 4] - point;
        Ok(FaceCurve { curve: Line3::new(plane.point_at(point), plane.vector_at(delta))?.into(),
            pcurve: Line2::new(point, delta)?.into(), range: ParameterRange::new(0.0, 1.0)?,
            provenance: Default::default(), loop_end: index == 3,
            loop_kind: LoopKind::Outer, parameter_seam: false })
    }).collect()
}

fn cap_boundaries(patches: &[BrepPatch], plane: Plane3, tolerance: ModelTolerance) -> KernelResult<Vec<FaceCurve>> {
    let mut groups = Vec::<([Point3; 3], BrepPatchSegment, usize)>::new();
    let linear = tolerance.linear * 4.0;
    for segment in patches.iter().flat_map(|patch| &patch.loops).flat_map(|loop_| &loop_.segments) {
        let sample = [segment.range.start, segment.range.middle(), segment.range.end]
            .map(|t| segment.curve.evaluate(t).map(|value| value.point));
        let sample = [sample[0].clone()?, sample[1].clone()?, sample[2].clone()?];
        let matching = groups.iter().position(|(other, candidate, _)| {
            same_unoriented_edge(sample, *other, linear)
                || (((sample[0].distance(other[0]) <= linear && sample[2].distance(other[2]) <= linear)
                    || (sample[0].distance(other[2]) <= linear && sample[2].distance(other[0]) <= linear))
                    && segment.curve.project_on_trim(other[1], segment.range, tolerance).is_ok_and(|p| p.distance <= linear)
                    && candidate.curve.project_on_trim(sample[1], candidate.range, tolerance).is_ok_and(|p| p.distance <= linear))
        });
        if let Some(index) = matching { groups[index].2 += 1; }
        else { groups.push((sample, segment.clone(), 1)); }
    }
    let mut boundary = Vec::new();
    for (sample, segment, count) in groups {
        if count != 1 { continue; }
        if sample.iter().any(|point| (point - plane.origin()).dot(plane.normal()).abs() > linear) {
            return Err(super::topology_error("a retained section face has an exposed edge off the section plane"));
        }
        let (curve, pcurve, range) = if matches!(&segment.curve, Curve3::Circle(_)) {
            let PlaneCurveProjection::Curve(image) = plane.project_curve(&segment.curve, segment.range, tolerance)? else {
                return Err(super::topology_error("a section cap edge collapsed in the plane"));
            };
            (plane.lift_curve(image.curve(), tolerance)?, image.curve().clone(), image.range())
        } else {
            let pcurve = plane.parameterized_curve_image(&segment.curve, segment.range, tolerance)?;
            (segment.curve, pcurve, segment.range)
        };
        boundary.push(FaceCurve { curve, pcurve, range: range.reversed(),
            provenance: segment.provenance.reversed(), loop_end: false,
            loop_kind: LoopKind::Outer, parameter_seam: false });
    }
    // Give the classifier closed contours, including the tiny UV bridges
    // between independently corrected edge endpoints.
    let mut remaining = boundary.into_iter().map(|edge| {
        let start = edge.pcurve.evaluate(edge.range.start)?.point;
        let end = edge.pcurve.evaluate(edge.range.end)?.point;
        Ok((edge, start, end))
    }).collect::<KernelResult<Vec<_>>>()?;
    let mut ordered = Vec::<FaceCurve>::new();
    while let Some((edge, start, mut end)) = remaining.pop() {
        ordered.push(edge);
        while end.distance(start) > linear * 4.0 {
            let next = remaining.iter().enumerate()
                .min_by(|(_, a), (_, b)| a.1.distance2(end).total_cmp(&b.1.distance2(end)))
                .filter(|(_, candidate)| candidate.1.distance(end) <= linear * 4.0)
                .map(|(index, _)| index)
                .ok_or_else(|| super::topology_error("section cap boundary is open"))?;
            let (edge, _, next_end) = remaining.swap_remove(next);
            ordered.push(edge);
            end = next_end;
        }
        ordered.last_mut().unwrap().loop_end = true;
    }
    Ok(ordered)
}

/// Retained faces already determine the cap graph. Preserve those exact trims
/// instead of intersecting and classifying the same boundaries a second time.
fn cap_patches(boundary: Vec<FaceCurve>, plane: Plane3, orientation: FaceOrientation,
    tolerance: ModelTolerance) -> KernelResult<Vec<BrepPatch>> {
    let mut contours = Vec::new();
    let mut start = 0;
    for (index, edge) in boundary.iter().enumerate() {
        if !edge.loop_end { continue; }
        let edges = &boundary[start..=index];
        let mut area = 0.0;
        for (i, edge) in edges.iter().enumerate() {
            area += edge.pcurve.precise_area_contribution(edge.range, tolerance.parameter)?;
            let end = edge.pcurve.evaluate(edge.range.end)?.point;
            let next = &edges[(i + 1) % edges.len()];
            let next = next.pcurve.evaluate(next.range.start)?.point;
            area += (end.x * next.y - end.y * next.x) * 0.5;
        }
        contours.push((edges, area, trim_samples(edges)?));
        start = index + 1;
    }
    let patch_loop = |edges: &[FaceCurve], kind| BrepPatchLoop { kind,
        segments: edges.iter().map(|edge| BrepPatchSegment { curve: edge.curve.clone(),
            pcurve: edge.pcurve.clone(), range: edge.range, provenance: edge.provenance.clone() }).collect() };
    let mut patches = Vec::new();
    for (index, (edges, area, _)) in contours.iter().enumerate() {
        if area.signum() != orientation.sign() { continue; }
        let mut loops = vec![patch_loop(edges, LoopKind::Outer)];
        for (hole, hole_area, _) in &contours {
            if hole_area.signum() == orientation.sign() { continue; }
            let point = hole[0].pcurve.evaluate(hole[0].range.middle())?.point;
            let mut enclosing = None;
            for (candidate, (edges, area, samples)) in contours.iter().enumerate() {
                if area.signum() != orientation.sign() || area.abs() <= hole_area.abs() { continue; }
                if point_in_sampled_trim(point, edges, samples)?
                    && enclosing.is_none_or(|(_, smallest): (usize, f64)| area.abs() < smallest) {
                    enclosing = Some((candidate, area.abs()));
                }
            }
            if enclosing.is_some_and(|(candidate, _)| candidate == index) {
                loops.push(patch_loop(hole, LoopKind::Inner));
            }
        }
        patches.push(BrepPatch { surface: plane.into(), orientation, loops,
            sources: Vec::new(), preserved: Vec::new() });
    }
    Ok(patches)
}

fn publish(source: &Compound, mut patches: Vec<BrepPatch>, side: SplitKeep,
    precision: ModelTolerance) -> KernelResult<Compound> {
    // Sewing accumulates several weld allowances. Use the cut's clearance
    // budget during construction, then certify at the model's tolerance.
    let tolerance = ModelTolerance { linear: precision.linear * 0.5, ..precision };
    patches.sort_by_key(patch_complexity_key);
    let mut components = patch_components(patches, tolerance)?;
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    let mut bodies = Vec::new();
    for component in &mut components {
        component.sort_by_key(patch_complexity_key);
        bodies.push(transaction.brep_shell(component)?);
    }
    transaction.nest_brep_shells(&bodies)?;
    transaction.unify_same_domain_edges()?;
    transaction.kernel.tolerance = source.tolerance();
    let committed = transaction.finish()?;
    Compound::from_operation_with_parents(kernel, committed, &[source],
        super::DerivationRole::Boolean(if side == SplitKeep::Left { BooleanKind::Difference } else { BooleanKind::Intersection }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vec3;

    fn check_halves(body: &Compound, origin: Point3, normal: Vec3) {
        let plane = Plane3::from_origin_normal(origin, normal).unwrap();
        let left = body.split(plane, SplitKeep::Left)
            .unwrap_or_else(|error| panic!("{origin:?}, {normal:?}, left: {error}"));
        let right = body.split(plane, SplitKeep::Right)
            .unwrap_or_else(|error| panic!("{origin:?}, {normal:?}, right: {error}"));
        assert!((left.volume() + right.volume() - body.volume()).abs() < body.volume() * 1e-7);
        for half in [&left, &right] {
            half.tessellate(super::super::TessellationOptions::new(0.1, 0.15, 24).unwrap()).unwrap();
        }
    }

    #[test]
    fn toroidal_sections_keep_nearly_tangent_branches() {
        use super::super::{Axis3, Profile, TrimmedCurve2, Wire2};
        let tolerance = ModelTolerance::default();
        let profile = Profile::new(Wire2::new(
            [TrimmedCurve2::circle(Point2::new(19.5, 0.0), 2.0).unwrap()], tolerance).unwrap(),
            [], tolerance).unwrap();
        let body = profile.revolve(Plane3::xy(),
            Axis3::new(Point3::origin(), Vec3::unit_y()).unwrap(), std::f64::consts::TAU).unwrap();
        assert!(body.faces().any(|face| matches!(face.surface(), super::super::Surface3::Torus(_))));
        for height in [-1e-6, 0.0, 1e-6, 2.0 - 1e-6, 2.0 + 1e-6] {
            check_halves(&body, Point3::new(0.0, height, 0.0), Vec3::new(0.0, 1.0, 1e-6));
        }
    }

    #[test]
    fn toroidal_sections_meet_a_tangent_trim() {
        use super::super::{Axis3, Profile, TrimmedCurve2, Wire2};
        let t = ModelTolerance::default();
        let corner = Point2::new(19.5, 0.0);
        let start = corner - Vec2::unit_x() * 2.0;
        let end = corner + Vec2::unit_y() * 2.0;
        let profile = Profile::new(Wire2::new([
            TrimmedCurve2::line(corner, start).unwrap(),
            TrimmedCurve2::arc_through(start, corner + Vec2::new(-2.0_f64.sqrt(), 2.0_f64.sqrt()), end).unwrap(),
            TrimmedCurve2::line(end, corner).unwrap(),
        ], t).unwrap(), [], t).unwrap();
        let body = profile.revolve(Plane3::xy(), Axis3::new(Point3::origin(), Vec3::unit_y()).unwrap(),
            std::f64::consts::TAU).unwrap();
        for height in [2.0 - 1e-6, 2.0, 2.0 + 1e-6] {
            check_halves(&body, Point3::new(0.0, height, 0.0), Vec3::new(0.0, 1.0, 1e-6));
        }
    }

    #[test]
    fn section_intersections_cover_every_surface_kind() {
        use super::super::{Surface3, Cylinder3, Cone3, Sphere3, Torus3, Frame3,
            NurbsSurface3, NurbsCurve2, NurbsCurve3, RevolvedSurface3, SweptSurface3,
            OffsetSurface3, EdgeBlendSurface3, TrimmedCurve2};
        let t = ModelTolerance::default();
        let range = ParameterRange::new(0.0, 1.0).unwrap();
        let knots = vec![0.0, 0.0, 1.0, 1.0];
        let nurbs: Surface3 = NurbsSurface3::new((1, 1), vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
            vec![Point3::new(0.0, 2.0, 0.0), Point3::new(2.0, 2.0, 1.0)]],
            vec![vec![1.0; 2]; 2], (knots.clone(), knots)).unwrap().into();
        let meridian = NurbsCurve2::new(2,
            vec![Point2::new(3.0, 0.0), Point2::new(4.0, 1.0), Point2::new(3.0, 2.0)],
            vec![1.0; 3], vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
        let rail: Curve3 = NurbsCurve3::new(2,
            vec![Point3::origin(), Point3::new(0.0, 0.0, 2.0), Point3::new(1.0, 0.0, 4.0)],
            vec![1.0; 3], vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap().into();
        let center: Curve3 = Line3::new(Point3::new(1.0, 1.0, 0.0), Vec3::unit_z()).unwrap().into();
        let first: Surface3 = Plane3::new(Point3::origin(), Vec3::unit_y(), Vec3::unit_z(), t).unwrap().into();
        let second: Surface3 = Plane3::new(Point3::origin(), Vec3::unit_z(), Vec3::unit_x(), t).unwrap().into();
        let blend = |ruled| EdgeBlendSurface3::with_section(center.clone(), first.clone(), second.clone(),
            Line2::new(Point2::new(1.0, 0.0), Vec2::unit_y()).unwrap().into(),
            Line2::new(Point2::new(0.0, 1.0), Vec2::unit_x()).unwrap().into(), range, 1.0, t, ruled).unwrap();
        let angular = ParameterRange::new(0.2, 1.2).unwrap();
        let surfaces = [
            (Surface3::Plane(Plane3::xy()), [range; 2]),
            (Cylinder3::new(Point3::origin(), Vec3::unit_z(), Vec3::unit_x() * 2.0, Vec3::unit_y() * 2.0, t).unwrap().into(), [angular, range]),
            (Cone3::new(Point3::origin(), Vec3::unit_z(), Vec3::unit_x() * 2.0, Vec3::unit_y() * 2.0, t).unwrap().into(), [angular, ParameterRange::new(0.5, 1.5).unwrap()]),
            (Sphere3::new(Point3::origin(), Vec3::unit_x() * 3.0, Vec3::unit_y() * 3.0, Vec3::unit_z() * 3.0, t).unwrap().into(), [angular, ParameterRange::new(-0.4, 0.4).unwrap()]),
            (Torus3::new(Point3::origin(), Vec3::unit_x() * 4.0, Vec3::unit_y() * 4.0, 1.0, t).unwrap().into(), [angular; 2]),
            (RevolvedSurface3::new(Frame3::new(Point3::origin(), Vec3::unit_x(), Vec3::unit_y(), t).unwrap(), meridian.into(), range, t).unwrap().into(), [angular, range]),
            (Surface3::Swept(SweptSurface3::new(Plane3::xy(), TrimmedCurve2::circle(Point2::origin(), 0.5).unwrap(), rail, range, t).unwrap()), [angular, range]),
            (nurbs.clone(), [range; 2]),
            (OffsetSurface3::new(nurbs, 0.2, t).unwrap().into(), [range; 2]),
            (Surface3::RollingBall(blend(false)), [range; 2]),
            (Surface3::Ruled(blend(true)), [range; 2]),
        ];
        let kinds = surfaces.iter().map(|(surface, _)| surface.kind_name()).collect::<std::collections::HashSet<_>>();
        assert_eq!(kinds.len(), 11);
        for (surface, domains) in surfaces {
            let center = surface.evaluate(domains[0].middle(), domains[1].middle()).unwrap().point;
            for normal in [Vec3::unit_x(), Vec3::unit_z(), Vec3::new(0.37, 0.63, 0.68).normalize()] {
                let plane = Plane3::from_origin_normal(center, normal).unwrap();
                let sections = surface.intersect_section_plane(plane, domains,
                    [ParameterRange::new(-20.0, 20.0).unwrap(); 2], t)
                    .unwrap_or_else(|error| panic!("{}, {normal:?}: {error}", surface.kind_name()));
                let IntersectionSet::Isolated(curves) = sections else {
                    assert!(matches!(sections, IntersectionSet::Coincident), "{} must intersect", surface.kind_name());
                    continue;
                };
                assert!(!curves.is_empty(), "{} must intersect", surface.kind_name());
                for intersection in curves {
                    let super::super::DomainBounds::Bounded(range) = intersection.curve.domain().bounds else { continue; };
                    for fraction in [0.0, 0.13, 0.5, 0.87, 1.0] {
                        let parameter = range.start + range.span() * fraction;
                        let point = intersection.curve.evaluate(parameter).unwrap().point;
                        let uv = intersection.first_pcurve.evaluate(parameter).unwrap().point;
                        assert!(point.distance(surface.evaluate(uv.x, uv.y).unwrap().point) < t.linear * 2.0, "{} support", surface.kind_name());
                        assert!(plane.project(point).unwrap().distance < t.linear * 2.0, "{} plane", surface.kind_name());
                    }
                }
            }
        }
    }

    #[test]
    fn rolling_ball_sections_resolve_near_contact_endpoints() {
        use super::super::{Surface3, Cylinder3, NurbsCurve3, EdgeBlendSurface3, SurfaceProjectionPcurve2};
        let t = ModelTolerance::default();
        let range = ParameterRange::new(0.0, 1.0).unwrap();
        let top = Plane3::new(Point3::new(0.0, 30.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), t).unwrap();
        let cylinder = Cylinder3::new(Point3::origin(), Vec3::unit_y(),
            Vec3::unit_x() * 20.0, -Vec3::unit_z() * 20.0, t).unwrap();
        let circle = |radius| -> Curve3 {
            let points = [(1.0, 0.0), (1.0, -1.0), (0.0, -1.0), (-1.0, -1.0),
                (-1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]
                .map(|(x, z)| Point3::new(x * radius, 28.0, z * radius)).to_vec();
            let w = std::f64::consts::FRAC_1_SQRT_2;
            NurbsCurve3::new(2, points, vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0],
                vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0]).unwrap().into()
        };
        let center = circle(18.0);
        let contact = Plane3::new(Point3::new(0.0, 28.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), t).unwrap()
            .project_curve(&center, range, t).unwrap();
        let PlaneCurveProjection::Curve(contact) = contact else { panic!("circular contact") };
        let cylinder_contact = SurfaceProjectionPcurve2::new(circle(20.0), cylinder.into(),
            Line2::new(Point2::new(0.0, 28.0), Vec2::unit_x() * std::f64::consts::TAU).unwrap().into(), range, t).unwrap();
        let blend = Surface3::RollingBall(EdgeBlendSurface3::new(center, top.into(), cylinder.into(),
            contact.curve().clone(), cylinder_contact.into(), range, 2.0, t).unwrap());
        let plane = Plane3::from_origin_normal(Point3::new(0.0, 30.0 + 1e-6, 0.0), Vec3::new(0.0, 1.0, 1e-6)).unwrap();
        let sections = blend.intersect_section_plane(plane, [range, ParameterRange::new(0.0, 1.0).unwrap()],
            [ParameterRange::new(-30.0, 30.0).unwrap(); 2], t).unwrap();
        let IntersectionSet::Isolated(curves) = sections else { panic!("a transverse contact section is required") };
        assert!(!curves.is_empty());
        for intersection in curves {
            let super::super::DomainBounds::Bounded(range) = intersection.curve.domain().bounds else { panic!("bounded section") };
            for fraction in [0.0, 1e-6, 1e-5, 0.5, 1.0 - 1e-5, 1.0 - 1e-6, 1.0] {
                let parameter = range.start + range.span() * fraction;
                let point = intersection.curve.first_order(parameter).unwrap().0;
                assert!(plane.project(point).unwrap().distance < t.linear);
            }
            intersection.second_pcurve.precise_area_contribution(range, 1e-8).unwrap();
            assert!(super::super::TrimmedCurve2::new(intersection.second_pcurve, range).unwrap()
                .self_intersections(t).unwrap().is_empty());
        }
    }

    #[test]
    fn nearly_parallel_cylinder_sections_keep_thin_caps() {
        let tolerance = ModelTolerance::default();
        let profile = super::super::Profile::new(super::super::Wire2::new(
            [super::super::TrimmedCurve2::circle(Point2::origin(), 32.27865580766479).unwrap()], tolerance).unwrap(), [], tolerance).unwrap();
        let frame = Plane3::new(Point3::origin(), Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
        let body = profile.extrude(frame, Vec3::unit_y() * 31.0).unwrap();
        for height in [-1e-6, 1e-6] {
            check_halves(&body, Point3::new(0.0, height, 0.0), Vec3::new(0.0, 1.0, 1e-6));
        }
        for degrees in [5.0_f64, 30.0, 60.0, 90.0] {
            let (sine, cosine) = degrees.to_radians().sin_cos();
            for normal in [Vec3::new(sine, cosine, 0.0), Vec3::new(0.0, cosine, sine)] {
                check_halves(&body, Point3::new(0.0, 15.5, 0.0), normal);
            }
        }
    }

    #[test]
    fn spherical_sections_keep_poles_and_near_tangent_caps() {
        let mut kernel = Kernel::new(ModelTolerance::default()).unwrap();
        let mut operation = kernel.begin_operation();
        operation.sphere(Point3::origin(), Vec3::unit_x() * 3.0,
            Vec3::unit_y() * 3.0, Vec3::unit_z() * 3.0).unwrap();
        let committed = operation.finish().unwrap();
        let sphere = Compound::from_operation(kernel, committed, []).unwrap();
        let oblique = Vec3::new(0.37, 0.63, 0.68).normalize();
        for (normal, height) in [(Vec3::unit_x(), 0.0), (Vec3::unit_z(), 0.0),
            (oblique, 0.0), (Vec3::unit_z(), -3.0), (Vec3::unit_z(), 3.0),
            (oblique, -2.99), (oblique, 2.99)] {
            check_halves(&sphere, Point3::origin() + normal * height, normal);
        }
    }
}
