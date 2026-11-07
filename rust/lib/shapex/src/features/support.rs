use super::*;

pub(super) fn rebuild_supports(
    source: &Compound,
    edit: SupportEdit,
    role: crate::DerivationRole,
) -> KernelResult<Compound> {
    let changed = edit.changed.clone();
    let patches_by_shell = support_patches(source.topology(), source.tolerance(), edit)?;
    commit_support_patches(source, patches_by_shell, changed, Some(role))
}

/// Publish rebuilt shells, attributed to `source` under `role` when given.
pub(super) fn commit_support_patches(
    source: &Compound,
    patches_by_shell: BTreeMap<crate::ShellId, Vec<BrepPatch>>,
    changed_supports: BTreeSet<FaceId>,
    role: Option<crate::DerivationRole>,
) -> KernelResult<Compound> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    let mut affected_faces = changed_supports.clone();
    for face in changed_supports {
        for edge in topology.face_edges(face)? {
            affected_faces.extend(topology.edge_faces(edge)?);
        }
    }
    let affected = affected_faces.into_iter().map(|face| topology.face(face).map(|value| value.name()))
        .collect::<KernelResult<BTreeSet<_>>>()?;
    let mut bodies = BTreeMap::new();
    for (shell, patches) in patches_by_shell {
        bodies.insert(shell, transaction.brep_shell(&patches)?);
    }
    transaction.restore_brep_shells(topology, &bodies, &affected)?;
    let committed = transaction.finish()?;
    match role {
        Some(role) => Compound::from_operation_with_parents(kernel, committed, &[source], role),
        None => Compound::from_operation(kernel, committed, []),
    }
}

pub(crate) fn support_patches(
    topology: &Topology,
    tolerance: crate::ModelTolerance,
    edit: SupportEdit,
) -> KernelResult<BTreeMap<crate::ShellId, Vec<BrepPatch>>> {
    let SupportEdit {
        supports,
        bounds,
        changed: changed_supports,
        preserve_orientation,
        reconstruct_edges,
        reparameterized,
        collapsed,
    } = edit;
    let (reparameterized, collapsed) = (&reparameterized, &collapsed);
    let source_faces = supports.keys().copied().collect::<BTreeSet<_>>();
    let mut source_edges = BTreeSet::new();
    for face_id in &source_faces {
        source_edges.extend(topology.face_edges(*face_id)?);
    }
    let mut edges = BTreeMap::new();
    for edge_id in source_edges {
        let faces = topology.edge_faces(edge_id)?;
        if let Some(support) = faces.iter().find_map(|face| collapsed.get(face)) {
            if let CollapsedSupport::Curve(curve) = support {
                edges.insert(
                    edge_id,
                    OffsetEdge {
                        curve: curve.clone(),
                        pcurves: BTreeMap::new(),
                        range: topology.edge(edge_id)?.range(),
                    },
                );
            }
            continue;
        }
        edges.insert(
            edge_id,
            offset_edge(
                topology,
                edge_id,
                &supports,
                &bounds,
                tolerance,
                !reconstruct_edges,
                reparameterized,
            )?,
        );
    }

    let mut required_vertices = BTreeSet::new();
    for (edge_id, edge) in &edges {
        let source_edge = topology.edge(*edge_id)?;
        if source_edge.vertices()[0] != source_edge.vertices()[1]
            || edge.curve.domain().period.is_none()
        {
            required_vertices.extend(source_edge.vertices());
        }
    }
    let mut vertices = BTreeMap::new();
    let mut split_vertices = BTreeMap::<(EdgeId, VertexId), Point3>::new();
    let mut corner_edges = BTreeMap::<(VertexId, FaceId), Curve3>::new();
    let incident_edges = topology.vertex_edge_index();
    for vertex in required_vertices {
        if incident_edges[&vertex].iter().copied().any(|edge| {
            topology
                .edge_faces(edge)
                .is_ok_and(|faces| faces.iter().any(|face| collapsed.contains_key(face)))
        }) {
            vertices.insert(
                vertex,
                collapsed_offset_vertex(topology, vertex, &edges, &supports, collapsed, tolerance)?,
            );
            continue;
        }
        match offset_vertex(topology, vertex, &incident_edges[&vertex], &edges, &supports, reparameterized, tolerance) {
            Ok(point) => {
                vertices.insert(vertex, point);
            }
            Err(error) => {
                // A four-face corner need not survive an offset as one vertex.
                // Try the two diagonals of its face fan, retaining the split
                // whose triple intersections lie inside the remaining faces.
                let incident = incident_edges[&vertex].clone();
                let faces = incident
                    .iter()
                    .map(|edge| topology.edge_faces(*edge))
                    .collect::<KernelResult<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<BTreeSet<_>>();
                if incident.len() != 4 || faces.len() != 4 {
                    return Err(error);
                }
                let source_point = topology.vertex(vertex)?.point();
                let mut solution = None;
                for first in &faces {
                    for second in
                        faces.range((std::ops::Bound::Excluded(first), std::ops::Bound::Unbounded))
                    {
                        if incident.iter().any(|edge| {
                            topology
                                .edge_faces(*edge)
                                .is_ok_and(|f| f.contains(first) && f.contains(second))
                        }) {
                            continue;
                        }
                        let others = faces
                            .iter()
                            .filter(|f| *f != first && *f != second)
                            .copied()
                            .collect::<Vec<_>>();
                        let mut ends = Vec::new();
                        for other in &others {
                            let subset = BTreeSet::from([*first, *second, *other]);
                            let uses = incident
                                .iter()
                                .filter(|edge| {
                                    topology
                                        .edge_faces(**edge)
                                        .is_ok_and(|f| f.iter().all(|face| subset.contains(face)))
                                })
                                .copied()
                                .collect::<Vec<_>>();
                            if let Ok(point) = refine_offset_corner(
                                topology,
                                vertex,
                                source_point,
                                &subset,
                                &uses,
                                &edges,
                                &supports,
                                tolerance,
                            ).or_else(|error| {
                                // The old corner can be on the wrong side of
                                // a nearly tangent wall. Seed the same triple
                                // from its retained pairwise intersections.
                                for id in &uses {
                                    let edge_faces = topology.edge_faces(*id)?;
                                    let Some(third) = subset.iter().find(|face| !edge_faces.contains(face)) else { continue; };
                                    let surface = &supports[third];
                                    let curve = &edges[id].curve;
                                    let (Ok(rail), Ok(uv)) = (curve.project(source_point, tolerance),
                                        surface.project(source_point, tolerance)) else { continue; };
                                    if let Ok(hit) = curve.intersect_surface_near(surface, rail.parameter, uv.parameters, tolerance) {
                                        if let Ok(point) = refine_offset_corner(
                                            topology, vertex, hit.point, &subset, &uses, &edges, &supports, tolerance,
                                        ) {
                                            return Ok(point);
                                        }
                                    }
                                }
                                Err(error)
                            }) {
                                ends.push((point, uses));
                            }
                        }
                        if ends.len() != 2 {
                            continue;
                        }
                        let mut inside = true;
                        for side in 0..2 {
                            let excluded = others[1 - side];
                            let projection =
                                supports[&excluded].project(ends[side].0, tolerance)?;
                            let uv = projection.parameters;
                            let trim = contact_sector(topology, excluded, tolerance)?;
                            let domains = supports[&excluded].domains();
                            // Angular supports constrain only their retained
                            // sector. Their continuation is not a global
                            // half-space beyond that sector.
                            if [uv.0, uv.1].into_iter().enumerate().any(|(axis, value)| {
                                let value = if let Some(period) = domains[axis].period {
                                    value
                                        + (((trim[axis].start + trim[axis].end) * 0.5 - value)
                                            / period)
                                            .round()
                                            * period
                                } else if axis == 1
                                    && matches!(
                                        supports[&excluded],
                                        Surface3::RollingBall(_) | Surface3::Blend(_) | Surface3::Ruled(_)
                                    )
                                {
                                    value
                                } else {
                                    return false;
                                };
                                !trim[axis].contains(value, tolerance)
                            }) {
                                continue;
                            }
                            let sign = topology.face(excluded)?.orientation().sign();
                            if (ends[side].0 - projection.point)
                                .dot(supports[&excluded].normal(uv.0, uv.1)?)
                                * sign
                                > tolerance.linear
                            {
                                inside = false;
                            }
                        }
                        if !inside {
                            continue;
                        }
                        solution = Some((*first, *second, ends));
                        break;
                    }
                    if solution.is_some() {
                        break;
                    }
                }
                let Some((first, second, ends)) = solution else {
                    return Err(error);
                };
                let mut guides = [Vec::new(), Vec::new()];
                for index in 0..=8 {
                    let point = ends[0].0 + (ends[1].0 - ends[0].0) * (index as f64 / 8.0);
                    let a = supports[&first].project(point, tolerance)?.parameters;
                    let b = supports[&second].project(point, tolerance)?.parameters;
                    let corrected = crate::intersection::correct_surface_pair_near(
                        &supports[&first],
                        &supports[&second],
                        [a.0, a.1, b.0, b.1],
                        tolerance,
                    )?;
                    guides[0].push(Point2::new(corrected[0], corrected[1]));
                    guides[1].push(Point2::new(corrected[2], corrected[3]));
                }
                let range = ParameterRange::new(0.0, 1.0)?;
                for (side, face) in [first, second].into_iter().enumerate() {
                    crate::intersection::unwrap_periodic_points(
                        &mut guides[side],
                        supports[&face].domains(),
                    );
                }
                let curve: Curve3 = SurfaceIntersectionCurve::new(
                    supports[&first].clone(),
                    supports[&second].clone(),
                    crate::intersection::smooth_nurbs_guide(&guides[0], range)?.into(),
                    crate::intersection::smooth_nurbs_guide(&guides[1], range)?.into(),
                    range,
                    tolerance,
                    tolerance.linear,
                )?
                .into();
                corner_edges.insert((vertex, first), curve.clone());
                corner_edges.insert((vertex, second), curve);
                for (point, uses) in ends {
                    for edge in uses {
                        split_vertices.insert((edge, vertex), point);
                    }
                }
            }
        }
    }
    if !collapsed.is_empty() {
        edges.retain(|id, _| {
            let [first, second] = topology.edge(*id).unwrap().vertices();
            first == second || vertices[&first].distance(vertices[&second]) > tolerance.linear
        });
    }
    for (edge_id, edge) in &mut edges {
        let endpoints: BTreeMap<_, _> = topology
            .edge(*edge_id)?
            .vertices()
            .into_iter()
            .filter_map(|vertex| {
                split_vertices
                    .get(&(*edge_id, vertex))
                    .or_else(|| vertices.get(&vertex))
                    .map(|point| (vertex, *point))
            })
            .collect();
        let faces = topology.edge_faces(*edge_id)?;
        if let [face] = faces.as_slice() {
            let original = topology.edge(*edge_id)?;
            if endpoints.len() == 2 && endpoints.values().any(|point| {
                edge.curve.project(*point, tolerance).is_ok_and(|p| p.distance > tolerance.linear)
            }) {
                let range = original.range();
                let mut uv = original.vertices().map(|vertex| {
                    supports[face].project(endpoints[&vertex], tolerance).map(|p| Point2::new(p.parameters.0, p.parameters.1))
                }).into_iter().collect::<KernelResult<Vec<_>>>()?;
                crate::intersection::unwrap_periodic_points(&mut uv, supports[face].domains());
                let direction = (uv[1] - uv[0]) / range.span();
                let pcurve: Curve2 = crate::Line2::new(uv[0] - direction * range.start, direction)?.into();
                edge.curve = supports[face].boundary_curve(pcurve.clone(), range, tolerance)?;
                for id in original.coedges() {
                    let old_uv = topology.coedge(*id)?.pcurve().evaluate(range.start)?.point;
                    let shift = whole_periods(supports[face].domains(), old_uv - uv[0]);
                    edge.pcurves.insert(*id, crate::Line2::new(uv[0] + shift - direction * range.start, direction)?.into());
                }
            }
        }
        retarget_offset_intersection_endpoints(
            topology, *edge_id, edge, &endpoints, &supports, tolerance,
        )?;
        edge.range = trim_offset_edge(topology, *edge_id, edge, &endpoints, tolerance)?;
        if edge.pcurves.is_empty() {
            for id in topology.edge(*edge_id)?.coedges() {
                let face = topology.loop_(topology.coedge(*id)?.loop_id())?.face();
                let Some(surface) = supports.get(&face) else {
                    continue;
                };
                let mut points = (0..=16)
                    .map(|i| {
                        let point = edge
                            .curve
                            .evaluate(edge.range.start + edge.range.span() * i as f64 / 16.0)?
                            .point;
                        let uv = surface.project(point, tolerance)?.parameters;
                        Ok(Point2::new(uv.0, uv.1))
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                crate::intersection::unwrap_periodic_points(&mut points, surface.domains());
                let guide = crate::intersection::smooth_nurbs_guide(&points, edge.range)?.into();
                let pcurve = SurfaceProjectionPcurve2::new(
                    edge.curve.clone(),
                    surface.clone(),
                    guide,
                    edge.range,
                    tolerance,
                )?
                .into();
                edge.pcurves.insert(*id, pcurve);
            }
        }
    }

    let mut patches_by_shell = BTreeMap::<crate::ShellId, Vec<BrepPatch>>::new();
    let mut changed_vertices = BTreeSet::new();
    for id in &changed_supports {
        for edge in topology.face_edges(*id)? {
            changed_vertices.extend(topology.edge(edge)?.vertices());
        }
    }
    for face_id in source_faces {
        let face = topology.face(face_id)?;
        let surface = &supports[&face_id];
        let mut face_retained = !changed_supports.contains(&face_id);
        let mut loops = Vec::new();
        for loop_id in face.loops() {
            let loop_value = topology.loop_(*loop_id)?;
            let mut segments = Vec::new();
            for coedge_id in topology.loop_coedges(*loop_id)? {
                let coedge = topology.coedge(coedge_id)?;
                let Some(edge) = edges.get(&coedge.edge()) else {
                    continue;
                };
                let range = if coedge.is_forward() {
                    edge.range
                } else {
                    edge.range.reversed()
                };
                let pcurve = projected_pcurve(
                    &edge.curve,
                    surface,
                    &edge.pcurves[&coedge_id],
                    range,
                    tolerance,
                )?;
                let endpoints = topology.coedge_vertices(coedge_id)?;
                let affected = endpoints
                    .iter()
                    .any(|vertex| changed_vertices.contains(vertex))
                    || topology
                        .edge_faces(coedge.edge())?
                        .iter()
                        .any(|face| changed_supports.contains(face));
                face_retained &= !affected;
                let original = SegmentProvenance::from_coedge(topology, coedge_id)?;
                let mut provenance = if affected {
                    original.modified()
                } else {
                    original.clone()
                };
                for (end, endpoint) in endpoints.iter().enumerate() {
                    if !changed_vertices.contains(endpoint) {
                        provenance.vertices[end] = original.vertices[end].clone();
                    }
                }
                let mut parameters = vec![range.start, range.end];
                for neighbor in topology.edge_faces(coedge.edge())? {
                    if !collapsed.contains_key(&neighbor) {
                        continue;
                    }
                    // Both rails of a collapsed blend become one boundary.
                    // Retain the seams from both neighboring faces by splitting
                    // that boundary at every surviving rail vertex.
                    for rail in topology.face_edges(neighbor)? {
                        for vertex in topology.edge(rail)?.vertices() {
                            if let Some(point) = vertices.get(&vertex) {
                                let projection = edge.curve.project_on_trim(*point, range, tolerance)?;
                                if projection.distance <= tolerance.linear * 8.0 {
                                    parameters.push(projection.parameter);
                                }
                            }
                        }
                    }
                }
                parameters.sort_by(|a, b| ((a - range.start) / range.span())
                    .total_cmp(&((b - range.start) / range.span())));
                parameters.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
                for pair in parameters.windows(2) {
                    let retained = ParameterRange::new(pair[0], pair[1])?;
                    let start = edge.curve.evaluate(retained.start)?.point;
                    let end = edge.curve.evaluate(retained.end)?.point;
                    let middle = edge.curve.evaluate((retained.start + retained.end) * 0.5)?.point;
                    if start.distance(end).max(start.distance(middle)) <= tolerance.linear {
                        continue;
                    }
                    segments.push(BrepPatchSegment {
                        provenance: provenance.trimmed(range, retained),
                        curve: edge.curve.clone(),
                        pcurve: pcurve.clone(),
                        range: retained,
                    });
                }
                let source_edge = topology.edge(coedge.edge())?;
                let vertex = source_edge.vertices()[if coedge.is_forward() { 1 } else { 0 }];
                if let Some(curve) = corner_edges.get(&(vertex, face_id)) {
                    let next = topology.coedge(coedge.next())?;
                    let start = split_vertices[&(coedge.edge(), vertex)];
                    let end = split_vertices[&(next.edge(), vertex)];
                    let range = ParameterRange::new(
                        curve.project(start, tolerance)?.parameter,
                        curve.project(end, tolerance)?.parameter,
                    )?;
                    let Curve3::SurfaceIntersection(intersection) = curve else {
                        unreachable!()
                    };
                    let exact = intersection.exact_pcurves();
                    let pcurve = if intersection.first_surface() == surface {
                        exact[0].clone()
                    } else {
                        exact[1].clone()
                    };
                    segments.push(BrepPatchSegment {
                        provenance: Default::default(),
                        curve: curve.clone(),
                        pcurve,
                        range,
                    });
                }
            }
            loops.push(BrepPatchLoop {
                kind: loop_value.kind(),
                segments,
            });
        }
        patches_by_shell
            .entry(face.shell())
            .or_default()
            .push(BrepPatch {
                preserved: if face_retained {
                    vec![face.name()]
                } else {
                    Vec::new()
                },
                sources: vec![face.name()],
                surface: surface.clone(),
                orientation: if preserve_orientation {
                    face.orientation()
                } else {
                    let sample = topology.face_edges(face_id)?[0];
                    let edge = topology.edge(sample)?;
                    let point = edge
                        .curve()
                        .evaluate((edge.range().start + edge.range().end) * 0.5)?
                        .point;
                    let old_uv = face.surface().project(point, tolerance)?.parameters;
                    let new_uv = surface.project(point, tolerance)?.parameters;
                    let same = face
                        .surface()
                        .normal(old_uv.0, old_uv.1)?
                        .dot(surface.normal(new_uv.0, new_uv.1)?)
                        >= 0.0;
                    if same {
                        face.orientation()
                    } else {
                        face.orientation().reversed()
                    }
                },
                loops,
            });
    }

    Ok(patches_by_shell)
}

pub(crate) fn face_patch(
    topology: &Topology,
    face_id: FaceId,
    reverse: bool,
) -> KernelResult<BrepPatch> {
    let face = topology.face(face_id)?;
    let mut patch = BrepPatch {
        preserved: vec![face.name()],
        sources: vec![face.name()],
        surface: face.surface().clone(),
        orientation: face.orientation(),
        loops: face
            .loops()
            .iter()
            .map(|loop_id| BrepPatchLoop::from_loop(topology, *loop_id))
            .collect::<KernelResult<_>>()?,
    };
    if reverse {
        patch.preserved.clear();
        patch.reverse();
    }
    Ok(patch)
}

pub(super) fn loop_patch(
    topology: &Topology,
    loop_id: crate::LoopId,
    output_kind: crate::LoopKind,
) -> KernelResult<BrepPatchLoop> {
    let mut patch = BrepPatchLoop::from_loop(topology, loop_id)?;
    if patch.kind != output_kind {
        patch.kind = output_kind;
        patch.reverse();
    }
    Ok(patch)
}

pub(super) fn rim_segment_points(segment: &BrepPatchSegment) -> KernelResult<[Point3; 3]> {
    Ok([
        segment.curve.evaluate(segment.range.start)?.point,
        segment
            .curve
            .evaluate(segment.range.start + segment.range.span() * 0.5)?
            .point,
        segment.curve.evaluate(segment.range.end)?.point,
    ])
}

pub(super) fn split_rim_segments(
    segments: impl IntoIterator<Item = BrepPatchSegment>,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Vec<BrepPatchSegment>> {
    let segments = segments.into_iter().collect::<Vec<_>>();
    let endpoints = segments
        .iter()
        .map(rim_segment_points)
        .collect::<KernelResult<Vec<_>>>()?;
    let linear = tolerance.linear * 16.0;
    let mut result = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let mut parameters = vec![segment.range.start, segment.range.end];
        for (other_index, points) in endpoints.iter().enumerate() {
            if other_index == index {
                continue;
            }
            for point in [points[0], points[2]] {
                let Ok(projection) = segment.curve.project(point, tolerance) else {
                    continue;
                };
                if projection.distance > linear {
                    continue;
                }
                let parameter = segment
                    .curve
                    .domain()
                    .nearest(projection.parameter, segment.range.middle());
                if segment.range.contains(parameter, tolerance) {
                    parameters.push(parameter);
                }
            }
        }
        parameters.sort_by(|first, second| {
            if segment.range.span() > 0.0 {
                first.total_cmp(second)
            } else {
                second.total_cmp(first)
            }
        });
        parameters.dedup_by(|first, second| (*first - *second).abs() <= tolerance.parameter * 16.0);
        for bounds in parameters.windows(2) {
            if (bounds[1] - bounds[0]).abs() <= tolerance.parameter * 16.0 {
                continue;
            }
            result.push(BrepPatchSegment {
                provenance: segment
                    .provenance
                    .trimmed(segment.range, ParameterRange::new(bounds[0], bounds[1])?),
                curve: segment.curve.clone(),
                pcurve: segment.pcurve.clone(),
                range: ParameterRange::new(bounds[0], bounds[1])?,
            });
        }
    }
    Ok(result)
}

pub(super) fn rim_piece_matches(
    first: &BrepPatchSegment,
    second: &BrepPatchSegment,
    tolerance: crate::ModelTolerance,
) -> KernelResult<bool> {
    let first = rim_segment_points(first)?;
    let second = rim_segment_points(second)?;
    let linear = tolerance.linear * 64.0;
    Ok(first[1].distance(second[1]) <= linear
        && ((first[0].distance(second[0]) <= linear && first[2].distance(second[2]) <= linear)
            || (first[0].distance(second[2]) <= linear && first[2].distance(second[0]) <= linear)))
}

pub(super) fn rim_face_patch(
    surface: &Surface3,
    orientation: FaceOrientation,
    outer: BrepPatchLoop,
    inner: BrepPatchLoop,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Option<BrepPatch>> {
    let original = [outer.clone(), inner.clone()];
    let pieces = split_rim_segments(outer.segments.into_iter().chain(inner.segments), tolerance)?;
    let mut cancelled = vec![false; pieces.len()];
    let mut removed = false;
    for first in 0..pieces.len() {
        if cancelled[first] {
            continue;
        }
        let first_points = rim_segment_points(&pieces[first])?;
        for second in first + 1..pieces.len() {
            if cancelled[second] || !rim_piece_matches(&pieces[first], &pieces[second], tolerance)?
            {
                continue;
            }
            let second_points = rim_segment_points(&pieces[second])?;
            if first_points[0].distance(second_points[2]) <= tolerance.linear * 64.0
                && first_points[2].distance(second_points[0]) <= tolerance.linear * 64.0
            {
                cancelled[first] = true;
                cancelled[second] = true;
                removed = true;
                break;
            }
        }
    }
    if !removed {
        return Ok(Some(BrepPatch {
            preserved: Vec::new(),
            sources: Vec::new(),
            surface: surface.clone(),
            orientation,
            loops: original.into(),
        }));
    }

    let mut unused = pieces
        .into_iter()
        .enumerate()
        .filter_map(|(index, segment)| (!cancelled[index]).then_some(segment))
        .collect::<Vec<_>>();
    if unused.is_empty() {
        return Ok(None);
    }
    let linear = tolerance.linear * 64.0;
    let mut loops = Vec::new();
    while let Some(first) = unused.pop() {
        let start = rim_segment_points(&first)?[0];
        let mut end = rim_segment_points(&first)?[2];
        let mut segments = vec![first];
        while end.distance(start) > linear {
            let Some((index, reverse)) =
                unused.iter().enumerate().find_map(|(index, candidate)| {
                    let points = rim_segment_points(candidate).ok()?;
                    if end.distance(points[0]) <= linear {
                        Some((index, false))
                    } else if end.distance(points[2]) <= linear {
                        Some((index, true))
                    } else {
                        None
                    }
                })
            else {
                return Err(topology_error("an analytical shell opening rim did not form a closed boundary"));
            };
            let mut next = unused.swap_remove(index);
            if reverse {
                next.range = next.range.reversed();
                next.provenance = next.provenance.reversed();
            }
            end = rim_segment_points(&next)?[2];
            segments.push(next);
        }
        let area = segments.iter().try_fold(0.0, |sum, segment| {
            segment
                .pcurve
                .signed_area_contribution(segment.range, tolerance.parameter)
                .map(|area| sum + area)
        })?;
        let orientation_sign = match orientation {
            FaceOrientation::Forward => 1.0,
            FaceOrientation::Reversed => -1.0,
        };
        loops.push(BrepPatchLoop {
            kind: if area * orientation_sign > 0.0 {
                crate::LoopKind::Outer
            } else {
                crate::LoopKind::Inner
            },
            segments,
        });
    }
    if loops
        .iter()
        .filter(|loop_| loop_.kind == crate::LoopKind::Outer)
        .count()
        != 1
    {
        return Err(topology_error("an analytical shell opening rim must have one outer boundary"));
    }
    Ok(Some(BrepPatch {
        preserved: Vec::new(),
        sources: Vec::new(),
        surface: surface.clone(),
        orientation,
        loops,
    }))
}

/// Build the shell boundary directly from the source skin, the exact offset
/// skin, and one planar/analytical rim for every removed-face boundary.  A
/// generic Boolean is deliberately unnecessary here: these patches are already
/// the complete boundary representation of a shell operation.
pub(super) fn assemble_shell_boundary(
    source: &Compound,
    raw_offset: &Compound,
    openings: &BTreeSet<FaceId>,
    distance: f64,
) -> KernelResult<Compound> {
    let namespace = crate::identity::fresh_uuid();
    let mut tokens = BTreeMap::new();
    let mut tag = |patch: &mut BrepPatch, role: crate::CreationRole| {
        let name = crate::PersistentId { namespace, local: tokens.len() as u64 };
        tokens.insert(name, role);
        patch.sources.push(name);
    };
    let source_topology = source.topology();
    let offset_topology = raw_offset.topology();
    let source_by_name = source
        .faces()
        .map(|face| (face.persistent_id(), face.topology_id()))
        .collect::<BTreeMap<_, _>>();
    let offset_by_source = raw_offset
        .faces()
        .map(|face| {
            let source_name = raw_offset
                .topology_history()
                .primary_source(face.persistent_id());
            (source_by_name[&source_name], face.topology_id())
        })
        .collect::<BTreeMap<_, _>>();

    let source_shells = source
        .faces()
        .map(|face| {
            source_topology
                .face(face.topology_id())
                .map(|value| value.shell())
        })
        .collect::<KernelResult<BTreeSet<_>>>()?;
    let direct_closed_pair = source_shells.len() == 1 && openings.is_empty();
    let mut groups = Vec::<Vec<BrepPatch>>::new();
    for shell_id in source_shells {
        let shell_faces = source_topology.shell(shell_id)?.faces();
        let shell_openings = shell_faces
            .iter()
            .copied()
            .filter(|face| openings.contains(face))
            .collect::<Vec<_>>();

        let source_skin = shell_faces
            .iter()
            .copied()
            .filter(|face| !openings.contains(face))
            .map(|face| {
                let mut patch = face_patch(source_topology, face, distance > 0.0)?;
                tag(&mut patch, crate::CreationRole::ShellSource);
                Ok(patch)
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let offset_skin = raw_offset.faces()
            .filter(|face| {
                let name = raw_offset.topology_history().primary_source(face.persistent_id());
                let source_face = source_by_name[&name];
                shell_faces.contains(&source_face) && !openings.contains(&source_face)
            })
            .map(|face| {
                let mut patch = face_patch(offset_topology, face.topology_id(), distance < 0.0)?;
                tag(&mut patch, crate::CreationRole::ShellOffset);
                Ok(patch)
            })
            .collect::<KernelResult<Vec<_>>>()?;

        if shell_openings.is_empty() {
            groups.push(source_skin);
            groups.push(offset_skin);
            continue;
        }

        let mut connected = source_skin;
        connected.extend(offset_skin);
        for source_face_id in shell_openings {
            let source_face = source_topology.face(source_face_id)?;
            let offset_face = offset_topology.face(offset_by_source[&source_face_id])?;
            if source_face.loops().len() != offset_face.loops().len() {
                return Err(topology_error("an opening face and its offset have different loop counts"));
            }
            for (source_loop, offset_loop) in source_face
                .loops()
                .iter()
                .copied()
                .zip(offset_face.loops().iter().copied())
            {
                let kind = source_topology.loop_(source_loop)?.kind();
                let source_is_outer = (distance < 0.0) == (kind == crate::LoopKind::Outer);
                let (outer_loop, inner_loop) = if source_is_outer {
                    (
                        loop_patch(source_topology, source_loop, crate::LoopKind::Outer)?,
                        loop_patch(offset_topology, offset_loop, crate::LoopKind::Inner)?,
                    )
                } else {
                    (
                        loop_patch(offset_topology, offset_loop, crate::LoopKind::Outer)?,
                        loop_patch(source_topology, source_loop, crate::LoopKind::Inner)?,
                    )
                };
                if let Some(mut rim) = rim_face_patch(
                    source_face.surface(),
                    source_face.orientation(),
                    outer_loop,
                    inner_loop,
                    source.tolerance(),
                )? {
                    rim.sources.push(source_face.name());
                    tag(&mut rim, crate::CreationRole::ShellRim);
                    connected.push(rim);
                }
            }
        }
        groups.push(connected);
    }

    let mut kernel = Kernel::new(source.tolerance())?;
    let mut transaction = kernel.begin_operation();
    let mut bodies = Vec::new();
    for patch in groups.iter_mut().flatten() {
        let mut preserved = Vec::new();
        let mut sources = Vec::new();
        for name in &patch.sources {
            if let Some(records) = raw_offset.topology_history().construction.get(name) {
                for record in records {
                    sources.push(record.name);
                    if patch.preserved.contains(name) && record.unchanged {
                        preserved.push(record.name);
                    }
                }
            } else {
                sources.push(*name);
                if patch.preserved.contains(name) {
                    preserved.push(*name);
                }
            }
        }
        patch.sources = sources;
        patch.preserved = preserved;
        for segment in patch.loops.iter_mut().flat_map(|l| &mut l.segments) {
            segment.provenance.through(raw_offset.topology_history());
        }
    }
    if direct_closed_pair {
        let mut groups = groups.into_iter();
        let source_patches = groups
            .next()
            .expect("a closed source shell has a source skin");
        let offset_patches = groups
            .next()
            .expect("a closed source shell has an offset skin");
        debug_assert!(groups.next().is_none());
        let source_body = transaction.brep_shell(&source_patches)?;
        let offset_body = transaction.brep_shell(&offset_patches)?;
        let (outer_body, cavity_body) = if distance < 0.0 {
            (source_body, offset_body)
        } else {
            (offset_body, source_body)
        };
        transaction.merge_shell_into_body(outer_body, cavity_body)?;
        bodies.push(outer_body);
    } else {
        for patches in groups {
            bodies.push(transaction.brep_shell(&patches)?);
        }
        transaction.nest_brep_shells(&bodies)?;
    }
    let mut committed = transaction.finish()?;
    committed.history.record_patch_roles(&tokens);
    Compound::from_operation_with_parents(kernel, committed, &[source], crate::DerivationRole::Shell)
}
