use super::*;

pub(super) fn offset_body(
    source: &Compound,
    openings: &BTreeSet<FaceId>,
    distance: f64,
    adjust_blends: bool,
) -> KernelResult<(Compound, bool)> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let source_faces = source
        .faces()
        .map(|face| face.topology_id())
        .collect::<BTreeSet<_>>();
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    let mut collapsed = BTreeMap::new();
    let mut vanished_blends = BTreeSet::new();
    for face_id in &source_faces {
        let face = topology.face(*face_id)?;
        let face_bounds = face_parameter_bounds(topology, *face_id, tolerance)?;
        let base = extended_support(face.surface(), face_bounds, distance, tolerance)?;
        let surface = if openings.contains(face_id) {
            base
        } else {
            let orientation = match face.orientation() {
                FaceOrientation::Forward => 1.0,
                FaceOrientation::Reversed => -1.0,
            };
            if let Some((support, past_radius)) = collapsed_support(&base, distance * orientation, tolerance)? {
                collapsed.insert(*face_id, support);
                if past_radius {
                    vanished_blends.insert(*face_id);
                }
                continue;
            }
            base.offset(distance * orientation, tolerance)?
        };
        let expanded = expanded_offset_bounds(&surface, face_bounds, distance, tolerance)?;
        supports.insert(*face_id, surface);
        bounds.insert(*face_id, expanded);
    }

    let has_vanished_blends = !vanished_blends.is_empty();
    let mut corner_blends = Vec::new();
    for face_id in vanished_blends {
        let neighbors = tangent_neighbors(topology, face_id, tolerance, |face| supports.contains_key(&face))?
            .into_iter().collect::<Vec<_>>();
        let (first, second) = match neighbors.as_slice() {
            [first, second] => (first, second),
            [face] => {
                corner_blends.push((face_id, *face));
                continue;
            },
            _ => return Err(not_implemented("removal of a blend without surviving contact supports")),
        };
        let CollapsedSupport::Curve(guide) = &collapsed[&face_id] else {
            unreachable!()
        };
        let mut period = None;
        for edge_id in topology.face_edges(face_id)? {
            let edge = topology.edge(edge_id)?;
            if edge.vertices()[0] == edge.vertices()[1]
                && topology.edge_faces(edge_id)?.len() == 2 {
                period = Some(edge.range().span().abs());
                break;
            }
        }
        let range = match guide.domain().bounds {
            crate::DomainBounds::Bounded(range) => range,
            _ => match topology.face(face_id)?.surface() {
                Surface3::Cylinder(_) => face_parameter_bounds(topology, face_id, tolerance)?[1],
                Surface3::RollingBall(blend) => blend.center_range(),
                _ => return Err(not_implemented("unbounded collapsed blend guide")),
            },
        };
        let curve = intersect_collapsed_blend(
            &supports[first], &supports[second], guide, range, period, tolerance,
        )?;
        collapsed.insert(face_id, CollapsedSupport::Curve(curve));
    }
    for (face_id, contact) in corner_blends {
        let range = face_parameter_bounds(topology, face_id, tolerance)?;
        let point = topology.face(face_id)?.surface().evaluate(
            (range[0].start + range[0].end) * 0.5, (range[1].start + range[1].end) * 0.5,
        )?.point;
        let mut best: Option<(f64, &Curve3, f64, (f64, f64))> = None;
        for edge in topology.face_edges(face_id)? {
            for neighbor in topology.edge_faces(edge)? {
                if neighbor == face_id { continue; }
                let Some(CollapsedSupport::Curve(curve)) = collapsed.get(&neighbor) else { continue; };
                let projection = curve.project(point, tolerance)?;
                let value = curve.evaluate(projection.parameter)?;
                let uv = supports[&contact].project(value.point, tolerance)?.parameters;
                let transverse = value.first_derivative.normalize()
                    .dot(supports[&contact].normal(uv.0, uv.1)?).abs();
                if transverse > tolerance.angular && best.as_ref().is_none_or(|candidate| transverse > candidate.0) {
                    best = Some((transverse, curve, projection.parameter, uv));
                }
            }
        }
        let (_, guide, parameter, uv) = best
            .ok_or_else(|| not_implemented("collapsed blend corner without a transverse surviving rail"))?;
        let point = guide.intersect_surface_near(&supports[&contact], parameter, uv, tolerance)?.point;
        collapsed.insert(face_id, CollapsedSupport::Point(point));
    }

    let mut changed_supports = source_faces
        .difference(openings)
        .copied()
        .collect::<BTreeSet<_>>();
    if adjust_blends {
        changed_supports.extend(adjust_offset_blends(
            source,
            openings,
            distance,
            &mut supports,
        )?);
    }
    let patches = loop {
        let patches = support_patches(
            topology,
            tolerance,
            SupportEdit {
                supports: supports.clone(),
                bounds: bounds.clone(),
                changed: changed_supports.clone(),
                collapsed: collapsed.clone(),
                ..Default::default()
            },
        )?;
        let mut removed = BTreeSet::new();
        for patch in patches.values().flatten() {
            if offset_patch_collapsed(patch, tolerance)? {
                for face_id in supports.keys() {
                    if patch.sources.contains(&topology.face(*face_id)?.name()) {
                        removed.insert(*face_id);
                    }
                }
            }
        }
        if removed.is_empty() {
            break patches;
        }
        // A thin offset face can reverse after its two boundary rails cross.
        // Replace the entire strip by the intersection of the surviving supports.
        for face_id in &removed {
            supports.remove(face_id);
            bounds.remove(face_id);
        }
        let previous_collapsed = collapsed.clone();
        for face_id in removed {
            let edges = topology.face_edges(face_id)?;
            let neighbors = edges
                .iter()
                .map(|edge| topology.edge_faces(*edge))
                .collect::<KernelResult<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<BTreeSet<_>>();
            let mut surviving = neighbors
                .iter()
                .filter(|face| supports.contains_key(face))
                .copied()
                .collect::<Vec<_>>();
            if surviving.len() > 2 && matches!(topology.face(face_id)?.surface(), Surface3::Cone(_)) {
                // A chamfer's end faces clip its angular sector; its two
                // constant-height rails identify the supports that replace
                // the strip when those rails cross.
                let mut rails = BTreeSet::new();
                for edge in &edges {
                    let coedge = topology.edge(*edge)?.coedges().iter().map(|id| topology.coedge(*id))
                        .collect::<KernelResult<Vec<_>>>()?.into_iter().find(|coedge|
                            topology.loop_(coedge.loop_id()).unwrap().face() == face_id).unwrap();
                    let range = coedge.range();
                    let heights = [range.start, (range.start + range.end) * 0.5, range.end]
                        .map(|t| coedge.pcurve().evaluate(t).map(|value| value.point.y));
                    let heights = heights.into_iter().collect::<KernelResult<Vec<_>>>()?;
                    if heights.iter().all(|height| (height - heights[0]).abs() <= tolerance.parameter * 64.0) {
                        rails.extend(topology.edge_faces(*edge)?.into_iter().filter(|face| supports.contains_key(face)));
                    }
                }
                surviving = rails.into_iter().collect();
            }
            let support = match surviving.as_slice() {
                [first, second] => {
                    let edge_id = edges
                        .iter()
                        .find(|edge| {
                            topology.edge_faces(**edge).is_ok_and(|faces| faces.contains(first))
                        })
                        .unwrap();
                    let edge = topology.edge(*edge_id)?;
                    CollapsedSupport::Curve(intersect_collapsed_blend(
                        &supports[first],
                        &supports[second],
                        edge.curve(),
                        edge.range(),
                        None,
                        tolerance,
                    )?)
                }
                [face] => {
                    let guide = neighbors
                        .iter()
                        .find_map(|neighbor| match previous_collapsed.get(neighbor) {
                            Some(CollapsedSupport::Curve(curve)) => Some(curve),
                            _ => None,
                        })
                        .ok_or_else(|| not_implemented("collapsed offset face without a surviving rail"))?;
                    let range = face_parameter_bounds(topology, face_id, tolerance)?;
                    let source_point = topology.face(face_id)?
                        .surface()
                        .evaluate(
                            (range[0].start + range[0].end) * 0.5,
                            (range[1].start + range[1].end) * 0.5,
                        )?
                        .point;
                    let parameter = guide.project(source_point, tolerance)?.parameter;
                    let uv = supports[face]
                        .project(guide.evaluate(parameter)?.point, tolerance)?
                        .parameters;
                    CollapsedSupport::Point(
                        guide
                            .intersect_surface_near(&supports[face], parameter, uv, tolerance)?
                            .point,
                    )
                }
                _ => {
                    return Err(not_implemented(
                        "removal of an offset face without two surviving supports",
                    ));
                }
            };
            collapsed.insert(face_id, support);
        }
    };
    let mut patches = patches;
    for shell in patches.values_mut() {
        regularize_coincident_offset_faces(shell, distance, tolerance)?;
    }
    let offset = commit_support_patches(source, patches, changed_supports)?;
    Ok((offset, has_vanished_blends))
}

pub(super) fn regularize_coincident_offset_faces(
    patches: &mut Vec<BrepPatch>,
    distance: f64,
    tolerance: crate::ModelTolerance,
) -> KernelResult<()> {
    // Move untouched geometry directly to the result. Only planar patches
    // need grouping, with one representative comparison per existing group.
    let mut groups: Vec<Vec<BrepPatch>> = Vec::new();
    let mut result = Vec::new();
    for patch in std::mem::take(patches) {
        if !matches!(patch.surface, Surface3::Plane(_)) {
            result.push(patch);
            continue;
        }
        let mut group = None;
        for (candidate, members) in groups.iter().enumerate() {
            if crate::trim::coincident_supports(&patch.surface, &members[0].surface, tolerance)? {
                group = Some(candidate);
                break;
            }
        }
        if let Some(group) = group { groups[group].push(patch); }
        else { groups.push(vec![patch]); }
    }
    let boundary = |patch: &BrepPatch, surface: &Surface3| {
        patch.loops.iter().flat_map(|loop_| loop_.segments.iter().enumerate().map(|(index, segment)| {
            let pcurve = if &patch.surface == surface { segment.pcurve.clone() } else {
                SurfaceProjectionPcurve2::new(segment.curve.clone(), surface.clone(),
                    segment.pcurve.clone(), segment.range, tolerance)?.into()
            };
            Ok(crate::trim::FaceCurve {
                provenance: segment.provenance.clone(), curve: segment.curve.clone(),
                pcurve, range: segment.range,
                loop_end: index + 1 == loop_.segments.len(), loop_kind: loop_.kind,
                parameter_seam: false,
            })
        })).collect::<KernelResult<Vec<_>>>()
    };
    for group in groups {
        let patch = &group[0];
        let normal = patch.surface.normal(0.0, 0.0)?;
        let orientations = group.iter().map(|other| {
            Ok(if normal.dot(other.surface.normal(0.0, 0.0)?) < 0.0 {
                other.orientation.reversed()
            } else { other.orientation })
        }).collect::<KernelResult<Vec<_>>>()?;
        if orientations.iter().all(|orientation| *orientation == orientations[0]) {
            result.extend(group);
            continue;
        }
        // Opposite offset caps can meet when a wall reaches zero thickness.
        // Cancel their overlap while retaining the surrounding cap regions.
        let boundaries = group.iter().map(|other| boundary(other, &patch.surface))
            .collect::<KernelResult<Vec<_>>>()?;
        let curves = boundaries.iter().flatten().cloned().collect::<Vec<_>>();
        let bounds = crate::trim::face_parameter_bounds(&curves, tolerance)?;
        let mut regions = crate::boolean::arrange_surface_regions(&patch.surface, &curves,
            distance.abs() * 1.0e-3, bounds[0].span().abs().min(bounds[1].span().abs()) * 0.05,
            tolerance, |uv| {
                let mut winding = 0;
                for (orientation, boundary) in orientations.iter().zip(&boundaries) {
                    if crate::trim::point_in_trim(uv, boundary)? {
                        winding += if *orientation == FaceOrientation::Forward { 1 } else { -1 };
                    }
                }
                Ok(match winding.cmp(&0) {
                    std::cmp::Ordering::Greater => Some(FaceOrientation::Forward),
                    std::cmp::Ordering::Less => Some(FaceOrientation::Reversed),
                    std::cmp::Ordering::Equal => None,
                })
            })?;
        for region in &mut regions {
            region.sources = group.iter().zip(&orientations).filter(|(_, orientation)| **orientation == region.orientation)
                .flat_map(|(patch, _)| patch.sources.iter().copied()).collect();
        }
        result.extend(regions);
    }
    *patches = result;
    Ok(())
}

#[cfg(test)]
mod offset_cap_tests {
    use super::*;

    #[test]
    fn cancel_caps_in_different_plane_charts() {
        let tolerance = crate::ModelTolerance::default();
        let rectangle = |plane: Plane3, left: f64, orientation| {
            let mut points = [Point3::new(left, 0.0, 0.0), Point3::new(left + 2.0, 0.0, 0.0),
                Point3::new(left + 2.0, 1.0, 0.0), Point3::new(left, 1.0, 0.0)];
            if plane.normal().z < 0.0 { points.reverse(); }
            let segments = (0..4).map(|index| {
                let a = points[index];
                let b = points[(index + 1) % 4];
                let uv = |point| {
                    let (u, v) = plane.project(point).unwrap().parameters;
                    Point2::new(u, v)
                };
                BrepPatchSegment {
                    curve: Line3::new(a, b - a).unwrap().into(),
                    pcurve: crate::Line2::new(uv(a), uv(b) - uv(a)).unwrap().into(),
                    range: ParameterRange::new(0.0, 1.0).unwrap(),
                    provenance: SegmentProvenance::default(),
                }
            }).collect();
            BrepPatch {
                surface: plane.into(), orientation, sources: Vec::new(), preserved: Vec::new(),
                loops: vec![BrepPatchLoop { kind: crate::LoopKind::Outer, segments }],
            }
        };
        for plane in [
            Plane3::xy(),
            Plane3::new(Point3::new(11.0, -7.0, 0.0), crate::Vec3::unit_y() * 2.0,
                -crate::Vec3::unit_x() * 3.0, tolerance).unwrap(),
            Plane3::new(Point3::new(11.0, -7.0, 0.0), crate::Vec3::unit_y() * 2.0,
                crate::Vec3::unit_x() * 3.0, tolerance).unwrap(),
        ] {
            let orientation = if plane.normal().z > 0.0 { FaceOrientation::Reversed }
                else { FaceOrientation::Forward };
            let mut patches = vec![rectangle(Plane3::xy(), 0.0, FaceOrientation::Forward),
                rectangle(plane, 1.0, orientation)];
            regularize_coincident_offset_faces(&mut patches, -1.0, tolerance).unwrap();
            assert_eq!(patches.len(), 2);
            for patch in patches {
                assert_eq!(patch.surface, Surface3::Plane(Plane3::xy()));
                let segments = &patch.loops[0].segments;
                let area = segments.iter().map(|segment| segment.pcurve
                    .signed_area_contribution(segment.range, tolerance.parameter).unwrap()).sum::<f64>();
                assert!((area.abs() - 1.0).abs() < tolerance.linear);
                let left = if patch.orientation == FaceOrientation::Forward { 0.0 } else { 2.0 };
                for segment in segments {
                    for t in [segment.range.start, segment.range.end] {
                        let x = segment.curve.evaluate(t).unwrap().point.x;
                        assert!(x >= left - tolerance.linear && x <= left + 1.0 + tolerance.linear);
                    }
                }
            }
        }
    }
}

pub(super) fn offset_patch_collapsed(
    patch: &BrepPatch,
    tolerance: crate::ModelTolerance,
) -> KernelResult<bool> {
    let sign = patch.orientation.sign();
    let domains = patch.surface.domains();
    for loop_ in &patch.loops {
        if loop_.kind != crate::LoopKind::Outer {
            continue;
        }
        let mut area = 0.0;
        let mut principal_area = 0.0;
        let mut previous_end: Option<Point2> = None;
        for segment in &loop_.segments {
            let start = segment.pcurve.evaluate(segment.range.start)?.point;
            let end = segment.pcurve.evaluate(segment.range.end)?.point;
            let mut shift = crate::Vec2::new(0.0, 0.0);
            if let Some(previous) = previous_end {
                for axis in 0..2 {
                    if let Some(period) = domains[axis].period {
                        let delta = previous[axis] - start[axis];
                        let candidate = (delta / period).round() * period;
                        if (delta - candidate).abs() <= tolerance.parameter * 64.0 {
                            shift[axis] = candidate;
                        }
                    }
                }
            }
            let contribution = segment
                .pcurve
                .signed_area_contribution(segment.range, tolerance.parameter)?;
            principal_area += contribution;
            area += contribution + 0.5 * (shift.x * (end.y - start.y) - shift.y * (end.x - start.x));
            previous_end = Some(end + shift);
        }
        if principal_area * sign <= tolerance.parameter.powi(2)
            && area * sign <= tolerance.parameter.powi(2)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Clone)]
pub(crate) enum CollapsedSupport {
    Curve(Curve3),
    Point(Point3),
}

pub(super) fn collapsed_support(
    surface: &Surface3,
    distance: f64,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Option<(CollapsedSupport, bool)>> {
    let zero = |radius: f64| radius.abs() <= tolerance.linear;
    Ok(match surface {
        Surface3::Cylinder(cylinder) => {
            let sign = cylinder
                .axis()
                .dot(cylinder.x_radius().cross(cylinder.y_radius()))
                .signum();
            let radius = cylinder.radius() + distance * sign;
            if radius <= tolerance.linear {
                Some((CollapsedSupport::Curve(
                    Line3::new(cylinder.origin(), cylinder.axis())?.into(),
                ), radius < -tolerance.linear))
            } else {
                None
            }
        }
        Surface3::Sphere(sphere) if zero(sphere.radius() + distance) => {
            Some((CollapsedSupport::Point(sphere.center()), false))
        }
        Surface3::Torus(torus) if torus.minor_radius() + distance <= tolerance.linear => {
            Some((CollapsedSupport::Curve(
                crate::Circle3::new(torus.center(), torus.x_major(), torus.y_major(), tolerance)?
                    .into(),
            ), torus.minor_radius() + distance < -tolerance.linear))
        }
        Surface3::RollingBall(blend) => {
            let range = blend.center_range();
            let parameter = (range.start + range.end) * 0.5;
            let value = surface.evaluate(parameter, 0.5)?;
            let radial = value.point - blend.center_curve().evaluate(parameter)?.point;
            let sign = value.normal()?.dot(radial).signum();
            let radius = blend.radius() + distance * sign;
            (radius <= tolerance.linear)
                .then(|| (CollapsedSupport::Curve(blend.center_curve().clone()), radius < -tolerance.linear))
        }
        _ => None,
    })
}

pub(super) fn intersect_collapsed_blend(
    first: &Surface3,
    second: &Surface3,
    guide: &Curve3,
    mut range: ParameterRange,
    period: Option<f64>,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Curve3> {
    let intersections = first.intersect_surface(second, tolerance).or_else(|error| {
        if !error.diagnostics().iter().all(|d| d.kind == DiagnosticKind::UnsupportedGeometry) {
            return Err(error);
        }
        // The removed blend identifies the local intersection branch of its
        // surviving offset supports. Keep exact surface intersection evaluation;
        // the old center curve supplies only seeds for the parameter guides.
        let mut points = [Vec::new(), Vec::new()];
        let center = (range.start + range.end) * 0.5;
        if let Some(period) = period {
            range = ParameterRange::new(center - 1.5 * period, center + 1.5 * period)?;
        }
        let indices = if period.is_some() { -1..=97 } else { 0..=96 };
        for index in indices {
            let parameter = range.start + range.span() * index as f64 / 96.0;
            let parameter = period.map_or(parameter, |period| {
                center - period * 0.5 + (parameter - center + period * 0.5).rem_euclid(period)
            });
            let point = guide.evaluate(parameter)?.point;
            let a = first.project(point, tolerance)?.parameters;
            let b = second.project(point, tolerance)?.parameters;
            let uv = crate::intersection::correct_surface_pair_near(
                first, second, [a.0, a.1, b.0, b.1], tolerance,
            )?;
            points[0].push(Point2::new(uv[0], uv[1]));
            points[1].push(Point2::new(uv[2], uv[3]));
        }
        for (points, surface) in points.iter_mut().zip([first, second]) {
            crate::intersection::unwrap_periodic_points(points, surface.domains());
        }
        let pcurve = |points: &[Point2]| {
            if period.is_some() {
                crate::intersection::periodic_nurbs_guide(points, range)
            } else {
                crate::intersection::smooth_nurbs_guide(points, range)
            }
        };
        let curve = SurfaceIntersectionCurve::new(
            first.clone(), second.clone(),
            pcurve(&points[0])?.into(), pcurve(&points[1])?.into(),
            range, tolerance, tolerance.linear,
        )?;
        let exact = curve.exact_pcurves();
        Ok(IntersectionSet::Isolated(vec![crate::SurfaceSurfaceCurve {
            curve: curve.into(), first_pcurve: exact[0].clone(), second_pcurve: exact[1].clone(),
            achieved_error: tolerance.linear,
        }]))
    })?;
    let IntersectionSet::Isolated(candidates) = intersections else {
        return Err(invalid("surviving supports of a removed blend have no distinct intersection"));
    };
    let reference = guide.evaluate((range.start + range.end) * 0.5)?.point;
    candidates.into_iter().map(|candidate| {
        candidate.curve.project(reference, tolerance).map(|projection| (projection.distance, candidate.curve))
    }).collect::<KernelResult<Vec<_>>>()?.into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, curve)| curve)
        .ok_or_else(|| invalid("surviving supports of a removed blend do not intersect"))
}

pub(super) fn collapsed_offset_vertex(
    topology: &Topology,
    vertex: VertexId,
    edges: &BTreeMap<EdgeId, OffsetEdge>,
    supports: &BTreeMap<FaceId, Surface3>,
    collapsed: &BTreeMap<FaceId, CollapsedSupport>,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Point3> {
    let incident = topology.vertex_edges(vertex)?;
    let faces = incident.iter().copied()
        .map(|edge| topology.edge_faces(edge))
        .collect::<KernelResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    let mut support_faces = faces.iter().filter(|face| supports.contains_key(face)).copied().collect::<BTreeSet<_>>();
    let mut constraints = Vec::new();
    for face in &faces {
        let Some(collapsed) = collapsed.get(face) else { continue; };
        let contacts = tangent_neighbors(topology, *face, tolerance, |face| supports.contains_key(&face))?;
        match collapsed {
            CollapsedSupport::Curve(curve) if contacts.is_empty() => constraints.push(CornerConstraint::Curve(curve)),
            CollapsedSupport::Point(center) => constraints.push(CornerConstraint::Point(*center)),
            _ => {},
        }
        support_faces.extend(contacts);
    }
    constraints.extend(support_faces.iter().map(|face| CornerConstraint::Surface(&supports[face], None)));
    // Surviving parameter seams still constrain the vertex after the blend
    // disappears. A nearest point on the new boundary alone can slide away
    // from a neighboring face's seam and leave its loop disconnected.
    let mut contact_edges = BTreeSet::new();
    for face in &support_faces {
        for edge_id in topology.face_edges(*face)? {
            let faces = topology.edge_faces(edge_id)?;
            if faces.len() == 2 && faces.iter().all(|face| support_faces.contains(face))
                && !tangent_contact(topology, edge_id, faces[0], faces[1], tolerance)?.is_empty() {
                contact_edges.insert(edge_id);
            }
        }
    }
    for edge_id in incident {
        let faces = topology.edge_faces(edge_id)?;
        if faces.len() == 1 && !collapsed.contains_key(&faces[0]) {
            constraints.push(CornerConstraint::Curve(&edges[&edge_id].curve));
        }
    }
    constraints.extend(contact_edges.iter().map(|edge| CornerConstraint::Curve(&edges[edge].curve)));
    // At tangent supports, start on the replacement rail rather than correcting
    // from the old corner through a singular surface intersection.
    let mut seed = topology.vertex(vertex)?.point();
    if let Some(support) = faces.iter().find_map(|face| collapsed.get(face)) {
        seed = match support {
            CollapsedSupport::Curve(curve) => curve.project(seed, tolerance)?.point,
            CollapsedSupport::Point(point) => *point,
        };
    }
    refine_support_corner(seed, &constraints, tolerance)
}

/// A body's faces with edited supports, from which support reconstruction
/// rebuilds every affected boundary.
#[derive(Default)]
pub(crate) struct SupportEdit {
    /// The (possibly new) support of every face.
    pub supports: BTreeMap<FaceId, Surface3>,
    /// The parameter box each rebuilt face trim may occupy.
    pub bounds: BTreeMap<FaceId, [ParameterRange; 2]>,
    /// Faces whose supports were edited.
    pub changed: BTreeSet<FaceId>,
    /// Keep every face's orientation instead of deriving it from its support.
    pub preserve_orientation: bool,
    /// Recompute every edge from its supports, retaining none unchanged.
    pub reconstruct_edges: bool,
    /// Faces whose parameterization changed although their support did not.
    pub reparameterized: BTreeSet<FaceId>,
    /// Edited supports that collapse to a curve or a point.
    pub collapsed: BTreeMap<FaceId, CollapsedSupport>,
}
