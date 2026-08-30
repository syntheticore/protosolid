use cgmath::prelude::*;

use super::{Compound, KernelResult, PersistentId, Surface3};

/// Material-set operation requested by the high-level exact BREP Boolean API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanKind {
    Union,
    Intersection,
    Difference,
}

pub(crate) fn coincident_opposite_face_names(
    first: &Compound,
    second: &Compound,
) -> KernelResult<Option<(PersistentId, PersistentId)>> {
    if first.solids().len() != 1 || second.solids().len() != 1 {
        return Ok(None);
    }
    let tolerance = first.tolerance();
    for first_face in first.faces() {
        let Surface3::Plane(first_plane) = first_face.surface() else {
            continue;
        };
        for second_face in second.faces() {
            let Surface3::Plane(second_plane) = second_face.surface() else {
                continue;
            };
            if first_face
                .effective_normal()?
                .dot(second_face.effective_normal()?)
                > -1.0 + tolerance.angular
                || first_plane.project(second_plane.origin())?.distance.abs()
                    > tolerance.linear_at_scale(second_plane.origin().to_vec().magnitude())
            {
                continue;
            }
            let Some(first_profile) = first.profile_on_planar_face(&first_face, *first_plane)?
            else {
                continue;
            };
            let Some(second_profile) = second.profile_on_planar_face(&second_face, *first_plane)?
            else {
                continue;
            };
            if first_profile
                .boolean_planar(&second_profile, BooleanKind::Difference)?
                .is_empty()
                && second_profile
                    .boolean_planar(&first_profile, BooleanKind::Difference)?
                    .is_empty()
            {
                return Ok(Some((
                    first_face.persistent_id(),
                    second_face.persistent_id(),
                )));
            }
        }
    }
    Ok(None)
}

pub(crate) fn coincident_opposite_face_patch_names(
    first: &Compound,
    second: &Compound,
) -> KernelResult<Option<(bool, PersistentId, PersistentId)>> {
    if first.solids().len() != 1 || second.solids().len() != 1 {
        return Ok(None);
    }
    let tolerance = first.tolerance();
    let mut candidates = Vec::new();
    for first_face in first.faces() {
        let Surface3::Plane(first_plane) = first_face.surface() else {
            continue;
        };
        for second_face in second.faces() {
            let Surface3::Plane(second_plane) = second_face.surface() else {
                continue;
            };
            if first_face
                .effective_normal()?
                .dot(second_face.effective_normal()?)
                > -1.0 + tolerance.angular
                || first_plane.project(second_plane.origin())?.distance.abs()
                    > tolerance.linear_at_scale(second_plane.origin().to_vec().magnitude())
            {
                continue;
            }
            let Some(first_profile) = first.profile_on_planar_face(&first_face, *first_plane)?
            else {
                continue;
            };
            let Some(second_profile) = second.profile_on_planar_face(&second_face, *first_plane)?
            else {
                continue;
            };
            let first_remainder =
                first_profile.boolean_planar(&second_profile, BooleanKind::Difference)?;
            let second_remainder =
                second_profile.boolean_planar(&first_profile, BooleanKind::Difference)?;
            if first_remainder.is_empty() == second_remainder.is_empty() {
                continue;
            }
            if first_remainder.is_empty() {
                candidates.push((
                    second_profile.area() - first_profile.area(),
                    false,
                    second_face.persistent_id(),
                    first_face.persistent_id(),
                ));
            } else if second_remainder.is_empty() {
                candidates.push((
                    first_profile.area() - second_profile.area(),
                    true,
                    first_face.persistent_id(),
                    second_face.persistent_id(),
                ));
            }
        }
    }
    candidates.sort_by(|first, second| first.0.total_cmp(&second.0));
    Ok(candidates
        .into_iter()
        .next()
        .map(|(_, target_is_first, target, source)| (target_is_first, target, source)))
}
