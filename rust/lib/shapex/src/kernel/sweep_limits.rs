//! Face-limited sweeps. Construction envelopes are disposable; published faces
//! retain the exact surfaces, trim curves and certified Boolean topology.
use super::{
    Axis3, Compound, DiagnosticKind, FaceHandle, KernelError, KernelResult, ModelTolerance,
    PersistentId, Plane3, Profile, Resolution, ResolvableEntity, Surface3, TessellationOptions,
};
use crate::{Matrix3, Point3, Vec3};
use cgmath::prelude::*;
use std::collections::BTreeSet;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn invalid(message: &str) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}

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
    super::euler::conform_brep_interfaces(&mut patches, first.tolerance())?;
    let samples = patches
        .iter()
        .map(super::boolean::patch_edge_samples)
        .collect::<KernelResult<Vec<_>>>()?;
    let mut removed = BTreeSet::new();
    for i in 0..first.faces().count() {
        for j in first.faces().count()..patches.len() {
            if !super::boolean::coincident_supports(
                &patches[i].surface,
                &patches[j].surface,
                first.tolerance(),
            )? {
                continue;
            }
            let point = samples[i][0][1];
            let oriented_normal = |patch: &super::euler::BrepPatch| -> KernelResult<Vec3> {
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
    super::euler::conform_brep_boundaries(&mut patches, first.tolerance())?;
    let mut kernel = super::Kernel::new(first.tolerance())?;
    let mut transaction = kernel.begin_operation();
    let body = transaction.brep_shell(&patches)?;
    transaction.nest_brep_shells(&[body], TessellationOptions::default())?;
    transaction.unify_same_domain()?;
    let committed = transaction.finish()?;
    Ok(Some(
        Compound::from_operation(kernel, committed, [first, second])?.with_boolean_parents(
            first,
            second,
            "sweep.join_cells",
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
        vec![tool.split(plane, "both")?]
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

fn profile_center(profile: &Profile, plane: Plane3) -> KernelResult<Point3> {
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
mod tests {
    use super::super::{TrimmedCurve2, Wire2};
    use super::*;
    use crate::Point2;

    fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Profile {
        let tol = ModelTolerance::default();
        Profile::new(
            Wire2::polygon(
                [
                    Point2::new(x0, y0),
                    Point2::new(x1, y0),
                    Point2::new(x1, y1),
                    Point2::new(x0, y1),
                ],
                tol,
            )
            .unwrap(),
            [],
            tol,
        )
        .unwrap()
    }

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-6 * expected.abs().max(1.0),
            "{actual} != {expected}"
        );
    }

    #[test]
    fn face_limited_circular_extrusion_preserves_start_rim() {
        let tol = ModelTolerance::default();
        let profile = Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(
                    Point2::new(10.202437506387206, 7.213174143629522),
                    20.834734325948986,
                )
                .unwrap()],
                tol,
            )
            .unwrap(),
            [],
            tol,
        )
        .unwrap();
        let plane = Plane3::new(
            Point3::new(38.0, 0.0, 0.0),
            -Vec3::unit_z(),
            Vec3::unit_y(),
            tol,
        )
        .unwrap();
        let body = profile.extrude(plane, -Vec3::unit_x() * 38.0).unwrap();
        let limit_plane = shifted(plane, -Vec3::unit_x() * 38.0, tol).unwrap();
        let limit = cap(&body, limit_plane).unwrap();
        let result = profile
            .extrude_up_to_face(plane, &body, &limit, 0.0)
            .unwrap();
        near(result.volume(), PI * 20.834734325948986_f64.powi(2) * 38.0);
    }

    #[test]
    fn face_limited_extrusion_offset_and_invalid_start() {
        let profile = rectangle(-1.0, -1.0, 1.0, 1.0);
        let plane = Plane3::xy();
        let target = rectangle(20.0, 20.0, 21.0, 21.0)
            .extrude(plane, Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        let limit = cap(
            &target,
            shifted(plane, Vec3::new(0.0, 0.0, 10.0), profile.tolerance()).unwrap(),
        )
        .unwrap();
        near(
            profile
                .extrude_up_to_face(plane, &target, &limit, 2.0)
                .unwrap()
                .volume(),
            32.0,
        );
        assert!(profile
            .extrude_up_to_face(plane, &target, &limit, 11.0)
            .is_err());
        assert!(profile
            .extrude_up_to_face(plane, &target, &limit, f64::NAN)
            .is_err());
    }

    #[test]
    fn face_limited_curved_extrusion_has_exact_volume() {
        let tol = ModelTolerance::default();
        let circle = Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 5.0).unwrap()],
                tol,
            )
            .unwrap(),
            [],
            tol,
        )
        .unwrap();
        let plane = Plane3::new(
            Point3::new(0.0, -10.0, 10.0),
            Vec3::unit_x(),
            Vec3::unit_z(),
            tol,
        )
        .unwrap();
        let target = circle.extrude(plane, Vec3::new(0.0, 20.0, 0.0)).unwrap();
        let limit = target
            .faces()
            .find(|f| matches!(f.surface(), Surface3::Cylinder(_)))
            .unwrap();
        let profile = rectangle(-1.0, -1.0, 1.0, 1.0);
        let result = profile
            .extrude_up_to_face(Plane3::xy(), &target, limit, 0.0)
            .unwrap();
        near(
            result.volume(),
            40.0 - 2.0 * (24.0_f64.sqrt() + 25.0 * 0.2_f64.asin()),
        );
        assert_eq!(result.solids().len(), 1);
        assert!(rectangle(-6.0, -1.0, 6.0, 1.0)
            .extrude_up_to_face(Plane3::xy(), &target, limit, 0.0)
            .is_err());
    }

    #[test]
    fn face_limited_revolution_between_radial_planes_wraps() {
        let profile = rectangle(2.0, -1.0, 3.0, 1.0);
        let plane = Plane3::xz();
        let axis = Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_z()).unwrap();
        let from_plane = turned(plane, axis, PI / 3.0, profile.tolerance()).unwrap();
        let until_plane = turned(plane, axis, PI / 6.0, profile.tolerance()).unwrap();
        let from_body = profile.extrude(from_plane, from_plane.normal()).unwrap();
        let until_body = profile.extrude(until_plane, until_plane.normal()).unwrap();
        let from = cap(&from_body, from_plane).unwrap();
        let until = cap(&until_body, until_plane).unwrap();
        near(
            profile
                .revolve_up_to_face(plane, axis, &from_body, &from, true)
                .unwrap()
                .volume(),
            5.0 * PI / 3.0,
        );
        near(
            profile
                .revolve_between_faces(plane, axis, &from_body, &from, &until_body, &until, true)
                .unwrap()
                .volume(),
            55.0 * PI / 6.0,
        );
        assert!(profile
            .revolve_between_faces(plane, axis, &from_body, &from, &from_body, &from, true)
            .is_err());
    }

    #[test]
    fn face_limited_circle_touching_cylinder_cut_document_8() {
        let tol = ModelTolerance::default();
        let circle = |x, y, radius| {
            Profile::new(
                Wire2::new(
                    [TrimmedCurve2::circle(Point2::new(x, y), radius).unwrap()],
                    tol,
                )
                .unwrap(),
                [],
                tol,
            )
            .unwrap()
        };
        let cylinder_plane = Plane3::new(
            Point3::new(-41.0, 0.0, 0.0),
            -Vec3::unit_z(),
            Vec3::unit_y(),
            tol,
        )
        .unwrap();
        let body = circle(9.609744834740159, 11.348191136502194, 26.23894553307264)
            .extrude(cylinder_plane, Vec3::new(82.0, 0.0, 0.0))
            .unwrap();
        let profile_plane = Plane3::new(
            Point3::new(0.0, -49.0, 0.0),
            Vec3::unit_x(),
            -Vec3::unit_z(),
            tol,
        )
        .unwrap();
        let profile = circle(-1.3457094482737189, 12.454480426114497, 14.912577936970921);
        let limit = body
            .faces()
            .find(|f| matches!(f.surface(), Surface3::Cylinder(_)))
            .unwrap();
        let tool = profile
            .extrude_up_to_face(profile_plane, &body, limit, 0.0)
            .unwrap();
        near(
            body.union(&tool).unwrap().volume(),
            body.volume() + tool.volume(),
        );
        near(body.difference(&tool).unwrap().volume(), body.volume());
        assert_eq!(body.intersection(&tool).unwrap().solids().len(), 0);
    }

    fn torus_to_spline_document_9() -> (Profile, Plane3, Axis3, Compound, Compound) {
        let tol = ModelTolerance::default();
        let region = |controls: [[f64; 2]; 4], closure: &[[f64; 2]]| {
            let points = controls.map(|p| Point2::new(p[0], p[1]));
            let curve = super::super::NurbsCurve2::new(
                3,
                points.to_vec(),
                vec![1.0; 4],
                vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            )
            .unwrap();
            let mut segments = vec![TrimmedCurve2::new(
                curve,
                super::super::ParameterRange::new(0.0, 1.0).unwrap(),
            )
            .unwrap()];
            let mut last = points[3];
            for next in closure
                .iter()
                .map(|p| Point2::new(p[0], p[1]))
                .chain([points[0]])
            {
                segments.push(TrimmedCurve2::line(last, next).unwrap());
                last = next;
            }
            Profile::new(Wire2::new(segments, tol).unwrap(), [], tol).unwrap()
        };
        let plane = Plane3::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::unit_x(),
            -Vec3::unit_z(),
            tol,
        )
        .unwrap();
        let base = rectangle(
            44.19017168491189,
            -23.728570731280954,
            180.02001605513482,
            49.88186507061719,
        )
        .extrude(plane, Vec3::new(0.0, 128.0, 0.0))
        .unwrap();
        let cut_plane = Plane3::new(
            Point3::new(180.02001605513482, 0.0, -49.88186507061719 + 91.0),
            -Vec3::unit_x(),
            Vec3::unit_y(),
            tol,
        )
        .unwrap();
        let first = region(
            [
                [137.80435932379862, -31.752511261917917],
                [46.965475801696556, 25.533271139407766],
                [165.62888220444262, 103.68744570121633],
                [44.51037084163971, 145.01504586217268],
            ],
            &[
                [-72.19280582114749, 145.01504586217268],
                [-76.50276353016889, -157.11517903614077],
            ],
        );
        let second = region(
            [
                [162.35540892436683, -27.251485501813743],
                [70.87331025848803, 19.42492660074398],
                [167.92365564135562, 54.830915795796656],
                [95.24920668281388, 144.60586170216317],
            ],
            &[
                [191.31689850237254, 147.45551252928263],
                [202.75925920931505, -46.456012293223154],
                [154.20382390545947, -136.02596598718438],
            ],
        );
        let cutters = first
            .extrude(cut_plane, Vec3::new(0.0, 0.0, -182.0))
            .unwrap()
            .union(
                &second
                    .extrude(cut_plane, Vec3::new(0.0, 0.0, -182.0))
                    .unwrap(),
            )
            .unwrap();
        let upper = base.difference(&cutters).unwrap();
        let lower = region(
            [
                [-102.68630243985827, -75.05002357528167],
                [-32.6186594006202, -26.488290775809737],
                [21.526512486995706, -58.63012427391142],
                [43.563408352900865, 1.634039930808811],
            ],
            &[
                [164.40322795723728, -83.72176157518734],
                [69.36097947827082, -113.55254029486298],
                [-78.05856652012606, -130.54914677467818],
            ],
        )
        .extrude(
            shifted(Plane3::xy(), Vec3::new(0.0, 0.0, -76.0), tol).unwrap(),
            Vec3::new(0.0, 0.0, 152.0),
        )
        .unwrap();
        let profile = Profile::new(
            Wire2::new(
                [
                    TrimmedCurve2::circle(Point2::new(-23.61389560011505, 9.513164485593606), 20.0)
                        .unwrap(),
                ],
                tol,
            )
            .unwrap(),
            [],
            tol,
        )
        .unwrap();
        let axis = Axis3::through(
            Point3::new(26.49354390191479, 0.0, -68.45286216623794),
            Point3::new(34.56264356747255, 0.0, 61.459642455738184),
        )
        .unwrap();
        (profile, plane, axis, upper, lower)
    }

    #[test]
    fn shell_revolved_spline_junction_document_5() {
        // untitled(5).cad.cad.cad.cad.cad.cad has the same source sketches
        // as document 9. Cover the saved body first, then its rounded variant.
        let (profile, plane, axis, upper, lower) = torus_to_spline_document_9();
        let from = upper
            .faces()
            .filter(|f| matches!(f.surface(), Surface3::Nurbs(_)))
            .nth(1)
            .unwrap();
        let until = lower
            .faces()
            .find(|f| matches!(f.surface(), Surface3::Nurbs(_)))
            .unwrap();
        let tool = profile
            .revolve_between_faces(plane, axis, &upper, from, &lower, until, true)
            .unwrap();
        let joined = upper.combined(&[lower]).unwrap().union(&tool).unwrap();
        let closed = joined.shell(&[], -1.0).expect("unfilleted closed shell");
        assert_eq!(closed.solids().len(), 1);
        assert!(closed.volume() > 0.0 && closed.volume() < joined.volume());
        closed
            .tessellate(super::super::TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap();
        for opening in joined
            .faces()
            .filter(|face| matches!(face.surface(), Surface3::Plane(_)))
        {
            let shell = joined
                .shell(&[opening.clone()], -1.0)
                .unwrap_or_else(|error| panic!("unfilleted opening {:?}: {error}", opening.surface()));
            assert_eq!(shell.solids().len(), 1);
            assert!(shell.volume() > 0.0 && shell.volume() < joined.volume());
            shell
                .tessellate(super::super::TessellationOptions::new(0.1, 0.1, 24).unwrap())
                .unwrap();
        }
        let rims = joined
            .edges()
            .filter(|edge| {
                let faces = edge.connected_faces().unwrap();
                faces.iter().any(|f| matches!(f.surface(), Surface3::Torus(_)))
                    && faces.iter().any(|f| matches!(f.surface(), Surface3::Nurbs(_)))
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(rims.len(), 7); // The redundant subdivision of one closed junction is unified.
        let rounded = joined.fillet(&rims, 2.0).unwrap();
        for opening in rounded
            .faces()
            .filter(|face| matches!(face.surface(), Surface3::Plane(_)))
        {
            let shell = rounded
                .shell(&[opening.clone()], -1.0)
                .unwrap_or_else(|error| panic!("opening {:?}: {error}", opening.surface()));
            assert_eq!(shell.solids().len(), 1);
            assert!(shell.volume() > 0.0 && shell.volume() < rounded.volume());
            shell
                .tessellate(super::super::TessellationOptions::new(0.1, 0.1, 24).unwrap())
                .unwrap();
        }
    }

    #[test]
    fn fillet_revolved_spline_junction_document_9() {
        let (profile, plane, axis, upper, _) = torus_to_spline_document_9();
        let face = upper
            .faces()
            .filter(|f| matches!(f.surface(), Surface3::Nurbs(_)))
            .nth(1)
            .unwrap();
        let tool = profile
            .revolve_up_to_face(plane, axis, &upper, face, false)
            .unwrap();
        let joined = upper.union(&tool).unwrap();
        let rim = joined
            .edges()
            .find(|edge| {
                let faces = edge.connected_faces().unwrap();
                faces
                    .iter()
                    .any(|f| matches!(f.surface(), Surface3::Torus(_)))
                    && faces
                        .iter()
                        .any(|f| matches!(f.surface(), Surface3::Nurbs(_)))
            })
            .unwrap()
            .clone();
        for radius in [0.5, 1.0, 2.0, 5.0] {
            joined
                .fillet(&[rim.clone()], radius)
                .unwrap_or_else(|error| {
                    panic!("radius {radius}: {error}");
                });
        }
    }

    #[test]
    fn face_limited_torus_to_spline_document_9() {
        let (profile, plane, axis, upper, lower) = torus_to_spline_document_9();
        let mut failures = Vec::new();
        let body = upper.combined(&[lower.clone()]).unwrap();
        let from = upper
            .faces()
            .filter(|f| matches!(f.surface(), Surface3::Nurbs(_)))
            .nth(1)
            .unwrap();
        let until = lower
            .faces()
            .find(|f| matches!(f.surface(), Surface3::Nurbs(_)))
            .unwrap();
        for (order, (from_body, from, until_body, until)) in
            [(&upper, from, &lower, until), (&lower, until, &upper, from)]
                .into_iter()
                .enumerate()
        {
            for forward in [true, false] {
                match profile
                    .revolve_between_faces(plane, axis, from_body, from, until_body, until, forward)
                {
                    Ok(result) => {
                        assert_eq!(result.solids().len(), 1);
                        for (operation, output) in [
                            ("join", body.union(&result)),
                            ("cut", body.difference(&result)),
                            ("intersect", body.intersection(&result)),
                        ] {
                            if let Err(error) = output {
                                failures.push(format!(
                                    "between {order}/{forward} {operation}: {error}"
                                ));
                            }
                        }
                    }
                    Err(error) => failures.push(format!("between {order}/{forward}: {error}")),
                }
            }
        }
        for (label, body) in [("upper", upper), ("lower", lower)] {
            for (index, face) in body
                .faces()
                .filter(|f| matches!(f.surface(), Surface3::Nurbs(_)))
                .enumerate()
            {
                for forward in [true, false] {
                    if label == "upper" && index == 0 {
                        // The first cutter's wall ends inside the torus at y=0.
                        // Its intersection is open, unlike the two full caps.
                        let error = profile
                            .revolve_up_to_face(plane, axis, &body, face, forward)
                            .unwrap_err();
                        assert!(error.to_string().contains("must cap the entire profile"));
                        continue;
                    }
                    match profile.revolve_up_to_face(plane, axis, &body, face, forward) {
                        Ok(result) => {
                            assert_eq!(result.solids().len(), 1);
                            for (operation, output) in [
                                ("join", body.union(&result)),
                                ("cut", body.difference(&result)),
                                ("intersect", body.intersection(&result)),
                            ] {
                                if let Err(error) = output {
                                    failures.push(format!(
                                        "{label} {index} {forward} {operation}: {error}"
                                    ));
                                }
                            }
                        }
                        Err(error) => failures.push(format!("{label} {index} {forward}: {error}")),
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

impl Profile {
    /// Sweep to the selected surface. Planar limits use their infinite support;
    /// curved limits must cover the full profile with their trimmed face.
    /// The offset is measured from the sketch toward the automatically chosen limit.
    pub fn extrude_up_to_face(
        &self,
        plane: Plane3,
        target: &Compound,
        limit: &FaceHandle,
        offset: f64,
    ) -> KernelResult<Compound> {
        check_limit(target, limit)?;
        if !offset.is_finite() {
            return Err(invalid("extrusion offset must be finite"));
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
        let start_plane = shifted(plane, direction * offset, tol)?;
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
                .map(|point| (point - (origin + direction * offset)).dot(direction))
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
        // Publish the high-level extrusion's roles after trimming, so cap and
        // sketch-segment names remain stable when switching extent modes.
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
        Ok(result.with_creation_roles(roles))
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
                return Ok(result);
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
        let result = match (start, end) {
            (None, None) => return Err(invalid("limiting planes coincide")),
            (None, Some(end)) => end,
            (Some(start), None) => complement(&start)?,
            (Some(start), Some(end)) => {
                let end_outside = end.difference(&start)?;
                if end_outside.solids().len() == 0 {
                    let rest = complement(&start)?;
                    // Remove the common sketch cap and sew the exact patches;
                    // no new torus intersection is needed at this interface.
                    join_cells(&rest, &end, from)?.ok_or_else(|| {
                        invalid("wrapped sweep sections do not share a complete sketch cap")
                    })?
                } else if start.difference(&end)?.solids().len() == 0 {
                    end_outside
                } else {
                    return Err(invalid("limiting faces cross within the revolved profile"));
                }
            }
        };
        if result.solids().len() != 1 || result.volume() <= self.tolerance().linear.powi(3) {
            return Err(invalid("limiting faces do not enclose a valid revolution"));
        }
        Ok(result)
    }
}
