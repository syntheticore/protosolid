use super::*;

pub(crate) fn replace_face(
    source: &Compound,
    target: &FaceHandle,
    replacement: &FaceHandle,
) -> KernelResult<Compound> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    if target == replacement {
        return source.unchanged();
    }
    let id = target.topology_id();
    let mut displacement = 0.0_f64;
    for edge in target.boundary_edges()? {
        for point in edge.endpoints()? {
            displacement = displacement.max(replacement.surface().project(point, tolerance)?.distance);
        }
    }
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    for face in source.faces() {
        let face_id = face.topology_id();
        let (surface, face_bounds) = if face_id == id {
            // The replacement supplies its support, not its trim or orientation.
            // Its UV chart is independent of the face being replaced.
            (replacement.surface(), replacement.parameter_bounds()?)
        } else {
            (face.surface(), face_parameter_bounds(topology, face_id, tolerance)?)
        };
        let surface = extended_support(surface, face_bounds, displacement, tolerance)?;
        bounds.insert(face_id, expanded_offset_bounds(
            &surface, face_bounds, displacement, tolerance,
        )?);
        supports.insert(face_id, surface);
    }
    let changed = BTreeSet::from([id]);
    let patches = support_patches(
        topology,
        tolerance,
        SupportEdit {
            supports,
            bounds,
            changed: changed.clone(),
            reparameterized: changed.clone(),
            ..Default::default()
        },
    )?;
    commit_support_patches(source, patches, changed, Some(crate::DerivationRole::ReplaceFace))
}
