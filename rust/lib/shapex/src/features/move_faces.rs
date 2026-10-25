use super::*;

pub(crate) fn move_faces(
    source: &Compound,
    faces: &[FaceHandle],
    transform: RigidTransform3,
) -> KernelResult<Compound> {
    if transform == RigidTransform3::identity() {
        return source.unchanged();
    }
    if faces.len() == source.faces().count() {
        return source.transformed(transform);
    }
    let topology = source.topology();
    let tolerance = source.tolerance();
    let selected = faces
        .iter()
        .map(FaceHandle::persistent_id)
        .collect::<BTreeSet<_>>();
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    let mut displacement = transform.translation().magnitude();
    for face in faces {
        for edge in topology.face_edges(face.topology_id())? {
            for vertex in topology.edge(edge)?.vertices() {
                let point = topology.vertex(vertex)?.point();
                displacement = displacement.max(point.distance(transform.transform_point(point)));
            }
        }
    }
    // Blends along a moved face must continue to meet it. Others, including
    // moved blends, keep their exact source support so that their unchanged
    // boundaries are retained.
    let mut reaching = BTreeSet::new();
    for face in faces {
        for edge in topology.face_edges(face.topology_id())? {
            reaching.extend(topology.edge_faces(edge)?);
        }
    }
    for face in faces {
        reaching.remove(&face.topology_id());
    }
    for face in source.faces() {
        let id = face.topology_id();
        let original_bounds = face_parameter_bounds(topology, id, tolerance)?;
        let mut surface = extended_support(face.surface(), original_bounds, displacement, tolerance)?;
        if let Surface3::RollingBall(blend) | Surface3::Ruled(blend) = &surface {
            // Extend only a spine that is too short: an edited support also
            // rebuilds the blend's boundaries with its unchanged neighbors.
            // A spine continues only while its supports still admit the blend
            // section; beyond that, its source range is all that exists.
            let padding = offset_parameter_padding(&surface, original_bounds, displacement, tolerance)?[0];
            let (spine, trim) = (blend.center_range(), original_bounds[0]);
            let short = spine.start.min(spine.end) > trim.start.min(trim.end) - padding
                || spine.start.max(spine.end) < trim.start.max(trim.end) + padding;
            if reaching.contains(&id) && short {
                if let Some(extended) = [1.0, 0.5, 0.25].into_iter()
                    .find_map(|scale| blend.extended(padding * scale).ok())
                {
                    surface = extended.into();
                }
            }
        }
        if selected.contains(&face.persistent_id()) {
            surface = crate::transform::transform_surface(surface, transform, tolerance)?;
        }
        let mut face_bounds = expanded_offset_bounds(&surface, original_bounds, displacement, tolerance)?;
        if matches!(surface, Surface3::RollingBall(_)) {
            // A rolling-ball section spans less than half a turn between its
            // rails. Padding it by its displacement on a long move would wrap
            // the circle, so iso-curves across it would no longer be simple;
            // half a section on either side keeps less than a full turn.
            let section = original_bounds[1];
            face_bounds[1] = ParameterRange::new(
                face_bounds[1].start.max(section.start.min(section.end) - 0.5),
                face_bounds[1].end.min(section.start.max(section.end) + 0.5),
            )?;
        }
        bounds.insert(id, face_bounds);
        supports.insert(id, surface);
    }
    let contacts = move_blend_contacts(source)?;
    let mut changed_supports = faces
        .iter()
        .map(FaceHandle::topology_id)
        .collect::<BTreeSet<_>>();
    changed_supports.extend(adjust_move_blends(
        source,
        &selected,
        transform,
        &contacts,
        &mut supports,
    )?);
    let result = rebuild_supports(
        source,
        SupportEdit {
            supports,
            bounds,
            changed: changed_supports,
            preserve_orientation: true,
            ..Default::default()
        },
    )?;
    // A valid closed shell alone does not certify that a former fillet is still
    // tangent. Check the reconstructed contact rails before publishing it.
    let mut tangent_pairs = BTreeSet::new();
    for (id, rails) in &contacts {
        let name = topology.face(*id)?.name();
        for (other, _, _) in rails {
            tangent_pairs.insert((name, topology.face(*other)?.name()));
        }
    }
    let result_topology = result.topology();
    for (edge_id, _) in result_topology.edges() {
        let adjacent = result_topology.edge_faces(edge_id)?;
        let [first, second] = adjacent.as_slice() else {
            continue;
        };
        let pair = (
            result
                .topology_history()
                .primary_source(result_topology.face(*first)?.name()),
            result
                .topology_history()
                .primary_source(result_topology.face(*second)?.name()),
        );
        if !tangent_pairs.contains(&pair) {
            continue;
        }
        if tangent_contact(
            result_topology,
            edge_id,
            *first,
            *second,
            tolerance,
        )?
        .is_empty()
        {
            return Err(not_implemented(
                "face movement that cannot preserve blend tangency",
            ));
        }
    }
    result.with_feature_parent(source, crate::DerivationRole::MoveFaces)
}

pub(super) type BlendContacts = BTreeMap<FaceId, Vec<(FaceId, Point3, crate::Vec3)>>;

pub(super) fn tangent_neighbors(
    topology: &Topology, face: FaceId, tolerance: crate::ModelTolerance,
    include: impl Fn(FaceId) -> bool,
) -> KernelResult<BTreeSet<FaceId>> {
    let mut neighbors = BTreeSet::new();
    for edge in topology.face_edges(face)? {
        for other in topology.edge_faces(edge)? {
            if other != face && include(other) && !neighbors.contains(&other)
                && !tangent_contact(topology, edge, face, other, tolerance)?.is_empty() {
                neighbors.insert(other);
            }
        }
    }
    Ok(neighbors)
}

pub(super) fn tangent_contact(
    topology: &Topology,
    edge: EdgeId,
    first: FaceId,
    second: FaceId,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Vec<(FaceId, Point3, crate::Vec3)>> {
    let edge = topology.edge(edge)?;
    let first = topology.face(first)?;
    let other = topology.face(second)?;
    let mut contacts = Vec::new();
    // Sample the contact curve, not just a vertex at which two sharp faces
    // could happen to share a normal. Offset and Move use the same criterion.
    for fraction in [0.05, 0.2, 0.35, 0.5, 0.65, 0.8, 0.95] {
        let point = edge
            .curve()
            .evaluate(edge.range().start + edge.range().span() * fraction)?
            .point;
        let n = first.oriented_normal_near(point, tolerance)?;
        if (n - other.oriented_normal_near(point, tolerance)?).magnitude() > tolerance.angular * 8.0 {
            return Ok(Vec::new());
        }
        contacts.push((second, point, n));
    }
    Ok(contacts)
}

pub(super) fn move_blend_contacts(source: &Compound) -> KernelResult<BlendContacts> {
    let topology = source.topology();
    let mut contacts = BlendContacts::new();
    for face in source.faces() {
        let id = face.topology_id();
        for edge in topology.face_edges(id)? {
            for other in topology
                .edge_faces(edge)?
                .into_iter()
                .filter(|other| *other != id)
            {
                contacts.entry(id).or_default().extend(tangent_contact(
                    topology,
                    edge,
                    id,
                    other,
                    source.tolerance(),
                )?);
            }
        }
    }
    Ok(contacts)
}

pub(super) fn adjust_move_blends(
    source: &Compound,
    selected: &BTreeSet<PersistentId>,
    transform: RigidTransform3,
    contacts: &BlendContacts,
    supports: &mut BTreeMap<FaceId, Surface3>,
) -> KernelResult<BTreeSet<FaceId>> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let selected_faces = source
        .faces()
        .filter(|face| selected.contains(&face.persistent_id()))
        .map(FaceHandle::topology_id)
        .collect::<BTreeSet<_>>();
    let mut changed = BTreeSet::new();
    let mut blends = BTreeSet::new();
    for face in source.faces() {
        let id = face.topology_id();
        if matches!(face.surface(), Surface3::Plane(_)) {
            continue;
        }
        // A full cylindrical wall between rounded rims is a support, not a
        // blend. Propagating through it would move the opposite end of a part.
        if matches!(face.surface(), Surface3::Cylinder(_))
            && face_parameter_bounds(topology, id, tolerance)?[0]
                .span()
                .abs()
                >= std::f64::consts::TAU - tolerance.parameter * 8.0
        {
            continue;
        }
        blends.insert(id);
    }
    let mut pending = BTreeSet::new();
    loop {
        let additions = contacts
            .iter()
            .filter(|(id, rails)| {
                !selected_faces.contains(*id)
                    && !pending.contains(*id)
                    && blends.contains(*id)
                    && rails
                        .iter()
                        .map(|(other, _, _)| other)
                        .collect::<BTreeSet<_>>()
                        .len()
                        >= 2
                    && rails
                        .iter()
                        .any(|(other, _, _)| selected_faces.contains(other) || pending.contains(other))
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        if additions.is_empty() {
            break;
        }
        pending.extend(additions);
    }
    while !pending.is_empty() {
        let mut resolved = None;
        for id in &pending {
            let rails = contacts[id]
                .iter()
                .filter(|(other, _, _)| !pending.contains(other))
                .copied()
                .collect::<Vec<_>>();
            if rails
                .iter()
                .map(|(other, _, _)| other)
                .collect::<BTreeSet<_>>()
                .len()
                < 2
            {
                continue;
            }
            let original = topology.face(*id)?.surface();
            let fits = |motion: RigidTransform3| -> KernelResult<bool> {
                for (other, point, normal) in &rails {
                    let point = motion.transform_point(*point);
                    let projection = supports[other].project(point, tolerance)?;
                    let uv = projection.parameters;
                    let sign = topology.face(*other)?.orientation().sign();
                    if projection.distance
                        > tolerance.linear_at_scale(point.to_vec().magnitude()) * 8.0
                        || (supports[other].normal(uv.0, uv.1)? * sign
                            - motion.transform_vector(*normal))
                        .magnitude()
                            > tolerance.angular * 8.0
                    {
                        return Ok(false);
                    }
                }
                Ok(true)
            };
            // Moving an entire blend along an invariant neighboring support is
            // exact (for example, raising a rounded cylindrical cap).
            let surface = if fits(transform)? {
                crate::transform::transform_surface(original.clone(), transform, tolerance)?
            } else if let Surface3::Cylinder(cylinder) = original {
                let planes = rails
                    .iter()
                    .map(|(other, _, _)| *other)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .filter(|other| matches!(supports[other], Surface3::Plane(_)))
                    .collect::<Vec<_>>();
                if planes.len() != 2 {
                    continue;
                }
                let mut normals = Vec::new();
                let mut equations = Vec::new();
                for other in planes {
                    let Surface3::Plane(old) = topology.face(other)?.surface() else {
                        continue;
                    };
                    let Surface3::Plane(new) = supports[&other] else {
                        unreachable!()
                    };
                    let normal = new.normal();
                    let side = (cylinder.origin() - old.origin())
                        .dot(old.normal())
                        .signum();
                    equations.push((
                        [normal.x, normal.y, normal.z, -side],
                        (new.origin() - cylinder.origin()).dot(normal) + side * cylinder.radius(),
                    ));
                    normals.push(normal);
                }
                if normals.len() != 2 {
                    continue;
                }
                let axis = normals[0].cross(normals[1]);
                let parallel = axis.magnitude() <= tolerance.angular;
                // A semicircular slot end joins parallel walls. Moving one
                // wall changes both its center and radius by half the width
                // change; a fixed-radius fillet cannot satisfy both contacts.
                let solution = solve_blend_contacts(&equations, false, tolerance).or_else(|| {
                    if parallel {
                        solve_blend_contacts(&equations, true, tolerance)
                    } else {
                        None
                    }
                });
                let Some(solution) = solution else {
                    continue;
                };
                let radius = cylinder.radius() + solution[3];
                if radius <= tolerance.linear {
                    return Err(invalid("moving these faces collapses the rounded end"));
                }
                let axis = if parallel {
                    cylinder.axis().normalize()
                } else {
                    axis.normalize() * axis.dot(cylinder.axis()).signum()
                };
                let rotation = crate::Quaternion::from_arc(cylinder.axis().normalize(), axis, None);
                let scale = radius / cylinder.radius();
                crate::Cylinder3::new(
                    cylinder.origin() + crate::Vec3::new(solution[0], solution[1], solution[2]),
                    axis * cylinder.axis().magnitude(),
                    rotation.rotate_vector(cylinder.x_radius()) * scale,
                    rotation.rotate_vector(cylinder.y_radius()) * scale,
                    tolerance,
                )?
                .into()
            } else if let Surface3::Sphere(sphere) = original {
                // Equal-radius cylindrical edge blends meet a spherical corner
                // with its center on all their axes, including after rotation.
                let mut equations = Vec::new();
                for other in rails
                    .iter()
                    .map(|(other, _, _)| *other)
                    .collect::<BTreeSet<_>>()
                {
                    if let Surface3::Cylinder(cylinder) = &supports[&other] {
                        if (cylinder.radius() - sphere.radius()).abs() > tolerance.linear {
                            continue;
                        }
                        for normal in [
                            cylinder.x_radius().normalize(),
                            cylinder.y_radius().normalize(),
                        ] {
                            equations.push((
                                [normal.x, normal.y, normal.z, 0.0],
                                (cylinder.origin() - sphere.center()).dot(normal),
                            ));
                        }
                    }
                }
                if equations.len() < 4 {
                    continue;
                }
                let Some(solution) = solve_blend_contacts(&equations, false, tolerance) else {
                    continue;
                };
                let motion = RigidTransform3::new(
                    crate::Matrix3::identity(),
                    crate::Vec3::new(solution[0], solution[1], solution[2]),
                    tolerance,
                )?;
                crate::transform::transform_surface(original.clone(), motion, tolerance)?
            } else {
                let equations = rails
                    .iter()
                    .map(|(other, point, normal)| {
                        let projection = supports[other].project(*point, tolerance)?;
                        Ok((
                            [normal.x, normal.y, normal.z, 0.0],
                            (projection.point - point).dot(*normal),
                        ))
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                let motion = solve_blend_contacts(&equations, false, tolerance)
                    .map(|solution| RigidTransform3::new(
                        crate::Matrix3::identity(),
                        crate::Vec3::new(solution[0], solution[1], solution[2]),
                        tolerance,
                    ))
                    .transpose()?;
                let motion = match motion {
                    Some(motion) if fits(motion)? => Some(motion),
                    _ => None,
                };
                if let Some(motion) = motion {
                    crate::transform::transform_surface(original.clone(), motion, tolerance)?
                } else if let Surface3::RollingBall(blend) = original {
                    // No rigid motion keeps both contacts. Roll the same ball
                    // between the edited supports instead.
                    let Some(surface) = rolled_blend(topology, *id, blend, &rails, supports, tolerance)? else {
                        continue;
                    };
                    surface
                } else {
                    continue;
                }
            };
            resolved = Some((*id, surface));
            break;
        }
        let Some((id, surface)) = resolved else {
            return Err(not_implemented("face movement of this connected blend network"));
        };
        pending.remove(&id);
        supports.insert(id, surface);
        changed.insert(id);
    }
    Ok(changed)
}

/// Roll `blend`'s ball between the edited supports of its two contact faces.
/// The radius and the side of each support on which the ball rolls persist;
/// the spine is traced from the old contacts across the face and a margin.
/// Returns `None` when the edited supports no longer admit that ball.
pub(super) fn rolled_blend(
    topology: &Topology,
    id: FaceId,
    blend: &crate::RollingBallSurface3,
    rails: &[(FaceId, Point3, crate::Vec3)],
    supports: &BTreeMap<FaceId, Surface3>,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Option<Surface3>> {
    let mut contacts = rails.iter().map(|(other, _, _)| *other).collect::<BTreeSet<_>>()
        .into_iter().collect::<Vec<_>>();
    if contacts.len() != 2 {
        return Ok(None);
    }
    let old = [blend.first_surface(), blend.second_surface()];
    let point = |face: FaceId| rails.iter().find(|(other, _, _)| *other == face).unwrap().1;
    if old[0].project(point(contacts[0]), tolerance)?.distance
        > old[1].project(point(contacts[0]), tolerance)?.distance
    {
        contacts.swap(0, 1);
    }
    let pcurves = [blend.first_pcurve(), blend.second_pcurve()];
    let mut range = blend.center_range();
    let middle = (range.start + range.end) * 0.5;
    let center = blend.center_curve().evaluate(middle)?.point;
    let mut parents = Vec::new();
    let mut offsets = Vec::new();
    for side in 0..2 {
        let uv = pcurves[side].evaluate(middle)?.point;
        let normal = old[side].normal(uv.x, uv.y)?;
        let distance = blend.radius()
            * (center - old[side].evaluate(uv.x, uv.y)?.point).dot(normal).signum();
        parents.push(supports[&contacts[side]].clone());
        offsets.push(Surface3::Offset(crate::OffsetSurface3::new(
            parents[side].clone(), distance, tolerance,
        )?));
    }
    if blend.center_period().is_none() {
        let bounds = face_parameter_bounds(topology, id, tolerance)?[0];
        let margin = bounds.span().abs() * 0.25;
        let low = (bounds.start.min(bounds.end) - margin).max(range.start.min(range.end));
        let high = (bounds.start.max(bounds.end) + margin).min(range.start.max(range.end));
        range = if range.span() >= 0.0 {
            ParameterRange::new(low, high)?
        } else {
            ParameterRange::new(high, low)?
        };
    }
    let mut guides = [Vec::new(), Vec::new()];
    for index in 0..=32 {
        let parameter = range.start + range.span() * index as f64 / 32.0;
        let first = pcurves[0].evaluate(parameter)?.point;
        let second = pcurves[1].evaluate(parameter)?.point;
        let Ok(corrected) = crate::intersection::correct_surface_pair_near(
            &offsets[0], &offsets[1], [first.x, first.y, second.x, second.y], tolerance,
        ) else {
            return Ok(None);
        };
        guides[0].push(Point2::new(corrected[0], corrected[1]));
        guides[1].push(Point2::new(corrected[2], corrected[3]));
    }
    for side in 0..2 {
        crate::intersection::unwrap_periodic_points(&mut guides[side], offsets[side].domains());
    }
    let rolled = (|| -> KernelResult<Surface3> {
        let center = SurfaceIntersectionCurve::new(
            offsets[0].clone(),
            offsets[1].clone(),
            crate::intersection::smooth_nurbs_guide(&guides[0], range)?.into(),
            crate::intersection::smooth_nurbs_guide(&guides[1], range)?.into(),
            range,
            tolerance,
            tolerance.linear,
        )?;
        let [first, second] = center.exact_pcurves();
        Ok(crate::RollingBallSurface3::new(
            center.into(), parents[0].clone(), parents[1].clone(), first, second,
            range, blend.radius(), tolerance,
        )?
        .with_center_period(blend.center_period())
        .into())
    })();
    Ok(rolled.ok())
}
