use super::super::{ShellJoin, TrimmedCurve2, Wire2};
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
        .extrude_up_to_face(plane, &body, &limit, 0.0, 0.0)
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
            .extrude_up_to_face(plane, &target, &limit, 2.0, 0.0)
            .unwrap()
            .volume(),
        32.0,
    );
    assert!(profile
        .extrude_up_to_face(plane, &target, &limit, 11.0, 0.0)
        .is_err());
    assert!(profile
        .extrude_up_to_face(plane, &target, &limit, f64::NAN, 0.0)
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
        .extrude_up_to_face(Plane3::xy(), &target, limit, 0.0, 0.0)
        .unwrap();
    near(
        result.volume(),
        40.0 - 2.0 * (24.0_f64.sqrt() + 25.0 * 0.2_f64.asin()),
    );
    assert_eq!(result.solids().len(), 1);
    assert!(rectangle(-6.0, -1.0, 6.0, 1.0)
        .extrude_up_to_face(Plane3::xy(), &target, limit, 0.0, 0.0)
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
        .extrude_up_to_face(profile_plane, &body, limit, 0.0, 0.0)
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
    let closed = joined
        .shell_with_join(&[], -1.0, ShellJoin::Sharp)
        .expect("unfilleted closed shell");
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
            .shell_with_join(&[opening.clone()], -1.0, ShellJoin::Sharp)
            .unwrap_or_else(|error| {
                panic!("unfilleted opening {:?}: {error}", opening.surface())
            });
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
    assert_eq!(rims.len(), 2); // Each closed junction is unified across its chart subdivisions.
    let rounded = joined.fillet(&rims, 2.0).unwrap();
    for opening in rounded
        .faces()
        .filter(|face| matches!(face.surface(), Surface3::Plane(_)))
    {
        let shell = rounded
            .shell_with_join(&[opening.clone()], -1.0, ShellJoin::Sharp)
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
