//! Extrusions, revolutions, sweeps and lofts.

use super::*;

#[test]
fn revolved_surface_retains_its_axis_and_exact_meridian() {
    let tolerance = ModelTolerance::default();
    let frame = crate::Frame3::new(
        Point3::new(1.0, 2.0, 3.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("revolution frame");
    let profile: Curve2 = crate::Circle2::new(Point2::new(4.0, 0.0), 1.0)
        .expect("circular meridian")
        .into();
    let surface: Surface3 = RevolvedSurface3::new(
        frame,
        profile,
        ParameterRange::new(-1.0, 1.0).expect("profile range"),
        tolerance,
    )
    .expect("regular revolved surface")
    .into();
    let point = surface
        .evaluate(std::f64::consts::FRAC_PI_2, 0.0)
        .expect("revolved evaluation")
        .point;
    assert!(point.distance(Point3::new(1.0, 7.0, 3.0)) < 1.0e-12);
    let projection = surface
        .project(Point3::new(1.0, 7.25, 3.0), tolerance)
        .expect("revolved projection");
    assert!(projection.point.distance(point) < 1.0e-12);
    assert!((projection.parameters.0 - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);

    let periodic: Surface3 = RevolvedSurface3::new(
        frame,
        crate::Circle2::new(Point2::new(4.0, 0.0), 1.0)
            .expect("periodic circular meridian")
            .into(),
        ParameterRange::new(0.0, std::f64::consts::TAU).expect("full profile period"),
        tolerance,
    )
    .expect("periodic revolved surface")
    .into();
    assert_eq!(periodic.domains()[1].period, Some(std::f64::consts::TAU));
    let first = periodic
        .evaluate(0.25, 0.5)
        .expect("base periodic evaluation")
        .point;
    let wrapped = periodic
        .evaluate(0.25, std::f64::consts::TAU + 0.5)
        .expect("wrapped periodic evaluation")
        .point;
    assert!(first.distance(wrapped) < 1.0e-12);
}

#[test]
fn profile_with_hole_extrudes_to_the_expected_volume() {
    let tolerance = ModelTolerance::default();
    let outer = Wire2::polygon(
        [
            Point2::new(-3.0, -2.0),
            Point2::new(3.0, -2.0),
            Point2::new(3.0, 2.0),
            Point2::new(-3.0, 2.0),
        ],
        tolerance,
    )
    .expect("outer polygon");
    let hole = Wire2::polygon(
        [
            Point2::new(-1.0, -1.0),
            Point2::new(1.0, -1.0),
            Point2::new(1.0, 1.0),
            Point2::new(-1.0, 1.0),
        ],
        tolerance,
    )
    .expect("hole polygon");
    let profile = Profile::new(outer, [hole], tolerance).expect("valid profile");
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("plane");
    let extrusion = profile
        .extrude(plane, Vec3::new(0.0, 0.0, 2.5))
        .expect("profile should publish");
    assert!((extrusion.volume() - 50.0).abs() < 1.0e-9);
}

#[test]
fn semicircular_profile_extrudes_without_pathological_work() {
    let tolerance = ModelTolerance::default();
    let radius = 5.0;
    let outer = Wire2::new(
        [
            TrimmedCurve2::arc_through(
                Point2::new(-radius, 0.0),
                Point2::new(0.0, -radius),
                Point2::new(radius, 0.0),
            )
            .expect("semicircular arc"),
            TrimmedCurve2::line(Point2::new(radius, 0.0), Point2::new(-radius, 0.0))
                .expect("diameter"),
        ],
        tolerance,
    )
    .expect("semicircular wire");
    let profile = Profile::new(outer, [], tolerance).expect("semicircular profile");
    let extrusion = profile
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .expect("semicircular extrusion");

    let expected = 0.5 * std::f64::consts::PI * radius * radius * 10.0;
    assert!((extrusion.volume() - expected).abs() < 1.0e-8);
}

#[test]
fn two_arc_lobe_profile_extrudes() {
    let tolerance = ModelTolerance::default();
    let intersection_y = 18.75_f64.sqrt();
    let top = Point2::new(0.0, intersection_y);
    let bottom = Point2::new(0.0, -intersection_y);
    let outer = Wire2::new(
        [
            TrimmedCurve2::arc_through(top, Point2::new(-7.5, 0.0), bottom)
                .expect("outer circular arc"),
            TrimmedCurve2::arc_through(bottom, Point2::new(-2.5, 0.0), top)
                .expect("inner circular arc"),
        ],
        tolerance,
    )
    .expect("two-arc lobe wire");
    let profile = Profile::new(outer, [], tolerance).expect("two-arc lobe profile");
    let extrusion = profile
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .expect("two-arc lobe extrusion");
    assert!(extrusion.volume() > 0.0);
}

#[test]
fn adjacent_curved_extrusions_union_across_their_shared_seam() {
    let tolerance = ModelTolerance::default();
    let intersection_y = 18.75_f64.sqrt();
    let top = Point2::new(0.0, intersection_y);
    let bottom = Point2::new(0.0, -intersection_y);
    let profile = |first_middle, second_middle| {
        let wire = Wire2::new(
            [
                TrimmedCurve2::arc_through(top, first_middle, bottom).expect("first arc"),
                TrimmedCurve2::arc_through(bottom, second_middle, top).expect("second arc"),
            ],
            tolerance,
        )
        .expect("two-arc wire");
        Profile::new(wire, [], tolerance).expect("two-arc profile")
    };
    let lobe = profile(Point2::new(-7.5, 0.0), Point2::new(-2.5, 0.0))
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("left lobe");
    let lens = profile(Point2::new(2.5, 0.0), Point2::new(-2.5, 0.0))
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("center lens");
    let joined = lobe.union(&lens).expect("adjacent curved union");
    assert!(
        (joined.volume() - std::f64::consts::PI * 25.0 * 20.0).abs() < 1.0e-6,
        "joined volume was {}",
        joined.volume()
    );
    assert_eq!(joined.faces().count(), 3);
    assert_eq!(
        joined
            .faces()
            .filter(|face| matches!(face.surface(), Surface3::Cylinder(_)))
            .count(),
        1
    );
    assert!(!joined
        .tessellate(TessellationOptions::default())
        .expect("joined cylinder tessellation")
        .is_empty());
}

#[test]
fn curved_extrusion_cuts_a_prismatic_body() {
    let tolerance = ModelTolerance::default();
    let block = Profile::new(
        Wire2::polygon(
            [
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 10.0),
                Point2::new(0.0, 10.0),
            ],
            tolerance,
        )
        .expect("block wire"),
        [],
        tolerance,
    )
    .expect("block profile")
    .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
    .expect("block");
    let cutter = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(5.0, 5.0), 2.0).expect("circle")],
            tolerance,
        )
        .expect("circle wire"),
        [],
        tolerance,
    )
    .expect("circle profile")
    .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
    .expect("cutter");
    let cut = block.difference(&cutter).expect("curved cut");
    assert!(cut.volume() > 0.0);
    assert!(cut
        .faces()
        .all(|face| matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_))));
    assert!(cut
        .edges()
        .all(|edge| matches!(edge.curve(), Curve3::Line(_) | Curve3::Circle(_))));
    assert!(!cut
        .tessellate(TessellationOptions::default())
        .expect("mesh")
        .is_empty());
    let intersection = block.intersection(&cutter).expect("curved intersection");
    assert!(intersection.volume() > 0.0);
    assert!(intersection
        .faces()
        .all(|face| matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_))));
    assert!(!intersection
        .tessellate(TessellationOptions::default())
        .expect("intersection mesh")
        .is_empty());
    let union = block.union(&cutter).expect("curved contained union");
    assert!((union.volume() - block.volume()).abs() < 1.0e-3);
    assert!(union
        .faces()
        .all(|face| matches!(face.surface(), Surface3::Plane(_))));
}

#[test]
fn profile_hole_bounds_preserve_touching_overlap_and_nesting_checks() {
    let tolerance = ModelTolerance::default();
    let outer = Wire2::polygon([
        Point2::new(-10.0, -10.0), Point2::new(10.0, -10.0),
        Point2::new(10.0, 10.0), Point2::new(-10.0, 10.0),
    ], tolerance).unwrap();
    let circle = |x, radius| Wire2::new([
        TrimmedCurve2::circle(Point2::new(x, 0.0), radius).unwrap(),
    ], tolerance).unwrap();
    for (x, radius) in [(3.0, 2.0), (4.0, 2.0), (0.0, 1.0)] {
        assert!(Profile::new(outer.clone(), [circle(0.0, 2.0), circle(x, radius)], tolerance).is_err());
    }
    assert!(Profile::new(outer, [circle(0.0, 2.0), circle(5.0, 1.0)], tolerance).is_ok());
}

#[test]
fn nurbs_profile_extrudes_as_an_analytic_certified_prism() {
    let tolerance = ModelTolerance::default();
    let curved_top = TrimmedCurve2::new(
        NurbsCurve2::new(
            2,
            vec![
                Point2::new(4.0, 2.0),
                Point2::new(2.0, 4.0),
                Point2::new(0.0, 2.0),
            ],
            vec![1.0; 3],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .expect("spline"),
        ParameterRange::new(0.0, 1.0).expect("range"),
    )
    .expect("trimmed spline");
    let wire = Wire2::new(
        [
            TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)).expect("bottom"),
            TrimmedCurve2::line(Point2::new(4.0, 0.0), Point2::new(4.0, 2.0)).expect("right"),
            curved_top,
            TrimmedCurve2::line(Point2::new(0.0, 2.0), Point2::new(0.0, 0.0)).expect("left"),
        ],
        tolerance,
    )
    .expect("wire");
    let prism = Profile::new(wire, [], tolerance)
        .expect("profile")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))
        .expect("analytic NURBS prism");

    assert!(prism.volume() > 40.0);
    assert!(prism
        .edges()
        .any(|edge| matches!(edge.curve(), Curve3::Nurbs(_))));

    let separate_circle = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(12.0, 2.0), 1.0).expect("circle")],
        tolerance,
    )
    .expect("circle wire");
    let separate_prism = Profile::new(separate_circle, [], tolerance)
        .expect("circle profile")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))
        .expect("separate prism");
    let compound = prism
        .union(&separate_prism)
        .expect("separated analytic prisms");
    assert_eq!(compound.solids().len(), 2);
}

#[test]
fn nurbs_profile_boolean_isolated_on_exact_curve_parameters() {
    let tolerance = ModelTolerance::default();
    let curved_top = TrimmedCurve2::new(
        NurbsCurve2::new(
            2,
            vec![
                Point2::new(4.0, 2.0),
                Point2::new(2.0, 4.0),
                Point2::new(0.0, 2.0),
            ],
            vec![1.0; 3],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .expect("spline"),
        ParameterRange::new(0.0, 1.0).expect("range"),
    )
    .expect("trimmed spline");
    let spline_profile = Profile::new(
        Wire2::new(
            [
                TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)).expect("bottom"),
                TrimmedCurve2::line(Point2::new(4.0, 0.0), Point2::new(4.0, 2.0)).expect("right"),
                curved_top,
                TrimmedCurve2::line(Point2::new(0.0, 2.0), Point2::new(0.0, 0.0)).expect("left"),
            ],
            tolerance,
        )
        .expect("spline wire"),
        [],
        tolerance,
    )
    .expect("spline profile");
    let rectangle = Profile::new(
        Wire2::polygon(
            [
                Point2::new(2.0, 1.0),
                Point2::new(5.0, 1.0),
                Point2::new(5.0, 3.0),
                Point2::new(2.0, 3.0),
            ],
            tolerance,
        )
        .expect("rectangle"),
        [],
        tolerance,
    )
    .expect("rectangle profile");

    let intersection = spline_profile
        .boolean_planar(&rectangle, BooleanKind::Intersection)
        .expect("analytic NURBS/line intersection");
    assert_eq!(intersection.len(), 1);
    assert!(intersection[0].area() > 2.0 && intersection[0].area() < 6.0);
    assert!(intersection[0]
        .outer()
        .segments()
        .iter()
        .any(|segment| matches!(segment.curve(), Curve2::Nurbs(_))));

    let expected_volume = intersection[0].area() * 5.0;
    let spline_prism = spline_profile
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))
        .expect("spline prism");
    let rectangle_prism = rectangle
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))
        .expect("rectangle prism");
    let solid_intersection = spline_prism
        .intersection(&rectangle_prism)
        .expect("analytic NURBS prism intersection");
    assert!((solid_intersection.volume() - expected_volume).abs() < 1.0e-6);
}

#[test]
fn high_level_profile_extrusion_owns_results_and_supports_curved_holes() {
    let tolerance = ModelTolerance::default();
    let outer = Wire2::new(
        [
            TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(20.0, 0.0)).expect("line"),
            TrimmedCurve2::line(Point2::new(20.0, 0.0), Point2::new(20.0, 10.0)).expect("line"),
            TrimmedCurve2::line(Point2::new(20.0, 10.0), Point2::new(0.0, 10.0)).expect("line"),
            TrimmedCurve2::line(Point2::new(0.0, 10.0), Point2::new(0.0, 0.0)).expect("line"),
        ],
        tolerance,
    )
    .expect("outer wire");
    let hole = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(5.0, 5.0), 2.0).expect("circle")],
        tolerance,
    )
    .expect("hole wire");
    let profile = Profile::new(outer, [hole], tolerance).expect("profile");
    let extrusion = profile
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 3.0))
        .expect("curved profile extrusion");
    assert!((extrusion.volume() - (200.0 - 4.0 * std::f64::consts::PI) * 3.0).abs() < 1.0e-7);
    let face = extrusion.faces().next().expect("face").clone();
    assert!(matches!(extrusion.resolve(&face), Resolution::Preserved(_)));
    let mesh = extrusion
        .tessellate(TessellationOptions::default())
        .expect("mesh");
    assert!(!mesh.is_empty());
}

#[test]
fn high_level_profile_extrudes_a_circular_outer_wire() {
    let tolerance = ModelTolerance::default();
    let outer = Wire2::new(
        [
            TrimmedCurve2::circle(Point2::new(-7.196977129201897, 0.0), 17.553807290465304)
                .expect("circle"),
        ],
        tolerance,
    )
    .expect("outer wire");
    let profile = Profile::new(outer, [], tolerance).expect("profile");
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::new(2.220446049250313e-16, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
        tolerance,
    )
    .expect("rotated plane");
    let extrusion = profile
        .extrude(plane, Vec3::new(3.0, 0.0, 3.0 * 2.220446049250313e-16))
        .expect("circular profile extrusion");

    assert!(
        (extrusion.volume() - std::f64::consts::PI * 17.553807290465304_f64.powi(2) * 3.0).abs()
            < 1.0e-7
    );
}

#[test]
fn high_level_profile_revolution_is_euler_built_and_certified() {
    let tolerance = ModelTolerance::default();
    let wire = Wire2::polygon(
        [
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 3.0),
            Point2::new(0.0, 3.0),
        ],
        tolerance,
    )
    .expect("revolution profile");
    let profile = Profile::new(wire, [], tolerance).expect("profile");
    let axis = Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).expect("axis");
    let revolution = profile
        .revolve(Plane3::xy(), axis, std::f64::consts::TAU)
        .expect("revolution");

    assert!((revolution.volume() - 12.0 * std::f64::consts::PI).abs() < 0.1);
    assert!(!revolution
        .tessellate(TessellationOptions::default())
        .expect("mesh")
        .is_empty());
}

#[test]
fn full_off_axis_revolutions_publish_exact_genus_one_solids() {
    let tolerance = ModelTolerance::default();
    let axis = Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).expect("axis");
    let rectangle = Profile::new(
        Wire2::polygon(
            [
                Point2::new(2.0, 0.0),
                Point2::new(4.0, 0.0),
                Point2::new(4.0, 3.0),
                Point2::new(2.0, 3.0),
            ],
            tolerance,
        )
        .expect("rectangular section"),
        [],
        tolerance,
    )
    .expect("rectangular profile");
    let washer = rectangle
        .revolve(Plane3::xy(), axis, std::f64::consts::TAU)
        .expect("annular revolution");
    assert!((washer.volume() - 36.0 * std::f64::consts::PI).abs() < 1.0e-8);

    let circle = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(5.0, 0.0), 1.0).expect("circle")],
            tolerance,
        )
        .expect("circle wire"),
        [],
        tolerance,
    )
    .expect("circle profile");
    let torus = circle
        .revolve(Plane3::xy(), axis, std::f64::consts::TAU)
        .expect("toroidal revolution");
    assert!((torus.volume() - 10.0 * std::f64::consts::PI.powi(2)).abs() < 1.0e-8);
}

#[test]
fn lofts_publish_exact_section_surfaces() {
    let tolerance = ModelTolerance::default();
    let wire = Wire2::polygon(
        [
            Point2::new(-1.0, -1.0),
            Point2::new(1.0, -1.0),
            Point2::new(1.0, 1.0),
            Point2::new(-1.0, 1.0),
        ],
        tolerance,
    )
    .expect("profile wire");
    let profile = Profile::new(wire, [], tolerance).expect("profile");
    let upper_plane = Plane3::new(
        Point3::new(0.0, 0.0, 3.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("upper plane");
    let loft = profile
        .loft(Plane3::xy(), [(upper_plane, &profile)])
        .expect("exact loft between identical sections");
    assert!((loft.volume() - 12.0).abs() < 1.0e-7);
    assert!(!loft
        .tessellate(TessellationOptions::default())
        .expect("loft mesh")
        .is_empty());
}

#[test]
fn contained_nurbs_prism_difference_is_analytical_and_empty() {
    let tolerance = ModelTolerance::default();
    let curved_top = TrimmedCurve2::new(
        NurbsCurve2::new(
            2,
            vec![
                Point2::new(4.0, 2.0),
                Point2::new(2.0, 4.0),
                Point2::new(0.0, 2.0),
            ],
            vec![1.0; 3],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .expect("spline"),
        ParameterRange::new(0.0, 1.0).expect("range"),
    )
    .expect("trimmed spline");
    let spline_wire = Wire2::new(
        [
            TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)).expect("bottom"),
            TrimmedCurve2::line(Point2::new(4.0, 0.0), Point2::new(4.0, 2.0)).expect("right"),
            curved_top,
            TrimmedCurve2::line(Point2::new(0.0, 2.0), Point2::new(0.0, 0.0)).expect("left"),
        ],
        tolerance,
    )
    .expect("spline wire");
    let spline_prism = Profile::new(spline_wire, [], tolerance)
        .expect("spline profile")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .expect("spline prism");
    let tool = owned_cuboid(Point3::new(0.0, -4.0, -1.0), Point3::new(4.0, 4.0, 11.0));
    let result = spline_prism
        .difference(&tool)
        .expect("the exact profile arrangement proves complete removal");
    assert_eq!(result.solids().count(), 0);
    assert!(result.volume().abs() < tolerance.linear.powi(3));
}

#[test]
fn circular_prism_can_join_partially_across_a_planar_face() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 20.0, 20.0));
    let circle = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(10.0, 10.0), 4.0).expect("circle")],
        tolerance,
    )
    .expect("circle wire");
    let boss = Profile::new(circle, [], tolerance)
        .expect("circle profile")
        .extrude(
            Plane3::new(
                Point3::new(0.0, 0.0, 0.0),
                Vec3::unit_z(),
                Vec3::unit_x(),
                tolerance,
            )
            .expect("top plane"),
            Vec3::new(0.0, -1.0, 0.0),
        )
        .expect("boss");

    let joined = block.union(&boss).expect("partial-height curved union");
    assert_eq!(joined.solids().len(), 1);
    assert!(
        (joined.volume() - (8000.0 + 16.0 * std::f64::consts::PI)).abs() < 0.1,
        "unexpected joined volume {}",
        joined.volume()
    );
}

#[test]
fn circular_prism_can_join_through_a_shallow_planar_overlap() {
    let tolerance = ModelTolerance::default();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        -Vec3::unit_z(),
        tolerance,
    )
    .expect("profile plane");
    let body = Profile::new(
        Wire2::polygon(
            [
                Point2::new(0.0, 34.901492966646394),
                Point2::new(56.18948530462976, 34.901492966646394),
                Point2::new(56.18948530462976, -2.6849427865935285),
                Point2::new(38.189485304629756, -20.684942786593528),
                Point2::new(-37.511054555851985, -20.684942786593528),
                Point2::new(-55.51105455585198, -2.684942786593526),
                Point2::new(-55.51105455585198, 3.930647011080744),
                Point2::new(0.0, 3.930647011080744),
            ],
            tolerance,
        )
        .expect("body outline"),
        [],
        tolerance,
    )
    .expect("body profile")
    .extrude(plane, Vec3::new(0.0, 200.0, 0.0))
    .expect("body");
    let center = Point2::new(-24.206992697151726, 2.570753756415947);
    let radius = 19.0;
    let boss = |center: Point2, radius: f64| {
        Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(center, radius).expect("boss circle")],
                tolerance,
            )
            .expect("boss wire"),
            [],
            tolerance,
        )
        .expect("boss profile")
        .extrude(plane, Vec3::new(0.0, -20.0, 0.0))
        .expect("boss")
    };
    let body = body
        .union(&boss(Point2::new(center.x - 10.0, center.y), 9.0))
        .expect("existing cylindrical boss");
    let circle = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(center, radius).expect("circle")],
            tolerance,
        )
        .expect("circle wire"),
        [],
        tolerance,
    )
    .expect("circle profile")
    .extrude(
        Plane3::new(
            Point3::new(0.0, -50.0, 0.0),
            Vec3::unit_x(),
            -Vec3::unit_z(),
            tolerance,
        )
        .expect("tool plane"),
        Vec3::new(0.0, 300.0, 0.0),
    )
    .expect("circle tool");

    let joined = body.union(&circle).expect("shallow through union");
    assert_eq!(joined.solids().len(), 1);
}

#[test]
fn circular_prism_can_join_a_planar_face_with_an_existing_hole() {
    let tolerance = ModelTolerance::default();
    let rectangle = |minimum: f64, maximum: f64| {
        Wire2::polygon(
            [
                Point2::new(minimum, minimum),
                Point2::new(maximum, minimum),
                Point2::new(maximum, maximum),
                Point2::new(minimum, maximum),
            ],
            tolerance,
        )
        .expect("rectangle")
    };
    let frame = Profile::new(rectangle(0.0, 20.0), [rectangle(7.0, 13.0)], tolerance)
        .expect("frame profile")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("frame");
    let circle = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(3.5, 3.5), 2.0).expect("circle")],
        tolerance,
    )
    .expect("circle wire");
    let boss = Profile::new(circle, [], tolerance)
        .expect("circle profile")
        .extrude(
            Plane3::new(
                Point3::new(0.0, 0.0, 20.0),
                Vec3::unit_x(),
                Vec3::unit_y(),
                tolerance,
            )
            .expect("top plane"),
            Vec3::new(0.0, 0.0, 5.0),
        )
        .expect("boss");

    let joined = frame.union(&boss).expect("circular boss on frame");
    assert_eq!(joined.solids().len(), 1);
    let expected = (20.0 * 20.0 - 6.0 * 6.0) * 20.0 + 4.0 * std::f64::consts::PI * 5.0;
    assert!((joined.volume() - expected).abs() < 0.1);
}

#[test]
fn circular_prism_can_cut_a_planar_face_with_an_existing_hole() {
    let tolerance = ModelTolerance::default();
    let rectangle = |minimum: f64, maximum: f64| {
        Wire2::polygon(
            [
                Point2::new(minimum, minimum),
                Point2::new(maximum, minimum),
                Point2::new(maximum, maximum),
                Point2::new(minimum, maximum),
            ],
            tolerance,
        )
        .expect("rectangle")
    };
    let frame = Profile::new(rectangle(0.0, 20.0), [rectangle(7.0, 13.0)], tolerance)
        .expect("frame profile")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("frame");
    let circle_profile = || {
        Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(3.5, 3.5), 2.0).expect("circle")],
                tolerance,
            )
            .expect("circle wire"),
            [],
            tolerance,
        )
        .expect("circle profile")
    };
    let top_plane = Plane3::new(
        Point3::new(0.0, 0.0, 20.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("top plane");

    let outward = circle_profile()
        .extrude(top_plane, Vec3::new(0.0, 0.0, 7.0))
        .expect("outward tool");
    let unchanged = frame
        .difference(&outward)
        .expect("non-overlapping outward cut");
    assert!((unchanged.volume() - frame.volume()).abs() < 1.0e-6);
    assert!(
        unchanged
            .edges()
            .any(|edge| unchanged.is_semantic_edge(edge)),
        "no-op Boolean edge classification must terminate and retain physical creases"
    );

    let inward = circle_profile()
        .extrude(top_plane, Vec3::new(0.0, 0.0, -7.0))
        .expect("inward tool");
    let cut = frame.difference(&inward).expect("circular pocket in frame");
    let expected = frame.volume() - 4.0 * std::f64::consts::PI * 7.0;
    assert!((cut.volume() - expected).abs() < 0.1);
}

#[test]
fn rectangular_prism_can_join_partially_across_a_planar_face() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 20.0, 20.0));
    let boss = Profile::new(
        Wire2::polygon(
            [
                Point2::new(4.0, -16.0),
                Point2::new(16.0, -16.0),
                Point2::new(16.0, -4.0),
                Point2::new(4.0, -4.0),
            ],
            tolerance,
        )
        .expect("boss wire"),
        [],
        tolerance,
    )
    .expect("boss profile")
    .extrude(
        Plane3::new(
            Point3::new(0.0, 0.0, 20.0),
            Vec3::unit_y(),
            -Vec3::unit_x(),
            tolerance,
        )
        .expect("top plane"),
        Vec3::new(0.0, 0.0, 11.0),
    )
    .expect("boss");

    let joined = block.union(&boss).expect("rectangular face-patch union");
    assert_eq!(joined.solids().len(), 1);
    assert!((joined.volume() - (8_000.0 + 12.0 * 12.0 * 11.0)).abs() < 1.0e-6);
}

#[test]
fn rectangular_prism_can_join_through_a_body_and_extend_below_it() {
    let tolerance = ModelTolerance::default();
    let rectangle = |minimum: f64, maximum: f64| {
        Wire2::polygon(
            [
                Point2::new(minimum, minimum),
                Point2::new(maximum, minimum),
                Point2::new(maximum, maximum),
                Point2::new(minimum, maximum),
            ],
            tolerance,
        )
        .expect("rectangle")
    };
    let frame = Profile::new(rectangle(0.0, 20.0), [rectangle(7.0, 13.0)], tolerance)
        .expect("frame profile")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("frame");
    let post = Profile::new(rectangle(2.0, 5.0), [], tolerance)
        .expect("post profile")
        .extrude(
            Plane3::new(
                Point3::new(0.0, 0.0, 20.0),
                Vec3::unit_x(),
                Vec3::unit_y(),
                tolerance,
            )
            .expect("top plane"),
            Vec3::new(0.0, 0.0, -32.0),
        )
        .expect("post");

    let joined = frame.union(&post).expect("post through and below frame");
    assert_eq!(joined.solids().len(), 1);
    let expected = (20.0 * 20.0 - 6.0 * 6.0) * 20.0 + 3.0 * 3.0 * 12.0;
    assert!((joined.volume() - expected).abs() < 1.0e-6);

    let reversed = post.union(&frame).expect("reversed through union");
    assert_eq!(reversed.solids().len(), 1);
    assert!((reversed.volume() - expected).abs() < 1.0e-6);
}

#[test]
fn disconnected_prisms_can_join_through_a_body_and_extend_below_it_together() {
    let tolerance = ModelTolerance::default();
    let rectangle = |minimum: f64, maximum: f64| {
        Wire2::polygon(
            [
                Point2::new(minimum, minimum),
                Point2::new(maximum, minimum),
                Point2::new(maximum, maximum),
                Point2::new(minimum, maximum),
            ],
            tolerance,
        )
        .expect("rectangle")
    };
    let frame = Profile::new(rectangle(0.0, 20.0), [rectangle(7.0, 13.0)], tolerance)
        .expect("frame profile")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("frame");
    let top_plane = Plane3::new(
        Point3::new(0.0, 0.0, 20.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("top plane");
    let post = Profile::new(rectangle(2.0, 5.0), [], tolerance)
        .expect("post profile")
        .extrude(top_plane, Vec3::new(0.0, 0.0, -32.0))
        .expect("post");
    let cylinder = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(16.0, 4.0), 1.5).expect("circle")],
            tolerance,
        )
        .expect("circle wire"),
        [],
        tolerance,
    )
    .expect("circle profile")
    .extrude(top_plane, Vec3::new(0.0, 0.0, -32.0))
    .expect("cylinder");
    let tools = post.combined(&[cylinder]).expect("disconnected tools");

    let joined = frame.union(&tools).expect("two posts through frame");
    assert_eq!(joined.solids().len(), 1);
    let expected = (20.0 * 20.0 - 6.0 * 6.0) * 20.0
        + (3.0 * 3.0 + 1.5_f64.powi(2) * std::f64::consts::PI) * 12.0;
    assert!(
        (joined.volume() - expected).abs() < 0.1,
        "unexpected joined volume {} instead of {expected}",
        joined.volume()
    );

    let reversed = tools.union(&frame).expect("reversed two-post union");
    assert_eq!(reversed.solids().len(), 1);
    assert!((reversed.volume() - expected).abs() < 0.1);
}

#[test]
fn disconnected_profiles_cut_two_blind_pockets_in_one_operation() {
    let tolerance = ModelTolerance::default();
    let rectangle = |minimum: Point2, maximum: Point2| {
        Wire2::polygon(
            [
                minimum,
                Point2::new(maximum.x, minimum.y),
                maximum,
                Point2::new(minimum.x, maximum.y),
            ],
            tolerance,
        )
        .expect("rectangle wire")
    };
    let existing_circle = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(25.0, 5.0), 2.0).expect("existing circle")],
        tolerance,
    )
    .expect("existing circle wire");
    let block = Profile::new(
        rectangle(Point2::new(0.0, 0.0), Point2::new(30.0, 20.0)),
        [
            rectangle(Point2::new(10.0, 5.0), Point2::new(22.0, 15.0)),
            existing_circle,
        ],
        tolerance,
    )
    .expect("multiply perforated block profile")
    .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
    .expect("multiply perforated block");
    let top_plane = Plane3::new(
        Point3::new(0.0, 0.0, 20.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("top plane");
    let rectangle = Profile::new(
        rectangle(Point2::new(2.0, 2.0), Point2::new(5.0, 5.0)),
        [],
        tolerance,
    )
    .expect("rectangle profile")
    .extrude(top_plane, Vec3::new(0.0, 0.0, -12.0))
    .expect("rectangular cutter");
    let circle = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(7.0, 4.0), 1.5).expect("circle")],
            tolerance,
        )
        .expect("circle wire"),
        [],
        tolerance,
    )
    .expect("circle profile")
    .extrude(top_plane, Vec3::new(0.0, 0.0, -12.0))
    .expect("circular cutter");
    let cutters = rectangle.combined(&[circle]).expect("disconnected cutters");

    let pocketed = block
        .difference(&cutters)
        .expect("two simultaneous blind pockets");
    assert_eq!(pocketed.solids().len(), 1);
    let base_area = 30.0 * 20.0 - 12.0 * 10.0 - 2.0_f64.powi(2) * std::f64::consts::PI;
    let expected = base_area * 20.0 - (3.0 * 3.0 + 1.5_f64.powi(2) * std::f64::consts::PI) * 12.0;
    assert!((pocketed.volume() - expected).abs() < 0.1);
}

#[test]
fn extrusion_exposes_semantic_creation_provenance() {
    let extrusion = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 3.0, 4.0));
    let roles = extrusion
        .faces()
        .filter_map(|face| extrusion.origin(face))
        .filter_map(|origin| match origin {
            crate::Origin::Created { role } => role.as_ref(),
            crate::Origin::Derived { .. } => None,
        })
        .collect::<Vec<_>>();
    assert!(roles.contains(&&crate::CreationRole::ExtrusionStart));
    assert!(roles.contains(&&crate::CreationRole::ExtrusionEnd));
    assert_eq!(
        roles
            .iter()
            .filter(|role| matches!(role, crate::CreationRole::ExtrusionSide { .. }))
            .count(),
        4
    );

    // Construction metadata must not rewrite ancestry or entity resolution.
    let face = extrusion.faces().next().unwrap();
    let tagged = extrusion.clone().with_creation_roles([
        (face.persistent_id(), crate::CreationRole::ProfileEnd),
    ]);
    assert_eq!(tagged.origin(face), extrusion.origin(face));
    assert!(matches!(tagged.resolve(face), crate::Resolution::Preserved(_)));
    assert_eq!(tagged.creation_roles(face), vec![crate::CreationRole::ProfileEnd]);
    let moved = tagged.transformed(RigidTransform3::new(
        Matrix3::identity(), Vec3::new(10.0, 0.0, 0.0), ModelTolerance::default(),
    ).unwrap()).unwrap();
    let output = match moved.resolve(face) {
        crate::Resolution::Modified(face) => face,
        other => panic!("unexpected transformed face resolution: {other:?}"),
    };
    assert_eq!(moved.creation_roles(&output), vec![crate::CreationRole::ProfileEnd]);
}

#[test]
fn extrude_circle_on_open_torus_shell() {
    let tol = ModelTolerance::default();
    let body = rounded_chamfered_torus(true);
    let opening = body
        .faces()
        .find(|f| matches!(f.surface(), Surface3::Plane(_)))
        .unwrap()
        .clone();
    let shell = body.shell(&[opening], -3.0).unwrap();
    let plane = Plane3::new(
        Point3::new(0.0, 21.848456911735045, -12.045831777920995),
        Vec3::unit_z(),
        -Vec3::unit_y(),
        tol,
    )
    .unwrap();
    let profile = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(
                Point2::new(66.24627742902457, 20.76851015338629),
                21.902415063989423,
            )
            .unwrap()],
            tol,
        )
        .unwrap(),
        [],
        tol,
    )
    .unwrap();
    for distance in [0.5, -0.5, 1.0, -1.0, 5.0, -5.0, 20.0, -20.0] {
        let tool = profile
            .extrude(plane, Vec3::new(distance, 0.0, 0.0))
            .unwrap();
        for cut in [false, true] {
            let result = if cut {
                shell.difference(&tool)
            } else {
                shell.union(&tool)
            };
            let result =
                result.unwrap_or_else(|error| panic!("distance {distance} cut {cut}: {error}"));
            assert_eq!(result.solids().len(), 1);
            if cut {
                assert!(result.volume() <= shell.volume() * (1.0 + 1e-6));
            } else {
                assert!(result.volume() > shell.volume());
            }
            result
                .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                .unwrap();
        }
    }
}
