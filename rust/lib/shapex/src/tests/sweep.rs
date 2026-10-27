use super::*;
/// A rail given in XY plane coordinates, lifted into space
fn xy(rail: &TrimmedCurve2) -> Curve3 {
    Plane3::xy().lift_curve(rail.curve(), ModelTolerance::default()).unwrap()
}
/// A one-segment rail wire
fn rail3(curve: &Curve3, range: ParameterRange) -> Wire3 {
    Wire3::new([TrimmedCurve3::new(curve.clone(), range).unwrap()], ModelTolerance::default()).unwrap()
}
fn sweep_xy(p: &Profile, plane: Plane3, rail: &TrimmedCurve2) -> KernelResult<Compound> {
    p.sweep(plane, &rail3(&xy(rail), rail.range()))
}
fn circle() -> Profile {
    let tolerance = ModelTolerance::default();
    Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 1.0).unwrap()],
            tolerance,
        )
        .unwrap(),
        [],
        tolerance,
    )
    .unwrap()
}
fn verify(body: &Compound, expected: f64) {
    assert!(
        (body.volume() - expected).abs() < expected * 1e-4,
        "volume {} != {expected}",
        body.volume()
    );
    assert_eq!(body.solids().len(), 1);
    assert!(!body
        .tessellate(TessellationOptions::new(0.05, 0.15, 24).unwrap())
        .unwrap()
        .triangles
        .is_empty());
}
#[test]
fn sweep_lines_arcs_and_circles() {
    let p = circle();
    let t = p.tolerance();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_y(),
        Vec3::unit_z(),
        t,
    )
    .unwrap();
    let line = TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)).unwrap();
    verify(
        &sweep_xy(&p, plane, &line).unwrap(),
        p.area() * 10.0,
    );
    let plane = Plane3::new(
        Point3::new(10.0, 0.0, 0.0),
        Vec3::unit_x(),
        Vec3::unit_z(),
        t,
    )
    .unwrap();
    for angle in [
        std::f64::consts::PI,
        -std::f64::consts::PI,
        std::f64::consts::TAU,
    ] {
        let rail = TrimmedCurve2::new(
            Circle2::new(Point2::new(0.0, 0.0), 10.0).unwrap(),
            ParameterRange::new(0.0, angle).unwrap(),
        )
        .unwrap();
        verify(
            &sweep_xy(&p, plane, &rail).unwrap(),
            p.area() * 10.0 * angle.abs(),
        );
    }
}
#[test]
fn sweep_spline_preserves_section_and_reverses() {
    let p = circle();
    let t = p.tolerance();
    let rail = TrimmedCurve2::new(
        NurbsCurve2::new(
            3,
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 10.0),
                Point2::new(20.0, 10.0),
            ],
            vec![1.0; 4],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        )
        .unwrap(),
        ParameterRange::new(0.0, 1.0).unwrap(),
    )
    .unwrap();
    let length = (0..4096)
        .map(|i| {
            rail.curve()
                .evaluate((i as f64 + 0.5) / 4096.0)
                .unwrap()
                .first_derivative
                .magnitude()
                / 4096.0
        })
        .sum::<f64>();
    for reverse in [false, true] {
        let rail = if reverse {
            rail.reversed()
        } else {
            rail.clone()
        };
        let origin = Plane3::xy().point_at(rail.start().unwrap());
        let plane = Plane3::new(origin, Vec3::unit_y(), Vec3::unit_z(), t).unwrap();
        let result = sweep_xy(&p, plane, &rail).unwrap();
        verify(&result, p.area() * length);
        let transform = RigidTransform3::new(
            crate::Matrix3::from_angle_y(cgmath::Deg(35.0)),
            Vec3::new(3.0, 5.0, 7.0),
            t,
        )
        .unwrap();
        verify(&result.transformed(transform).unwrap(), p.area() * length);
    }
}
fn rail() -> TrimmedCurve2 {
    TrimmedCurve2::new(
        NurbsCurve2::new(
            3,
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(5.0, 0.0),
                Point2::new(5.0, 5.0),
                Point2::new(10.0, 5.0),
                Point2::new(15.0, 5.0),
                Point2::new(15.0, 10.0),
                Point2::new(20.0, 10.0),
            ],
            vec![1.0, 0.9, 1.1, 1.0, 1.1, 0.9, 1.0],
            vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0],
        )
        .unwrap(),
        ParameterRange::new(0.0, 1.0).unwrap(),
    )
    .unwrap()
}
#[test]
fn sweep_rational_spline_derivatives_and_boolean() {
    let t = ModelTolerance::default();
    let rail = rail();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_y(),
        Vec3::unit_z(),
        t,
    )
    .unwrap();
    let section = TrimmedCurve2::circle(Point2::new(0.0, 0.0), 0.5).unwrap();
    let surface =
        SweptSurface3::new(plane, section.clone(), xy(&rail), rail.range(), t).unwrap();
    for v in [0.1, 0.4, 0.7, 0.9] {
        let value = surface.evaluate(0.4, v).unwrap();
        let step = 1e-5;
        let before = surface.first_order(0.4, v - step).unwrap();
        let after = surface.first_order(0.4, v + step).unwrap();
        assert!(((after.0 - before.0) / (2.0 * step) - value.dv).magnitude() < 1e-4);
        assert!(((after.2 - before.2) / (2.0 * step) - value.dvv).magnitude() < 1e-3);
    }
    let p = Profile::new(Wire2::new([section], t).unwrap(), [], t).unwrap();
    let body = sweep_xy(&p, plane, &rail).unwrap();
    let block = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-2.0, -2.0),
                Point2::new(2.0, -2.0),
                Point2::new(2.0, 2.0),
                Point2::new(-2.0, 2.0),
            ],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap()
    .extrude(
        Plane3::new(
            Point3::new(-2.0, 0.0, 0.0),
            Vec3::unit_y(),
            Vec3::unit_z(),
            t,
        )
        .unwrap(),
        Vec3::new(4.0, 0.0, 0.0),
    )
    .unwrap();
    let joined = body.union(&block).unwrap();
    assert!(
        joined.volume() > block.volume() && joined.volume() < body.volume() + block.volume()
    );
    assert_eq!(joined.solids().len(), 1);
    let cut = body.difference(&block).unwrap();
    let common = body.intersection(&block).unwrap();
    assert_eq!(cut.solids().len(), 1);
    assert_eq!(common.solids().len(), 1);
    assert!(cut.volume() > 0.0 && common.volume() > 0.0);
    // Mass-property quadrature uses the whole solid's spatial scale;
    // this long, thin sweep has a much smaller volume than that scale cubed.
    let volume_tolerance = body.volume() * 1e-3;
    assert!((cut.volume() + common.volume() - body.volume()).abs() < volume_tolerance);
    assert!(
        (joined.volume() + common.volume() - body.volume() - block.volume()).abs()
            < volume_tolerance
    );
}
#[test]
fn sweep_closed_rational_rail() {
    let t = ModelTolerance::default();
    let w = 0.5_f64.sqrt();
    let points = [
        (10.0, 0.0),
        (10.0, 10.0),
        (0.0, 10.0),
        (-10.0, 10.0),
        (-10.0, 0.0),
        (-10.0, -10.0),
        (0.0, -10.0),
        (10.0, -10.0),
        (10.0, 0.0),
    ];
    let rail = TrimmedCurve2::new(
        NurbsCurve2::new(
            2,
            points.map(|(x, y)| Point2::new(x, y)).to_vec(),
            vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
        )
        .unwrap(),
        ParameterRange::new(0.0, 4.0).unwrap(),
    )
    .unwrap();
    let p = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-0.5, -0.5),
                Point2::new(0.5, -0.5),
                Point2::new(0.5, 0.5),
                Point2::new(-0.5, 0.5),
            ],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap();
    let plane = Plane3::new(
        Point3::new(10.0, 0.0, 0.0),
        Vec3::unit_x(),
        Vec3::unit_z(),
        t,
    )
    .unwrap();
    let body = sweep_xy(&p, plane, &rail).unwrap();
    assert!((body.volume() - std::f64::consts::TAU * 10.0).abs() < 1e-4);
    assert!(!body
        .tessellate(TessellationOptions::new(0.05, 0.15, 24).unwrap())
        .unwrap()
        .triangles
        .is_empty());
}
fn document_65_sweep() -> Compound {
    // untitled(65).cad.cad: circle on the application's YZ workplane
    // (local X points along -Z), swept along the XY cubic sketch spline.
    let t = ModelTolerance::default();
    let profile = Profile::new(
        Wire2::new(
            [
                TrimmedCurve2::circle(Point2::new(0.0, 8.442693762145785), 31.213640464146206)
                    .unwrap(),
            ],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap();
    let rail = TrimmedCurve2::new(
        NurbsCurve2::new(
            3,
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(89.2170942259588, 11.288693555121299),
                Point2::new(163.50398084675714, -89.94539703596661),
                Point2::new(274.2060079679468, 31.681172235340455),
            ],
            vec![1.0; 4],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        )
        .unwrap(),
        ParameterRange::new(0.0, 1.0).unwrap(),
    )
    .unwrap();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        -Vec3::unit_z(),
        Vec3::unit_y(),
        t,
    )
    .unwrap();
    sweep_xy(&profile, plane, &rail).unwrap()
}
#[test]
fn swept_circle_end_fillets_from_document_65() {
    let body = document_65_sweep();
    assert_eq!(body.faces().count(), 3);
    let rims = body
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(rims.len(), 2);
    for edge in &rims {
        assert!(edge.endpoints().unwrap()[0].distance(edge.endpoints().unwrap()[1]) < 1e-7);
    }
    for radius in [1.0, 5.0] {
        for selection in [&rims[..1], &rims[1..], &rims[..]] {
            let result = body.fillet(selection, radius).unwrap_or_else(|error| {
                panic!("{} rims, radius {radius}: {error}", selection.len())
            });
            assert!(result.volume() < body.volume() && result.volume() > body.volume() * 0.99);
            assert!(!result
                .tessellate(TessellationOptions::new(0.1, 0.15, 24).unwrap())
                .unwrap()
                .triangles
                .is_empty());
        }
    }
}
#[test]
fn swept_circle_fillet_cut_from_document_65() {
    let start = std::time::Instant::now();
    let body = document_65_sweep();
    eprintln!("sweep {:?}", start.elapsed());
    let rim = body
        .edges()
        .find(|edge| matches!(edge.curve(), Curve3::Circle(c) if c.center().x > 200.0))
        .unwrap()
        .clone();
    let start = std::time::Instant::now();
    let rounded = body.fillet(&[rim], 3.0).unwrap();
    eprintln!("fillet {:?}", start.elapsed());
    let t = body.tolerance();
    let plane = body
        .faces()
        .find_map(|face| match face.surface() {
            Surface3::Plane(plane) if plane.origin().x > 200.0 => Some(*plane),
            _ => None,
        })
        .unwrap();
    let tool = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(
                Point2::new(-0.3730080029745935, 7.445467850896364),
                15.095354657612303,
            )
            .unwrap()],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap()
    .extrude(plane, -plane.normal() * 3.0)
    .unwrap();
    for (label, source) in [("plain", &body), ("rounded", &rounded)] {
        let start = std::time::Instant::now();
        let result = source.difference(&tool).unwrap();
        eprintln!("{label} cut {:?}", start.elapsed());
        assert_eq!(result.solids().len(), 1);
        assert!(
            (source.volume() - result.volume() - tool.volume()).abs() < tool.volume() * 1e-3
        );
    }
}
#[test]
fn sweep_starts_at_rail_end_holding_the_profile() {
    let p = circle();
    let t = p.tolerance();
    let rail = rail();
    // Profile placed at the rail's end rather than its start
    let end = rail.curve().evaluate(rail.range().end).unwrap();
    let tangent = Plane3::xy().vector_at(end.first_derivative).normalize();
    let frame = Frame3::from_z(Plane3::xy().point_at(end.point), tangent).unwrap();
    let plane = Plane3::new(frame.origin(), frame.x(), frame.y(), t).unwrap();
    let body = sweep_xy(&p, plane, &rail).unwrap();
    verify(&body, p.area() * curve_length(&xy(&rail), rail.range()));
    // The tube must stay around the rail: within its control hull plus radius
    let mesh = body
        .tessellate(TessellationOptions::new(0.05, 0.15, 24).unwrap())
        .unwrap();
    for point in mesh.positions {
        assert!(
            point.x > -1.01 && point.x < 21.01 && point.y > -1.01 && point.y < 11.01,
            "{point:?} left the rail"
        );
    }
}
fn curve_length(curve: &Curve3, range: ParameterRange) -> f64 {
    (0..8192)
        .map(|i| {
            let parameter = range.start + range.span() * (i as f64 + 0.5) / 8192.0;
            curve.evaluate(parameter).unwrap().first_derivative.magnitude()
                * range.span().abs()
                / 8192.0
        })
        .sum()
}
/// A plane through the rail start, facing along the rail
fn start_plane(curve: &Curve3, range: ParameterRange) -> Plane3 {
    let start = curve.evaluate(range.start).unwrap();
    let tangent = start.first_derivative * range.span().signum();
    let frame = Frame3::from_z(start.point, tangent).unwrap();
    Plane3::new(frame.origin(), frame.x(), frame.y(), ModelTolerance::default()).unwrap()
}
/// The planar test rail lifted onto a tilted plane, optionally with one
/// control point lifted out of that plane.
fn tilted_rail(lift: f64) -> Curve3 {
    let t = ModelTolerance::default();
    let plane = Plane3::new(
        Point3::new(1.0, 2.0, 3.0),
        Vec3::new(1.0, 0.0, 1.0).normalize(),
        Vec3::unit_y(),
        t,
    )
    .unwrap();
    let Curve2::Nurbs(nurbs) = rail().curve().clone() else { unreachable!() };
    let mut points: Vec<_> = nurbs.control_points().iter().map(|point| plane.point_at(*point)).collect();
    points[2] += plane.normal() * lift;
    NurbsCurve3::new(nurbs.degree(), points, nurbs.weights().to_vec(), nurbs.knots().to_vec())
        .unwrap()
        .into()
}
#[test]
fn sweep_along_planar_3d_rails() {
    let p = circle();
    let t = p.tolerance();
    let spline = tilted_rail(0.0);
    let range = rail().range();
    let surface = SweptSurface3::new(
        start_plane(&spline, range), p.outer().segments()[0].clone(), spline.clone(), range, t,
    )
    .unwrap();
    assert!(matches!(surface.orientation, Orientation::Planar(_)));
    verify(
        &p.sweep(start_plane(&spline, range), &rail3(&spline, range)).unwrap(),
        p.area() * curve_length(&spline, range),
    );

    let line: Curve3 = Line3::new(Point3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 1.0, 0.0)).unwrap().into();
    let range = ParameterRange::new(0.0, 5.0).unwrap();
    verify(
        &p.sweep(start_plane(&line, range), &rail3(&line, range)).unwrap(),
        p.area() * 50.0_f64.sqrt(),
    );

    let arc: Curve3 = Circle3::new(
        Point3::new(1.0, 2.0, 3.0),
        Vec3::new(1.0, 0.0, 1.0).normalize() * 10.0,
        Vec3::unit_y() * 10.0,
        t,
    )
    .unwrap()
    .into();
    let range = ParameterRange::new(0.0, std::f64::consts::FRAC_PI_2).unwrap();
    verify(
        &p.sweep(start_plane(&arc, range), &rail3(&arc, range)).unwrap(),
        p.area() * 10.0 * std::f64::consts::FRAC_PI_2,
    );
}
#[test]
fn integrated_frame_matches_planar_closed_form() {
    let t = ModelTolerance::default();
    let spline = tilted_rail(0.0);
    let range = rail().range();
    let section = TrimmedCurve2::circle(Point2::new(0.3, 0.2), 0.5).unwrap();
    let planar = SweptSurface3::new(start_plane(&spline, range), section, spline, range, t).unwrap();
    let mut integrated = planar.clone();
    integrated.orientation = Orientation::Integrated { checkpoints: Arc::from([]), twist: 0.0 };
    integrated.orientation = integrated.integrate().unwrap();
    for i in 0..=40 {
        let v = range.start + range.span() * i as f64 / 40.0;
        let [a, b] = [&planar, &integrated].map(|surface| surface.section_plane(v).unwrap());
        assert!(a.origin().distance(b.origin()) < 1e-9, "origin at {v}");
        assert!((a.u() - b.u()).magnitude() < 1e-9 && (a.v() - b.v()).magnitude() < 1e-9, "axes at {v}");
    }
}
#[test]
fn sweep_along_spatial_rail() {
    let p = circle();
    let t = p.tolerance();
    let spline = tilted_rail(2.0);
    let range = rail().range();
    let plane = start_plane(&spline, range);
    let section = TrimmedCurve2::circle(Point2::new(0.3, 0.2), 0.5).unwrap();
    let surface = SweptSurface3::new(plane, section, spline.clone(), range, t).unwrap();
    assert!(matches!(surface.orientation, Orientation::Integrated { .. }));
    let step = 1e-5;
    // Away from knots, where the rail jerk and so the surface curvature jump
    for v in [0.05, 0.2, 0.45, 0.7, 0.95] {
        // Sections stay perpendicular to the rail
        let tangent = spline.evaluate(v).unwrap().first_derivative.normalize();
        assert!((surface.section_plane(v).unwrap().normal().dot(tangent).abs() - 1.0).abs() < 1e-9);
        // Analytic derivatives agree with the integrated surface
        let value = surface.evaluate(1.0, v).unwrap();
        let [ahead, behind] = [v + step, v - step].map(|v| surface.evaluate(1.0, v).unwrap());
        let dv = (ahead.point - behind.point) / (2.0 * step);
        let dvv = (ahead.dv - behind.dv) / (2.0 * step);
        assert!((dv - value.dv).magnitude() < 1e-6 * value.dv.magnitude(), "dv at {v}");
        assert!((dvv - value.dvv).magnitude() < 1e-4 * value.dvv.magnitude().max(1.0), "dvv at {v}");
    }
    verify(&p.sweep(plane, &rail3(&spline, range)).unwrap(), p.area() * curve_length(&spline, range));
}
#[test]
fn sweep_along_closed_spatial_rail() {
    let t = ModelTolerance::default();
    // An irregular spatial loop as a periodic cubic B-spline
    let mut points: Vec<_> = (0..8)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / 8.0;
            let radius = 10.0 + 3.0 * angle.cos();
            Point3::new(radius * angle.cos(), radius * angle.sin(), 4.0 * angle.sin() + 3.0 * (2.0 * angle).sin())
        })
        .collect();
    points.extend(points[..3].to_vec());
    let knots = (0..15).map(f64::from).collect();
    let rail: Curve3 = NurbsCurve3::new(3, points, vec![1.0; 11], knots).unwrap().into();
    let range = ParameterRange::new(3.0, 11.0).unwrap();
    let p = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-0.5, -0.5),
                Point2::new(0.5, -0.5),
                Point2::new(0.5, 0.5),
                Point2::new(-0.5, 0.5),
            ],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap();
    let plane = start_plane(&rail, range);
    let surface =
        SweptSurface3::new(plane, p.outer().segments()[0].clone(), rail.clone(), range, t).unwrap();
    // Transport alone would leave the section turned at the seam
    let Orientation::Integrated { twist, .. } = surface.orientation else { unreachable!() };
    assert!(twist.abs() > 1e-4);
    // The section closes up at the seam
    let [start, end] = [range.start, range.end].map(|v| surface.section_plane(v).unwrap());
    assert!((start.u() - end.u()).magnitude() < 1e-8 && start.origin().distance(end.origin()) < 1e-8);
    verify(&p.sweep(plane, &rail3(&rail, range)).unwrap(), p.area() * curve_length(&rail, range));
}
#[test]
fn sweep_along_surface_intersection_edge() {
    let t = ModelTolerance::default();
    let disc = |radius: f64| {
        Profile::new(
            Wire2::new([TrimmedCurve2::circle(Point2::new(0.0, 0.0), radius).unwrap()], t).unwrap(),
            [],
            t,
        )
        .unwrap()
    };
    let upright = disc(5.0).extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0)).unwrap();
    let across = disc(4.0)
        .extrude(
            Plane3::new(Point3::new(-10.0, 0.0, 10.0), Vec3::unit_y(), Vec3::unit_z(), t).unwrap(),
            Vec3::new(20.0, 0.0, 0.0),
        )
        .unwrap();
    let body = upright.union(&across).unwrap();
    let edge = body
        .edges()
        .find(|edge| matches!(edge.curve(), Curve3::SurfaceIntersection(_)))
        .unwrap();
    let (rail, range) = (edge.curve(), edge.range().unwrap());
    // The rail frame relies on the curve's exact curvature
    let step = 1e-6;
    for v in [0.1, 0.25, 0.4, 0.6, 0.9] {
        let value = rail.evaluate(v).unwrap();
        let difference = (rail.evaluate(v + step).unwrap().first_derivative
            - rail.evaluate(v - step).unwrap().first_derivative)
            / (2.0 * step);
        assert!(
            (difference - value.second_derivative).magnitude()
                < 1e-6 * value.second_derivative.magnitude(),
            "curvature at {v}"
        );
    }
    let p = disc(0.3);
    verify(
        &p.sweep(start_plane(rail, range), &rail3(rail, range)).unwrap(),
        p.area() * curve_length(rail, range),
    );
}

#[test]
fn swept_circle_through_cuts_from_cut_fails_document() {
    let t = ModelTolerance::default();
    let rail: Curve3 = NurbsCurve3::new(
        4,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(-37.169861177805814, 0.0, 0.0),
            Point3::new(-45.73255171091292, 37.85098428839387, 0.0),
            Point3::new(-71.80983651628452, 68.59882756636935, 0.0),
            Point3::new(-118.12620803627291, 41.353903142846775, 0.0),
        ],
        vec![1.0; 5],
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    ).unwrap().into();
    let profile = Profile::new(
        Wire2::new([TrimmedCurve2::circle(Point2::new(0.0, 0.0), 15.3255258862366).unwrap()], t).unwrap(),
        [], t,
    ).unwrap();
    let plane = Plane3::new(Point3::new(0.0, 0.0, 0.0), -Vec3::unit_z(), Vec3::unit_y(), t).unwrap();
    let body = profile.sweep(plane, &rail3(&rail, ParameterRange::new(0.0, 1.0).unwrap())).unwrap();
    let polygons = [
        [(-106.20972552812498, 11.868346132014786), (-53.986287235871465, -14.172223915621164),
         (-44.940053217144865, 12.550922107568695), (-64.08968090020764, 34.50590926012473)],
        [(-49.85963952340458, 74.76414919788961), (-58.356989810373555, 39.26282314317854),
         (-46.525691178162795, 28.773218170290647), (1.9369041779040828, 51.85413948384922)],
    ];
    let cutters = polygons.map(|points| {
        Profile::new(Wire2::polygon(points.map(|(x, y)| Point2::new(x, y)), t).unwrap(), [], t).unwrap()
            .extrude(Plane3::new(Point3::new(0.0, 0.0, -15.7255258862366), Vec3::unit_x(), Vec3::unit_y(), t).unwrap(),
                Vec3::new(0.0, 0.0, 31.4510517724732)).unwrap()
    });
    let cutters = cutters.map(|cutter| cutter.intersection(&body).unwrap());
    let tool = cutters[1].union(&cutters[0]).unwrap();
    let cut = body.difference(&tool).expect("both through cuts must close on the periodic swept face");
    assert!(cut.volume() > 0.0 && cut.volume() < body.volume());
    assert!(!cut.tessellate(TessellationOptions::new(0.05, 0.15, 24).unwrap()).unwrap().triangles.is_empty());
}

/// A rail segment given in XY plane coordinates
fn segment(curve: TrimmedCurve2) -> TrimmedCurve3 {
    TrimmedCurve3::new(xy(&curve), curve.range()).unwrap()
}
fn line(a: (f64, f64), b: (f64, f64)) -> TrimmedCurve2 {
    TrimmedCurve2::line(Point2::new(a.0, a.1), Point2::new(b.0, b.1)).unwrap()
}
fn arc(center: (f64, f64), start: f64, end: f64) -> TrimmedCurve2 {
    TrimmedCurve2::new(
        Circle2::new(Point2::new(center.0, center.1), 5.0).unwrap(),
        ParameterRange::new(start, end).unwrap(),
    )
    .unwrap()
}
fn x_normal_plane(x: f64) -> Plane3 {
    Plane3::new(Point3::new(x, 0.0, 0.0), Vec3::unit_y(), Vec3::unit_z(), ModelTolerance::default()).unwrap()
}
use std::f64::consts::{FRAC_PI_2, PI, TAU};

#[test]
fn sweep_along_unordered_tangent_chain() {
    let p = circle();
    let segments = [
        segment(arc((10.0, 5.0), 1.5 * PI, TAU).reversed()),
        segment(line((15.0, 5.0), (15.0, 15.0))),
        segment(line((0.0, 0.0), (10.0, 0.0)).reversed()),
    ];
    let rail = Wire3::connect(segments, p.tolerance()).unwrap();
    assert_eq!(rail.segments().len(), 3);
    verify(
        &p.sweep(x_normal_plane(0.0), &rail).unwrap(),
        p.area() * (20.0 + 2.5 * PI),
    );
}

#[test]
fn sweep_rail_propagates_through_tangent_segments() {
    let p = circle();
    let candidates = [
        segment(line((15.0, 15.0), (15.0, 5.0))),
        segment(line((15.0, 15.0), (25.0, 15.0))),
        segment(line((0.0, 0.0), (0.0, -10.0))),
        segment(arc((10.0, 5.0), 1.5 * PI, TAU)),
    ];
    let rail = Wire3::connect([segment(line((0.0, 0.0), (10.0, 0.0)))], p.tolerance())
        .unwrap()
        .extended(&candidates, true)
        .unwrap();
    assert_eq!(rail.segments().len(), 3);
    verify(
        &p.sweep(x_normal_plane(0.0), &rail).unwrap(),
        p.area() * (20.0 + 2.5 * PI),
    );
}

#[test]
fn sweep_along_closed_tangent_chain_from_its_middle() {
    let p = circle();
    let segments = [
        segment(line((0.0, 0.0), (10.0, 0.0))),
        segment(arc((10.0, 5.0), 1.5 * PI, TAU)),
        segment(line((15.0, 5.0), (15.0, 15.0))),
        segment(arc((10.0, 15.0), 0.0, FRAC_PI_2)),
        segment(line((10.0, 20.0), (0.0, 20.0))),
        segment(arc((0.0, 15.0), FRAC_PI_2, PI)),
        segment(line((-5.0, 15.0), (-5.0, 5.0))),
        segment(arc((0.0, 5.0), PI, 1.5 * PI)),
    ];
    let rail = Wire3::connect(segments, p.tolerance()).unwrap();
    assert!(rail.is_closed());
    verify(
        &p.sweep(x_normal_plane(5.0), &rail).unwrap(),
        p.area() * (40.0 + 10.0 * PI),
    );
}

#[test]
fn sweep_rail_rejects_corners_and_gaps() {
    let p = circle();
    let t = p.tolerance();
    let first = segment(line((0.0, 0.0), (10.0, 0.0)));
    let corner = Wire3::connect([first.clone(), segment(line((10.0, 0.0), (10.0, 10.0)))], t).unwrap();
    assert!(p.sweep(x_normal_plane(0.0), &corner).is_err());
    assert!(Wire3::connect([first, segment(line((11.0, 0.0), (20.0, 0.0)))], t).is_err());
}
