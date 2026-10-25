use super::*;

// Solve contact-normal constraints with a minimum-length solution in the
// unconstrained directions. Reorthogonalization also handles repeated normals
// along straight contact edges without making the system singular.
pub(super) fn solve_blend_contacts(
    equations: &[([f64; 4], f64)],
    change_radius: bool,
    tolerance: crate::ModelTolerance,
) -> Option<[f64; 4]> {
    let mut basis = Vec::<([f64; 4], f64)>::new();
    for &(mut row, mut rhs) in equations {
        if !change_radius {
            row[3] = 0.0;
        }
        for _ in 0..2 {
            for (normal, value) in &basis {
                let projection: f64 = row.iter().zip(normal).map(|(a, b)| a * b).sum();
                for i in 0..4 {
                    row[i] -= projection * normal[i];
                }
                rhs -= projection * value;
            }
        }
        let length = row.iter().map(|value| value * value).sum::<f64>().sqrt();
        if length <= tolerance.angular * 8.0 {
            if rhs.abs() > tolerance.linear * 8.0 {
                return None;
            }
        } else {
            for value in &mut row {
                *value /= length;
            }
            basis.push((row, rhs / length));
        }
    }
    let mut result = [0.0; 4];
    for (normal, value) in basis {
        for i in 0..4 {
            result[i] += normal[i] * value;
        }
    }
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

pub(super) fn adjust_offset_blends(
    source: &Compound,
    retained: &BTreeSet<FaceId>,
    distance: f64,
    supports: &mut BTreeMap<FaceId, Surface3>,
) -> KernelResult<BTreeSet<FaceId>> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let mut replacements = Vec::new();
    for face_id in retained {
        let face = topology.face(*face_id)?;
        if matches!(face.surface(), Surface3::Plane(_)) {
            continue;
        }
        let edges = topology.face_edges(*face_id)?;
        let adjacent = edges
            .iter()
            .map(|edge| topology.edge_faces(*edge))
            .collect::<KernelResult<Vec<_>>>()?;
        if adjacent.iter().flatten().all(|id| retained.contains(id)) {
            continue;
        }
        let orientation = face.orientation().sign();
        let mut neighbors = BTreeSet::new();
        let mut contacts = Vec::new();
        for (edge_id, adjacent) in edges.into_iter().zip(adjacent) {
            let Some(&neighbor) = adjacent.iter().find(|id| *id != face_id) else {
                continue;
            };
            let samples = tangent_contact(topology, edge_id, *face_id, neighbor, tolerance)?;
            if !samples.is_empty() {
                neighbors.insert(neighbor);
                contacts.extend(samples);
            }
        }
        if neighbors.len() < 2 || neighbors.iter().all(|id| retained.contains(id)) {
            continue;
        }
        let equations = contacts
            .iter()
            .map(|(neighbor, _, normal)| {
                (
                    [normal.x, normal.y, normal.z, 1.0],
                    if retained.contains(neighbor) {
                        0.0
                    } else {
                        distance
                    },
                )
            })
            .collect::<Vec<_>>();
        // Prefer preserving the blend radius. When varying contact normals
        // prevent a translation alone, also solve for a uniform normal offset.
        let adjustment = solve_blend_contacts(&equations, false, tolerance)
            .or_else(|| solve_blend_contacts(&equations, true, tolerance))
            .ok_or_else(|| not_implemented("blend adjustment for incompatible face offsets"))?;
        let translation = crate::Vec3::new(adjustment[0], adjustment[1], adjustment[2]);
        let offset = adjustment[3];
        let surface = if offset.abs() > tolerance.linear {
            supports[face_id].offset(offset * orientation, tolerance)?
        } else {
            supports[face_id].clone()
        };
        let surface = crate::transform::transform_surface(
            surface,
            RigidTransform3::new(crate::Matrix3::identity(), translation, tolerance)?,
            tolerance,
        )?;
        // The normal equations are only a candidate construction on curved
        // supports. Check the actual moved contacts against the exact target
        // surfaces before allowing topology reconstruction to publish a solid.
        for (neighbor, point, normal) in contacts {
            let moved = point + normal * offset + translation;
            let target = supports.get(&neighbor).ok_or_else(|| not_implemented(
                "tangent blend adjustment against a collapsed offset support",
            ))?;
            let projection = target.project(moved, tolerance)?;
            let allowed = tolerance.linear_at_scale(moved.to_vec().magnitude()) * 8.0;
            let sign = topology.face(neighbor)?.orientation().sign();
            let target_normal =
                target.normal(projection.parameters.0, projection.parameters.1)? * sign;
            if projection.distance > allowed
                || (target_normal - normal).magnitude() > tolerance.angular * 8.0
            {
                return Err(not_implemented(
                    "blend adjustment that cannot preserve tangent contacts",
                ));
            }
        }
        replacements.push((*face_id, surface));
    }
    let changed = replacements.iter().map(|(id, _)| *id).collect();
    supports.extend(replacements);
    Ok(changed)
}

pub(crate) fn offset_faces(
    source: &Compound,
    faces: &[FaceHandle],
    distance: f64,
) -> KernelResult<Compound> {
    let selected = faces
        .iter()
        .map(FaceHandle::topology_id)
        .collect::<BTreeSet<_>>();
    let retained = source
        .faces()
        .filter(|face| !selected.contains(&face.topology_id()))
        .map(|face| face.topology_id())
        .collect();
    // Rebuild only the offset skin; shelling additionally adds the source skin
    // and opening rims to create a wall.
    let (result, _) = offset_body(source, &retained, distance, true, Some(crate::DerivationRole::OffsetFaces))?;
    if result.solids().len() != 1 || result.volume() <= source.tolerance().linear.powi(3) {
        return Err(invalid("face offset must preserve a nonempty solid"));
    }
    Ok(result)
}

pub(super) fn rounded_join_center(
    topology: &Topology,
    edge_id: EdgeId,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Curve3> {
    let edge = topology.edge(edge_id)?;
    let Curve3::SurfaceIntersection(intersection) = edge.curve() else { return Ok(edge.curve().clone()); };
    if edge.vertices()[0] == edge.vertices()[1] { return Ok(edge.curve().clone()); }
    let [start, end] = edge.vertices().map(|vertex| {
        let faces = topology.vertex_edges(vertex)?.into_iter().map(|edge| topology.edge_faces(edge))
            .collect::<KernelResult<Vec<_>>>()?.into_iter().flatten().collect::<BTreeSet<_>>();
        let constraints = faces.iter().map(|face| Ok(CornerConstraint::Surface(topology.face(*face)?.surface(), None)))
            .collect::<KernelResult<Vec<_>>>()?;
        // Tangential intersections amplify a model-tolerance residual into a
        // visible trim gap. Refine the shared center before constructing joins.
        let mut exact = tolerance;
        exact.linear = 1.0e-12;
        exact.relative = 1.0e-14;
        refine_support_corner(topology.vertex(vertex)?.point(), &constraints, exact)
    });
    let mut corners = [start?, end?];
    let range = edge.range();
    let mut parameters = vec![range.start, range.end];
    parameters.extend(intersection.first_pcurve().integration_breakpoints(range));
    parameters.extend(intersection.second_pcurve().integration_breakpoints(range));
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
    if range.span() < 0.0 { corners.swap(0, 1); }
    let exact = intersection.exact_pcurves();
    let guide = |curve: &Curve2, surface: &Surface3| -> KernelResult<Curve2> {
        let mut points = parameters.iter().map(|parameter| curve.evaluate(*parameter).map(|value| value.point))
            .collect::<KernelResult<Vec<_>>>()?;
        for (index, point) in [(0, corners[0]), (points.len() - 1, corners[1])] {
            let uv = surface.project(point, tolerance)?.parameters;
            points[index] = Point2::new(uv.0, uv.1);
        }
        crate::intersection::unwrap_periodic_points(&mut points, surface.domains());
        let sampled = crate::intersection::interpolating_nurbs_guide(&points, &parameters)?;
        crate::intersection::smooth_interpolating_guide(&sampled)
    };
    // Reparameterize only the retained edge interval. Its untrimmed support
    // can extend past either shared corner and has different endpoints.
    Ok(SurfaceIntersectionCurve::new(intersection.first_surface().clone(), intersection.second_surface().clone(),
        guide(&exact[0], intersection.first_surface())?, guide(&exact[1], intersection.second_surface())?, range,
        tolerance, intersection.achieved_error())?.into())
}

pub(super) fn reparameterize_join_intersection(
    intersection: crate::SurfaceSurfaceCurve,
    domains: [[ParameterRange; 2]; 2],
    tolerance: crate::ModelTolerance,
) -> KernelResult<crate::SurfaceSurfaceCurve> {
    let Curve3::SurfaceIntersection(curve) = &intersection.curve else { return Ok(intersection); };
    let range = curve.range();
    let exact = curve.exact_pcurves();
    let start = exact[0].evaluate(range.start)?.point;
    let end = exact[0].evaluate(range.end)?.point;
    if (start.y - end.y).abs() < 1.0 - tolerance.parameter * 64.0 { return Ok(intersection); }
    // A contact rail is a second intersection branch at the end of a join.
    // Continue across the join angle, rather than allowing the guide's gauge
    // to flatten onto that rail near a consumed fillet.
    let mut points = [vec![start], vec![exact[1].evaluate(range.start)?.point]];
    for index in 1..64 {
        let fraction = index as f64 / 64.0;
        let v = start.y + (end.y - start.y) * fraction;
        let mut low = range.start;
        let mut high = range.end;
        for _ in 0..32 {
            let middle = (low + high) * 0.5;
            let value = curve.first_pcurve().evaluate(middle)?.point.y;
            if (value < v) == (start.y < end.y) { low = middle; } else { high = middle; }
        }
        let parameter = (low + high) * 0.5;
        let a = curve.first_pcurve().evaluate(parameter)?.point;
        let b = curve.second_pcurve().evaluate(parameter)?.point;
        let uv = crate::intersection::correct_surface_pair_with_gauge(curve.first_surface(), curve.second_surface(),
                [a.x, v, b.x, b.y], [0.0, 1.0, 0.0, 0.0], domains[0], domains[1], tolerance)?
                .ok_or_else(|| invalid("join intersection could not continue across its contact angle"))?;
        points[0].push(Point2::new(uv[0], uv[1]));
        points[1].push(Point2::new(uv[2], uv[3]));
    }
    points[0].push(end);
    points[1].push(exact[1].evaluate(range.end)?.point);
    let rebuilt = SurfaceIntersectionCurve::new(curve.first_surface().clone(), curve.second_surface().clone(),
        crate::intersection::smooth_nurbs_guide(&points[0], range)?.into(),
        crate::intersection::smooth_nurbs_guide(&points[1], range)?.into(), range, tolerance, curve.achieved_error())?;
    let exact = rebuilt.exact_pcurves();
    Ok(crate::SurfaceSurfaceCurve { curve: rebuilt.into(), first_pcurve: exact[0].clone(),
        second_pcurve: exact[1].clone(), achieved_error: intersection.achieved_error })
}

/// Arrange offset supports and optional rolling joins when their boundary graph
/// changes. Intersections between newly adjacent faces trim overlaps after a
/// smaller blend disappears or opposing offset caps cross.
pub(super) fn offset_regions_with_joins(
    source: &Compound,
    offset: Option<&Compound>,
    joins: &[EdgeId],
    openings: &BTreeSet<FaceId>,
    radius: f64,
    sharp: bool,
) -> KernelResult<Compound> {
    use crate::trim::{FaceCurve, align_pcurve_to_face_chart, face_boundary_curves, intersection_chart_copies,
        face_parameter_bounds as trim_bounds, point_in_trim, trim_to_faces};
    use crate::brep_classification::{BrepClassifier, PointClassification};
    let tolerance = source.tolerance();
    let topology = source.topology();
    let skin = offset.unwrap_or(source);
    let skin_topology = skin.topology();
    let mut skin_faces = Vec::new();
    let mut supports = Vec::new();
    let mut orientations = Vec::new();
    let mut ancestry = Vec::new();
    let mut boundaries = Vec::new();
    for face in skin.faces() {
        let value = skin_topology.face(face.topology_id())?;
        let surface = if offset.is_some() || openings.contains(&face.topology_id()) {
            face.surface().clone()
        } else {
            let sign = value.orientation().sign();
            let bounds = face_parameter_bounds(topology, face.topology_id(), tolerance)?;
            let base = extended_support(face.surface(), bounds, -radius, tolerance)?;
            let distance = -radius * sign;
            if collapsed_support(&base, distance, tolerance)?.is_some() {
                continue;
            }
            base.offset(distance, tolerance)?
        };
        let boundary = face_boundary_curves(skin_topology, face.topology_id(), tolerance)?.into_iter().map(|mut curve| {
            let mut provenance = SegmentProvenance::from_coedge(skin_topology, curve.provenance)?;
            if let Some(offset) = offset {
                provenance.through(offset.topology_history());
            } else {
                provenance = provenance.modified();
                curve.curve = surface.boundary_curve(curve.pcurve.clone(), curve.range, tolerance)?;
            }
            Ok(curve.with_provenance(provenance))
        }).collect::<KernelResult<Vec<_>>>()?;
        ancestry.push(vec![offset.map_or(face.persistent_id(), |offset| {
            offset.topology_history().primary_source(face.persistent_id())
        })]);
        orientations.push(value.orientation());
        supports.push(surface);
        boundaries.push(boundary);
        skin_faces.push(face.clone());
    }
    let by_source = ancestry.iter().enumerate().map(|(index, names)| (names[0], index)).collect::<BTreeMap<_, _>>();
    let mut curves = boundaries.clone();
    let mut parents = Vec::new();
    let candidate = |curve: Curve3, pcurve: Curve2, range: ParameterRange, seam: bool| FaceCurve {
        provenance: SegmentProvenance::default(), curve, pcurve, range,
        loop_end: false, loop_kind: crate::LoopKind::Outer, parameter_seam: seam,
    };
    let mut clipping_boundaries = boundaries.clone();
    if offset.is_none() {
        let mut extended_faces = BTreeSet::new();
        let surviving = skin_faces.iter().map(FaceHandle::topology_id).collect::<BTreeSet<_>>();
        let mut neighbors = BTreeMap::new();
        for face in &skin_faces {
            let mut pending = vec![face.topology_id()];
            let mut visited = BTreeSet::new();
            let mut adjacent = BTreeSet::new();
            while let Some(id) = pending.pop() {
                if !visited.insert(id) { continue; }
                if id != face.topology_id() { extended_faces.insert(face.topology_id()); }
                for edge in topology.face_edges(id)? {
                    for neighbor in topology.edge_faces(edge)? {
                        if neighbor == face.topology_id() { continue; }
                        if surviving.contains(&neighbor) { adjacent.insert(neighbor); }
                        else { pending.push(neighbor); }
                    }
                }
            }
            neighbors.insert(face.topology_id(), adjacent);
        }
        for (index, face) in skin_faces.iter().enumerate() {
            if !sharp && !extended_faces.contains(&face.topology_id()) { continue; }
            let original = trim_bounds(&boundaries[index], tolerance)?;
            let mut bounds = expanded_offset_bounds(&supports[index], original, -radius, tolerance)?;
            // Surviving tangent neighbors retain their isoparametric contact
            // rails. Expand across consumed blends, while keeping these chart
            // limits so an intersection stops at the shared corner.
            for edge_id in topology.face_edges(face.topology_id())? {
                let adjacent = topology.edge_faces(edge_id)?;
                let Some(other) = adjacent.into_iter().find(|id| *id != face.topology_id() && surviving.contains(id)) else { continue; };
                if tangent_contact(topology, edge_id, face.topology_id(), other, tolerance)?.is_empty() { continue; }
                let edge = topology.edge(edge_id)?;
                let coedge = edge.coedges().iter().map(|id| topology.coedge(*id)).collect::<KernelResult<Vec<_>>>()?
                    .into_iter().find(|coedge| topology.loop_(coedge.loop_id()).unwrap().face() == face.topology_id()).unwrap();
                let range = coedge.range();
                let points = [range.start, range.start + range.span() * 0.5, range.end].map(|t| coedge.pcurve().evaluate(t).map(|v|v.point));
                let points = points.into_iter().collect::<KernelResult<Vec<_>>>()?;
                for axis in 0..2 {
                    if points.iter().all(|point| (point[axis] - original[axis].start).abs() <= tolerance.parameter * 64.0) { bounds[axis].start = original[axis].start; }
                    if points.iter().all(|point| (point[axis] - original[axis].end).abs() <= tolerance.parameter * 64.0) { bounds[axis].end = original[axis].end; }
                }
            }
            let points = [Point2::new(bounds[0].start, bounds[1].start),
                Point2::new(bounds[0].end, bounds[1].start), Point2::new(bounds[0].end, bounds[1].end),
                Point2::new(bounds[0].start, bounds[1].end)];
            clipping_boundaries[index] = (0..4).map(|side| {
                let pcurve: Curve2 = crate::Line2::new(points[side], points[(side + 1) % 4] - points[side])?.into();
                let range = ParameterRange::new(0.0, 1.0)?;
                Ok(candidate(supports[index].boundary_curve(pcurve.clone(), range, tolerance)?, pcurve, range, false))
            }).collect::<KernelResult<Vec<_>>>()?;
            clipping_boundaries[index].last_mut().unwrap().loop_end = true;
        }
        for first in 0..supports.len() {
            for second in 0..first {
                let first_id = skin_faces[first].topology_id();
                let second_id = skin_faces[second].topology_id();
                let shared = topology.face_edges(first_id)?.into_iter().find(|edge| {
                    topology.edge_faces(*edge).unwrap().contains(&second_id)
                });
                if !sharp && !neighbors[&first_id].contains(&second_id)
                    && !extended_faces.contains(&first_id) && !extended_faces.contains(&second_id) { continue; }
                if let Some(edge) = shared {
                    if !sharp && (joins.contains(&edge) || !tangent_contact(topology, edge, first_id, second_id, tolerance)?.is_empty()) { continue; }
                }
                let intersections = supports[first].intersect_surface_in_bounds(&supports[second],
                    trim_bounds(&clipping_boundaries[first], tolerance)?, trim_bounds(&clipping_boundaries[second], tolerance)?, tolerance)?;
                let IntersectionSet::Isolated(intersections) = intersections else { continue; };
                for intersection in intersections {
                    let first_pcurve = align_pcurve_to_face_chart(&intersection.first_pcurve, None,
                        &supports[first], &boundaries[first], tolerance)?;
                    let second_pcurve = align_pcurve_to_face_chart(&intersection.second_pcurve, None,
                        &supports[second], &boundaries[second], tolerance)?;
                    let first_copies = intersection_chart_copies(intersection.curve.domain().bounds, &first_pcurve,
                        &supports[first], trim_bounds(&boundaries[first], tolerance)?, tolerance)?;
                    let second_copies = intersection_chart_copies(intersection.curve.domain().bounds, &second_pcurve,
                        &supports[second], trim_bounds(&boundaries[second], tolerance)?, tolerance)?;
                    for first_pcurve in &first_copies {
                        for second_pcurve in &second_copies {
                            for range in trim_to_faces(&intersection.curve, first_pcurve, &clipping_boundaries[first],
                                second_pcurve, &clipping_boundaries[second], tolerance)? {
                                curves[first].push(candidate(intersection.curve.clone(), first_pcurve.clone(), range, false));
                                curves[second].push(candidate(intersection.curve.clone(), second_pcurve.clone(), range, false));
                            }
                        }
                    }
                }
            }
        }
    }
    for edge_id in joins.iter().filter(|_| !sharp) {
        let edge = topology.edge(*edge_id)?;
        let uses = edge.coedges().iter().map(|id| topology.coedge(*id)).collect::<KernelResult<Vec<_>>>()?;
        let faces = topology.edge_faces(*edge_id)?;
        let parent_names = faces.iter().map(|id| topology.face(*id).map(|face| face.name())).collect::<KernelResult<Vec<_>>>()?;
        // Contact guides must follow the coedge order used by the center curve.
        let parent_indices = uses.iter().map(|coedge| {
            Ok(by_source.get(&topology.face(topology.loop_(coedge.loop_id())?.face())?.name()).copied())
        }).collect::<KernelResult<Vec<_>>>()?;
        let contact_surfaces = uses.iter().zip(&parent_indices).map(|(coedge, index)| {
            if let Some(index) = index { return Ok(supports[*index].clone()); }
            let face = topology.face(topology.loop_(coedge.loop_id())?.face())?;
            let sign = face.orientation().sign();
            // A consumed face still defines the contact direction of its
            // adjacent concave-edge envelope. Keep that continuation solely
            // as a rail support; it cannot contribute an offset face.
            crate::OffsetSurface3::new(face.surface().clone(), -radius * sign, tolerance).map(Surface3::Offset)
        }).collect::<KernelResult<Vec<_>>>()?;
        let range = edge.range();
        let closed = edge.vertices()[0] == edge.vertices()[1];
        let center = if offset.is_none() {
            rounded_join_center(topology, *edge_id, tolerance)?
        } else { edge.curve().clone() };
        let contact_guides = uses.iter().map(|coedge| {
            if offset.is_some() { return Ok(coedge.pcurve().clone()); }
            let face = topology.face(topology.loop_(coedge.loop_id())?.face())?;
            Ok(crate::SurfaceProjectionPcurve2::new(center.clone(), face.surface().clone(),
                coedge.pcurve().clone(), range, tolerance)?.into())
        }).collect::<KernelResult<Vec<Curve2>>>()?;
        let blend: Surface3 = crate::RollingBallSurface3::new(
            center.clone(), contact_surfaces[0].clone(), contact_surfaces[1].clone(),
            contact_guides[0].clone(), contact_guides[1].clone(), range, radius, tolerance,
        )?.with_center_period(closed.then_some(range.span().abs())).into();
        let mut boundary = Vec::new();
        for (start, end, trim, seam) in [
            (Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), range, false),
            (Point2::new(range.end, 0.0), Point2::new(range.end, 1.0), ParameterRange::new(0.0, 1.0)?, closed),
            (Point2::new(0.0, 1.0), Point2::new(1.0, 1.0), range.reversed(), false),
            (Point2::new(range.start, 0.0), Point2::new(range.start, 1.0), ParameterRange::new(1.0, 0.0)?, closed),
        ] {
            let pcurve: Curve2 = crate::Line2::new(start, end - start)?.into();
            let curve = blend.boundary_curve(pcurve.clone(), trim, tolerance)?;
            boundary.push(candidate(curve, pcurve, trim, seam));
        }
        boundary.last_mut().unwrap().loop_end = true;
        if offset.is_none() {
            for (side, coedge) in uses.iter().enumerate() {
                if parent_indices[side].is_some() { continue; }
                let face_id = topology.loop_(coedge.loop_id())?.face();
                let face = topology.face(face_id)?;
                let sign = face.orientation().sign();
                if !matches!(collapsed_support(face.surface(), -radius * sign, tolerance)?, Some((_, false))) { continue; }
                // At exact collapse the join's constructive contact rail lies
                // on the surviving tangent support. Share that rail directly;
                // projecting a different curve onto the singular join corner
                // gives an unstable p-curve even when the points agree.
                for contact in tangent_neighbors(topology, face_id, tolerance, |_| true)? {
                    let Some(&parent) = by_source.get(&topology.face(contact)?.name()) else { continue; };
                    let rail = &boundary[if side == 0 { 0 } else { 2 }];
                    let points = (0..=16).map(|index| {
                        let point = rail.curve.evaluate(range.start + range.span() * index as f64 / 16.0)?.point;
                        let uv = supports[parent].project(point, tolerance)?.parameters;
                        Ok(Point2::new(uv.0, uv.1))
                    }).collect::<KernelResult<Vec<_>>>()?;
                    let guide = crate::intersection::smooth_nurbs_guide(&points, range)?.into();
                    let pcurve = SurfaceProjectionPcurve2::new(rail.curve.clone(), supports[parent].clone(), guide, range, tolerance)?.into();
                    let pcurve = align_pcurve_to_face_chart(&pcurve, Some(range), &supports[parent], &boundaries[parent], tolerance)?;
                    curves[parent].insert(0, candidate(rail.curve.clone(), pcurve, range, false));
                }
            }
        }
        if offset.is_some() {
            for side in 0..2 {
                let parent = parent_indices[side].unwrap();
                let rail = &boundary[if side == 0 { 0 } else { 2 }];
                let pcurve = align_pcurve_to_face_chart(&contact_guides[side], Some(range),
                    &supports[parent], &boundaries[parent], tolerance)?;
                curves[parent].push(candidate(rail.curve.clone(), pcurve, range, false));
            }
        }
        parents.push(parent_indices.into_iter().flatten().collect::<Vec<_>>());
        supports.push(blend);
        ancestry.push(parent_names);
        curves.push(boundary.clone());
        boundaries.push(boundary);
    }
    let base_count = skin_faces.len();
    for first in base_count..supports.len() {
        for second in 0..first {
            if parents[first - base_count].contains(&second) { continue; }
            let intersections = supports[first].intersect_surface_in_bounds(&supports[second],
                trim_bounds(&boundaries[first], tolerance)?, trim_bounds(&boundaries[second], tolerance)?, tolerance)?;
            let IntersectionSet::Isolated(intersections) = intersections else { continue; };
            for intersection in intersections {
                let intersection = reparameterize_join_intersection(intersection,
                    [trim_bounds(&boundaries[first], tolerance)?, trim_bounds(&boundaries[second], tolerance)?], tolerance)?;
                let first_pcurve = align_pcurve_to_face_chart(&intersection.first_pcurve, None,
                    &supports[first], &boundaries[first], tolerance)?;
                let second_pcurve = align_pcurve_to_face_chart(&intersection.second_pcurve, None,
                    &supports[second], &boundaries[second], tolerance)?;
                // The join can extend beyond the old offset trim. Clip to its
                // own chart here; the source-distance classifier replaces the
                // competing support's old boundary during arrangement.
                for range in trim_to_faces(&intersection.curve, &first_pcurve, &boundaries[first],
                    &first_pcurve, &boundaries[first], tolerance)? {
                    curves[first].push(candidate(intersection.curve.clone(), first_pcurve.clone(), range, false));
                    curves[second].push(candidate(intersection.curve.clone(), second_pcurve.clone(), range, false));
                }
            }
        }
    }
    let original_boundaries = source.faces().map(|face| {
        let boundary = face_boundary_curves(topology, face.topology_id(), tolerance)?;
        let bounds = trim_bounds(&boundary, tolerance)?;
        Ok((face.persistent_id(), (face.surface(), boundary, bounds)))
    }).collect::<KernelResult<BTreeMap<_, _>>>()?;
    let opening_names = openings.iter().map(|id| topology.face(*id).map(|face| face.name()))
        .collect::<KernelResult<BTreeSet<_>>>()?;
    let in_source_chart = |uv: Point2, surface: &Surface3, bounds: [ParameterRange; 2]| {
        nearest_uv(surface.domains(), uv, box_center(bounds))
    };
    let classifier = BrepClassifier::for_compound(source)?;
    let contact_cache = std::cell::RefCell::new(BTreeMap::new());
    // Keep the radius-distance envelope of the trimmed source boundary.
    // Extended supports alone include pieces hidden behind another source face.
    let distance_to_source = |point: Point3| -> KernelResult<f64> {
        let mut distance = f64::INFINITY;
        for (name, (surface, boundary, bounds)) in &original_boundaries {
            if opening_names.contains(name) { continue; }
            let projection = surface.project(point, tolerance)?;
            let uv = in_source_chart(Point2::new(projection.parameters.0, projection.parameters.1), surface, *bounds);
            if point_in_trim(uv, boundary)? {
                distance = distance.min(point.distance(surface.evaluate(uv.x, uv.y)?.point));
            }
        }
        for (edge_id, edge) in topology.edges() {
            let projection = edge.curve().project_on_trim(point, edge.range(), tolerance)?;
            let edge_distance = if sharp && joins.contains(&edge_id) {
                // A sharp concave crease uses the larger signed depth of its
                // contact supports inside their common wedge. Include the
                // corner contacts when the nearest point reaches an end.
                let mut faces = topology.edge_faces(edge_id)?.into_iter().collect::<BTreeSet<_>>();
                if edge.vertices()[0] != edge.vertices()[1] {
                    let closest = edge.curve().evaluate(projection.parameter)?.point;
                    for vertex in edge.vertices() {
                        if closest.distance(topology.vertex(vertex)?.point()) <= tolerance.linear_at_scale(closest.to_vec().magnitude()) * 16.0 {
                            faces.extend(topology.vertex_faces(vertex)?);
                        }
                    }
                }
                // The same incident-face set recurs at many probes. Resolve
                // its surviving tangent contacts only on the first visit.
                let mut cache = contact_cache.borrow_mut();
                if !cache.contains_key(&faces) {
                    let mut pending = faces.iter().copied().collect::<Vec<_>>();
                    let mut visited = BTreeSet::new();
                    let mut contacts = BTreeSet::new();
                    while let Some(id) = pending.pop() {
                        if !visited.insert(id) { continue; }
                        let face = topology.face(id)?;
                        let sign = face.orientation().sign();
                        if collapsed_support(face.surface(), -radius * sign, tolerance)?.is_none() {
                            contacts.insert(id);
                            continue;
                        }
                        pending.extend(tangent_neighbors(topology, id, tolerance, |_| true)?);
                    }
                    cache.insert(faces.clone(), contacts);
                }
                let depths = cache[&faces].iter().map(|id| {
                    let face = topology.face(*id)?;
                    let face_projection = face.surface().project(point, tolerance)?;
                    let bounds = original_boundaries[&face.name()].2;
                    let mut uv = in_source_chart(Point2::new(face_projection.parameters.0, face_projection.parameters.1), face.surface(), bounds);
                    // An ended crease cannot constrain a distant angular
                    // sector of a curved support. Beyond that sector use the
                    // corner's contact normal, rather than another periodic
                    // branch of the untrimmed surface.
                    if (0..2).any(|axis| face.surface().domains()[axis].period.is_some()
                        && !bounds[axis].contains(uv[axis], tolerance)) {
                        let closest = edge.curve().evaluate(projection.parameter)?.point;
                        let contact = face.surface().project(closest, tolerance)?.parameters;
                        uv = Point2::new(contact.0, contact.1);
                    }
                    let value = face.surface().evaluate(uv.x, uv.y)?;
                    let sign = face.orientation().sign();
                    Ok((value.point - point).dot(value.normal()? * sign))
                }).collect::<KernelResult<Vec<_>>>()?;
                if depths.iter().any(|depth| *depth < -tolerance.linear) {
                    projection.distance
                } else { depths.into_iter().fold(f64::NEG_INFINITY, f64::max) }
            } else {
                projection.distance
            };
            distance = distance.min(edge_distance);
        }
        Ok(distance)
    };
    let mut patches = Vec::new();
    for index in 0..supports.len() {
        let surface = &supports[index];
        let bounds = trim_bounds(&boundaries[index], tolerance)?;
        let uv_probe_limit = bounds[0].span().abs().min(bounds[1].span().abs()) * 0.05;
        let mut regions = crate::boolean::arrange_surface_regions(surface, &curves[index],
            radius * 1.0e-3, uv_probe_limit, tolerance, |uv| {
                if index >= base_count && !point_in_trim(uv, &boundaries[index])? { return Ok(None); }
                if index < base_count && !sharp {
                    let (_, original, bounds) = &original_boundaries[&ancestry[index][0]];
                    let original_uv = in_source_chart(uv, surface, *bounds);
                    // On tangent supports a distance probe alone cannot
                    // distinguish the two sides of the contact rail.
                    if !point_in_trim(original_uv, original)? { return Ok(None); }
                }
                let point = surface.evaluate(uv.x, uv.y)?.point;
                if classifier.classify(point)? == PointClassification::Outside {
                    return Ok(None);
                }
                let distance = distance_to_source(point)?;
                let is_opening = index < base_count && opening_names.contains(&ancestry[index][0]);
                let retained = if is_opening {
                    point_in_trim(uv, &boundaries[index])? && distance >= radius - tolerance.linear * 8.0
                } else {
                    // These are points on exact offset supports. A model-sized
                    // band admits both sides of a narrow overlap; allow only
                    // evaluation roundoff on the side hidden by another face.
                    distance >= radius - f64::EPSILON * point.to_vec().magnitude().max(radius) * 2048.0
                        && distance <= radius + tolerance.linear * 8.0
                };
                if !retained { return Ok(None); }
                let orientation = if index >= base_count {
                    let Surface3::RollingBall(blend) = surface else { unreachable!() };
                    let center = blend.center_curve().evaluate(uv.x)?.point;
                    let normal = surface.normal(uv.x, uv.y)?;
                    // Continuing across consumed blends can reverse the join's
                    // chart normal along the source edge. Orient each retained
                    // region toward its center, rather than using one sample
                    // for the entire support.
                    if normal.dot(point - center) < 0.0 { FaceOrientation::Forward }
                    else { FaceOrientation::Reversed }
                } else { orientations[index] };
                Ok(Some(orientation))
            })?;
        for region in &mut regions { region.sources = ancestry[index].clone(); }
        patches.extend(regions);
    }
    if sharp { regularize_coincident_offset_faces(&mut patches, -radius, tolerance)?; }
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    transaction.brep_shell(&patches)?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [])
}

pub(crate) fn shell(
    source: &Compound,
    openings: &[FaceHandle],
    distance: f64,
    join: ShellJoin,
) -> KernelResult<Compound> {
    if !distance.is_finite() || distance.abs() <= source.tolerance().linear {
        return Err(invalid(
            "shell thickness must be finite and exceed model tolerance",
        ));
    }
    let opening_ids = openings
        .iter()
        .map(FaceHandle::topology_id)
        .collect::<BTreeSet<_>>();

    // A rolling ball of radius |distance| between the diverging offset
    // supports has its center on their original edge. Only convex edges of
    // the offset skin need these joins; converging offsets remain intersected.
    // Exclude the unchanged source skin and the opening rims.
    let opening_names = openings
        .iter()
        .map(FaceHandle::persistent_id)
        .collect::<BTreeSet<_>>();
    // Classify the original edge, not an offset intersection: numerical
    // support reconstruction must not turn a tangent seam into a new join.
    let topology = source.topology();
    let mut divergent = BTreeSet::new();
    let mut divergent_edges = Vec::new();
    for (edge_id, edge) in topology.edges() {
        if edge.coedges().len() != 2 {
            continue;
        }
        let uses = edge
            .coedges()
            .iter()
            .map(|id| topology.coedge(*id))
            .collect::<KernelResult<Vec<_>>>()?;
        let faces = uses
            .iter()
            .map(|coedge| topology.face(topology.loop_(coedge.loop_id())?.face()))
            .collect::<KernelResult<Vec<_>>>()?;
        if faces
            .iter()
            .any(|face| opening_names.contains(&face.name()))
        {
            continue;
        }
        let parameter = (edge.range().start + edge.range().end) * 0.5;
        let sample = edge.curve().evaluate(parameter)?;
        let tangent = sample.first_derivative.normalize() * uses[0].range().span().signum();
        let normals = faces
            .iter()
            .map(|face| face.oriented_normal_near(sample.point, source.tolerance()))
            .collect::<KernelResult<Vec<_>>>()?;
        if tangent.dot(normals[0].cross(normals[1])) * distance.signum()
            > source.tolerance().angular
        {
            let mut pair = [faces[0].name(), faces[1].name()];
            pair.sort();
            divergent.insert(pair);
            divergent_edges.push(edge_id);
        }
    }
    let closed_joins = divergent_edges.iter().try_fold(true, |closed, id| {
        let edge = topology.edge(*id)?;
        Ok::<_, KernelError>(closed && edge.vertices()[0] == edge.vertices()[1])
    })?;
    let curved_open_join = divergent_edges.iter().any(|id| {
        let edge = topology.edge(*id).unwrap();
        edge.vertices()[0] != edge.vertices()[1] && matches!(edge.curve(), Curve3::SurfaceIntersection(_))
    });
    if join == ShellJoin::Sharp {
        let mut disjoint_contacts = false;
        let mut crossed_caps = false;
        if distance < 0.0 && curved_open_join {
            let caps = source.faces().filter(|face| !opening_ids.contains(&face.topology_id()))
                .filter_map(|face| match face.surface() {
                    Surface3::Plane(plane) => Some((face.topology_id(), plane)),
                    _ => None,
                }).collect::<Vec<_>>();
            for first in 0..caps.len() {
                let sign = topology.face(caps[first].0)?.orientation().sign();
                let normal = caps[first].1.normal() * sign;
                for second in 0..first {
                    let other_sign = topology.face(caps[second].0)?.orientation().sign();
                    if normal.dot(caps[second].1.normal() * other_sign) > -1.0 + source.tolerance().angular { continue; }
                    let gap = (caps[second].1.origin() - caps[first].1.origin()).dot(normal);
                    if gap < -source.tolerance().linear && gap - 2.0 * distance > source.tolerance().linear {
                        crossed_caps = true;
                    }
                }
            }
            for face in source.faces() {
                let id = face.topology_id();
                let value = topology.face(id)?;
                let sign = value.orientation().sign();
                if collapsed_support(face.surface(), distance * sign, source.tolerance())?.is_none() { continue; }
                let mut contacts = Vec::new();
                for neighbor in tangent_neighbors(topology, id, source.tolerance(), |_| true)? {
                    let face = topology.face(neighbor)?;
                    if !matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_)) { continue; }
                    let sign = face.orientation().sign();
                    if collapsed_support(face.surface(), distance * sign, source.tolerance())?.is_some() { continue; }
                    contacts.push(face.surface().offset(distance * sign, source.tolerance())?);
                }
                for first in 0..contacts.len() {
                    for second in 0..first {
                        if matches!(contacts[first].intersect_surface(&contacts[second], source.tolerance())?, IntersectionSet::Empty) {
                            disjoint_contacts = true;
                        }
                    }
                }
            }
        }
        // Reusing the original edge graph cannot represent a wall that has
        // disappeared, or caps that have passed each other. Arrange the sharp
        // support envelope directly at those topology transitions.
        let raw_offset = if disjoint_contacts || crossed_caps {
            offset_regions_with_joins(source, None, &divergent_edges, &opening_ids, distance.abs(), true)?
        } else { offset_body(source, &opening_ids, distance, false, None)?.0 };
        return assemble_shell_boundary(source, &raw_offset, &opening_ids, distance);
    }
    if distance < 0.0 && curved_open_join {
        let rounded = offset_regions_with_joins(source, None, &divergent_edges, &opening_ids, distance.abs(), false)?;
        return assemble_shell_boundary(source, &rounded, &opening_ids, distance);
    }
    let (raw_offset, has_vanished_blends) = offset_body(source, &opening_ids, distance, false, None)?;
    if distance < 0.0 && has_vanished_blends && closed_joins
        && !divergent_edges.is_empty()
    {
        let rounded = offset_regions_with_joins(source, Some(&raw_offset), &divergent_edges, &opening_ids, distance.abs(), false)?;
        return assemble_shell_boundary(source, &rounded, &opening_ids, distance);
    }
    let shell = assemble_shell_boundary(source, &raw_offset, &opening_ids, distance)?;
    let offset_surfaces = raw_offset
        .faces()
        .filter(|face| {
            !opening_names.contains(
                &raw_offset.topology_history().primary_source(face.persistent_id()),
            )
        })
        .map(|face| face.surface().clone())
        .collect::<Vec<_>>();
    let topology = shell.topology();
    let mut joins = Vec::new();
    for handle in shell.edges() {
        let Some(crate::EntityId::Edge(edge_id)) = topology.entity(handle.persistent_id()) else {
            return Err(invalid("shell join edge is missing from its topology"));
        };
        let faces = topology.edge_faces(edge_id)?;
        if faces.len() != 2 {
            continue;
        }
        let faces = [topology.face(faces[0])?, topology.face(faces[1])?];
        if faces
            .iter()
            .any(|face| !offset_surfaces.contains(face.surface()))
        {
            continue;
        }
        let mut pair = [
            shell.topology_history().primary_source(faces[0].name()),
            shell.topology_history().primary_source(faces[1].name()),
        ];
        pair.sort();
        if divergent.contains(&pair) {
            joins.push(handle.clone());
        }
    }
    if joins.is_empty() {
        return Ok(shell);
    }
    shell.fillet_with_options(
        &joins,
        crate::FilletOptions::new(distance.abs()).with_tangent_propagation(false),
    ).map(|result| result.with_construction_roles_from(&[&shell], &[]))
}
