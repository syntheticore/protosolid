use cgmath::prelude::*;

use crate::{Matrix3, Point2, Point3, Vec3};

use super::{
    BodyId, BooleanKind, Circle3, Curve3, IntersectionSet, Kernel, Line3, MassProperties,
    ModelTolerance, NurbsCurve3, NurbsSurface3, Plane3, PolyhedralMesh, Profile, Resolution,
    RigidTransform3, Sphere3, Surface3, TessellationOptions, TrimmedCurve2, Wire2,
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
fn exact_sphere_is_published_and_round_trips() {
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

    let encoded = kernel.to_ron().expect("archive should encode");
    let restored = Kernel::from_ron(&encoded).expect("archive should restore");
    assert!(restored.validate().is_valid());
    assert_eq!(restored.topology().bodies().count(), 1);
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
