use cgmath::prelude::*;

use crate::{Matrix3, Point2, Point3, Vec3};

use super::{
    Axis3, BodyId, BooleanKind, Circle3, Curve2, Curve3, FilletOptions, IntersectionSet, Kernel,
    Line3, MassProperties, ModelTolerance, NurbsCurve2, NurbsCurve3, NurbsSurface3, ParameterRange,
    Plane3, PolyhedralMesh, Profile, Resolution, RevolvedSurface3, RigidTransform3, Sphere3,
    ShellJoin, SplitKeep, Surface3, TessellationOptions, TrimmedCurve2, Wire2,
};

mod blend_overflow;
mod blends;
mod booleans;
mod features;
mod provenance;
mod sweeps;

// Run both cross sections on the same source revision and selection. Keeping
// these checks in the fillet fixtures prevents their supported domains drifting.
trait CheckEdgeBlends {
    fn check_edge_blends(
        &self,
        edges: &[super::EdgeHandle],
        size: f64,
    ) -> super::KernelResult<super::Compound>;
    fn check_edge_blends_with_options(
        &self,
        edges: &[super::EdgeHandle],
        options: FilletOptions,
    ) -> super::KernelResult<super::Compound>;
}

impl CheckEdgeBlends for super::Compound {
    fn check_edge_blends(
        &self,
        edges: &[super::EdgeHandle],
        size: f64,
    ) -> super::KernelResult<super::Compound> {
        self.check_edge_blends_with_options(edges, FilletOptions::new(size))
    }
    fn check_edge_blends_with_options(
        &self,
        edges: &[super::EdgeHandle],
        options: FilletOptions,
    ) -> super::KernelResult<super::Compound> {
        let fillet = self.fillet_with_options(edges, options)?;
        let chamfer = self
            .chamfer_with_tangent_propagation(edges, options.radius, options.follow_tangent_edges)
            .unwrap_or_else(|error| {
                panic!(
                    "chamfer counterpart of supported fillet ({} edges, size {}): {error}",
                    edges.len(),
                    options.radius
                )
            });
        assert_eq!(chamfer.solids().len(), fillet.solids().len());
        chamfer
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .expect("chamfer counterpart display mesh");
        Ok(fillet)
    }
}

fn certified_properties(kernel: &Kernel, body: BodyId) -> MassProperties {
    super::physical::certify_topology(kernel.topology(), kernel.tolerance())
        .expect("certified topology")[&body]
        .properties
}

fn cut_shell_before_lower_corner_fillets() -> super::Compound {
    let tolerance = ModelTolerance::default();
    let plane = Plane3::new(Point3::origin(), Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
    let body = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 36.74094683895966).unwrap()],
            tolerance,
        ).unwrap(),
        [],
        tolerance,
    ).unwrap().extrude(plane, Vec3::unit_y() * 48.0).unwrap();
    let rim = body.edges().find(|edge| {
        edge.endpoints().unwrap().iter().all(|point| (point.y - 48.0).abs() < tolerance.linear)
    }).unwrap().clone();
    let body = body.fillet(&[rim], 18.0).unwrap();
    let opening = body.faces().find(|face| {
        matches!(face.surface(), Surface3::Plane(plane) if plane.origin().y.abs() < tolerance.linear)
    }).unwrap().clone();
    let body = body.shell(&[opening], -9.0).unwrap();
    let half_width = 13.249811649142934;
    let upper = 48.725361129211365;
    let lower = -51.204523903515636;
    let profile = Profile::new(
        Wire2::new([
            TrimmedCurve2::arc_through(
                Point2::new(half_width, upper),
                Point2::new(-2.433950913814231e-15, 61.9751727783543),
                Point2::new(-half_width, upper),
            ).unwrap(),
            TrimmedCurve2::line(Point2::new(-half_width, upper), Point2::new(-half_width, lower)).unwrap(),
            TrimmedCurve2::arc_through(
                Point2::new(-half_width, lower),
                Point2::new(8.113169712714104e-16, -64.45433555265856),
                Point2::new(half_width, lower),
            ).unwrap(),
            TrimmedCurve2::line(Point2::new(half_width, lower), Point2::new(half_width, upper)).unwrap(),
        ], tolerance).unwrap(),
        [],
        tolerance,
    ).unwrap();
    let limit = body.faces().find(|face| {
        matches!(face.surface(), Surface3::Plane(plane) if (plane.origin().y - 39.0).abs() < tolerance.linear)
    }).unwrap().clone();
    let tool = profile.extrude_up_to_face(
        Plane3::new(Point3::origin(), Vec3::unit_x(), Vec3::unit_z(), tolerance).unwrap(),
        &body, &limit, 0.0, 0.0,
    ).unwrap();
    body.difference(&tool).unwrap()
}

fn cut_shell_after_lower_corner_fillets() -> super::Compound {
    let body = cut_shell_before_lower_corner_fillets();
    let tolerance = ModelTolerance::default();
    let corners = body.edges().filter(|edge| {
        matches!(edge.curve(), Curve3::Line(_))
            && edge.endpoints().unwrap().iter().all(|point| point.y.abs() < tolerance.linear)
    }).cloned().collect::<Vec<_>>();
    assert_eq!(corners.len(), 4);
    body.fillet(&corners, 6.0).unwrap()
}

fn upper_cut_shell_rims(body: &super::Compound) -> Vec<super::EdgeHandle> {
    body.edges().filter(|edge| {
        let points = edge.endpoints().unwrap();
        points.iter().all(|point| (point.y - 39.0).abs() < 1e-7)
            && edge.connected_faces().unwrap().iter().any(|face| {
                matches!(face.surface(), Surface3::Torus(torus) if torus.minor_radius() > 17.0)
            })
    }).cloned().collect()
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

fn check_replace_face_boolean_contact(
    upper: &super::Compound, lower: &super::Compound,
    top: &super::FaceHandle, underside: &super::FaceHandle,
) {
    let bodies = upper.combined(&[lower.clone()]).unwrap();
    let replaced = bodies.replace_face(top, underside).unwrap();
    assert_eq!(replaced.solids().len(), 2);
    let lower_result = replaced.solids().find(|solid| solid.persistent_id() != upper.solids().next().unwrap().persistent_id())
        .unwrap().to_compound();
    assert!((lower_result.volume() - lower.volume()).abs() > 1.0e-3);
    let motion = RigidTransform3::new(
        Matrix3::from_angle_z(cgmath::Rad(0.37)) * Matrix3::from_angle_y(cgmath::Rad(-0.21)),
        Vec3::new(3.0, -7.0, 2.0), ModelTolerance::default(),
    ).unwrap();
    for (upper, lower) in [(upper.clone(), lower_result.clone()),
        (upper.transformed(motion).unwrap(), lower_result.transformed(motion).unwrap())] {
        for (first, second) in [(&upper, &lower), (&lower, &upper)] {
            // Different exact trim partitions use independent surface quadrature.
            let volume_tolerance = (first.volume() + second.volume()) * 1.0e-5;
            let joined = first.union(second).unwrap_or_else(|e| panic!("join (first volume {}): {e}", first.volume()));
            assert_eq!(joined.solids().len(), 1);
            assert!((joined.volume() - first.volume() - second.volume()).abs() < volume_tolerance,
                "join volume {} != {} + {}", joined.volume(), first.volume(), second.volume());
            joined.tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap()).unwrap();
            let difference = first.difference(second).unwrap();
            assert_eq!(difference.solids().len(), 1);
            assert!((difference.volume() - first.volume()).abs() < volume_tolerance,
                "difference volume {} != {}", difference.volume(), first.volume());
            assert_eq!(first.intersection(second).unwrap().solids().len(), 0);
        }
    }
}

#[test]
fn brep_point_classification_and_surface_properties() {
    let body = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0))
        .shell(&[], -2.0).unwrap();
    let topology = body.topology();
    let shells = topology.bodies().next().unwrap().1.shells();
    let outer = super::brep_classification::BrepClassifier::new(topology, shells[0], body.tolerance()).unwrap();
    let cavity = super::brep_classification::BrepClassifier::new(topology, shells[1], body.tolerance()).unwrap();
    for x in [-1.0, 1.0, 5.0, 9.0, 11.0] {
        for y in [-1.0, 1.0, 5.0, 9.0, 11.0] {
            for z in [-1.0, 1.0, 5.0, 9.0, 11.0] {
                let point = Point3::new(x, y, z);
                assert_eq!(outer.contains(point).unwrap(), [x,y,z].iter().all(|c| *c >= 0.0 && *c <= 10.0));
                assert_eq!(cavity.contains(point).unwrap(), [x,y,z].iter().all(|c| *c >= 2.0 && *c <= 8.0));
            }
        }
    }
    assert!(outer.contains(Point3::new(0.0, 5.0, 5.0)).unwrap());
    let mut area = 0.0;
    for face in body.faces() {
        let properties = face.surface_properties().unwrap();
        let expected = if shells[0] == topology.face(face.topology_id()).unwrap().shell() {100.0} else {36.0};
        assert!((properties.area - expected).abs() < 1e-7);
        assert!(face.surface().project(properties.centroid, body.tolerance()).unwrap().distance < 1e-7);
        area += properties.area;
    }
    assert!((area - body.surface_area()).abs() < 1e-7);
}

#[test]
fn brep_compound_classification_respects_solids_and_cavities() {
    use super::brep_classification::BrepClassifier;
    let first = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let second = owned_cuboid(Point3::new(5.0, 0.0, 0.0), Point3::new(15.0, 10.0, 10.0));
    let combined = first.combined(&[second.clone()]).unwrap();
    let classifier = BrepClassifier::for_compound(&combined).unwrap();
    for x in [1.0, 7.0, 13.0] {
        assert!(classifier.contains(Point3::new(x, 5.0, 5.0)).unwrap());
    }
    assert!(!classifier.contains(Point3::new(16.0, 5.0, 5.0)).unwrap());
    let subset = combined.solids().next().unwrap().to_compound();
    assert!(!BrepClassifier::for_compound(&subset).unwrap()
        .contains(Point3::new(13.0, 5.0, 5.0)).unwrap());
    let hollow = first.shell(&[], -2.0).unwrap();
    let classifier = BrepClassifier::for_compound(&hollow).unwrap();
    assert!(classifier.contains(Point3::new(1.0, 5.0, 5.0)).unwrap());
    assert!(!classifier.contains(Point3::new(3.0, 5.0, 5.0)).unwrap());
    let filled = hollow.combined(&[second]).unwrap();
    let classifier = BrepClassifier::for_compound(&filled).unwrap();
    assert!(!classifier.contains(Point3::new(3.0, 5.0, 5.0)).unwrap());
    assert!(classifier.contains(Point3::new(7.0, 5.0, 5.0)).unwrap());
    let profile = Profile::new(Wire2::new([
        TrimmedCurve2::circle(Point2::origin(), 10.0).unwrap()], first.tolerance()).unwrap(),
        [], first.tolerance()).unwrap();
    let cylinder = profile.extrude(Plane3::xy(), Vec3::unit_z() * 20.0).unwrap();
    let classifier = BrepClassifier::for_compound(&cylinder).unwrap();
    for (x, inside) in [(0.0, true), (9.95, true), (10.05, false), (20.0, false)] {
        assert_eq!(classifier.contains(Point3::new(x, 0.0, 10.0)).unwrap(), inside);
    }
}

#[test]
fn brep_classifies_torus_without_a_mesh() {
    let tolerance = ModelTolerance::default();
    let mut kernel = Kernel::new(tolerance).unwrap();
    let mut operation = kernel.begin_operation();
    let body = operation.torus(Point3::new(1.0, 2.0, 3.0),
        Vec3::unit_x() * 4.0, Vec3::unit_y() * 4.0, 1.25).unwrap();
    operation.commit().unwrap();
    let shell = kernel.topology().body(body).unwrap().shells()[0];
    let classifier = super::brep_classification::BrepClassifier::new(kernel.topology(), shell, tolerance).unwrap();
    for (point, expected) in [
        (Point3::new(5.0, 2.0, 3.0), true),
        (Point3::new(5.0, 2.0, 4.0), true),
        (Point3::new(1.0, 2.0, 3.0), false),
        (Point3::new(5.0, 2.0, 4.5), false),
        (Point3::new(7.0, 2.0, 3.0), false),
    ] {
        assert_eq!(classifier.contains(point).unwrap(), expected, "{point:?}");
    }
    let mut area = 0.0;
    let mut moment = Vec3::zero();
    for face in kernel.topology().shell(shell).unwrap().faces() {
        let properties = super::physical::face_surface_properties(kernel.topology(), *face, tolerance).unwrap();
        area += properties.area;
        moment += properties.centroid.to_vec() * properties.area;
    }
    assert!((area - 4.0 * std::f64::consts::PI.powi(2) * 4.0 * 1.25).abs() < 1e-6);
    assert!((moment / area - Vec3::new(1.0, 2.0, 3.0)).magnitude() < 1e-7);
}

#[test]
fn curved_face_properties_and_cavity_ownership() {
    let tolerance = ModelTolerance::default();
    let profile = Profile::new(Wire2::new([
        TrimmedCurve2::circle(Point2::origin(), 5.0).unwrap()], tolerance).unwrap(),
        [], tolerance).unwrap();
    let body = profile.extrude(Plane3::xy(), Vec3::unit_z() * 10.0).unwrap();
    for face in body.faces() {
        let properties = face.surface_properties().unwrap();
        let (area, z) = if matches!(face.surface(), Surface3::Cylinder(_)) {
            (100.0 * std::f64::consts::PI, 5.0)
        } else {
            (25.0 * std::f64::consts::PI, if face.centroid().unwrap().z > 5.0 {10.0} else {0.0})
        };
        assert!((properties.area - area).abs() < 1e-6);
        assert!(properties.centroid.distance(Point3::new(0.0, 0.0, z)) < 1e-7);
    }
    let hollow = body.shell(&[], -1.0).unwrap();
    let inner = hollow.faces().filter(|face| hollow.topology().face(face.topology_id()).unwrap().shell()
        == hollow.topology().bodies().next().unwrap().1.shells()[1]).cloned().collect::<Vec<_>>();
    // A cavity must retain its parent even when a single operation translates
    // it completely outside without crossing any boundary at the final position.
    assert!(hollow.move_faces(&inner, RigidTransform3::new(Matrix3::identity(),
        Vec3::unit_x() * 20.0, tolerance).unwrap()).is_err());
}

fn compound_tilt_boss_document_30(z_angle: f64) -> super::modeling::Compound {
    let tolerance = ModelTolerance::default();
    let plane = |height| Plane3::new(Point3::new(0.0, height, 0.0),
        Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
    let cylinder = |x, y, radius, base, height| Profile::new(Wire2::new([
        TrimmedCurve2::circle(Point2::new(x, y), radius).unwrap()
    ], tolerance).unwrap(), [], tolerance).unwrap()
        .extrude(plane(base), Vec3::unit_y() * height).unwrap();
    let body = cylinder(0.0, 0.0, 34.48516397016436, 0.0, 19.0)
        .union(&cylinder(-17.48305641426953, -3.255357738829639, 8.840646636818086, 19.0, 15.0)).unwrap()
        .difference(&cylinder(18.923801555564232, 2.1259479110724135, 8.639117031005743, 19.0, -4.0)).unwrap();
    let cap = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if (p.origin().y - 34.0).abs() < 1e-7)).unwrap().clone();
    let center = Vec3::new(-17.48305641426953, 34.0, 3.255357738829639);
    let rotation = Matrix3::from_angle_x(cgmath::Deg(-6.605))
        * Matrix3::from_angle_y(cgmath::Deg(-4.955))
        * Matrix3::from_angle_z(cgmath::Deg(z_angle));
    body.move_faces(&[cap], RigidTransform3::new(rotation, center - rotation * center, tolerance).unwrap()).unwrap()
}

fn partial_chamfer_fixture() -> super::Compound {
    let body = owned_cuboid(
        Point3::new(-39.34898342911119, 0.0, -24.348125762908833),
        Point3::new(50.430018992481585, 26.0, 18.09148663497865),
    );
    let corner = body
        .edges()
        .find(|e| {
            e.endpoints().unwrap().iter().all(|p| {
                (p.x - 50.430018992481585).abs() < 1e-7 && (p.z - 18.09148663497865).abs() < 1e-7
            })
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[corner], 22.0).unwrap();
    let rims = body
        .edges()
        .filter(|e| {
            e.endpoints()
                .unwrap()
                .iter()
                .all(|p| (p.y - 26.0).abs() < 1e-7)
                && (matches!(e.curve(), Curve3::Circle(_))
                    || e.endpoints()
                        .unwrap()
                        .iter()
                        .all(|p| (p.z - 18.09148663497865).abs() < 1e-7))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(rims.len(), 2);
    let body = body
        .chamfer_with_tangent_propagation(&rims, 6.0, false)
        .unwrap();
    body
}

fn rounded_chamfered_torus(other_end: bool) -> super::Compound {
    let tol = ModelTolerance::default();
    let top = Point2::new(-1.9799571066866424, 21.848456911735045);
    let bottom = Point2::new(-1.9799571066866424, -21.84845691173505);
    let wire = Wire2::new(
        [
            TrimmedCurve2::arc_through(
                top,
                Point2::new(-28.888150160955835 - 34.66130295978159, 0.0),
                bottom,
            )
            .unwrap(),
            TrimmedCurve2::line(bottom, top).unwrap(),
        ],
        tol,
    )
    .unwrap();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        -Vec3::unit_z(),
        Vec3::unit_y(),
        tol,
    )
    .unwrap();
    let axis = Axis3::new(Point3::new(0.0, 0.0, -12.045831777920995), -Vec3::unit_y()).unwrap();
    let body = Profile::new(wire, [], tol)
        .unwrap()
        .revolve(plane, axis, std::f64::consts::PI)
        .unwrap();
    let edges = body
        .edges()
        .filter(|e| {
            e.connected_faces()
                .unwrap()
                .iter()
                .all(|f| !matches!(f.surface(), Surface3::Plane(_)))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 2);
    let rounded = body.fillet(&edges, 12.0).unwrap();
    let rim = rounded
        .edges()
        .find(|e| {
            matches!(e.curve(), Curve3::Circle(c) if !other_end || c.center().z < -12.045831777920995)
                && e.connected_faces()
                    .unwrap()
                    .iter()
                    .any(|f| matches!(f.surface(), Surface3::Plane(_)))
        })
        .unwrap()
        .clone();
    rounded
        .chamfer_with_tangent_propagation(&[rim], 5.0, true)
        .unwrap()
}

fn chamfered_slot_document_51() -> super::Compound {
    let tol = ModelTolerance::default();
    let plane = Plane3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tol).unwrap();
    let cylinder = Profile::new(Wire2::new([
        TrimmedCurve2::circle(Point2::new(-10.870896995734686, 3.449392582602713), 34.82132964454673).unwrap(),
    ], tol).unwrap(), [], tol).unwrap().extrude(plane, Vec3::new(0.0, 50.0, 0.0)).unwrap();
    let rim = cylinder.edges().find(|edge| matches!(edge.curve(), Curve3::Circle(c) if c.center().y > 49.0)).unwrap().clone();
    let rounded = cylinder.fillet(&[rim], 9.0).unwrap();
    let left = -8.257553330349758;
    let right = 45.67580613259616;
    let center = 8.675830244458682;
    let radius = 7.3476627361900695;
    let a = Point2::new(left, center + radius);
    let b = Point2::new(right, center + radius);
    let c = Point2::new(right, center - radius);
    let d = Point2::new(left, center - radius);
    let slot = Profile::new(Wire2::new([
        TrimmedCurve2::line(a, b).unwrap(),
        TrimmedCurve2::arc_through(b, Point2::new(right + radius, center), c).unwrap(),
        TrimmedCurve2::line(c, d).unwrap(),
        TrimmedCurve2::arc_through(d, Point2::new(left - radius, center), a).unwrap(),
    ], tol).unwrap(), [], tol).unwrap();
    let top = Plane3::new(Point3::new(0.0, 50.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tol).unwrap();
    let pocket = rounded.difference(&slot.extrude(top, Vec3::new(0.0, -30.0, 0.0)).unwrap()).unwrap();
    let rim = pocket.edges().find(|edge| {
        let faces = edge.connected_faces().unwrap();
        faces.iter().any(|f| matches!(f.surface(), Surface3::Plane(p) if (p.origin().y - 20.0).abs() < tol.linear))
            && faces.iter().any(|f| matches!(f.surface(), Surface3::Cylinder(c) if c.radius() > 34.0))
    }).unwrap().clone();
    pocket.chamfer_with_tangent_propagation(&[rim], 5.0, true).unwrap()
}

fn filleted_chamfered_slot_document_51() -> super::Compound {
    let tol = ModelTolerance::default();
    let body = chamfered_slot_document_51();
    let lip = body.edges().find(|edge| {
        let faces = edge.connected_faces().unwrap();
        faces.iter().any(|face| matches!(face.surface(), Surface3::Torus(_)))
            && faces.iter().any(|face| matches!(face.surface(), Surface3::Plane(p) if (p.origin().z + 16.02349298064875).abs() < tol.linear))
    }).unwrap().clone();
    body.fillet_with_options(&[lip], FilletOptions::new(2.0)).unwrap()
}

#[test]
#[ignore = "release-mode shell benchmark; run with --ignored --nocapture --test-threads=1"]
fn benchmark_shell_document_51() {
    let body = filleted_chamfered_slot_document_51();
    let volume = body.volume();
    for join in [ShellJoin::Sharp, ShellJoin::Rounded] {
        for thickness in [2.0, 10.0, 12.0] {
            let mut samples = Vec::new();
            for iteration in 0..4 {
                let start = std::time::Instant::now();
                let shell = body.shell_with_join(&[], -thickness, join).unwrap();
                let elapsed = start.elapsed().as_secs_f64();
                assert_eq!(shell.solids().len(), 1);
                assert!(shell.volume() > 0.0 && shell.volume() < volume);
                if iteration > 0 { samples.push(elapsed); }
            }
            samples.sort_by(f64::total_cmp);
            println!("{join:?} {thickness} mm: median {:.6}s, samples {samples:?}", samples[1]);
        }
    }
}

fn circular_cut_on_offset_rounded_shell() -> (super::Compound, super::EdgeHandle) {
    let tol = ModelTolerance::default();
    let circle = |center, radius| {
        Profile::new(
            Wire2::new([TrimmedCurve2::circle(center, radius).unwrap()], tol).unwrap(),
            [],
            tol,
        )
        .unwrap()
    };
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        -Vec3::unit_z(),
        tol,
    )
    .unwrap();
    let body = circle(Point2::new(0.0, 0.0), 39.239171029721085)
        .extrude(plane, Vec3::unit_y() * 36.0)
        .unwrap();
    let rim = body
        .edges()
        .find(|edge| edge.endpoints().unwrap().iter().all(|p| p.y > 35.9))
        .unwrap()
        .clone();
    let body = body.fillet(&[rim], 10.0).unwrap();
    let side = body
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Cylinder(_)))
        .unwrap()
        .clone();
    let body = body.offset_faces(&[side], 2.0).unwrap();
    let opening = body
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(p) if p.origin().y.abs() < 1e-7))
        .unwrap()
        .clone();
    let body = body.shell(&[opening], -4.0).unwrap();
    let tool = circle(
        Point2::new(-57.10808714585003, -13.339295246420402),
        46.57594303983382,
    )
    .extrude(plane, Vec3::unit_y() * 33.0)
    .unwrap();
    let cut = body.difference(&tool).unwrap();
    let rim = cut
        .edges()
        .find(|edge| {
            let faces = edge.connected_faces().unwrap();
            faces.iter().any(|face| matches!(face.surface(), Surface3::Torus(_)))
                && faces.iter().any(|face| {
                    matches!(face.surface(), Surface3::Plane(p) if (p.origin().y - 33.0).abs() < 1e-7)
                })
        })
        .unwrap()
        .clone();
    (cut, rim)
}

#[test]
fn unifies_subdivided_straight_edges_but_preserves_box_corners() {
    let source = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let patches = source
        .faces()
        .map(|face| {
            let mut patch =
                super::features::face_patch(source.topology(), face.topology_id(), false).unwrap();
            for loop_ in &mut patch.loops {
                loop_.segments = loop_
                    .segments
                    .iter()
                    .flat_map(|segment| {
                        (0..3)
                            .map(|index| {
                                let start =
                                    segment.range.start + segment.range.span() * index as f64 / 3.0;
                                let end = segment.range.start
                                    + segment.range.span() * (index + 1) as f64 / 3.0;
                                let a = segment.curve.evaluate(start).unwrap().point;
                                let b = segment.curve.evaluate(end).unwrap().point;
                                let uv_a = segment.pcurve.evaluate(start).unwrap().point;
                                let uv_b = segment.pcurve.evaluate(end).unwrap().point;
                                // Each span uses its own shifted, reversed and scaled chart.
                                let range = ParameterRange::new(7.0, -3.0).unwrap();
                                let direction = (b - a) / range.span();
                                let uv_direction = (uv_b - uv_a) / range.span();
                                super::sew::BrepPatchSegment {
                                    provenance: Default::default(),
                                    curve: Line3::new(a - direction * range.start, direction)
                                        .unwrap()
                                        .into(),
                                    pcurve: super::Line2::new(
                                        uv_a - uv_direction * range.start,
                                        uv_direction,
                                    )
                                    .unwrap()
                                    .into(),
                                    range,
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
            }
            patch
        })
        .collect::<Vec<_>>();
    let mut kernel = Kernel::new(source.tolerance()).unwrap();
    let mut transaction = kernel.begin_operation();
    transaction.brep_shell(&patches).unwrap();
    assert_eq!(transaction.working.edges().count(), 36);
    transaction.unify_same_domain_edges().unwrap();
    assert_eq!(transaction.working.edges().count(), 12);
    assert_eq!(transaction.working.vertices().count(), 8);
    assert_eq!(transaction.working.faces().count(), 6);
    transaction.unify_same_domain_edges().unwrap();
    assert_eq!(transaction.working.edges().count(), 12);
    transaction.finish().unwrap();
}

#[test]
fn unifies_periodic_edges_with_bounded_projection_pcurves() {
    let tol = ModelTolerance::default();
    let source = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 10.0).unwrap()],
            tol,
        )
        .unwrap(),
        [],
        tol,
    )
    .unwrap()
    .extrude(Plane3::xy(), Vec3::unit_z() * 5.0)
    .unwrap();
    let mut patches = source
        .faces()
        .map(|face| {
            super::features::face_patch(source.topology(), face.topology_id(), false).unwrap()
        })
        .collect::<Vec<_>>();
    for patch in &mut patches {
        for loop_ in &mut patch.loops {
            let mut segments = Vec::new();
            for segment in &loop_.segments {
                for index in 0..3 {
                    let range = ParameterRange::new(
                        segment.range.start + segment.range.span() * index as f64 / 3.0,
                        segment.range.start + segment.range.span() * (index + 1) as f64 / 3.0,
                    )
                    .unwrap();
                    segments.push(super::sew::BrepPatchSegment {
                        provenance: Default::default(),
                        curve: segment.curve.clone(),
                        pcurve: super::SurfaceProjectionPcurve2::new(
                            segment.curve.clone(),
                            patch.surface.clone(),
                            segment.pcurve.clone(),
                            range,
                            tol,
                        )
                        .unwrap()
                        .into(),
                        range,
                    });
                }
            }
            loop_.segments = segments;
        }
    }
    let mut baseline_kernel = Kernel::new(tol).unwrap();
    let mut baseline_transaction = baseline_kernel.begin_operation();
    baseline_transaction.brep_shell(&patches).unwrap();
    let baseline_committed = baseline_transaction.finish().unwrap();
    let baseline =
        super::Compound::from_operation(baseline_kernel, baseline_committed, []).unwrap();
    baseline
        .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
        .expect("before unification");
    let mut kernel = Kernel::new(tol).unwrap();
    let mut transaction = kernel.begin_operation();
    transaction.brep_shell(&patches).unwrap();
    transaction.unify_same_domain_edges().unwrap();
    let circles = transaction
        .working
        .edges()
        .filter(|(_, edge)| matches!(edge.curve, Curve3::Circle(_)))
        .collect::<Vec<_>>();
    assert_eq!(circles.len(), 2);
    assert!(circles
        .iter()
        .all(|(_, edge)| edge.vertices[0] == edge.vertices[1]));
    let committed = transaction.finish().unwrap();
    let result = super::Compound::from_operation(kernel, committed, []).unwrap();
    assert!((result.volume() - source.volume()).abs() < 1e-6);
    result
        .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
        .unwrap();
}

#[test]
fn surface_projection_refines_within_model_tolerance() {
    use super::{Line2, Line3, SurfaceProjectionPcurve2};
    use crate::Vec2;

    let tolerance = ModelTolerance::default();
    let range = ParameterRange::new(0.0, 1.0).unwrap();
    for normal_offset in [0.0, tolerance.linear * 0.5] {
        let curve = Line3::new(Point3::new(0.0, 0.0, normal_offset), Vec3::unit_x()).unwrap();
        // The guide is already within tolerance, but has tangential error.
        let guide = Line2::new(
            Point2::new(0.0, tolerance.linear * 0.5),
            Vec2::new(1.0, 0.0),
        )
        .unwrap();
        let projected = SurfaceProjectionPcurve2::new(
            curve.into(),
            Plane3::xy().into(),
            guide.into(),
            range,
            tolerance,
        )
        .unwrap();
        let projected = Curve2::from(projected);
        for parameter in [0.0, 0.4, 1.0] {
            let uv = projected.evaluate(parameter).unwrap().point;
            assert!(uv.distance(Point2::new(parameter, 0.0)) < 1e-12);
        }
    }
}

// Two spline sides joined by lines, meeting them at obtuse and acute corners.
fn spline_prism() -> super::Compound {
    let tolerance = ModelTolerance::default();
    let bezier = |points: &[(f64, f64)]| {
        let degree = points.len() - 1;
        let knots = [vec![0.0; degree + 1], vec![1.0; degree + 1]].concat();
        TrimmedCurve2::new(
            NurbsCurve2::new(
                degree,
                points.iter().map(|(x, y)| Point2::new(*x, *y)).collect(),
                vec![1.0; points.len()],
                knots,
            )
            .unwrap(),
            ParameterRange::new(0.0, 1.0).unwrap(),
        )
        .unwrap()
    };
    let a = Point2::new(21.075321066969174, -53.50849373430962);
    let p = Point2::new(-3.0051181743448665, 24.680402354050287);
    let q = Point2::new(29.778056848718805, 44.963876532934194);
    let r = Point2::new(69.96151398539615, -60.03632364917461);
    let wire = Wire2::new(
        [
            TrimmedCurve2::line(p, q).unwrap(),
            bezier(&[
                (q.x, q.y),
                (31.374786044094865, -9.119250315072938),
                (86.64374599030397, -43.0639658712998),
                (r.x, r.y),
            ]),
            TrimmedCurve2::line(r, a).unwrap(),
            bezier(&[
                (a.x, a.y),
                (19.044440649207882, -13.326074037749922),
                (-13.157746049310175, -14.295445894327267),
                (-12.597521875747521, -0.07939109751598394),
                (p.x, p.y),
            ]),
        ],
        tolerance,
    )
    .unwrap();
    Profile::new(wire, [], tolerance)
        .unwrap()
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 22.5))
        .unwrap()
}

fn spline_prism_cap_curves(body: &super::Compound) -> Vec<super::EdgeHandle> {
    body.edges()
        .filter(|edge| {
            let p = edge.endpoints().unwrap();
            (p[0].z - p[1].z).abs() < 1e-9 && !matches!(edge.curve(), Curve3::Line(_))
        })
        .cloned()
        .collect()
}

// Median wall time of `run` over five samples after one warm-up.
fn bench_median(run: &mut dyn FnMut()) -> f64 {
    run();
    let mut samples = (0..5).map(|_| {
        let start = std::time::Instant::now();
        run();
        start.elapsed().as_secs_f64()
    }).collect::<Vec<_>>();
    samples.sort_by(f64::total_cmp);
    samples[2]
}

#[test]
#[ignore = "release-mode streamlining benchmark; run with --ignored --nocapture --test-threads=1"]
fn benchmark_streamlining() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(Point3::origin(), Point3::new(40.0, 30.0, 20.0));
    let mut plate = owned_cuboid(Point3::origin(), Point3::new(66.0, 66.0, 3.0));
    let tools = (0..100).map(|index| {
        let center = Point2::new(6.0 + (index % 10) as f64 * 6.0, 6.0 + (index / 10) as f64 * 6.0);
        Profile::new(Wire2::new([TrimmedCurve2::circle(center, 1.5).unwrap()], tolerance).unwrap(), [], tolerance)
            .unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 3.0).unwrap()
    }).collect::<Vec<_>>();
    let drill = bench_median(&mut || {
        let mut body = owned_cuboid(Point3::origin(), Point3::new(66.0, 66.0, 3.0));
        for tool in &tools[..20] { body = body.difference(tool).unwrap(); }
    });
    println!("drill 20 holes: {drill:.6}s");
    for tool in &tools { plate = plate.difference(tool).unwrap(); }
    let outer_edge = |body: &super::Compound| body.edges().find(|edge| {
        let [a, b] = edge.endpoints().unwrap();
        matches!(edge.curve(), Curve3::Line(_)) && a.x.abs() < 1e-9 && b.x.abs() < 1e-9 && a.y.abs() < 1e-9 && b.y.abs() < 1e-9
    }).unwrap().clone();
    let edge = outer_edge(&block);
    println!("block fillet 1 edge: {:.6}s", bench_median(&mut || { block.fillet(&[edge.clone()], 2.0).unwrap(); }));
    let edge = outer_edge(&plate);
    println!("plate ({} faces) fillet 1 edge: {:.6}s", plate.faces().count(),
        bench_median(&mut || { plate.fillet(&[edge.clone()], 1.0).unwrap(); }));
    let rim = plate.edges().find(|edge| matches!(edge.curve(), Curve3::Circle(c) if c.center().z > 2.9)).unwrap().clone();
    println!("plate fillet 1 hole rim: {:.6}s", bench_median(&mut || { plate.fillet(&[rim.clone()], 0.5).unwrap(); }));
    println!("plate validate_structure: {:.6}s", bench_median(&mut || { assert!(super::validation::validate_structure(plate.topology(), tolerance, None).is_valid()); }));
    println!("plate certify: {:.6}s", bench_median(&mut || {
        super::physical::certify_topology(plate.topology(), tolerance).unwrap();
    }));
    let mut kernel = Kernel::new(tolerance).unwrap();
    let mut transaction = kernel.begin_operation();
    transaction.cylinder(Point3::origin(), Vec3::unit_x() * 2.0, Vec3::unit_y() * 2.0, Vec3::unit_z() * 5.0).unwrap();
    transaction.sphere(Point3::new(10.0, 0.0, 0.0), Vec3::unit_x() * 3.0, Vec3::unit_y() * 3.0, Vec3::unit_z() * 3.0).unwrap();
    transaction.torus(Point3::new(20.0, 0.0, 0.0), Vec3::unit_x() * 4.0, Vec3::unit_y() * 4.0, 1.25).unwrap();
    transaction.commit().unwrap();
    for (name, topology) in [("box", block.topology()), ("cylinder+sphere+torus", kernel.topology())] {
        println!("certify {name}: {:.6}s", bench_median(&mut || {
            super::physical::certify_topology(topology, tolerance).unwrap();
        }));
    }
}
