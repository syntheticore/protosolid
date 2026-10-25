//! Boolean combination, cuts and splits.

use super::*;

#[test]
fn boolean_cut_keeps_whole_periodic_shell_rims() {
    let body = cut_shell_before_lower_corner_fillets();
    let edges = upper_cut_shell_rims(&body);
    assert_eq!(edges.len(), 2, "Boolean publication must normalize the rims without a later blend");
    for edge in edges {
        let ends = edge.endpoints().unwrap();
        assert!(ends[0].x * ends[1].x < 0.0, "a rim must span both sides of the hidden seam");
    }
}

#[test]
fn analytical_planar_boolean_splits_lines_and_circles() {
    let tolerance = ModelTolerance::default();
    let circle = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 5.0).expect("circle")],
            tolerance,
        )
        .expect("circle wire"),
        [],
        tolerance,
    )
    .expect("circle profile");
    let right_half_plane = Profile::new(
        Wire2::polygon(
            [
                Point2::new(0.0, -6.0),
                Point2::new(6.0, -6.0),
                Point2::new(6.0, 6.0),
                Point2::new(0.0, 6.0),
            ],
            tolerance,
        )
        .expect("rectangle wire"),
        [],
        tolerance,
    )
    .expect("rectangle profile");
    let intersection = circle
        .boolean_planar(&right_half_plane, BooleanKind::Intersection)
        .expect("analytical intersection");
    let difference = circle
        .boolean_planar(&right_half_plane, BooleanKind::Difference)
        .expect("analytical difference");
    assert_eq!(intersection.len(), 1);
    assert_eq!(difference.len(), 1);
    let half_area = std::f64::consts::PI * 25.0 * 0.5;
    assert!((intersection[0].area() - half_area).abs() < 1.0e-8);
    assert!((difference[0].area() - half_area).abs() < 1.0e-8);
    assert!(intersection[0]
        .outer()
        .segments()
        .iter()
        .any(|segment| matches!(segment.curve(), Curve2::Circle(_))));
}

#[test]
fn coaxial_prism_boolean_preserves_partial_circular_boundaries() {
    let tolerance = ModelTolerance::default();
    let circle = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 5.0).expect("circle")],
            tolerance,
        )
        .expect("circle wire"),
        [],
        tolerance,
    )
    .expect("circle profile")
    .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 8.0))
    .expect("cylinder");
    let box_prism = Profile::new(
        Wire2::polygon(
            [
                Point2::new(0.0, -6.0),
                Point2::new(6.0, -6.0),
                Point2::new(6.0, 6.0),
                Point2::new(0.0, 6.0),
            ],
            tolerance,
        )
        .expect("rectangle wire"),
        [],
        tolerance,
    )
    .expect("rectangle profile")
    .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 8.0))
    .expect("box prism");
    let intersection = circle
        .intersection(&box_prism)
        .expect("analytical prism intersection");
    let difference = circle
        .difference(&box_prism)
        .expect("analytical prism difference");
    let union = circle.union(&box_prism).expect("analytical prism union");
    let half_volume = std::f64::consts::PI * 25.0 * 4.0;
    assert!((intersection.volume() - half_volume).abs() < 1.0e-7);
    assert!((difference.volume() - half_volume).abs() < 1.0e-7);
    assert!((union.volume() - (circle.volume() + box_prism.volume() - half_volume)).abs() < 1.0e-7);
    for result in [&intersection, &difference, &union] {
        assert!(result
            .faces()
            .all(|face| matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_))));
    }
}

#[test]
fn blind_circular_pocket_is_built_from_analytical_layers() {
    let tolerance = ModelTolerance::default();
    let block = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-10.0, -10.0),
                Point2::new(10.0, -10.0),
                Point2::new(10.0, 10.0),
                Point2::new(-10.0, 10.0),
            ],
            tolerance,
        )
        .expect("block wire"),
        [],
        tolerance,
    )
    .expect("block profile")
    .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
    .expect("block");
    let tool_plane = Plane3::new(
        Point3::new(0.0, 0.0, 20.0),
        Vec3::unit_y(),
        -Vec3::unit_x(),
        tolerance,
    )
    .expect("tool plane");
    let tool = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 3.0).expect("circle")],
            tolerance,
        )
        .expect("circle wire"),
        [],
        tolerance,
    )
    .expect("tool profile")
    .extrude(tool_plane, Vec3::new(0.0, 0.0, -10.0))
    .expect("tool");
    let pocket = block.difference(&tool).expect("blind analytical pocket");
    assert_eq!(pocket.solids().len(), 1);
    assert!((pocket.volume() - (8000.0 - 90.0 * std::f64::consts::PI)).abs() < 1.0e-6);
    assert!(pocket
        .faces()
        .all(|face| matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_))));
}

#[test]
fn blind_nurbs_pocket_has_exact_prismatic_mass_properties() {
    let tolerance = ModelTolerance::default();
    let block = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-10.0, -10.0),
                Point2::new(10.0, -10.0),
                Point2::new(10.0, 10.0),
                Point2::new(-10.0, 10.0),
            ],
            tolerance,
        )
        .expect("block wire"),
        [],
        tolerance,
    )
    .expect("block profile")
    .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
    .expect("block");
    let curved = TrimmedCurve2::new(
        NurbsCurve2::new(
            2,
            vec![
                Point2::new(3.0, 1.0),
                Point2::new(0.0, 4.0),
                Point2::new(-3.0, 1.0),
            ],
            vec![1.0; 3],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .expect("spline"),
        ParameterRange::new(0.0, 1.0).expect("range"),
    )
    .expect("trimmed spline");
    let pocket_profile = Profile::new(
        Wire2::new(
            [
                TrimmedCurve2::line(Point2::new(-3.0, -2.0), Point2::new(3.0, -2.0))
                    .expect("bottom"),
                TrimmedCurve2::line(Point2::new(3.0, -2.0), Point2::new(3.0, 1.0)).expect("right"),
                curved,
                TrimmedCurve2::line(Point2::new(-3.0, 1.0), Point2::new(-3.0, -2.0)).expect("left"),
            ],
            tolerance,
        )
        .expect("pocket wire"),
        [],
        tolerance,
    )
    .expect("pocket profile");
    let tool_plane = Plane3::new(
        Point3::new(0.0, 0.0, 20.0),
        Vec3::unit_y(),
        -Vec3::unit_x(),
        tolerance,
    )
    .expect("tool plane");
    let expected = 8_000.0 - pocket_profile.area() * 5.0;
    let tool = pocket_profile
        .extrude(tool_plane, Vec3::new(0.0, 0.0, -5.0))
        .expect("pocket tool");
    let pocket = block.difference(&tool).expect("exact NURBS pocket");
    assert!(
        (pocket.volume() - expected).abs() < 1.0e-6,
        "expected {expected}, got {}",
        pocket.volume()
    );
    assert!(pocket
        .faces()
        .any(|face| matches!(face.surface(), Surface3::Nurbs(_))));

    let boss = pocket_profile
        .extrude(tool_plane, Vec3::new(0.0, 0.0, 5.0))
        .expect("boss tool");
    let joined = block.union(&boss).expect("exact NURBS boss");
    assert!((joined.volume() - (8_000.0 + pocket_profile.area() * 5.0)).abs() < 1.0e-6);
    assert!(joined
        .faces()
        .any(|face| matches!(face.surface(), Surface3::Nurbs(_))));
}

#[test]
fn boolean_preserves_narrow_lenses_and_crescents_away_from_the_origin() {
    let tolerance = ModelTolerance::default();
    let separation: f64 = 0.002;
    let overlap = 2.0 * (separation * 0.5).acos()
        - separation * 0.5 * (4.0 - separation * separation).sqrt();
    let expected = (std::f64::consts::PI - overlap) * 2.0;
    for origin in [0.0, 10_000.0] {
        let cylinder = |x| {
            Profile::new(
                Wire2::new([TrimmedCurve2::circle(Point2::new(x, origin), 1.0).unwrap()], tolerance).unwrap(),
                [], tolerance,
            ).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 2.0).unwrap()
        };
        let first = cylinder(origin);
        let second = cylinder(origin + separation);
        for result in [first.difference(&second), second.difference(&first)] {
            let result = result.unwrap_or_else(|error| panic!("crescent at {origin}: {error}"));
            assert_eq!(result.solids().len(), 1);
            assert!((result.volume() - expected).abs() < 1e-6,
                "crescent at {origin}: {} versus {expected}", result.volume());
        }
        let separation: f64 = 1.999;
        let overlap = 2.0 * (separation * 0.5).acos()
            - separation * 0.5 * (4.0 - separation * separation).sqrt();
        let lens = first.intersection(&cylinder(origin + separation))
            .unwrap_or_else(|error| panic!("lens at {origin}: {error}"));
        assert_eq!(lens.solids().len(), 1);
        assert!((lens.volume() - overlap * 2.0).abs() < 1e-6);
    }
}

#[test]
fn boolean_repeated_bores_preserve_volume_and_topology() {
    let tolerance = ModelTolerance::default();
    let mut body = owned_cuboid(Point3::origin(), Point3::new(52.0, 6.0, 3.0));
    for index in 0..12 {
        let tool = Profile::new(
            Wire2::new([TrimmedCurve2::circle(Point2::new(3.0 + index as f64 * 4.0, 3.0), 0.6).unwrap()], tolerance).unwrap(),
            [], tolerance,
        ).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 3.0).unwrap();
        body = body.difference(&tool).unwrap_or_else(|error| panic!("bore {index}: {error}"));
        let expected = 52.0 * 6.0 * 3.0 - (index + 1) as f64 * std::f64::consts::PI * 0.36 * 3.0;
        assert!((body.volume() - expected).abs() < 1e-5);
        assert_eq!(body.solids().len(), 1);
        assert_eq!(body.faces().count(), 7 + index);
    }
}

#[test]
fn overlapping_cylinders_have_analytic_boolean_results() {
    let tolerance = ModelTolerance::default();
    let cylinder = |center: Point2, radius: f64| {
        Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(center, radius).expect("circle")],
                tolerance,
            )
            .expect("circle wire"),
            [],
            tolerance,
        )
        .expect("circle profile")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("cylinder")
    };
    let first = cylinder(Point2::new(0.0, 0.0), 15.0);
    let second = cylinder(Point2::new(6.0, 0.0), 10.0);
    assert_eq!(
        first
            .faces()
            .filter(|face| matches!(face.surface(), Surface3::Cylinder(_)))
            .count(),
        1
    );

    let intersection = first.intersection(&second).expect("intersection");
    let union = first.union(&second).expect("union");
    let difference = first.difference(&second).expect("difference");
    assert_eq!(intersection.solids().len(), 1);
    assert!(intersection.volume() > 0.0);
    assert!(intersection.volume() < first.volume());
    assert!(intersection.volume() < second.volume());
    assert!(
        (union.volume() - (first.volume() + second.volume() - intersection.volume())).abs()
            < 1.0e-5
    );
    assert!((difference.volume() - (first.volume() - intersection.volume())).abs() < 1.0e-5);
    for result in [&intersection, &union, &difference] {
        assert!(result
            .faces()
            .all(|face| matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_))));
        assert!(result
            .edges()
            .all(|edge| matches!(edge.curve(), Curve3::Line(_) | Curve3::Circle(_))));
    }
}

#[test]
fn analytic_intersection_reports_multiplicity_and_error() {
    let tolerance = ModelTolerance::default();
    let line: Curve3 = Line3::new(Point3::new(-2.0, 1.0, 0.0), Vec3::unit_x())
        .expect("line")
        .into();
    let sphere: Surface3 = Sphere3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        Vec3::unit_z(),
        tolerance,
    )
    .expect("sphere")
    .into();
    let IntersectionSet::Isolated(hits) = line
        .intersect_surface(&sphere, tolerance)
        .expect("intersection")
    else {
        panic!("tangent line should have one isolated hit");
    };
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].multiplicity, 2);
    assert!(hits[0].achieved_error <= tolerance.linear);

    let circle: Curve3 = Circle3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("circle")
    .into();
    let offset_sphere: Surface3 = Sphere3::from_radius(Point3::new(1.0, 0.0, 0.0), 1.0, tolerance)
        .expect("offset sphere")
        .into();
    let IntersectionSet::Isolated(circle_hits) = circle
        .intersect_surface(&offset_sphere, tolerance)
        .expect("circle-sphere intersection")
    else {
        panic!("offset sphere should cut the circle twice");
    };
    assert_eq!(circle_hits.len(), 2);
}

#[test]
fn nurbs_nurbs_intersections_are_isolated_without_polylines() {
    let make_curve = |points| {
        TrimmedCurve2::new(
            NurbsCurve2::new(2, points, vec![1.0; 3], vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
                .expect("quadratic spline"),
            ParameterRange::new(0.0, 1.0).expect("range"),
        )
        .expect("trimmed spline")
    };
    let first = make_curve(vec![
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 2.0),
        Point2::new(2.0, 0.0),
    ]);
    let second = make_curve(vec![
        Point2::new(0.0, 1.0),
        Point2::new(1.0, -1.0),
        Point2::new(2.0, 1.0),
    ]);
    let intersections =
        crate::wire::curve_pair_intersection(&first, &second, ModelTolerance::default())
            .expect("certified spline intersections");
    assert_eq!(intersections.hits.len(), 2);
    assert!(intersections
        .hits
        .iter()
        .all(|hit| (hit.point.y - 0.5).abs() < 1.0e-6));

    let circle = TrimmedCurve2::circle(Point2::new(1.0, 0.0), 1.0).expect("circle");
    let circle_hits =
        crate::wire::curve_pair_intersection(&first, &circle, ModelTolerance::default())
            .expect("certified spline/circle intersections");
    assert_eq!(circle_hits.hits.len(), 3);
}

#[test]
fn bounded_plane_cone_intersections_keep_both_nearby_branches() {
    let tolerance = ModelTolerance::default();
    let cone: Surface3 = crate::Cone3::new(
        Point3::origin(), Vec3::unit_z(), Vec3::unit_x(), Vec3::unit_y(), tolerance,
    ).unwrap().into();
    let plane: Surface3 = Plane3::new(
        Point3::new(1.0, 0.0, 0.0), Vec3::unit_y(), Vec3::unit_z(), tolerance,
    ).unwrap().into();
    let cone_bounds = [
        ParameterRange::new(0.0, std::f64::consts::TAU).unwrap(),
        ParameterRange::new(13.0, 14.0).unwrap(),
    ];
    // The half-space cutter is much larger than the small chamfer it cuts.
    // Its parameter span must not become a radius for merging distinct curves.
    for extent in [50.0, 500.0, 5000.0] {
        let plane_bounds = [ParameterRange::new(-extent, extent).unwrap(); 2];
        for reverse in [false, true] {
            let intersections = if reverse {
                plane.intersect_surface_in_bounds(&cone, plane_bounds, cone_bounds, tolerance)
            } else {
                cone.intersect_surface_in_bounds(&plane, cone_bounds, plane_bounds, tolerance)
            }.unwrap();
            let IntersectionSet::Isolated(curves) = intersections else {
                panic!("two hyperbola branches expected")
            };
            assert_eq!(curves.len(), 2, "extent {extent}, reverse {reverse}");
            for sign in [-1.0, 1.0] {
                assert!(curves.iter().any(|curve| {
                    let crate::DomainBounds::Bounded(range) = curve.curve.domain().bounds else {
                        panic!("bounded trace expected")
                    };
                    curve.curve.evaluate((range.start + range.end) * 0.5).unwrap().point.y * sign > 12.0
                }), "missing branch {sign}, extent {extent}, reverse {reverse}");
            }
        }
    }
}

#[test]
fn sections_across_chamfered_hole_keep_closed_caps() {
    // A wide body with a small chamfer reproduces complex.cad.cad's missing
    // section edges without its unrelated feature history.
    let tolerance = ModelTolerance::default();
    let plane = Plane3::new(
        Point3::new(0.0, 53.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tolerance,
    ).unwrap();
    let circle = |radius| Wire2::new(
        [TrimmedCurve2::circle(Point2::origin(), radius).unwrap()], tolerance,
    ).unwrap();
    let body = Profile::new(circle(300.0), [circle(14.0)], tolerance).unwrap()
        .extrude(plane, Vec3::unit_y() * 4.0).unwrap();
    let rims = body.edges().filter(|edge| matches!(edge.curve(), Curve3::Circle(c)
        if (c.radius() - 14.0).abs() < 1e-7)).cloned().collect::<Vec<_>>();
    assert_eq!(rims.len(), 2);
    let body = body.chamfer(&rims, 1.0).unwrap();
    let plane = Plane3::new(
        Point3::new(-4.0, 0.0, 0.0), -Vec3::unit_z(), Vec3::unit_y(), tolerance,
    ).unwrap();
    let left = body.split(plane, SplitKeep::Left).unwrap();
    let right = body.split(plane, SplitKeep::Right).unwrap();
    assert_eq!(left.solids().len(), 1);
    assert_eq!(right.solids().len(), 1);
    assert!((left.volume() + right.volume() - body.volume()).abs() < body.volume() * 1e-8);
    for cut in [&left, &right] {
        assert!(cut.volume() > 0.0);
        for face in cut.faces() {
            assert!(!face.tessellate(TessellationOptions::default()).unwrap().positions.is_empty());
        }
    }
}

#[test]
fn section_of_shelled_chamfered_cylinder() {
    // untitled(32).cad: sectioning at the outer cap must preserve both seam
    // uses; the inner cap also needs the offset fillet's exact contact rail.
    let tolerance = ModelTolerance::default();
    let plane = Plane3::new(Point3::origin(), Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
    let body = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 34.810472096424256).unwrap()],
            tolerance,
        )
        .unwrap(),
        [],
        tolerance,
    )
    .unwrap()
    .extrude(plane, Vec3::unit_y() * 32.0)
    .unwrap();
    let rim = body
        .edges()
        .find(|edge| edge.endpoints().unwrap().iter().all(|p| (p.y - 32.0).abs() < 1e-7))
        .unwrap()
        .clone();
    let body = body.chamfer_with_tangent_propagation(&[rim], 13.0, true).unwrap();
    let rims = body
        .edges()
        .filter(|edge| {
            let faces = edge.connected_faces().unwrap();
            faces.len() == 2 && faces.iter().any(|face| matches!(face.surface(), Surface3::Cone(_)))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(rims.len(), 2);
    let body = body
        .fillet_with_options(&rims, FilletOptions::new(5.0).with_tangent_propagation(true))
        .unwrap();
    let opening = body
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(p) if p.origin().y.abs() < 1e-7))
        .unwrap()
        .clone();
    let body = body.shell(&[opening], -4.0).unwrap();
    let volume = body.volume();
    for height in [0.0, 28.0, 32.0] {
        let section = Plane3::new(
            Point3::new(0.0, height, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tolerance,
        ).unwrap();
        let below = body.split(section, SplitKeep::Left).unwrap();
        let above = body.split(section, SplitKeep::Right).unwrap();
        assert!((below.volume() + above.volume() - volume).abs() < volume * 1e-8,
            "height {height}: {} + {} != {volume}", below.volume(), above.volume());
        if height == 28.0 {
            assert_eq!(below.solids().len(), 1);
            assert_eq!(above.solids().len(), 1);
            assert!(below.volume() > 0.0 && above.volume() > 0.0);
        } else {
            let (retained, empty) = if height == 0.0 { (&above, &below) } else { (&below, &above) };
            assert_eq!(retained.solids().len(), 1);
            assert_eq!(empty.solids().len(), 0);
        }
        for result in [below, above] {
            result.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
        }
    }
}

#[test]
fn section_of_chamfered_and_filleted_slot_frame() {
    // Reduced from untitled(28).cad.cad: the face move is not needed to expose
    // the missing offset-cone bounds or the tangent contact at the last sketch.
    let tolerance = ModelTolerance::default();
    let capsule = |left: f64, right: f64, y: f64, radius: f64| {
        Wire2::new(
            [
                TrimmedCurve2::line(Point2::new(left, y - radius), Point2::new(right, y - radius))
                    .unwrap(),
                TrimmedCurve2::arc_through(
                    Point2::new(right, y - radius),
                    Point2::new(right + radius, y),
                    Point2::new(right, y + radius),
                )
                .unwrap(),
                TrimmedCurve2::line(Point2::new(right, y + radius), Point2::new(left, y + radius))
                    .unwrap(),
                TrimmedCurve2::arc_through(
                    Point2::new(left, y + radius),
                    Point2::new(left - radius, y),
                    Point2::new(left, y - radius),
                )
                .unwrap(),
            ],
            tolerance,
        )
        .unwrap()
    };
    let plane = Plane3::new(Point3::origin(), Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
    let body = Profile::new(
        capsule(
            -41.08804749448451,
            39.24345469534933,
            1.828938871626858,
            22.669344768736725,
        ),
        [capsule(
            -35.09044151977286,
            32.38023301545568,
            0.6921057784646116,
            9.578459016090074,
        )],
        tolerance,
    )
    .unwrap()
    .extrude(plane, Vec3::unit_y() * 14.0)
    .unwrap();
    let at_height = |edge: &crate::EdgeHandle, heights: [f64; 2]| {
        let points = edge.endpoints().unwrap();
        heights.iter().any(|height| points.iter().all(|p| (p.y - height).abs() < 1e-7))
    };
    let rims = body.edges()
        .filter(|edge| at_height(edge, [0.0, 14.0]))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(rims.len(), 16);
    let chamfered = body
        .chamfer_with_tangent_propagation(&rims, 1.0, true)
        .unwrap();
    let rims = chamfered.edges()
        .filter(|edge| at_height(edge, [14.0, 1.0]))
        .cloned()
        .collect::<Vec<_>>();
    let rounded = chamfered
        .fillet_with_options(&rims, FilletOptions::new(0.75).with_tangent_propagation(true))
        .unwrap();
    let bounds = crate::bounds::compound(&rounded).unwrap();
    for (_, edge) in rounded.topology().edges() {
        for step in 0..=16 {
            let parameter = edge.range().start + edge.range().span() * step as f64 / 16.0;
            let point = edge.curve().evaluate(parameter).unwrap().point;
            assert!((0..3)
                .all(|axis| point[axis] >= bounds.0[axis] && point[axis] <= bounds.1[axis]));
        }
    }
    let section = Plane3::new(
        Point3::new(0.0, 14.0, 0.0),
        Vec3::unit_x(),
        -Vec3::unit_z(),
        tolerance,
    )
    .unwrap();
    let retained = rounded.split(section, SplitKeep::Left).unwrap();
    assert_eq!(retained.solids().len(), 1);
    assert!((retained.volume() - rounded.volume()).abs() < rounded.volume() * 1e-8);
    assert_eq!(rounded.split(section, SplitKeep::Right).unwrap().solids().len(), 0);
    let top = rounded
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().y.abs() > 0.99 && (p.origin().y - 14.0).abs() < 1e-7)
        })
        .unwrap();
    assert!(matches!(retained.resolve(top), Resolution::Preserved(_)));

    let boss = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(50.0, 1.8), 3.0).unwrap()],
            tolerance,
        )
        .unwrap(),
        [],
        tolerance,
    )
    .unwrap()
    .extrude(
        Plane3::new(
            Point3::new(0.0, 17.0, 0.0),
            Vec3::unit_x(),
            -Vec3::unit_z(),
            tolerance,
        )
        .unwrap(),
        -Vec3::unit_y() * 5.0,
    )
    .unwrap();
    let with_boss = rounded.union(&boss).unwrap();
    let top = with_boss
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().y.abs() > 0.99 && (p.origin().y - 14.0).abs() < 1e-7)
        })
        .unwrap();
    let retained = with_boss.split(section, SplitKeep::Left).unwrap();
    assert!(matches!(retained.resolve(top), Resolution::Preserved(_)));
    let section_faces = retained
        .faces()
        .filter(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().y.abs() > 0.99 && (p.origin().y - 14.0).abs() < 1e-7)
        })
        .count();
    assert_eq!(
        section_faces, 2,
        "the existing top and the new boss cap must remain separate"
    );
}

#[test]
fn enclosed_difference_nests_the_cavity_shell_in_its_body() {
    let block = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0));
    let cavity = owned_cuboid(Point3::new(4.0, 4.0, 4.0), Point3::new(6.0, 6.0, 6.0));

    let result = block
        .difference(&cavity)
        .expect("an enclosed subtraction should produce an inner BREP shell");

    assert_eq!(result.solids().len(), 1);
    assert!((result.volume() - 992.0).abs() < 1.0e-7);
}

#[test]
fn rectangular_blind_pocket_in_block_uses_valid_analytic_topology() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 20.0, 20.0));
    let cutter = Profile::new(
        Wire2::polygon(
            [
                Point2::new(4.0, -16.0),
                Point2::new(16.0, -16.0),
                Point2::new(16.0, -4.0),
                Point2::new(4.0, -4.0),
            ],
            tolerance,
        )
        .expect("cutter wire"),
        [],
        tolerance,
    )
    .expect("cutter profile")
    .extrude(
        Plane3::new(
            Point3::new(0.0, 0.0, 20.0),
            Vec3::unit_y(),
            -Vec3::unit_x(),
            tolerance,
        )
        .expect("top plane"),
        Vec3::new(0.0, 0.0, -17.0),
    )
    .expect("cutter");

    let pocket = block.difference(&cutter).expect("rectangular blind pocket");
    assert!((pocket.volume() - (8_000.0 - 12.0 * 12.0 * 17.0)).abs() < 1.0e-6);
    let edge_count = pocket.edges().count();
    assert_eq!(
        pocket
            .edges()
            .filter(|edge| pocket.is_semantic_edge(edge))
            .count(),
        edge_count,
        "every edge of an analytical rectangular pocket is a physical crease"
    );
}

#[test]
fn circular_blind_pocket_in_cylinder_uses_valid_analytic_topology() {
    let tolerance = ModelTolerance::default();
    let circle_profile = |radius| {
        Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(0.0, 0.0), radius).expect("circle")],
                tolerance,
            )
            .expect("circle wire"),
            [],
            tolerance,
        )
        .expect("circle profile")
    };
    let cylinder = circle_profile(10.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("cylinder");
    let cutter = circle_profile(7.0)
        .extrude(
            Plane3::new(
                Point3::new(0.0, 0.0, 20.0),
                Vec3::unit_y(),
                -Vec3::unit_x(),
                tolerance,
            )
            .expect("top plane"),
            Vec3::new(0.0, 0.0, -17.0),
        )
        .expect("cutter");

    for extrusion in [&cylinder, &cutter] {
        let wall = extrusion
            .faces()
            .find(|face| matches!(face.surface(), Surface3::Cylinder(_)))
            .unwrap();
        assert!(
            matches!(
                extrusion.origin(wall),
                Some(crate::Origin::Created {
                    role: Some(crate::CreationRole::ExtrusionSide {
                        boundary: crate::ProfileBoundary::Outer,
                        segment: 0,
                        part: 0,
                    }),
                })
            ),
            "merging periodic side faces must retain the extrusion's semantic name"
        );
    }

    let pocket = cylinder.difference(&cutter).expect("circular blind pocket");
    let expected = std::f64::consts::PI * (10.0_f64.powi(2) * 20.0 - 7.0_f64.powi(2) * 17.0);
    assert!((pocket.volume() - expected).abs() < 1.0e-6);
    assert!(
        pocket
            .edges()
            .filter(|edge| pocket.is_semantic_edge(edge))
            .count()
            >= 4
    );
}

#[test]
fn multi_solid_sections_preserve_uncut_face_history() {
    let first = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 13.0));
    let second = owned_cuboid(Point3::new(20.0, 0.0, 0.0), Point3::new(30.0, 10.0, 13.0));
    let pair = first.combined(&[second]).unwrap();
    let plane = |z| {
        Plane3::new(
            Point3::new(0.0, 0.0, z),
            Vec3::unit_x(),
            Vec3::unit_y(),
            ModelTolerance::default(),
        )
        .unwrap()
    };
    let unchanged = pair.split(plane(13.0), SplitKeep::Left).unwrap();
    assert!((unchanged.volume() - pair.volume()).abs() < 1e-6);
    for face in pair.faces() {
        assert!(
            matches!(unchanged.resolve(face), Resolution::Preserved(_)),
            "a tangent section must preserve face {:?}",
            face.persistent_id()
        );
    }
    // A second section also traverses the recombined result's lineage.
    let repeated = unchanged.split(plane(13.0), SplitKeep::Left).unwrap();
    for face in pair.faces().chain(first.faces()) {
        assert!(matches!(repeated.resolve(face), Resolution::Preserved(_)));
    }

    let cut = pair.split(plane(6.0), SplitKeep::Left).unwrap();
    assert!((cut.volume() - pair.volume() * 6.0 / 13.0).abs() < 1e-6);
    for face in pair.faces() {
        if let Surface3::Plane(p) = face.surface() {
            if p.normal().z.abs() > 0.9 {
                if p.origin().z == 0.0 {
                    assert!(matches!(cut.resolve(face), Resolution::Preserved(_)));
                } else {
                    assert!(matches!(cut.resolve(face), Resolution::Deleted));
                }
            } else {
                assert!(matches!(cut.resolve(face), Resolution::Modified(_)));
            }
        }
    }
    assert_eq!(
        cut.faces().filter(|face| matches!(face.surface(), Surface3::Plane(p)
            if p.normal().z.abs() > 0.9 && (p.origin().z - 6.0).abs() < 1e-7)).count(),
        2
    );
}

#[test]
fn multi_solid_booleans_use_set_semantics() {
    let left = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 2.0, 2.0));
    let right = owned_cuboid(Point3::new(4.0, 0.0, 0.0), Point3::new(6.0, 2.0, 2.0));
    let pair = left.combined(&[right]).expect("disconnected compound");
    let bridge = owned_cuboid(Point3::new(1.0, 0.0, 0.0), Point3::new(5.0, 2.0, 2.0));
    let united = pair.union(&bridge).expect("multi-solid union");
    assert_eq!(united.solids().len(), 1);
    assert!((united.volume() - 24.0).abs() < 1.0e-6);

    let cutter = owned_cuboid(Point3::new(0.5, -1.0, -1.0), Point3::new(5.5, 3.0, 3.0));
    let cut = pair.difference(&cutter).expect("multi-solid difference");
    assert_eq!(cut.solids().len(), 2);
    assert!((cut.volume() - 4.0).abs() < 1.0e-6);
}

#[test]
fn boolean_union_keeps_edge_touching_solids_separate() {
    let left = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 2.0, 1.0));
    let right = owned_cuboid(Point3::new(4.0, 0.0, 0.0), Point3::new(6.0, 2.0, 1.0));
    let pair = left.union(&right).unwrap();
    let extrude = |points: Vec<Point2>| {
        let tolerance = ModelTolerance::default();
        Profile::new(Wire2::polygon(points, tolerance).unwrap(), [], tolerance)
            .unwrap()
            .extrude(Plane3::xy(), Vec3::unit_z())
            .unwrap()
    };
    let lower = extrude(vec![
        Point2::new(2.0, 0.0),
        Point2::new(3.0, -2.0),
        Point2::new(4.0, 0.0),
        Point2::new(3.0, -0.5),
    ]);
    let diagonal = extrude(vec![
        Point2::new(0.0, 0.0),
        Point2::new(-2.0, -1.0),
        Point2::new(-1.0, -2.0),
    ]);

    for extra in [&lower, &diagonal] {
        for result in [pair.union(extra).unwrap(), extra.union(&pair).unwrap()] {
            assert_eq!(result.solids().len(), 3);
            assert!((result.volume() - pair.volume() - extra.volume()).abs() < 1e-7);
            result.tessellate(TessellationOptions::default()).unwrap();
            assert_eq!(
                result.edges().count(),
                pair.edges().count() + extra.edges().count(),
            );
        }
    }
    let all = pair.union(&lower).unwrap().union(&diagonal).unwrap();
    assert_eq!(all.solids().len(), 4);
    assert!((all.volume() - pair.volume() - lower.volume() - diagonal.volume()).abs() < 1e-7);
}

#[test]
fn boolean_history_keeps_disjoint_coplanar_face_sources_separate() {
    let block = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let slot = owned_cuboid(Point3::new(4.0, -1.0, 5.0), Point3::new(6.0, 11.0, 11.0));
    let slotted = block.difference(&slot).unwrap();
    let tops = slotted
        .faces()
        .filter(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().z.abs() > 0.9 && (p.origin().z - 10.0).abs() < 1e-7)
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(tops.len(), 2);
    let cutter = owned_cuboid(Point3::new(-1.0, 5.0, -1.0), Point3::new(11.0, 11.0, 11.0));
    let cut = slotted.difference(&cutter).unwrap();
    let mut descendants = std::collections::BTreeSet::new();
    for top in &tops {
        match cut.resolve(top) {
            Resolution::Modified(face) => {
                descendants.insert(face.persistent_id());
            }
            other => panic!("each trimmed top has one distinct descendant: {other:?}"),
        }
    }
    assert_eq!(descendants.len(), 2);
}

#[test]
fn boolean_construction_reports_split_edges_and_retained_vertices() {
    let source = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let cutter = owned_cuboid(Point3::new(4.0, -1.0, -1.0), Point3::new(6.0, 11.0, 11.0));
    let result = source.difference(&cutter).unwrap();
    for vertex in source.vertices() {
        assert!(
            matches!(result.resolve(vertex), Resolution::Preserved(_)),
            "retained corner"
        );
    }
    for edge in source.edges() {
        let points = edge.endpoints().unwrap();
        if (points[0].x - points[1].x).abs() > 9.0 {
            assert!(
                matches!(result.resolve(edge), Resolution::Split(parts) if parts.len() == 2),
                "split source edge"
            );
        } else {
            assert!(
                matches!(result.resolve(edge), Resolution::Preserved(_)),
                "untouched source edge"
            );
        }
    }
}

#[test]
fn torus_sections_preserve_exact_circles_in_scaled_and_skewed_plane_charts() {
    let tol = ModelTolerance::default();
    let torus: Surface3 = crate::Torus3::new(
        Point3::new(3.0, -2.0, 5.0),
        Vec3::unit_x() * 40.0,
        Vec3::unit_y() * 40.0,
        20.0,
        tol,
    )
    .unwrap()
    .into();
    for (u, v) in [
        (Vec3::unit_x(), Vec3::unit_y()),
        (Vec3::unit_x() * 24.0, Vec3::unit_y() * 60.0),
        (Vec3::new(24.0, 9.0, 0.0), Vec3::new(-8.0, 60.0, 0.0)),
        (Vec3::unit_x() * 24.0, -Vec3::unit_y() * 60.0),
    ] {
        let plane: Surface3 = Plane3::new(Point3::new(-6.0, 7.0, 17.0), u, v, tol)
            .unwrap()
            .into();
        for (first, second) in [(&plane, &torus), (&torus, &plane)] {
            let (Surface3::Plane(p), Surface3::Torus(t)) = (&plane, &torus) else {
                unreachable!()
            };
            let sections = crate::intersection::parallel_plane_torus_section(*p, *t, tol).unwrap();
            let sections = if matches!(first, Surface3::Plane(_)) {
                sections
            } else {
                crate::intersection::swap_surface_curves(sections)
            };
            let IntersectionSet::Isolated(curves) = sections else {
                panic!("two circular sections expected")
            };
            assert_eq!(curves.len(), 2);
            let mut radii = Vec::new();
            for intersection in curves {
                let Curve3::Circle(circle) = &intersection.curve else {
                    panic!("section must remain an exact circle")
                };
                radii.push(circle.radius());
                for parameter in [-1.0, 0.0, 0.4, 2.0, 5.0, 7.0] {
                    let curve = intersection.curve.evaluate(parameter).unwrap();
                    for (surface, pcurve) in [
                        (first, &intersection.first_pcurve),
                        (second, &intersection.second_pcurve),
                    ] {
                        let uv = pcurve.evaluate(parameter).unwrap();
                        let value = surface.evaluate(uv.point.x, uv.point.y).unwrap();
                        assert!(value.point.distance(curve.point) < tol.linear);
                        let tangent =
                            value.du * uv.first_derivative.x + value.dv * uv.first_derivative.y;
                        assert!((tangent - curve.first_derivative).magnitude() < tol.linear);
                    }
                }
            }
            radii.sort_by(f64::total_cmp);
            assert!((radii[0] - 24.0).abs() < tol.linear);
            assert!((radii[1] - 56.0).abs() < tol.linear);
        }
    }
}

#[test]
fn boolean_unifies_circular_rims_across_the_old_seam() {
    let (cut, _) = circular_cut_on_offset_rounded_shell();
    let rims = cut
        .edges()
        .filter(|edge| {
            matches!(edge.curve(), Curve3::Circle(circle)
            if circle.center().to_vec().magnitude() < 1e-7)
        })
        .collect::<Vec<_>>();
    assert_eq!(rims.len(), 2, "each bottom rim should be one circular arc");
    for edge in rims {
        let ends = edge.endpoints().unwrap();
        assert!(ends[0].distance(ends[1]) > 1.0);
        assert!(
            ends.iter().all(|p| p.x < -1.0),
            "old x=0 seam vertex survived"
        );
        assert_eq!(edge.connected_faces().unwrap().len(), 2);
    }
    cut.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
        .unwrap();
}
