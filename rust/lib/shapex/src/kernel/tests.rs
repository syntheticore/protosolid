use cgmath::prelude::*;

use crate::{Matrix3, Point2, Point3, Vec3};

use super::{
    Axis3, BodyId, BooleanKind, Circle3, Curve2, Curve3, IntersectionSet, Kernel, Line3,
    MassProperties, ModelTolerance, NurbsCurve2, NurbsCurve3, NurbsSurface3, ParameterRange,
    Plane3, PolyhedralMesh, Profile, Resolution, RigidTransform3, Sphere3, Surface3,
    TessellationOptions, TrimmedCurve2, Wire2,
};

fn certified_properties(kernel: &Kernel, body: BodyId) -> MassProperties {
    super::physical::certify_topology(kernel.topology(), kernel.tolerance())
        .expect("certified topology")[&body]
        .properties
}

fn cuboid(kernel: &mut Kernel, minimum: Point3, maximum: Point3) -> BodyId {
    let size = maximum - minimum;
    let tolerance = kernel.tolerance();
    let wire = Wire2::polygon(
        [
            Point2::new(0.0, 0.0),
            Point2::new(size.x, 0.0),
            Point2::new(size.x, size.y),
            Point2::new(0.0, size.y),
        ],
        tolerance,
    )
    .expect("cuboid wire");
    let profile = Profile::new(wire, [], tolerance).expect("cuboid profile");
    let plane = Plane3::new(minimum, Vec3::unit_x(), Vec3::unit_y(), tolerance).expect("plane");
    let mut transaction = kernel.begin_operation();
    let body = transaction
        .extrude_curve_profile(plane, &profile, Vec3::new(0.0, 0.0, size.z))
        .expect("cuboid extrusion");
    transaction.commit().expect("cuboid publication");
    body
}

fn owned_cuboid(minimum: Point3, maximum: Point3) -> super::Compound {
    let tolerance = ModelTolerance::default();
    let size = maximum - minimum;
    let wire = Wire2::polygon(
        [
            Point2::new(0.0, 0.0),
            Point2::new(size.x, 0.0),
            Point2::new(size.x, size.y),
            Point2::new(0.0, size.y),
        ],
        tolerance,
    )
    .expect("cuboid wire");
    let profile = Profile::new(wire, [], tolerance).expect("cuboid profile");
    let plane = Plane3::new(minimum, Vec3::unit_x(), Vec3::unit_y(), tolerance).expect("plane");
    profile
        .extrude(plane, Vec3::new(0.0, 0.0, size.z))
        .expect("owned cuboid")
}

fn boolean(
    kernel: &mut Kernel,
    first: BodyId,
    second: BodyId,
    kind: BooleanKind,
) -> (Vec<BodyId>, super::OperationHistory) {
    let mut transaction = kernel.begin_operation();
    transaction
        .boolean(first, second, kind)
        .expect("boolean construction");
    let committed = transaction.finish().expect("boolean publication");
    (committed.bodies, committed.history)
}

#[test]
fn exact_cylinder_is_published_and_certified() {
    let mut kernel = Kernel::new(ModelTolerance::default()).expect("valid tolerance");
    let mut transaction = kernel.begin_operation();
    let body = transaction
        .cylinder(
            Point3::new(2.0, -1.0, 3.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(0.0, 0.0, 5.0),
        )
        .expect("cylinder should publish");
    transaction.commit().expect("publish");
    assert!(kernel.validate().is_valid());
    let properties = certified_properties(&kernel, body);
    assert!((properties.volume - 20.0 * std::f64::consts::PI).abs() < 1.0e-10);
    assert!(properties.centroid.distance(Point3::new(2.0, -1.0, 5.5)) < 1.0e-12);
}

#[test]
fn exact_sphere_is_published_and_certified() {
    let mut kernel = Kernel::new(ModelTolerance::default()).expect("valid tolerance");
    let mut transaction = kernel.begin_operation();
    let body = transaction
        .sphere(
            Point3::new(-2.0, 4.0, 1.0),
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(0.0, 0.0, 3.0),
        )
        .expect("sphere should publish");
    transaction.commit().expect("publish");
    assert!(kernel.validate().is_valid());
    let properties = certified_properties(&kernel, body);
    assert!((properties.volume - 36.0 * std::f64::consts::PI).abs() < 1.0e-10);
}

#[test]
fn exact_cone_is_published_and_certified() {
    let mut kernel = Kernel::new(ModelTolerance::default()).expect("valid tolerance");
    let mut transaction = kernel.begin_operation();
    let body = transaction
        .cone(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 4.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
        )
        .expect("cone should publish");
    transaction.commit().expect("publish");
    assert!(kernel.validate().is_valid());
    let properties = certified_properties(&kernel, body);
    assert!((properties.volume - 16.0 * std::f64::consts::PI / 3.0).abs() < 1.0e-10);
    assert!(properties.centroid.distance(Point3::new(0.0, 0.0, 3.0)) < 1.0e-12);
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
fn concentric_circular_hole_is_certified_without_tessellation() {
    let tolerance = ModelTolerance::default();
    let outer = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 5.0).expect("outer circle")],
        tolerance,
    )
    .expect("outer wire");
    let hole = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 2.0).expect("inner circle")],
        tolerance,
    )
    .expect("hole wire");
    let profile = Profile::new(outer, [hole], tolerance).expect("annular profile");
    let solid = profile
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 3.0))
        .expect("annular prism");
    assert!((solid.volume() - 63.0 * std::f64::consts::PI).abs() < 1.0e-8);
    let cylindrical_faces = solid
        .faces()
        .filter(|face| matches!(face.surface(), Surface3::Cylinder(_)))
        .collect::<Vec<_>>();
    assert_eq!(
        cylindrical_faces.len(),
        2,
        "each full circular boundary must sweep to one periodic cylindrical face"
    );
    assert!(cylindrical_faces.iter().any(
        |face| matches!(face.surface(), Surface3::Cylinder(cylinder) if (cylinder.radius() - 2.0).abs() < 1.0e-9)
    ));
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
    assert!((pocket.volume() - expected).abs() < 1.0e-6);
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
fn rigid_transform_preserves_names_and_recertifies() {
    let tolerance = ModelTolerance::default();
    let mut kernel = Kernel::new(tolerance).expect("kernel");
    let mut transaction = kernel.begin_operation();
    let body = transaction
        .sphere(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        )
        .expect("sphere");
    transaction.commit().expect("publish");
    let names = kernel.topology().body_entity_names(body).expect("names");
    let transform = RigidTransform3::new(
        Matrix3::from_angle_y(crate::Rad(std::f64::consts::FRAC_PI_2)),
        Vec3::new(7.0, -3.0, 2.0),
        tolerance,
    )
    .expect("rigid transform");
    kernel
        .transform_body(body, transform)
        .expect("transform should recertify");
    assert_eq!(
        names,
        kernel.topology().body_entity_names(body).expect("names")
    );
    assert!(kernel.validate().is_valid());
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
fn exact_curved_edges_are_shared_by_face_tessellation() {
    let tolerance = ModelTolerance::default();
    let mut kernel = Kernel::new(tolerance).expect("kernel");
    let mut transaction = kernel.begin_operation();
    let body = transaction
        .cylinder(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        )
        .expect("cylinder");
    transaction.commit().expect("publish");
    let options = TessellationOptions::new(2.0e-3, 5.0e-2, 14).expect("options");
    let mesh = kernel.tessellate_body(body, options).expect("mesh");
    assert!(!mesh.is_empty());
    assert!(mesh.triangles.iter().all(|triangle| {
        triangle[0] < mesh.positions.len()
            && triangle[1] < mesh.positions.len()
            && triangle[2] < mesh.positions.len()
    }));
}

#[test]
fn torus_changes_genus_with_euler_operators_and_is_certified() {
    let tolerance = ModelTolerance::default();
    let mut kernel = Kernel::new(tolerance).expect("kernel");
    let mut transaction = kernel.begin_operation();
    let body = transaction
        .torus(
            Point3::new(1.0, 2.0, 3.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 4.0, 0.0),
            1.25,
        )
        .expect("torus should publish");
    transaction.commit().expect("publish");
    assert!(kernel.validate().is_valid());
    let properties = certified_properties(&kernel, body);
    let expected = 2.0 * std::f64::consts::PI.powi(2) * 4.0 * 1.25_f64.powi(2);
    assert!((properties.volume - expected).abs() < 1.0e-9);
    let shell = kernel.topology().body(body).expect("body").shells()[0];
    assert_eq!(
        kernel.topology().shell(shell).expect("shell").faces().len(),
        2
    );
    let mesh = kernel
        .tessellate_body(
            body,
            TessellationOptions::new(5.0e-2, 1.5e-1, 12).expect("options"),
        )
        .expect("torus tessellation");
    assert!(!mesh.is_empty());
}

#[test]
fn rational_splines_validate_domains_and_derivatives() {
    let curve: Curve3 = NurbsCurve3::new(
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
        ],
        vec![1.0, 0.5, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    )
    .expect("rational curve")
    .into();
    let curve_value = curve.evaluate(0.5).expect("curve evaluation");
    assert!(curve_value.point.x.is_finite());
    assert!(curve_value.first_derivative.magnitude2() > 0.0);

    let surface: Surface3 = NurbsSurface3::new(
        (1, 1),
        vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
            vec![Point3::new(0.0, 2.0, 0.0), Point3::new(2.0, 2.0, 1.0)],
        ],
        vec![vec![1.0, 1.0], vec![1.0, 1.0]],
        (vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]),
    )
    .expect("rational surface")
    .into();
    let surface_value = surface.evaluate(0.5, 0.5).expect("surface evaluation");
    assert!(surface_value.normal().is_ok());
    assert!(surface_value.curvature().is_ok());

    assert!(NurbsCurve3::new(
        2,
        vec![Point3::new(0.0, 0.0, 0.0); 3],
        vec![1.0; 3],
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0],
    )
    .is_err());
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
        super::wire::curve_pair_intersection(&first, &second, ModelTolerance::default())
            .expect("certified spline intersections");
    assert_eq!(intersections.hits.len(), 2);
    assert!(intersections
        .hits
        .iter()
        .all(|hit| (hit.point.y - 0.5).abs() < 1.0e-6));

    let circle = TrimmedCurve2::circle(Point2::new(1.0, 0.0), 1.0).expect("circle");
    let circle_hits =
        super::wire::curve_pair_intersection(&first, &circle, ModelTolerance::default())
            .expect("certified spline/circle intersections");
    assert_eq!(circle_hits.hits.len(), 3);
}

#[test]
fn folded_simple_nurbs_segment_is_certified_by_exact_subdivision() {
    let tolerance = ModelTolerance::default();
    let folded_top = TrimmedCurve2::new(
        NurbsCurve2::new(
            3,
            vec![
                Point2::new(4.0, 2.0),
                Point2::new(5.0, 4.0),
                Point2::new(-1.0, 4.0),
                Point2::new(0.0, 2.0),
            ],
            vec![1.0; 4],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        )
        .expect("folded spline"),
        ParameterRange::new(0.0, 1.0).expect("range"),
    )
    .expect("trimmed spline");
    let profile = Profile::new(
        Wire2::new(
            [
                TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)).expect("bottom"),
                TrimmedCurve2::line(Point2::new(4.0, 0.0), Point2::new(4.0, 2.0)).expect("right"),
                folded_top,
                TrimmedCurve2::line(Point2::new(0.0, 2.0), Point2::new(0.0, 0.0)).expect("left"),
            ],
            tolerance,
        )
        .expect("exactly certified folded wire"),
        [],
        tolerance,
    )
    .expect("folded profile");
    let prism = profile
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 3.0))
        .expect("folded analytic prism");
    assert!((prism.volume() - profile.area() * 3.0).abs() < 1.0e-6);
}

#[test]
fn arbitrary_triangle_polyhedron_is_euler_shelled_and_certified() {
    let tolerance = ModelTolerance::default();
    let mesh = PolyhedralMesh::new(
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(0.0, 0.0, 4.0),
        ],
        [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        tolerance,
    )
    .expect("tetrahedron input");
    let mut kernel = Kernel::new(tolerance).expect("kernel");
    let mut transaction = kernel.begin_operation();
    let body = transaction
        .polyhedron(&mesh)
        .expect("polyhedron should publish");
    transaction.commit().expect("publish");
    assert!((certified_properties(&kernel, body).volume - 4.0).abs() < 1.0e-10);
    assert!(kernel.validate().is_valid());

    let cube = PolyhedralMesh::new(
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
            Point3::new(1.0, 0.0, 1.0),
            Point3::new(1.0, 1.0, 1.0),
            Point3::new(0.0, 1.0, 1.0),
        ],
        [
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ],
        tolerance,
    )
    .expect("triangulated cube input");
    let mut transaction = kernel.begin_operation();
    let cube_body = transaction
        .polyhedron(&cube)
        .expect("triangulated cube should publish");
    transaction.commit().expect("publish");
    assert!((certified_properties(&kernel, cube_body).volume - 1.0).abs() < 1.0e-10);
}

#[test]
fn planar_boolean_is_rebuilt_and_physically_certified() {
    let mut kernel = Kernel::new(ModelTolerance::default()).expect("kernel");
    let first = cuboid(
        &mut kernel,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 1.0),
    );
    let second = cuboid(
        &mut kernel,
        Point3::new(0.5, 0.0, 0.0),
        Point3::new(1.5, 1.0, 1.0),
    );

    let (intersection, history) = boolean(&mut kernel, first, second, BooleanKind::Intersection);
    assert!((certified_properties(&kernel, intersection[0]).volume - 0.5).abs() < 1.0e-8);
    assert!(history.origins().values().any(|origin| {
        matches!(origin, super::Origin::Derived { sources, .. } if !sources.is_empty())
    }));

    let (union, _) = boolean(&mut kernel, first, second, BooleanKind::Union);
    assert!((certified_properties(&kernel, union[0]).volume - 1.5).abs() < 1.0e-8);

    let (difference, _) = boolean(&mut kernel, first, second, BooleanKind::Difference);
    assert!((certified_properties(&kernel, difference[0]).volume - 0.5).abs() < 1.0e-8);

    let distant = cuboid(
        &mut kernel,
        Point3::new(5.0, 5.0, 5.0),
        Point3::new(6.0, 6.0, 6.0),
    );
    let (empty, _) = boolean(&mut kernel, first, distant, BooleanKind::Intersection);
    assert!(empty.is_empty());

    let (disconnected, _) = boolean(&mut kernel, first, distant, BooleanKind::Union);
    assert_eq!(disconnected.len(), 2);
    assert!(
        (disconnected
            .iter()
            .map(|body| certified_properties(&kernel, *body).volume)
            .sum::<f64>()
            - 2.0)
            .abs()
            < 1.0e-8
    );
    assert!(kernel.validate().is_valid());
}

#[test]
fn planar_boolean_difference_preserves_a_closed_cavity_shell() {
    let mut kernel = Kernel::new(ModelTolerance::default()).expect("kernel");
    let outer = cuboid(
        &mut kernel,
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(3.0, 3.0, 3.0),
    );
    let void = cuboid(
        &mut kernel,
        Point3::new(1.0, 1.0, 1.0),
        Point3::new(2.0, 2.0, 2.0),
    );
    let (difference, _) = boolean(&mut kernel, outer, void, BooleanKind::Difference);
    assert_eq!(difference.len(), 1);
    let body = difference[0];
    assert_eq!(
        kernel.topology().body(body).expect("body").shells().len(),
        2
    );
    assert!((certified_properties(&kernel, body).volume - 26.0).abs() < 1.0e-8);
    assert!(kernel.validate().is_valid());
}

#[test]
fn low_level_euler_commit_is_atomic_and_needs_no_identity() {
    let tolerance = ModelTolerance::default();
    let mut kernel = Kernel::new(tolerance).expect("kernel");
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("plane");
    let wire = Wire2::polygon(
        [
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(0.0, 1.0),
        ],
        tolerance,
    )
    .expect("wire");
    let profile = Profile::new(wire, [], tolerance).expect("profile");
    let mut transaction = kernel.begin_operation();
    transaction
        .extrude_curve_profile(plane, &profile, Vec3::unit_z())
        .expect("Euler extrusion");
    transaction.commit().expect("publish");
    assert!(kernel.validate().is_valid());
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
fn high_level_profile_loft_is_euler_built_and_certified() {
    let tolerance = ModelTolerance::default();
    let lower = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-1.0, -1.0),
                Point2::new(1.0, -1.0),
                Point2::new(1.0, 1.0),
                Point2::new(-1.0, 1.0),
            ],
            tolerance,
        )
        .expect("lower wire"),
        [],
        tolerance,
    )
    .expect("lower profile");
    let upper = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-0.5, -0.5),
                Point2::new(0.5, -0.5),
                Point2::new(0.5, 0.5),
                Point2::new(-0.5, 0.5),
            ],
            tolerance,
        )
        .expect("upper wire"),
        [],
        tolerance,
    )
    .expect("upper profile");
    let upper_plane = Plane3::new(
        Point3::new(0.0, 0.0, 3.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("upper plane");
    let loft = lower
        .loft(Plane3::xy(), [(upper_plane, &upper)])
        .expect("loft");

    assert!(loft.volume() > 3.0 && loft.volume() < 12.0);
}

#[test]
fn circular_loft_uses_bounded_construction_sampling() {
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
    let lower = circle_profile(8.0);
    let upper = circle_profile(5.0);
    let upper_plane = Plane3::new(
        Point3::new(0.0, 0.0, 30.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("upper plane");
    let loft = lower
        .loft(Plane3::xy(), [(upper_plane, &upper)])
        .expect("circular loft");
    let analytic = 10.0 * std::f64::consts::PI * (64.0 + 40.0 + 25.0);
    assert!((loft.volume() - analytic).abs() / analytic < 0.01);

    for (u, v, offset) in [
        (Vec3::unit_z(), Vec3::unit_y(), Vec3::new(-30.0, 0.0, 0.0)),
        (Vec3::unit_z(), -Vec3::unit_y(), Vec3::new(30.0, 0.0, 0.0)),
        (-Vec3::unit_z(), Vec3::unit_y(), Vec3::new(30.0, 0.0, 0.0)),
        (Vec3::unit_x(), -Vec3::unit_z(), Vec3::new(0.0, 30.0, 0.0)),
    ] {
        let first_plane =
            Plane3::new(Point3::new(0.0, 0.0, 0.0), u, v, tolerance).expect("oriented first plane");
        let second_plane =
            Plane3::new(Point3::from_vec(offset), u, v, tolerance).expect("oriented second plane");
        lower
            .loft(first_plane, [(second_plane, &upper)])
            .expect("orientation-independent circular loft");
    }
    let first_plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_z(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("first plane");
    let reversed_plane = Plane3::new(
        Point3::new(-30.0, 0.0, 0.0),
        -Vec3::unit_z(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("oppositely oriented plane");
    lower
        .loft(first_plane, [(reversed_plane, &upper)])
        .expect("loft across oppositely oriented section frames");

    let browser_lower = circle_profile(13.867620046437446);
    let browser_upper = circle_profile(5.734290145784257);
    let browser_first = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        -Vec3::unit_z(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("browser first plane");
    let browser_second = Plane3::new(
        Point3::new(30.0, 0.0, 0.0),
        -Vec3::unit_z(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("browser second plane");
    browser_lower
        .loft(browser_first, [(browser_second, &browser_upper)])
        .expect("browser circular loft fixture");
}

#[test]
fn shell_with_opening_is_euler_built_and_certified() {
    let cube = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0));
    let top = cube
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.origin().z > 9.9))
        .expect("top face")
        .clone();
    let shell = cube.shell(&[top], -0.5).expect("open shell");

    assert!((shell.volume() - 230.5).abs() < 1.0e-6);
}

#[test]
fn closed_shell_is_a_certified_body_with_a_cavity() {
    let cube = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0));
    let shell = cube.shell(&[], -1.0).expect("closed shell");

    assert_eq!(shell.solids().len(), 1);
    assert!((shell.volume() - 488.0).abs() < 1.0e-6);
}

#[test]
fn closed_cylindrical_shell_is_certified() {
    let tolerance = ModelTolerance::default();
    let circle = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 5.0).expect("circle")],
        tolerance,
    )
    .expect("circle wire");
    let cylinder = Profile::new(circle, [], tolerance)
        .expect("circle profile")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .expect("cylinder");
    let shell = cylinder.shell(&[], -0.5).expect("closed cylinder shell");

    assert!(shell.volume() > 0.0 && shell.volume() < cylinder.volume());
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

    assert!(
        super::features::coincident_opposite_face_patch_names(&frame, &boss)
            .expect("face-patch detection")
            .is_some(),
        "the circular base must be detected inside the holed planar cap"
    );
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
fn owned_operations_resolve_entities_transitively_and_tessellate_them() {
    let cube = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(4.0, 4.0, 1.0));
    let top = cube
        .faces()
        .find(|face| match face.surface() {
            Surface3::Plane(plane) => (plane.origin().z - 1.0).abs() < 1.0e-10,
            _ => false,
        })
        .expect("top face")
        .clone();
    let first_tool = owned_cuboid(Point3::new(-0.5, -0.5, -1.0), Point3::new(1.0, 1.0, 2.0));
    let second_tool = owned_cuboid(Point3::new(3.0, 3.0, -1.0), Point3::new(4.5, 4.5, 2.0));

    let first_cut = cube.difference(&first_tool).expect("first cut");
    let second_cut = first_cut.difference(&second_tool).expect("second cut");
    let current_top = match second_cut.resolve(&top) {
        Resolution::Modified(face) => face,
        other => panic!("expected a transitively modified top face, got {other:?}"),
    };
    assert!(!current_top
        .tessellate(TessellationOptions::default())
        .expect("face mesh")
        .is_empty());
    let edge = second_cut.edges().next().expect("edge");
    assert!(
        edge.tessellate(TessellationOptions::default())
            .expect("edge polyline")
            .points
            .len()
            >= 2
    );

    let unrelated = owned_cuboid(Point3::new(10.0, 10.0, 10.0), Point3::new(11.0, 11.0, 11.0));
    let unrelated_face = unrelated.faces().next().expect("unrelated face");
    assert!(matches!(
        second_cut.resolve(unrelated_face),
        Resolution::Unrelated
    ));
}

#[test]
fn extrusion_exposes_semantic_creation_provenance() {
    let extrusion = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 3.0, 4.0));
    let roles = extrusion
        .faces()
        .filter_map(|face| extrusion.origin(face))
        .filter_map(|origin| match origin {
            super::Origin::Created { role } => role.as_ref(),
            super::Origin::Derived { .. } => None,
        })
        .collect::<Vec<_>>();
    assert!(roles.contains(&&super::CreationRole::ExtrusionStart));
    assert!(roles.contains(&&super::CreationRole::ExtrusionEnd));
    assert_eq!(
        roles
            .iter()
            .filter(|role| matches!(role, super::CreationRole::ExtrusionSide { .. }))
            .count(),
        4
    );
}
