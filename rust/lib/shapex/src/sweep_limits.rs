//! Face-limited sweeps. Construction envelopes are disposable; published faces
//! retain the exact surfaces, trim curves and certified Boolean topology.
use super::{
    invalid, Axis3, Compound, FaceHandle, KernelResult,
    ModelTolerance, PersistentId, Plane3, Profile, Resolution, ResolvableEntity, RigidTransform3,
    Surface3, TessellationOptions,
};
use crate::{Matrix3, Point3, Vec3};
use cgmath::prelude::*;
use std::collections::BTreeSet;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn names<T: ResolvableEntity>(result: &Compound, source: &T) -> BTreeSet<PersistentId> {
    match result.resolve(source) {
        Resolution::Preserved(v) | Resolution::Modified(v) | Resolution::Merged(v) => {
            [v.persistent_name()].into_iter().collect()
        }
        Resolution::Split(v) => v.iter().map(T::persistent_name).collect(),
        _ => BTreeSet::new(),
    }
}

fn on_plane(face: &FaceHandle, plane: Plane3, tolerance: ModelTolerance) -> bool {
    matches!(face.surface(), Surface3::Plane(other) if
        other.normal().dot(plane.normal()).abs() >= 1.0 - tolerance.angular &&
        (other.origin() - plane.origin()).dot(plane.normal()).abs() <= tolerance.linear)
}

fn cap(tool: &Compound, plane: Plane3) -> KernelResult<FaceHandle> {
    tool.faces()
        .find(|face| on_plane(face, plane, tool.tolerance()))
        .cloned()
        .ok_or_else(|| invalid("sweep has no distinct profile cap"))
}

fn complete_start(
    result: &Compound,
    original: &FaceHandle,
    output: &FaceHandle,
) -> KernelResult<bool> {
    let boundary = output
        .boundary_edges()?
        .iter()
        .map(|e| e.persistent_id())
        .collect::<BTreeSet<_>>();
    let mut retained = BTreeSet::new();
    for edge in original.boundary_edges()? {
        let ids = names(result, &edge)
            .intersection(&boundary)
            .copied()
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(false);
        }
        retained.extend(ids);
    }
    Ok(retained == boundary)
}

fn check_limit(target: &Compound, face: &FaceHandle) -> KernelResult<()> {
    if !target.faces().any(|available| available == face) {
        return Err(invalid(
            "limiting face must belong to the supplied body revision",
        ));
    }
    Ok(())
}

struct Trim {
    result: Option<Compound>,
    can_extend: bool,
}

fn join_cells(
    first: &Compound,
    second: &Compound,
    limit: &FaceHandle,
) -> KernelResult<Option<Compound>> {
    let mut patches = Vec::new();
    let mut barriers = Vec::new();
    for cell in [first, second] {
        let limits = names(cell, limit);
        for face in cell.faces() {
            patches.push(super::features::face_patch(
                cell.topology(),
                face.topology_id(),
                false,
            )?);
            barriers.push(limits.contains(&face.persistent_id()));
        }
    }
    super::sew::conform_brep_interfaces(&mut patches, first.tolerance())?;
    let samples = patches
        .iter()
        .map(super::boolean::patch_edge_samples)
        .collect::<KernelResult<Vec<_>>>()?;
    let mut removed = BTreeSet::new();
    for i in 0..first.faces().count() {
        for j in first.faces().count()..patches.len() {
            if !super::trim::coincident_supports(
                &patches[i].surface,
                &patches[j].surface,
                first.tolerance(),
            )? {
                continue;
            }
            let point = samples[i][0][1];
            let oriented_normal = |patch: &super::sew::BrepPatch| -> KernelResult<Vec3> {
                let uv = patch.surface.project(point, first.tolerance())?.parameters;
                let normal = patch.surface.normal(uv.0, uv.1)?;
                Ok(if patch.orientation == super::FaceOrientation::Forward {
                    normal
                } else {
                    -normal
                })
            };
            if oriented_normal(&patches[i])?.dot(oriented_normal(&patches[j])?) >= 0.0 {
                continue;
            }
            if samples[i].len() == samples[j].len()
                && samples[i].iter().all(|a| {
                    samples[j].iter().any(|b| {
                        super::boolean::same_unoriented_edge(*a, *b, first.tolerance().linear)
                    })
                })
            {
                // These cells also meet across the selected face. Joining them
                // would leave an internal sheet: this trimmed face is not a
                // complete barrier through the sweep.
                if barriers[i] || barriers[j] {
                    return Ok(None);
                }
                removed.insert(i);
                removed.insert(j);
            }
        }
    }
    if removed.is_empty() {
        return Ok(None);
    }
    let mut patches = patches
        .into_iter()
        .enumerate()
        .filter_map(|(i, p)| (!removed.contains(&i)).then_some(p))
        .collect::<Vec<_>>();
    super::sew::conform_brep_boundaries(&mut patches, first.tolerance())?;
    let mut kernel = super::Kernel::new(first.tolerance())?;
    let mut transaction = kernel.begin_operation();
    transaction.brep_shell(&patches)?;
    transaction.unify_same_domain()?;
    let committed = transaction.finish()?;
    Ok(Some(
        Compound::from_operation_with_parents(
            kernel,
            committed,
            &[first, second],
            super::DerivationRole::SweepJoinCells,
        )?,
    ))
}

fn trim(
    tool: &Compound,
    target: &Compound,
    limit: &FaceHandle,
    start: &FaceHandle,
    end: &FaceHandle,
) -> KernelResult<Trim> {
    let plane = match limit.surface() {
        Surface3::Plane(p) => Some(*p),
        _ => None,
    };
    let mut partitions = if let Some(plane) = plane {
        vec![tool.split(plane, super::SplitKeep::Both)?]
    } else {
        vec![tool.difference(target)?, tool.intersection(target)?]
    };
    if plane.is_none() {
        // A face limit is not a limit at the first entry into its owning body.
        // Join adjacent cells across the owner's other faces, retaining only
        // the selected face as a barrier along the sweep.
        let mut cells = partitions
            .iter()
            .flat_map(|p| p.solids().map(|s| s.to_compound()))
            .collect::<Vec<_>>();
        let barriers = target
            .faces()
            .filter(|face| *face != limit)
            .collect::<Vec<_>>();
        let mut changed = true;
        while changed {
            changed = false;
            'pairs: for first in 0..cells.len() {
                for second in first + 1..cells.len() {
                    let first_limit = names(&cells[first], limit);
                    let second_limit = names(&cells[second], limit);
                    let adjacent = barriers.iter().any(|face| {
                        names(&cells[first], *face)
                            .difference(&first_limit)
                            .next()
                            .is_some()
                            && names(&cells[second], *face)
                                .difference(&second_limit)
                                .next()
                                .is_some()
                    });
                    if !adjacent {
                        continue;
                    }
                    if let Some(united) = join_cells(&cells[first], &cells[second], limit)? {
                        cells[first] = united;
                        cells.remove(second);
                        changed = true;
                        break 'pairs;
                    }
                }
            }
        }
        partitions = cells;
    }
    let mut candidates = Vec::new();
    let mut can_extend = false;
    for partition in partitions {
        let start_names = names(&partition, start);
        let mut starts = BTreeSet::new();
        for face in partition
            .faces()
            .filter(|face| start_names.contains(&face.persistent_id()))
        {
            if complete_start(&partition, start, face)? {
                starts.insert(face.persistent_id());
            }
        }
        let ends = names(&partition, end);
        let limits = if let Some(plane) = plane {
            partition
                .faces()
                .filter(|f| on_plane(f, plane, tool.tolerance()))
                .map(FaceHandle::persistent_id)
                .collect()
        } else {
            names(&partition, limit)
        };
        let sides = tool
            .faces()
            .filter(|f| *f != start && *f != end)
            .flat_map(|f| names(&partition, f))
            .collect::<BTreeSet<_>>();
        for solid in partition.solids() {
            let start_faces = solid
                .faces()
                .filter(|f| starts.contains(&f.persistent_id()))
                .collect::<Vec<_>>();
            if start_faces.len() != 1 {
                continue;
            }
            let ids = solid
                .faces()
                .map(FaceHandle::persistent_id)
                .collect::<BTreeSet<_>>();
            let has_end = !ids.is_disjoint(&ends);
            let has_limit = !ids.is_disjoint(&limits);
            if has_end {
                can_extend = true;
            }
            if !has_limit
                || ids.iter().any(|id| {
                    (ends.contains(id) && !limits.contains(id))
                        || (starts.contains(id) && limits.contains(id))
                        || (!starts.contains(id) && !limits.contains(id) && !sides.contains(id))
                })
            {
                continue;
            }
            if solid.volume() > tool.tolerance().linear.powi(3) {
                candidates.push(solid.to_compound());
            }
        }
    }
    if candidates.len() > 1 {
        return Err(invalid("selected face defines more than one sweep limit"));
    }
    Ok(Trim {
        result: candidates.pop(),
        can_extend,
    })
}

fn shifted(plane: Plane3, vector: Vec3, tol: ModelTolerance) -> KernelResult<Plane3> {
    Plane3::new(plane.origin() + vector, plane.u(), plane.v(), tol)
}

fn turned(plane: Plane3, axis: Axis3, angle: f64, tol: ModelTolerance) -> KernelResult<Plane3> {
    let rotation = Matrix3::from_axis_angle(axis.direction(), cgmath::Rad(angle));
    Plane3::new(
        axis.origin() + rotation * (plane.origin() - axis.origin()),
        rotation * plane.u(),
        rotation * plane.v(),
        tol,
    )
}

pub(super) fn profile_center(profile: &Profile, plane: Plane3) -> KernelResult<Point3> {
    // Use the exact area centroid, including curved boundaries and holes. A
    // circle's sole seam vertex is not a representative profile location.
    let normal = plane.normal();
    Ok(profile.extrude(plane, normal)?.centroid() - normal * 0.5)
}

fn radial_angle(
    profile: &Profile,
    plane: Plane3,
    axis: Axis3,
    face: &FaceHandle,
    forward: bool,
) -> KernelResult<Option<f64>> {
    let Surface3::Plane(limit) = face.surface() else {
        return Ok(None);
    };
    let tol = profile.tolerance();
    if limit.normal().dot(axis.direction()).abs() > tol.angular
        || plane.normal().dot(axis.direction()).abs() > tol.angular
        || (limit.origin() - axis.origin()).dot(limit.normal()).abs() > tol.linear
        || (plane.origin() - axis.origin()).dot(plane.normal()).abs() > tol.linear
    {
        return Ok(None);
    }
    let radial = profile_center(profile, plane)? - axis.origin();
    let a = limit.normal().dot(radial);
    let b = limit.normal().dot(axis.direction().cross(radial)) * if forward { 1.0 } else { -1.0 };
    if a.hypot(b) <= tol.linear {
        return Err(invalid("profile lies on the revolution axis"));
    }
    Ok(Some(if a.abs() <= tol.linear {
        0.0
    } else {
        (-a).atan2(b).rem_euclid(PI)
    }))
}

#[cfg(test)]
#[path = "tests/sweep_limits.rs"]
mod tests;

impl Profile {
    /// Sweep to the selected surface. Planar limits use their infinite support;
    /// curved limits must cover the full profile with their trimmed face.
    /// Both offsets follow the automatically chosen extrusion direction: the
    /// start offset moves the sketch and the end offset moves the limiting surface.
    pub fn extrude_up_to_face(
        &self,
        plane: Plane3,
        target: &Compound,
        limit: &FaceHandle,
        offset: f64,
        end_offset: f64,
    ) -> KernelResult<Compound> {
        check_limit(target, limit)?;
        if !offset.is_finite() {
            return Err(invalid("extrusion offset must be finite"));
        }
        if !end_offset.is_finite() {
            return Err(invalid("extrusion end offset must be finite"));
        }
        let tol = self.tolerance();
        let origin = profile_center(self, plane)?;
        let mut direction = plane.normal();
        let options = TessellationOptions::default();
        let target_points = limit.tessellate(options)?.positions;
        if let Surface3::Plane(limit_plane) = limit.surface() {
            let denominator = limit_plane.normal().dot(direction);
            if denominator.abs() <= tol.angular {
                return Err(invalid(
                    "target plane is parallel to the extrusion direction",
                ));
            }
            if (limit_plane.origin() - origin).dot(limit_plane.normal()) / denominator < 0.0 {
                direction = -direction;
            }
        } else {
            let center = Point3::from_vec(
                target_points.iter().map(|p| p.to_vec()).sum::<Vec3>() / target_points.len() as f64,
            );
            if (center - origin).dot(direction) < 0.0 {
                direction = -direction;
            }
        }
        // Trim against the original face from a shifted start, then translate
        // the result. This moves even curved limits without changing the final
        // start position or letting the end offset reverse the chosen direction.
        let start_offset = offset - end_offset;
        let start_plane = shifted(plane, direction * start_offset, tol)?;
        let probe = self.extrude(start_plane, direction)?;
        let base = cap(&probe, start_plane)?;
        let reach = if let Surface3::Plane(p) = limit.surface() {
            base.tessellate(options)?
                .positions
                .iter()
                .map(|point| (p.origin() - point).dot(p.normal()) / p.normal().dot(direction))
                .fold(f64::NEG_INFINITY, f64::max)
        } else {
            target_points
                .iter()
                .map(|point| (point - (origin + direction * start_offset)).dot(direction))
                .fold(f64::NEG_INFINITY, f64::max)
        };
        if reach <= tol.linear {
            return Err(invalid("selected face is behind the extrusion start"));
        }
        let height = reach + reach.abs().max(1.0).max(options.chord_tolerance * 4.0);
        let tool = self.extrude(start_plane, direction * height)?;
        let start = cap(&tool, start_plane)?;
        let end = cap(&tool, shifted(start_plane, direction * height, tol)?)?;
        let result = trim(&tool, target, limit, &start, &end)?
            .result
            .ok_or_else(|| {
                invalid("selected face must cover the entire extrusion beyond its start")
            })?;
        // Report the high-level extrusion's construction correspondence after
        // trimming, independently of the Boolean's entity ancestry.
        let mut roles = result
            .faces()
            .map(|face| (face.persistent_id(), super::CreationRole::ExtrusionEnd))
            .collect::<std::collections::BTreeMap<_, _>>();
        for face in tool.faces() {
            if face == &end {
                continue;
            }
            if let Some(super::Origin::Created { role: Some(role) }) = tool.origin(face) {
                for id in names(&result, face) {
                    roles.insert(id, role.clone());
                }
            }
        }
        let result = result.with_creation_roles(roles);
        let result = if end_offset != 0.0 {
            result.transformed(RigidTransform3::new(
                Matrix3::identity(), direction * end_offset, tol,
            )?)?
        } else {
            result
        };
        Ok(result)
    }

    /// Revolve to the first complete crossing in the requested direction.
    pub fn revolve_up_to_face(
        &self,
        plane: Plane3,
        axis: Axis3,
        target: &Compound,
        limit: &FaceHandle,
        forward: bool,
    ) -> KernelResult<Compound> {
        check_limit(target, limit)?;
        let sign = if forward { 1.0 } else { -1.0 };
        if let Some(angle) = radial_angle(self, plane, axis, limit, forward)? {
            if angle <= self.tolerance().angular {
                return Err(invalid("target plane intersects the starting profile"));
            }
            return self.revolve(plane, axis, sign * angle);
        }
        // Grow an open sweep until the selected face caps the whole profile.
        // A limit can cross different profile points on either side of a
        // quadrant, so separate quarter-turn trims cannot be joined reliably.
        // Avoid 180/360 degrees where cap supports coincide or disappear.
        let mut angle = FRAC_PI_2;
        for _ in 0..32 {
            let end_plane = turned(plane, axis, sign * angle, self.tolerance())?;
            let tool = self.revolve(plane, axis, sign * angle)?;
            let start = cap(&tool, plane)?;
            let end = cap(&tool, end_plane)?;
            let trimmed = trim(&tool, target, limit, &start, &end)?;
            if let Some(result) = trimmed.result {
                let limits = names(&result, limit).into_iter()
                    .map(|id| (id, super::CreationRole::ProfileEnd)).collect::<Vec<_>>();
                return Ok(result.with_construction_roles_from(&[&tool], &limits));
            }
            if !trimmed.can_extend {
                return Err(invalid(
                    "selected face must cap the entire profile along the revolution",
                ));
            }
            angle = if angle == FRAC_PI_2 {
                3.0 * FRAC_PI_2
            } else {
                (angle + TAU) * 0.5
            };
            if TAU - angle <= self.tolerance().angular * 16.0 {
                break;
            }
        }
        Err(invalid("selected face does not cross the revolution"))
    }

    /// Revolve between selected limits, allowing the interval to cross the sketch.
    pub fn revolve_between_faces(
        &self,
        plane: Plane3,
        axis: Axis3,
        from_body: &Compound,
        from: &FaceHandle,
        until_body: &Compound,
        until: &FaceHandle,
        forward: bool,
    ) -> KernelResult<Compound> {
        check_limit(from_body, from)?;
        check_limit(until_body, until)?;
        if from == until {
            return Err(invalid("choose different starting and ending faces"));
        }
        let sign = if forward { 1.0 } else { -1.0 };
        if let (Some(start), Some(end)) = (
            radial_angle(self, plane, axis, from, forward)?,
            radial_angle(self, plane, axis, until, forward)?,
        ) {
            if (end - start).abs() <= self.tolerance().angular {
                return Err(invalid("limiting planes coincide"));
            }
            return self.revolve(
                turned(plane, axis, sign * start, self.tolerance())?,
                axis,
                sign * (end - start).rem_euclid(TAU),
            );
        }
        let start = if on_plane(from, plane, self.tolerance()) {
            None
        } else {
            Some(self.revolve_up_to_face(plane, axis, from_body, from, forward)?)
        };
        let end = if on_plane(until, plane, self.tolerance()) {
            None
        } else {
            Some(self.revolve_up_to_face(plane, axis, until_body, until, forward)?)
        };
        let complement = |start: &Compound| {
            // Keep a sketch cap while constructing the complementary interval.
            // The bounded forward cap identifies the same crossing even when
            // the original surface intersects the revolution more than once.
            let limits = names(start, from);
            let limit = start
                .faces()
                .find(|face| limits.contains(&face.persistent_id()))
                .ok_or_else(|| invalid("starting sweep has no limiting cap"))?;
            self.revolve_up_to_face(plane, axis, start, limit, !forward)
        };
        let mut tools = start.iter().chain(end.iter()).collect::<Vec<_>>();
        let rest;
        let result = match (&start, &end) {
            (None, None) => return Err(invalid("limiting planes coincide")),
            (None, Some(end)) => end.clone(),
            (Some(start), None) => {
                rest = complement(start)?;
                tools.push(&rest);
                rest.clone()
            },
            (Some(start), Some(end)) => {
                let end_outside = end.difference(start)?;
                if end_outside.solids().len() == 0 {
                    rest = complement(start)?;
                    tools.push(&rest);
                    // Remove the common sketch cap and sew the exact patches;
                    // no new torus intersection is needed at this interface.
                    join_cells(&rest, end, from)?.ok_or_else(|| {
                        invalid("wrapped sweep sections do not share a complete sketch cap")
                    })?
                } else if start.difference(end)?.solids().len() == 0 {
                    end_outside
                } else {
                    return Err(invalid("limiting faces cross within the revolved profile"));
                }
            }
        };
        if result.solids().len() != 1 || result.volume() <= self.tolerance().linear.powi(3) {
            return Err(invalid("limiting faces do not enclose a valid revolution"));
        }
        let limits = names(&result, from).into_iter()
            .map(|id| (id, super::CreationRole::ProfileStart))
            .chain(names(&result, until).into_iter().map(|id| (id, super::CreationRole::ProfileEnd)))
            .collect::<Vec<_>>();
        Ok(result.with_construction_roles_from(&tools, &limits))
    }
}
