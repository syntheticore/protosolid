use super::*;

pub(crate) fn split(source: &Compound, plane: Plane3, keep: SplitKeep) -> KernelResult<Compound> {
    crate::section::split(source, plane, keep)
}

pub(crate) fn dissolve_faces(source: &Compound, faces: &[FaceHandle]) -> KernelResult<Compound> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let removed = faces.iter().map(FaceHandle::topology_id).collect::<BTreeSet<_>>();
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    for face in source.faces().filter(|face| !removed.contains(&face.topology_id())) {
        supports.insert(face.topology_id(), face.surface().clone());
        bounds.insert(face.topology_id(), face_parameter_bounds(topology, face.topology_id(), tolerance)?);
    }
    if supports.is_empty() {
        return Err(invalid("dissolve must leave faces that can enclose a solid"));
    }

    // A pocket, boss or bore bounded only by complete inner loops heals by
    // removing those loops. Other features collapse onto intersections of the
    // remaining supports, using the same reconstruction as vanished blends.
    let mut collapsed = BTreeMap::new();
    let mut visited = BTreeSet::<FaceId>::new();
    for id in &removed {
        if visited.contains(id) { continue; }
        let mut component = BTreeSet::new();
        let mut pending = vec![*id];
        while let Some(face) = pending.pop() {
            if !component.insert(face) { continue; }
            for edge in topology.face_edges(face)? {
                pending.extend(topology.edge_faces(edge)?.into_iter().filter(|id| removed.contains(id)));
            }
        }
        visited.extend(component.iter().copied());
        let mut inner_only = true;
        let mut has_boundary = false;
        for face in supports.keys() {
            for loop_id in topology.face(*face)?.loops() {
                let edges = topology.loop_coedges(*loop_id)?.into_iter().map(|id| {
                    topology.edge_faces(topology.coedge(id)?.edge())
                }).collect::<KernelResult<Vec<_>>>()?;
                let touches = edges.iter().map(|faces| faces.iter().any(|id| component.contains(id))).collect::<Vec<_>>();
                if touches.iter().any(|touches| *touches) {
                    has_boundary = true;
                    inner_only &= topology.loop_(*loop_id)?.kind() == crate::LoopKind::Inner
                        && touches.iter().all(|touches| *touches);
                }
            }
        }
        if inner_only && has_boundary {
            // No retained boundary vertex lies on these consumed inner loops.
            // Point collapse suppresses their rails during reconstruction.
            for face in component {
                let edge = topology.edge(topology.face_edges(face)?[0])?;
                collapsed.insert(face, CollapsedSupport::Point(topology.vertex(edge.vertices()[0])?.point()));
            }
        }
    }

    let mut corners = Vec::new();
    for id in removed.iter().filter(|id| !collapsed.contains_key(id)).copied().collect::<Vec<_>>() {
        let edges = topology.face_edges(id)?;
        let mut neighbors = BTreeMap::<FaceId, BTreeSet<VertexId>>::new();
        for edge in &edges {
            let edge_value = topology.edge(*edge)?;
            for face in topology.edge_faces(*edge)?.into_iter().filter(|face| supports.contains_key(face)) {
                neighbors.entry(face).or_default().extend(edge_value.vertices());
            }
        }
        let face_bounds = face_parameter_bounds(topology, id, tolerance)?;
        let seed = topology.face(id)?.surface().evaluate(
            (face_bounds[0].start + face_bounds[0].end) * 0.5,
            (face_bounds[1].start + face_bounds[1].end) * 0.5,
        )?.point;
        if neighbors.len() < 2 || (neighbors.len() == 3 && edges.len() == 3) {
            corners.push((id, seed, neighbors.into_keys().collect::<Vec<_>>()));
            continue;
        }
        // Opposite contact boundaries define the rails. End caps meet the
        // rails at vertices and only trim their intersection. Choose the local
        // intersection if both pairs have nonparallel supports.
        let mut rails = neighbors.into_iter().collect::<Vec<_>>();
        let tangent = tangent_neighbors(topology, id, tolerance, |face| supports.contains_key(&face))?;
        if tangent.len() == 2 { rails.retain(|(face, _)| tangent.contains(face)); }
        let mut candidates = Vec::new();
        for first in 0..rails.len() {
            for second in first + 1..rails.len() {
                if !rails[first].1.is_disjoint(&rails[second].1) { continue; }
                let guide_edge = edges.iter().find(|edge| topology.edge_faces(**edge)
                    .is_ok_and(|faces| faces.contains(&rails[first].0))).unwrap();
                let guide = topology.edge(*guide_edge)?;
                if let Ok(curve) = intersect_collapsed_blend(&supports[&rails[first].0], &supports[&rails[second].0],
                    guide.curve(), guide.range(), guide.curve().domain().period, tolerance) {
                    candidates.push((curve.project(seed, tolerance)?.distance, curve));
                }
            }
        }
        let curve = candidates.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, curve)| curve)
            .ok_or_else(|| invalid("selected faces have no intersecting surviving support surfaces"))?;
        collapsed.insert(id, CollapsedSupport::Curve(curve));
    }
    for (id, seed, neighbors) in corners {
        let mut constraints = neighbors.iter().map(|face| CornerConstraint::Surface(&supports[face], None)).collect::<Vec<_>>();
        for edge in topology.face_edges(id)? {
            for neighbor in topology.edge_faces(edge)? {
                if let Some(CollapsedSupport::Curve(curve)) = collapsed.get(&neighbor) {
                    constraints.push(CornerConstraint::Curve(curve));
                }
            }
        }
        if constraints.len() < 2 {
            return Err(invalid("selected faces cannot be healed from the remaining support surfaces"));
        }
        let point = refine_support_corner(seed, &constraints, tolerance)?;
        collapsed.insert(id, CollapsedSupport::Point(point));
    }
    let mut changed = removed;
    for id in collapsed.keys() {
        for edge in topology.face_edges(*id)? { changed.extend(topology.edge_faces(edge)?); }
    }
    let mut patches = support_patches(
        topology,
        tolerance,
        SupportEdit {
            supports,
            bounds,
            changed: changed.clone(),
            preserve_orientation: true,
            collapsed,
            ..Default::default()
        },
    )?;
    for patch in patches.values_mut().flatten() {
        patch.loops.retain(|loop_| !loop_.segments.is_empty());
    }
    commit_support_patches(source, patches, changed, Some(crate::DerivationRole::DissolveFaces))
}
