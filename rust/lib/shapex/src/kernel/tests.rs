use cgmath::prelude::*;

use crate::{Matrix3, Point2, Point3, Vec3};

use super::{
    Axis3, BodyId, BooleanKind, Circle3, Curve2, Curve3, FilletOptions, IntersectionSet, Kernel,
    Line3, MassProperties, ModelTolerance, NurbsCurve2, NurbsCurve3, NurbsSurface3, ParameterRange,
    Plane3, PolyhedralMesh, Profile, Resolution, RevolvedSurface3, RigidTransform3, Sphere3,
    Surface3, TessellationOptions, TrimmedCurve2, Wire2,
};

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
fn rolling_ball_fillet_rewrites_only_local_brep() {
    let cuboid = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let edge = cuboid
        .edges()
        .find(|edge| {
            let endpoints = edge.endpoints().expect("edge endpoints");
            (endpoints[1].z - endpoints[0].z).abs() > 4.9
        })
        .expect("vertical edge")
        .clone();
    let filleted = cuboid
        .check_edge_blends(&[edge], 1.0)
        .expect("rolling-ball fillet");
    assert_eq!(filleted.faces().count(), cuboid.faces().count() + 1);
    assert!(filleted
        .faces()
        .any(|face| matches!(face.surface(), Surface3::Cylinder(cylinder) if (cylinder.radius() - 1.0).abs() < 1.0e-10)));
    let end_edge = filleted
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
        .find(|edge| {
            let endpoints = edge.endpoints().expect("fillet edge endpoints");
            endpoints[0].distance(endpoints[1]) < 2.0
        })
        .expect("fillet end edge")
        .clone();
    let end_points = end_edge.endpoints().expect("fillet end points");
    let arc_result = filleted.check_edge_blends(&[end_edge.clone()], 0.5);
    assert!(
        arc_result.is_ok(),
        "fillet generated end edge: {arc_result:?}"
    );
    assert_eq!(
        arc_result.unwrap().faces().count(),
        filleted.faces().count() + 3,
        "default tangent propagation follows both straight continuations",
    );
    let tangent_edge = filleted
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
        .find(|edge| {
            let endpoints = edge.endpoints().expect("line endpoints");
            endpoints
                .iter()
                .any(|point| point.distance(end_points[0]) < 1.0e-7)
        })
        .expect("straight edge tangent to the fillet end")
        .clone();
    let second = filleted
        .check_edge_blends_with_options(
            &[tangent_edge],
            super::FilletOptions::new(0.25).with_tangent_propagation(false),
        )
        .expect("fillet a straight edge leading into a previous fillet");
    assert_eq!(second.faces().count(), filleted.faces().count() + 1);
}

#[test]
fn rolling_ball_fillet_chain_certifies_at_application_scale() {
    let cuboid = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(50.0, 25.0, 8.0));
    let edge = cuboid
        .edges()
        .find(|edge| {
            let endpoints = edge.endpoints().expect("edge endpoints");
            (endpoints[1].z - endpoints[0].z).abs() > 7.9
        })
        .expect("vertical edge")
        .clone();
    let first = cuboid
        .check_edge_blends(&[edge], 2.0)
        .expect("first fillet");
    let end_edge = first
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
        .find(|edge| {
            let endpoints = edge.endpoints().expect("fillet edge endpoints");
            endpoints[0].distance(endpoints[1]) < 4.0
        })
        .expect("fillet end edge")
        .clone();
    let end_points = end_edge.endpoints().expect("fillet end points");
    let tangent_edges = first
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
        .filter(|edge| {
            let endpoints = edge.endpoints().expect("line endpoints");
            endpoints
                .iter()
                .any(|point| end_points.iter().any(|end| point.distance(*end) < 1.0e-7))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        !tangent_edges.is_empty(),
        "straight edges tangent to first fillet"
    );
    for (index, tangent_edge) in tangent_edges.into_iter().enumerate() {
        let second = first
            .check_edge_blends(&[tangent_edge], 1.0)
            .expect("tangent continuation fillet");
        assert!(second.faces().count() > first.faces().count());
        second
            .tessellate(
                TessellationOptions::new(0.1, 0.1, 24).expect("application tessellation options"),
            )
            .expect("tangent continuation display tessellation");
        if index == 0 {
            let shelled = second.shell(&[], -0.1).expect("twice-filleted shell");
            assert_eq!(shelled.solids().len(), 1);
        }
    }
}

#[test]
fn rolling_ball_fillet_supports_three_consecutive_perimeter_operations() {
    let cuboid = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(50.0, 25.0, 8.0));
    let top_edge_near = |solid: &super::Compound, target: Point3, direction: Vec3| {
        solid
            .edges()
            .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
            .filter_map(|edge| {
                let endpoints = edge.endpoints().ok()?;
                let midpoint =
                    Point3::from_vec((endpoints[0].to_vec() + endpoints[1].to_vec()) * 0.5);
                let tangent = (endpoints[1] - endpoints[0]).normalize();
                ((midpoint.z - 8.0).abs() < 1.0e-7 && tangent.dot(direction).abs() > 1.0 - 1.0e-10)
                    .then_some((midpoint.distance(target), edge.clone()))
            })
            .min_by(|first, second| first.0.total_cmp(&second.0))
            .map(|(_, edge)| edge)
            .expect("matching top perimeter edge")
    };

    let first_edge = top_edge_near(&cuboid, Point3::new(25.0, 0.0, 8.0), Vec3::unit_x());
    let first = cuboid
        .check_edge_blends(&[first_edge], 2.0)
        .expect("first perimeter fillet");
    let second_edge = top_edge_near(&first, Point3::new(50.0, 12.5, 8.0), Vec3::unit_y());
    let second = first
        .check_edge_blends(&[second_edge], 1.0)
        .expect("second perimeter fillet");
    let third_edge = top_edge_near(&second, Point3::new(25.0, 25.0, 8.0), Vec3::unit_x());
    second
        .check_edge_blends(&[third_edge], 0.5)
        .expect("third perimeter fillet");
}

#[test]
fn rolling_ball_fillet_batches_disconnected_edges_transactionally() {
    let cuboid = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let edges = cuboid
        .edges()
        .filter(|edge| {
            let endpoints = edge.endpoints().expect("edge endpoints");
            (endpoints[1].z - endpoints[0].z).abs() > 4.9
        })
        .take(2)
        .cloned()
        .collect::<Vec<_>>();
    let filleted = cuboid
        .check_edge_blends(&edges, 1.0)
        .expect("batched rolling-ball fillet");
    assert_eq!(filleted.faces().count(), cuboid.faces().count() + 2);
}

#[test]
fn rolling_ball_fillet_builds_exact_trihedral_vertex_transition() {
    let cuboid = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let corner = Point3::new(0.0, 0.0, 0.0);
    let edges = cuboid
        .edges()
        .filter(|edge| {
            edge.endpoints()
                .expect("edge endpoints")
                .iter()
                .any(|point| point.distance(corner) < 1.0e-7)
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 3);
    let filleted = cuboid
        .check_edge_blends(&edges, 0.5)
        .expect("trihedral rolling-ball fillet");
    assert!(filleted
        .faces()
        .any(|face| matches!(face.surface(), Surface3::Sphere(_))));
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
fn revolved_surface_retains_its_axis_and_exact_meridian() {
    let tolerance = ModelTolerance::default();
    let frame = super::Frame3::new(
        Point3::new(1.0, 2.0, 3.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("revolution frame");
    let profile: Curve2 = super::Circle2::new(Point2::new(4.0, 0.0), 1.0)
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
        super::Circle2::new(Point2::new(4.0, 0.0), 1.0)
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
fn rolling_ball_fillet_supports_closed_hole_rims() {
    let tolerance = ModelTolerance::default();
    let outer = Wire2::polygon(
        [
            Point2::new(0.0, 0.0),
            Point2::new(60.0, 0.0),
            Point2::new(60.0, 30.0),
            Point2::new(0.0, 30.0),
        ],
        tolerance,
    )
    .expect("outer wire");
    let hole = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(10.0, 15.0), 6.0).expect("hole circle")],
        tolerance,
    )
    .expect("hole wire");
    let solid = Profile::new(outer, [hole], tolerance)
        .expect("profile with hole")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("profile extrusion");
    assert_eq!(
        solid
            .edges()
            .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
            .count(),
        2,
        "one sketch circle must produce one circular edge on each cap",
    );
    let rim = solid
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
        .find(|edge| {
            let endpoints = edge.endpoints().expect("rim endpoints");
            let faces = edge.connected_faces().expect("rim faces");
            endpoints.iter().all(|point| point.z > 19.9)
                && faces
                    .iter()
                    .any(|face| matches!(face.surface(), Surface3::Plane(_)))
                && faces
                    .iter()
                    .any(|face| matches!(face.surface(), Surface3::Cylinder(_)))
        })
        .expect("top circular hole rim")
        .clone();
    let oversized = solid
        .check_edge_blends(&[rim.clone()], 8.0)
        .expect_err("a circular-rim fillet must not overlap the body's outer boundary");
    assert!(
        oversized.to_string().contains("blend contact boundary"),
        "unexpected oversized-fillet diagnostic: {oversized}",
    );
    let filleted = solid
        .check_edge_blends(&[rim], 0.25)
        .expect("closed circular rim fillet");
    assert!(filleted.faces().count() > solid.faces().count());
    assert!(filleted
        .faces()
        .any(|face| matches!(face.surface(), Surface3::Torus(_))));
    filleted
        .tessellate(
            TessellationOptions::new(0.1, 0.1, 24).expect("application tessellation options"),
        )
        .expect("closed-rim fillet display tessellation");
}

#[test]
fn rolling_ball_fillet_orients_direct_and_post_boolean_hole_rims_consistently() {
    let tolerance = ModelTolerance::default();
    let outer = Wire2::polygon(
        [
            Point2::new(0.0, 0.0),
            Point2::new(60.0, 0.0),
            Point2::new(60.0, 30.0),
            Point2::new(0.0, 30.0),
        ],
        tolerance,
    )
    .expect("outer wire");
    let left_hole = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(15.0, 15.0), 6.0).expect("left hole")],
        tolerance,
    )
    .expect("left hole wire");
    let base = Profile::new(outer, [left_hole], tolerance)
        .expect("profile with original hole")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .expect("base extrusion");

    let right_hole = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(45.0, 15.0), 6.0).expect("right hole")],
        tolerance,
    )
    .expect("right hole wire");
    let cutter = Profile::new(right_hole, [], tolerance)
        .expect("cutter profile")
        .extrude(
            Plane3::new(
                Point3::new(0.0, 0.0, 20.0),
                Vec3::unit_x(),
                Vec3::unit_y(),
                tolerance,
            )
            .expect("cutter plane"),
            Vec3::new(0.0, 0.0, -20.0),
        )
        .expect("cutter extrusion");
    let cut = base.difference(&cutter).expect("post-extrusion hole cut");
    let rims = cut
        .edges()
        .filter(|edge| {
            let Curve3::Circle(circle) = edge.curve() else {
                return false;
            };
            (circle.center().z - 20.0).abs() <= tolerance.linear * 8.0
                && ((circle.center().x - 15.0).abs() <= tolerance.linear * 8.0
                    || (circle.center().x - 45.0).abs() <= tolerance.linear * 8.0)
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(rims.len(), 2, "both top hole rims must remain selectable");

    let filleted = cut
        .check_edge_blends(&rims, 0.25)
        .expect("fillet original and post-Boolean hole rims");
    let mut torus_faces = filleted
        .faces()
        .filter(|face| matches!(face.surface(), Surface3::Torus(_)))
        .cloned()
        .collect::<Vec<_>>();
    torus_faces.sort_by(|first, second| {
        let Surface3::Torus(first) = first.surface() else {
            unreachable!()
        };
        let Surface3::Torus(second) = second.surface() else {
            unreachable!()
        };
        first.center().x.total_cmp(&second.center().x)
    });
    assert_eq!(
        torus_faces.len(),
        2,
        "each closed hole rim must publish as one toroidal blend face"
    );
    let tessellation_options =
        TessellationOptions::new(0.1, 0.1, 24).expect("application tessellation options");
    for face in &torus_faces {
        let Surface3::Torus(torus) = face.surface() else {
            unreachable!()
        };
        assert!(
            (torus.center().z - 19.75).abs() <= tolerance.linear * 32.0,
            "hole-rim fillet center lies on the wrong side of the top face: {:?}",
            torus.center(),
        );
        assert!(
            (torus.major_radius() - 6.25).abs() <= tolerance.linear * 32.0,
            "hole-rim fillet rolls on the wrong side of the cylindrical wall: {}",
            torus.major_radius(),
        );
        assert!(
            torus.normal().z > 0.0,
            "hole-rim torus has a flipped parameter frame at {:?}: {:?}",
            torus.center(),
            torus.normal(),
        );
        let mesh = face
            .tessellate(tessellation_options)
            .expect("unified torus face tessellation");
        let longest_edge = mesh
            .triangles
            .iter()
            .flat_map(|triangle| {
                [
                    (triangle[0], triangle[1]),
                    (triangle[1], triangle[2]),
                    (triangle[2], triangle[0]),
                ]
            })
            .map(|(first, second)| mesh.positions[first].distance(mesh.positions[second]))
            .fold(0.0_f64, f64::max);
        assert!(
            longest_edge < 1.0,
            "periodic torus tessellation contains a seam-crossing wedge of length {longest_edge}"
        );
        let mut normal_orientation = None;
        for (position, normal) in mesh.positions.iter().zip(&mesh.normals) {
            let projection = face
                .surface()
                .project(*position, tolerance)
                .expect("mesh vertex projects onto its analytical torus");
            let analytical = face
                .surface()
                .normal(projection.parameters.0, projection.parameters.1)
                .expect("analytical torus normal");
            let alignment = normal.dot(analytical);
            let sign = *normal_orientation.get_or_insert(alignment.signum());
            assert!(
                alignment * sign > 1.0 - 1.0e-10,
                "toroidal vertex normal changes orientation or belongs to another position"
            );
        }
        for triangle in &mesh.triangles {
            for (first, second) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                assert!(
                    mesh.normals[first].angle(mesh.normals[second]).0 < 0.2,
                    "a torus triangle interpolates across discontinuous analytical normals"
                );
            }
        }
        let mut occupied_angular_sectors = [false; 8];
        for triangle in &mesh.triangles {
            let centroid = Point3::from_vec(
                (mesh.positions[triangle[0]].to_vec()
                    + mesh.positions[triangle[1]].to_vec()
                    + mesh.positions[triangle[2]].to_vec())
                    / 3.0,
            );
            let angle = (centroid.y - torus.center().y)
                .atan2(centroid.x - torus.center().x)
                .rem_euclid(std::f64::consts::TAU);
            occupied_angular_sectors[(angle / std::f64::consts::TAU
                * occupied_angular_sectors.len() as f64)
                as usize] = true;
        }
        assert!(
            occupied_angular_sectors
                .into_iter()
                .all(|occupied| occupied),
            "periodic torus tessellation leaves an angular wedge uncovered"
        );
    }
    filleted
        .tessellate(tessellation_options)
        .expect("unified periodic fillet tessellation");

    let unrelated_boss = owned_cuboid(Point3::new(27.0, 0.0, 20.0), Point3::new(33.0, 5.0, 22.0));
    let modified = filleted
        .union(&unrelated_boss)
        .expect("unrelated operation after the two fillets");
    assert_eq!(
        modified
            .faces()
            .filter(|face| matches!(face.surface(), Surface3::Torus(_)))
            .count(),
        2,
        "later operations must preserve one selectable face per circular fillet"
    );
}

#[test]
fn rolling_ball_fillet_certifies_curved_and_post_fillet_face_loops() {
    let tolerance = ModelTolerance::default();
    let outer = Wire2::new(
        [
            TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(25.0, 0.0))
                .expect("lower outer edge"),
            TrimmedCurve2::arc_through(
                Point2::new(25.0, 0.0),
                Point2::new(30.0, 5.0),
                Point2::new(25.0, 10.0),
            )
            .expect("rounded outer end"),
            TrimmedCurve2::line(Point2::new(25.0, 10.0), Point2::new(0.0, 10.0))
                .expect("upper outer edge"),
            TrimmedCurve2::line(Point2::new(0.0, 10.0), Point2::new(0.0, 0.0))
                .expect("left outer edge"),
        ],
        tolerance,
    )
    .expect("curved outer wire");
    let circular_hole = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(7.0, 5.0), 2.0).expect("circular hole")],
        tolerance,
    )
    .expect("circular hole wire");
    let rectangular_hole = Wire2::polygon(
        [
            Point2::new(14.0, 3.0),
            Point2::new(20.0, 3.0),
            Point2::new(20.0, 7.0),
            Point2::new(14.0, 7.0),
        ],
        tolerance,
    )
    .expect("rectangular hole wire");
    let solid = Profile::new(outer, [circular_hole, rectangular_hole], tolerance)
        .expect("curved profile with holes")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 4.0))
        .expect("curved profile extrusion");

    let top_circle = solid
        .edges()
        .filter(|edge| {
            matches!(edge.curve(), Curve3::Circle(circle) if (circle.radius() - 2.0).abs() < 1.0e-9)
        })
        .find(|edge| {
            edge.endpoints()
                .is_ok_and(|points| points.iter().all(|point| point.z > 3.9))
        })
        .expect("top circular-hole rim")
        .clone();
    solid
        .check_edge_blends(&[top_circle], 0.5)
        .expect("circular-hole fillet on a curved multi-loop face");

    let rectangular_rim_edge = solid
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
        .find(|edge| {
            edge.endpoints().is_ok_and(|points| {
                points
                    .iter()
                    .all(|point| point.z > 3.9 && point.x > 13.9 && point.x < 20.1)
            })
        })
        .expect("top rectangular-hole rim")
        .clone();
    for radius in [1.0, 2.0] {
        solid
            .check_edge_blends(std::slice::from_ref(&rectangular_rim_edge), radius)
            .expect("rectangular-hole fillet after changing its radius")
            .tessellate(
                TessellationOptions::new(0.1, 0.1, 24).expect("application tessellation options"),
            )
            .expect("changed rectangular-hole fillet display tessellation");
    }

    let upper_edge = solid
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
        .find(|edge| {
            edge.endpoints()
                .is_ok_and(|points| points.iter().all(|point| point.z > 3.9 && point.y > 9.9))
        })
        .expect("upper top perimeter edge")
        .clone();
    let first = solid
        .check_edge_blends_with_options(
            &[upper_edge],
            FilletOptions::new(1.0).with_tangent_propagation(false),
        )
        .expect("first curved-perimeter fillet");
    let slot_edge = first
        .edges()
        .find(|edge| {
            edge.endpoints().is_ok_and(|points| {
                points
                    .iter()
                    .all(|point| point.z > 3.9 && point.y > 6.9 && point.x > 13.9 && point.x < 20.1)
            })
        })
        .expect("top rectangular-slot edge")
        .clone();
    first
        .check_edge_blends_with_options(
            &[slot_edge],
            FilletOptions::new(0.5).with_tangent_propagation(false),
        )
        .expect("second fillet on an existing post-fillet face loop");
}

#[test]
fn rolling_ball_fillet_supports_concave_straight_hole_edges() {
    let tolerance = ModelTolerance::default();
    let height = 20.0;
    let outer = Wire2::polygon(
        [
            Point2::new(0.0, 0.0),
            Point2::new(12.0, 0.0),
            Point2::new(12.0, 8.0),
            Point2::new(0.0, 8.0),
        ],
        tolerance,
    )
    .expect("outer wire");
    let hole = Wire2::polygon(
        [
            Point2::new(3.0, 2.0),
            Point2::new(3.0, 6.0),
            Point2::new(7.0, 6.0),
            Point2::new(7.0, 2.0),
        ],
        tolerance,
    )
    .expect("rectangular hole");
    let solid = Profile::new(outer, [hole], tolerance)
        .expect("profile with hole")
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, height))
        .expect("profile extrusion");
    let lower_rim = solid
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
        .filter(|edge| {
            edge.endpoints().is_ok_and(|points| {
                points.iter().all(|point| {
                    point.z < 0.1
                        && point.x > 2.9
                        && point.x < 7.1
                        && point.y > 1.9
                        && point.y < 6.1
                })
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(lower_rim.len(), 4, "rectangular lower hole rim");
    let top_rim = solid
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
        .filter(|edge| {
            let endpoints = edge.endpoints().expect("edge endpoints");
            endpoints.iter().all(|point| point.z > height - 0.1)
                && endpoints.iter().all(|point| point.x > 2.9 && point.x < 7.1)
                && endpoints.iter().all(|point| point.y > 1.9 && point.y < 6.1)
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(top_rim.len(), 4, "rectangular top hole rim");
    let edge = top_rim[0].clone();
    for radius in [0.25, 0.5, 1.0] {
        let filleted = solid
            .check_edge_blends(std::slice::from_ref(&edge), radius)
            .expect("concave straight hole-edge fillet");
        for lower_edge in &lower_rim {
            assert!(
                matches!(filleted.resolve(lower_edge), Resolution::Preserved(_)),
                "top-rim propagation must not cross a vertical edge to the lower rim",
            );
        }
        for unselected in &top_rim[1..] {
            assert!(
                matches!(
                    filleted.resolve(unselected),
                    Resolution::Preserved(_) | Resolution::Modified(_)
                ),
                "an unselected rim edge may be extended at the endpoint but must not be consumed",
            );
        }
        assert_eq!(
            filleted
                .faces()
                .filter(|face| matches!(face.surface(), Surface3::Cylinder(_)))
                .count(),
            1,
            "one straight rim edge should produce one analytical cylinder",
        );
        assert_eq!(filleted.faces().count(), solid.faces().count() + 1);
        filleted
            .tessellate(
                TessellationOptions::new(0.1, 0.1, 24).expect("application tessellation options"),
            )
            .expect("concave hole-edge fillet display tessellation");
    }

    let vertical = solid
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
        .find(|edge| {
            let endpoints = edge.endpoints().expect("vertical edge endpoints");
            let minimum_z = endpoints
                .iter()
                .map(|point| point.z)
                .fold(f64::INFINITY, f64::min);
            let maximum_z = endpoints
                .iter()
                .map(|point| point.z)
                .fold(f64::NEG_INFINITY, f64::max);
            minimum_z < 0.1
                && maximum_z > height - 0.1
                && endpoints
                    .iter()
                    .all(|point| point.x > 2.9 && point.x < 7.1 && point.y > 1.9 && point.y < 6.1)
        })
        .expect("vertical rectangular-hole edge")
        .clone();
    for follow_tangent_edges in [true, false] {
        let filleted = solid
            .check_edge_blends_with_options(
                std::slice::from_ref(&vertical),
                FilletOptions::new(0.5).with_tangent_propagation(follow_tangent_edges),
            )
            .expect("isolated vertical hole-edge fillet");
        for rim in top_rim.iter().chain(&lower_rim) {
            assert!(
                matches!(
                    filleted.resolve(rim),
                    Resolution::Preserved(_) | Resolution::Modified(_)
                ),
                "a vertical edge may trim adjacent rim endpoints but must not consume their edges",
            );
        }
        assert_eq!(filleted.faces().count(), solid.faces().count() + 1);
        assert_eq!(
            filleted
                .faces()
                .filter(|face| matches!(face.surface(), Surface3::Cylinder(_)))
                .count(),
            1,
            "one vertical straight edge should produce one analytical cylinder",
        );
        let cylinder = filleted
            .faces()
            .find(|face| matches!(face.surface(), Surface3::Cylinder(_)))
            .expect("vertical fillet cylinder");
        for boundary in cylinder
            .boundary_edges()
            .expect("vertical fillet cylinder boundary")
        {
            for point in boundary.endpoints().expect("cylinder boundary endpoints") {
                assert!(
                    point.x >= 3.0 - 1.0e-7
                        && point.x <= 7.0 + 1.0e-7
                        && point.y >= 2.0 - 1.0e-7
                        && point.y <= 6.0 + 1.0e-7,
                    "concave fillet contact {point:?} must remain on the rectangular hole boundary side",
                );
            }
        }
        let expected_added_volume = (1.0 - std::f64::consts::FRAC_PI_4) * 0.5_f64.powi(2) * height;
        let expected_volume = solid.volume() + expected_added_volume;
        assert!(
            (filleted.volume() - expected_volume).abs() < 5.0e-5,
            "vertical concave fillet volume is {}, expected {expected_volume}",
            filleted.volume(),
        );
        filleted
            .tessellate(
                TessellationOptions::new(0.1, 0.1, 24).expect("application tessellation options"),
            )
            .expect("vertical hole-edge fillet display tessellation");
    }
    solid
        .check_edge_blends(std::slice::from_ref(&vertical), 5.0)
        .expect_err("a vertical fillet must not cross the body boundary");

    let top_vertex = vertical
        .endpoints()
        .expect("vertical edge endpoints")
        .into_iter()
        .max_by(|first, second| first.z.total_cmp(&second.z))
        .expect("vertical edge top vertex");
    let mut trihedral = vec![vertical];
    trihedral.extend(
        top_rim
            .iter()
            .filter(|edge| {
                edge.endpoints().is_ok_and(|points| {
                    points
                        .iter()
                        .any(|point| point.distance(top_vertex) < 1.0e-7)
                })
            })
            .cloned(),
    );
    assert_eq!(trihedral.len(), 3, "three edges at the concave top corner");
    for radius in [0.5, 1.0] {
        let setback = solid
            .check_edge_blends_with_options(
                &trihedral,
                FilletOptions::new(radius).with_tangent_propagation(false),
            )
            .expect("mixed convex-concave three-edge setback fillet");
        assert!(
            setback
                .faces()
                .filter(|face| matches!(face.surface(), Surface3::Cylinder(_)))
                .count()
                >= 3,
            "the setback must retain analytical cylinders for its straight-edge blends",
        );
        setback
            .tessellate(
                TessellationOptions::new(0.1, 0.1, 24).expect("application tessellation options"),
            )
            .expect("mixed concave-convex setback display tessellation");
    }
}

#[test]
fn rolling_ball_fillet_supports_a_boss_base_edge() {
    let block = owned_cuboid(
        Point3::new(-30.227858917765495, -24.574269345846012, 0.0),
        Point3::new(43.4949491160865, 13.794758559171267, 41.0),
    );
    let boss = owned_cuboid(
        Point3::new(-9.799555260123473, -10.252252426049688, 41.0),
        Point3::new(29.77557175191365, 5.5777983795566515, 53.0),
    );
    let joined = block.union(&boss).expect("boss union");
    let base_edge = joined
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
        .find(|edge| {
            edge.endpoints().is_ok_and(|points| {
                points.iter().all(|point| {
                    (point.y + 10.252252426049688).abs() < 1.0e-7 && (point.z - 41.0).abs() < 1.0e-7
                }) && (points[0].x - points[1].x).abs() > 39.5
            })
        })
        .expect("straight boss-base edge")
        .clone();

    let filleted = joined
        .check_edge_blends(std::slice::from_ref(&base_edge), 1.0)
        .expect("boss-base fillet");
    assert_eq!(filleted.faces().count(), joined.faces().count() + 1);
    filleted
        .tessellate(
            TessellationOptions::new(0.1, 0.1, 24).expect("application tessellation options"),
        )
        .expect("boss-base fillet display tessellation");

    let vertical = joined
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Line(_)))
        .find(|edge| {
            edge.endpoints().is_ok_and(|points| {
                points.iter().all(|point| {
                    (point.x - 29.77557175191365).abs() < 1.0e-7
                        && (point.y + 10.252252426049688).abs() < 1.0e-7
                }) && (points[0].z - points[1].z).abs() > 11.9
            })
        })
        .expect("vertical boss edge")
        .clone();
    let rounded_boss = joined
        .check_edge_blends(std::slice::from_ref(&vertical), 5.0)
        .expect("vertical boss fillet");
    let modified_base_edge = match rounded_boss.resolve(&base_edge) {
        Resolution::Preserved(edge) | Resolution::Modified(edge) | Resolution::Merged(edge) => edge,
        resolution => panic!("boss-base edge must remain selectable: {resolution:?}"),
    };
    for radius in [1.0, 2.0, 3.0] {
        let final_shape = rounded_boss
            .check_edge_blends(std::slice::from_ref(&modified_base_edge), radius)
            .unwrap_or_else(|error| {
                panic!("{radius} mm boss-base fillet after vertical boss fillet: {error}")
            });
        final_shape
            .tessellate(
                TessellationOptions::new(0.1, 0.1, 24).expect("application tessellation options"),
            )
            .expect("post-fillet boss-base display tessellation");
    }
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
fn analytical_shell_offsets_a_closed_polyhedral_skin() {
    let cube = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0));
    let source_face = cube.faces().next().expect("source face").clone();
    let inward = cube.shell(&[], -1.0).expect("inward shell");
    assert_eq!(inward.solids().len(), 1);
    assert_eq!(inward.faces().count(), 12);
    assert!((inward.volume() - 488.0).abs() < 1.0e-7);
    assert!(matches!(
        inward.resolve(&source_face),
        Resolution::Split(ref faces) if faces.len() == 2
    ));

    let outward = cube.shell(&[], 1.0).expect("outward shell");
    assert_eq!(outward.solids().len(), 1);
    assert!((outward.volume() - 728.0).abs() < 1.0e-7);
}

#[test]
fn analytical_shell_can_leave_a_face_open() {
    let cube = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0));
    let opening = cube.faces().next().expect("opening face").clone();
    let shelled = cube
        .shell(std::slice::from_ref(&opening), -1.0)
        .expect("open shell");
    assert_eq!(shelled.solids().len(), 1);
    assert_eq!(shelled.faces().count(), 11);
    assert!((shelled.volume() - 424.0).abs() < 1.0e-7);
    assert!(matches!(shelled.resolve(&opening), Resolution::Modified(_)));
}

#[test]
fn analytical_shell_can_leave_adjacent_faces_open_without_shared_rim_geometry() {
    let cube = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0));
    let top = cube
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.origin().z > 9.9))
        .expect("top face")
        .clone();
    let side = cube
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.origin().x > 9.9))
        .expect("adjacent side face")
        .clone();
    let shelled = cube
        .shell(&[top, side], -1.0)
        .expect("shell with adjacent openings");
    assert_eq!(shelled.solids().len(), 1);
    assert_eq!(shelled.faces().count(), 10);
    shelled
        .tessellate(TessellationOptions::default())
        .expect("adjacent-opening shell tessellation");
}

#[test]
fn analytical_shell_offsets_a_closed_cylinder() {
    let tolerance = ModelTolerance::default();
    let profile = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 5.0).expect("circle")],
            tolerance,
        )
        .expect("wire"),
        [],
        tolerance,
    )
    .expect("profile");
    let cylinder = profile
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .expect("cylinder");
    let shelled = cylinder.shell(&[], -1.0).expect("cylindrical shell");
    assert_eq!(shelled.solids().len(), 1);
    assert!(
        (shelled.volume() - 122.0 * std::f64::consts::PI).abs() < 1.0e-7,
        "volume={}, centroid={:?}, faces={}",
        shelled.volume(),
        shelled.centroid(),
        shelled.faces().count()
    );
    let top = cylinder
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.origin().z > 9.9))
        .expect("top cap")
        .clone();
    let opened = cylinder.shell(&[top], -1.0).expect("open cylinder shell");
    assert!((opened.volume() - 106.0 * std::f64::consts::PI).abs() < 1.0e-7);
    let outward = cylinder.shell(&[], 1.0).expect("outward cylinder shell");
    assert!((outward.volume() - 182.0 * std::f64::consts::PI).abs() < 1.0e-7);
}

#[test]
fn analytical_shell_offsets_a_solid_with_a_circular_boss() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 20.0, 10.0));
    let profile = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(10.0, 10.0), 3.0).expect("circle")],
            tolerance,
        )
        .expect("wire"),
        [],
        tolerance,
    )
    .expect("profile");
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 10.0),
        Vec3::unit_x(),
        Vec3::unit_y(),
        tolerance,
    )
    .expect("boss plane");
    let boss = profile
        .extrude(plane, Vec3::new(0.0, 0.0, 5.0))
        .expect("boss");
    let joined = block.union(&boss).expect("boss union");
    let opening = joined
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.origin().z > 14.9))
        .expect("boss top")
        .clone();
    let shelled = joined.shell(&[opening], -0.5).expect("boss shell");
    assert_eq!(shelled.solids().len(), 1);
    assert!(shelled.volume() > 0.0);
    assert!(shelled
        .faces()
        .any(|face| matches!(face.surface(), Surface3::Cylinder(_))));

    let base_rim = joined
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
        .find(|edge| {
            edge.endpoints()
                .is_ok_and(|points| points.iter().all(|point| (point.z - 10.0).abs() < 1.0e-7))
        })
        .expect("boss base rim")
        .clone();
    let filleted = joined.fillet(&[base_rim], 0.5).expect("boss fillet");
    let opening = filleted
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.origin().z > 14.9))
        .expect("filleted boss top")
        .clone();
    let filleted_shell = filleted
        .shell(&[opening], -0.25)
        .expect("filleted boss shell");
    assert_eq!(filleted_shell.solids().len(), 1);
    assert!(filleted_shell.faces().any(|face| matches!(
        face.surface(),
        Surface3::Torus(_) | Surface3::Offset(_) | Surface3::RollingBall(_)
    )));
}

#[test]
fn analytical_shell_closes_a_reversed_cylinder_chart_at_two_pi() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(Point3::new(-35.0, 0.0, -20.0), Point3::new(5.0, 15.0, 15.0));
    let mut kernel = Kernel::new(tolerance).expect("kernel");
    let mut transaction = kernel.begin_operation();
    transaction
        .cylinder(
            Point3::new(-17.345868645558674, 0.0, -2.9254542313024605),
            Vec3::new(12.763607080181341, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -12.763607080181341),
            Vec3::new(0.0, -15.0, 0.0),
        )
        .expect("reversed-frame cylinder");
    let committed = transaction.finish().expect("cylinder commit");
    let boss = super::Compound::from_operation(kernel, committed, []).expect("boss");
    let joined = block.union(&boss).expect("side boss union");
    let opening = joined
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.origin().y < -14.9))
        .expect("boss end cap")
        .clone();
    let shelled = joined
        .shell(&[opening], -1.0)
        .expect("reversed cylinder shell");
    assert_eq!(shelled.solids().len(), 1);
    assert!(shelled.volume() > 0.0);
}

#[test]
fn analytical_shell_offsets_a_rolling_ball_fillet() {
    let block = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let edge = block
        .edges()
        .find(|edge| {
            edge.endpoints()
                .is_ok_and(|points| (points[1].z - points[0].z).abs() > 4.9)
        })
        .expect("vertical edge")
        .clone();
    let filleted = block.fillet(&[edge], 1.0).expect("fillet");
    let opening = filleted
        .faces()
        .filter(|face| matches!(face.surface(), Surface3::Plane(_)))
        .max_by(|first, second| {
            let first_z = match first.surface() {
                Surface3::Plane(plane) => plane.origin().z,
                _ => unreachable!(),
            };
            let second_z = match second.surface() {
                Surface3::Plane(plane) => plane.origin().z,
                _ => unreachable!(),
            };
            first_z.total_cmp(&second_z)
        })
        .expect("top face")
        .clone();
    let shelled = filleted.shell(&[opening], -0.25).expect("filleted shell");
    assert_eq!(shelled.solids().len(), 1);
    assert!(shelled.volume() > 0.0);
    assert!(shelled.faces().any(|face| matches!(
        face.surface(),
        Surface3::Cylinder(_) | Surface3::Offset(_) | Surface3::RollingBall(_)
    )));
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
fn transverse_circle_joins_a_shelled_filleted_boss_in_both_directions() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(
        Point3::new(-61.51105455585198, 0.0, -34.901492966646394),
        Point3::new(62.18948530462976, 37.0, 26.684942786593528),
    );
    let circle = |center, radius| {
        Profile::new(
            Wire2::new([TrimmedCurve2::circle(center, radius).unwrap()], tolerance).unwrap(),
            [],
            tolerance,
        )
        .unwrap()
    };
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        -Vec3::unit_z(),
        tolerance,
    )
    .unwrap();
    let boss = circle(Point2::new(-24.206992697151723, 2.570753756415946), 15.0)
        .extrude(plane, Vec3::new(0.0, -16.0, 0.0))
        .unwrap();
    let joined = block.union(&boss).unwrap();
    let rims = joined
        .edges()
        .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
        .cloned()
        .collect::<Vec<_>>();
    let rounded = joined.fillet(&rims, 4.0).unwrap();
    let openings = rounded
        .faces()
        .filter(|face| {
            matches!(face.surface(), Surface3::Plane(p) if
                ((p.origin().y-37.0).abs() < 1.0e-7 && p.normal().y.abs() > 0.99)
                || ((p.origin().z+34.901492966646394).abs() < 1.0e-7 && p.normal().z.abs() > 0.99)
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(openings.len(), 2);
    let shell = rounded.shell(&openings, -3.0).unwrap();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        -Vec3::unit_z(),
        Vec3::unit_y(),
        tolerance,
    )
    .unwrap();
    for distance in [20.0, -20.0] {
        let tool = circle(
            Point2::new(26.820213683179933, -2.8914822011701538),
            23.07147441660714,
        )
        .extrude(plane, Vec3::new(distance, 0.0, 0.0))
        .unwrap();
        let result = shell
            .union(&tool)
            .expect("transverse join through rounded shell");
        assert_eq!(result.solids().len(), 1);
        assert!(result.volume() > shell.volume());
    }

    let shallow_tool = circle(
        Point2::new(26.820213683179933, -2.8914822011701538),
        23.07147441660714,
    )
    .extrude(plane, Vec3::new(-20.0, 0.0, 0.0))
    .unwrap();
    let shallow_cut = shell
        .difference(&shallow_tool)
        .expect("shallow transverse cut");
    let rim = shallow_cut
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Circle(circle)
            if (circle.radius() - 23.07147441660714).abs() < tolerance.linear
                && circle.center().x.abs() < tolerance.linear)
        })
        .unwrap()
        .clone();
    let shallow_cut = shallow_cut
        .fillet(&[rim], 2.0)
        .expect("round shallow tool rim");
    let bottom_edge = shallow_cut
        .edges()
        .find(|edge| {
            let ends = edge.endpoints().unwrap();
            matches!(edge.curve(), Curve3::Circle(_))
                && ends
                    .iter()
                    .all(|p| (p.x + 20.0).abs() < tolerance.linear && p.y < -12.9)
                && (ends[0].y - ends[1].y).abs() > 2.9
        })
        .expect("short bottom thickness edge")
        .clone();
    let bottom_fillet = shallow_cut
        .fillet(&[bottom_edge], 1.0)
        .expect("round bottom edge through toroidal trims");
    assert!(bottom_fillet.volume() > 0.0);
    for body in [&shallow_cut, &bottom_fillet] {
        let mesh = body
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap();
        for triangle in &mesh.triangles {
            let [a, b, c] = *triangle;
            let geometric = (mesh.positions[b] - mesh.positions[a])
                .cross(mesh.positions[c] - mesh.positions[a]);
            let analytical = mesh.normals[a] + mesh.normals[b] + mesh.normals[c];
            assert!(
                geometric.dot(analytical) >= -1.0e-12,
                "curved triangles must agree with their analytical normals"
            );
        }
    }

    // A transverse cut opens the periodic boss and both of its blend skins.
    // Their former chart seams must disappear from the BREP itself.
    let tool = circle(
        Point2::new(26.820213683179933, -2.8914822011701538),
        23.07147441660714,
    )
    .extrude(plane, Vec3::new(-100.0, 0.0, 0.0))
    .unwrap();
    let cut = shell.difference(&tool).expect("cut through rounded shell");
    let check_partitions = |compound: &super::Compound| {
        for edge in compound.edges() {
            let faces = edge.connected_faces().unwrap();
            if faces.len() == 2
                && matches!(
                    faces[0].surface(),
                    Surface3::Cylinder(_) | Surface3::Torus(_)
                )
            {
                assert_ne!(
                    faces[0].surface(),
                    faces[1].surface(),
                    "connected periodic patches must form one face"
                );
            }
        }
    };
    check_partitions(&cut);
    let remote_edge = cut
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Circle(circle)
                if (circle.radius() - 23.07147441660714).abs() < tolerance.linear
                    && circle.center().x.abs() < tolerance.linear)
        })
        .expect("transverse tool rim")
        .clone();
    let rounded_cut = cut
        .fillet(&[remote_edge], 2.0)
        .expect("unrelated rim fillet");
    check_partitions(&rounded_cut);
    for edge in cut.edges() {
        if let Some(unchanged) = rounded_cut
            .edges()
            .find(|candidate| candidate.persistent_id() == edge.persistent_id())
        {
            assert_eq!(
                cut.is_semantic_edge(edge),
                rounded_cut.is_semantic_edge(unchanged),
                "an unrelated fillet must not change existing edge visibility"
            );
        }
    }
}

#[test]
fn moved_circle_joins_across_a_rounded_shell_opening() {
    let tolerance = ModelTolerance::default();
    let circle = |center, radius| {
        Profile::new(
            Wire2::new([TrimmedCurve2::circle(center, radius).unwrap()], tolerance).unwrap(),
            [],
            tolerance,
        )
        .unwrap()
    };
    let plane = |x| {
        Plane3::new(
            Point3::new(x, 0.0, 0.0),
            -Vec3::unit_z(),
            Vec3::unit_y(),
            tolerance,
        )
        .unwrap()
    };
    let base = circle(
        Point2::new(-17.094721967326727, 3.621763128670917),
        38.40462700511438,
    )
    .extrude(plane(0.0), Vec3::new(33.0, 0.0, 0.0))
    .unwrap();
    let bottom = base
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Circle(c) if c.center().x.abs() < tolerance.linear)
        })
        .unwrap()
        .clone();
    let rounded = base.fillet(&[bottom], 12.0).unwrap();
    let opening = rounded
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
                if (p.origin().x - 33.0).abs() < tolerance.linear)
        })
        .unwrap()
        .clone();
    let shell = rounded.shell(&[opening], -4.0).unwrap();
    let cutter = circle(
        Point2::new(26.107773279218943, 7.6318958166238975),
        26.954243397522823,
    )
    .extrude(plane(33.0), Vec3::new(-44.0, 0.0, 0.0))
    .unwrap();
    let cut = shell.difference(&cutter).unwrap();
    let rim = cut
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Circle(c)
                if c.center().x.abs() < tolerance.linear
                    && (c.radius() - 26.954243397522823).abs() < tolerance.linear)
        })
        .unwrap()
        .clone();
    let rounded_cut = cut.fillet(&[rim], 1.0).unwrap();
    // Boolean classification meshes must resolve the curved trims without
    // refining long triangle fans into hundreds of thousands of triangles.
    let mesh = rounded_cut
        .tessellate(TessellationOptions::new(0.03, 0.12, 12).unwrap())
        .unwrap();
    assert!(
        mesh.triangles.len() < 50_000,
        "{} triangles",
        mesh.triangles.len()
    );
    let tool = circle(
        Point2::new(-3.435024046379482, 5.236934939201773),
        11.980574220283309,
    )
    .extrude(plane(0.0), Vec3::new(20.0, 0.0, 0.0))
    .unwrap();
    let joined = rounded_cut
        .union(&tool)
        .expect("join across the rounded opening");
    assert_eq!(joined.solids().len(), 1);
    assert!(joined.volume() > rounded_cut.volume());
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

#[test]
fn chamfer_setbacks_on_oblique_planes_and_trihedral_corners() {
    let tolerance = ModelTolerance::default();
    let profile = Profile::new(
        Wire2::polygon(
            [
                Point2::new(0.0, 0.0),
                Point2::new(20.0, 0.0),
                Point2::new(20.0, 20.0),
                Point2::new(0.0, 20.0),
            ],
            tolerance,
        )
        .unwrap(),
        [],
        tolerance,
    )
    .unwrap();
    let body = profile
        .extrude(Plane3::xy(), Vec3::new(3.0, 2.0, 10.0))
        .unwrap();
    let corner = Point3::new(0.0, 0.0, 0.0);
    let edges = body
        .edges()
        .filter(|edge| {
            edge.endpoints()
                .unwrap()
                .iter()
                .any(|point| point.distance(corner) < 1e-7)
        })
        .cloned()
        .collect::<Vec<_>>();
    for edge in &edges {
        let result = body.chamfer(&[edge.clone()], 1.0).unwrap();
        let source = edge.curve();
        let new_face = result
            .faces()
            .find(|face| {
                body.faces()
                    .all(|old| old.persistent_id() != face.persistent_id())
            })
            .unwrap();
        // The long contact boundaries stay exactly one unit from the source line.
        let mut contacts = 0;
        for boundary in new_face.boundary_edges().unwrap() {
            let points = boundary.endpoints().unwrap();
            let midpoint = Point3::from_vec((points[0].to_vec() + points[1].to_vec()) * 0.5);
            if (source.project(midpoint, tolerance).unwrap().distance - 1.0).abs() < 1e-6 {
                contacts += 1;
            }
        }
        assert_eq!(contacts, 2);
    }
    body.check_edge_blends(&edges, 1.0)
        .expect("oblique trihedral blends");
}

#[test]
fn chamfer_planar_boundaries_are_visible_before_another_operation() {
    let source = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(50.0, 25.0, 20.0));
    let vertical = source
        .edges()
        .find(|edge| {
            edge.endpoints()
                .unwrap()
                .iter()
                .all(|point| point.x.abs() < 1e-7 && point.y.abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let rounded = source.fillet(&[vertical], 5.0).unwrap();
    let top = rounded
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Circle(_))
                && edge
                    .endpoints()
                    .unwrap()
                    .iter()
                    .all(|point| (point.z - 20.0).abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let chamfered = rounded
        .chamfer_with_tangent_propagation(&[top], 2.0, true)
        .unwrap();
    let boundaries = chamfered
        .edges()
        .filter(|edge| {
            let faces = edge.connected_faces().unwrap();
            edge.endpoints()
                .unwrap()
                .iter()
                .all(|point| point.z >= 18.0 - 1e-7)
                && faces.len() == 2
                && matches!((faces[0].surface(), faces[1].surface()),
            (Surface3::Plane(first), Surface3::Plane(second))
                if first.normal().cross(second.normal()).magnitude() > 1e-7)
        })
        .cloned()
        .collect::<Vec<_>>();
    assert!(!boundaries.is_empty());
    for edge in &boundaries {
        assert!(
            chamfered.is_semantic_edge(edge),
            "sharp planar boundary hidden immediately after chamfer"
        );
    }
    let bottom = chamfered
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Circle(_))
                && edge
                    .endpoints()
                    .unwrap()
                    .iter()
                    .all(|point| point.z.abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let next = chamfered.fillet(&[bottom], 1.0).unwrap();
    for edge in boundaries {
        match next.resolve(&edge) {
            Resolution::Preserved(edge) | Resolution::Modified(edge) => {
                assert!(next.is_semantic_edge(&edge))
            }
            _ => panic!("subsequent bottom fillet lost a planar boundary"),
        }
    }
}

#[test]
fn fillet_chamfer_rim_after_bottom_fillets() {
    let tolerance = ModelTolerance::default();
    let points = [
        Point2::new(-48.09320196891367, 20.428303658663456),
        Point2::new(54.19907870791369, 20.428303658663456),
        Point2::new(54.19907870791369, -26.081893232094234),
        Point2::new(-48.09320196891367, -26.081893232094234),
    ];
    let wire = Wire2::new(
        (0..4)
            .rev()
            .map(|i| TrimmedCurve2::line(points[(i + 1) % 4], points[i]).unwrap()),
        tolerance,
    )
    .unwrap();
    let profile = Profile::new(wire, [], tolerance).unwrap();
    let body = profile
        .extrude(
            Plane3::new(
                Point3::new(0.0, 0.0, 0.0),
                Vec3::unit_x(),
                -Vec3::unit_z(),
                tolerance,
            )
            .unwrap(),
            Vec3::new(0.0, 34.0, 0.0),
        )
        .unwrap();
    let corner = body
        .edges()
        .find(|edge| {
            edge.endpoints().unwrap().iter().all(|p| {
                (p.x - 54.19907870791369).abs() < 1e-7 && (p.z - 26.081893232094234).abs() < 1e-7
            })
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[corner], 21.0).unwrap();
    let top = body
        .edges()
        .find(|edge| {
            edge.endpoints()
                .unwrap()
                .iter()
                .all(|p| (p.z - 26.081893232094234).abs() < 1e-7)
                && edge
                    .endpoints()
                    .unwrap()
                    .iter()
                    .all(|p| (p.y - 34.0).abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let body = body
        .chamfer_with_tangent_propagation(&[top], 10.0, true)
        .unwrap();
    let bottom = body
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Circle(_))
                && edge.endpoints().unwrap().iter().all(|p| p.y.abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[bottom], 13.0).unwrap();
    let bottom = body
        .edges()
        .find(|edge| {
            edge.endpoints()
                .unwrap()
                .iter()
                .all(|p| p.y.abs() < 1e-7 && (p.z + 20.428303658663456).abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[bottom], 4.0).unwrap();
    let rim = body
        .edges()
        .find(|edge| {
            edge.endpoints()
                .unwrap()
                .iter()
                .all(|p| (p.y - 24.0).abs() < 1e-7 && (p.x - 54.19907870791369).abs() < 1e-7)
        })
        .unwrap()
        .clone();
    for radius in [1.0, 2.0, 3.0] {
        let result = body.fillet(&[rim.clone()], radius).unwrap();
        assert_eq!(result.solids().len(), 1);
        assert!(result.solids().next().unwrap().volume() < body.solids().next().unwrap().volume());
        result
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .expect("chamfer rim fillet display mesh");
    }
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

#[test]
fn fillet_short_edge_at_partial_chamfer_end() {
    let body = partial_chamfer_fixture();
    let short = body
        .edges()
        .find(|e| {
            let p = e.endpoints().unwrap();
            p.iter().all(|p| (p.z + 3.908513365021342).abs() < 1e-7)
                && (p[0].y - p[1].y).abs() > 5.9
                && (p[0].y - p[1].y).abs() < 6.1
                && (p[0].x - p[1].x).abs() > 5.9
        })
        .unwrap()
        .clone();
    // Preserve the certified geometry at both the original radius and the
    // larger radius that exposed repeated intersection-evaluation costs.
    for (radius, volume, area) in [
        (0.1, 94578.79148614826, 13670.747305236526),
        (0.7, 94579.58912582739, 13668.850104317711),
    ] {
        let result = body.fillet(&[short.clone()], radius).unwrap();
        assert_eq!(result.solids().len(), 1);
        assert_eq!(result.faces().count(), body.faces().count() + 1);
        let solid = result.solids().next().unwrap();
        assert!((solid.volume() - volume).abs() < 1e-6);
        assert!((solid.surface_area() - area).abs() < 1e-6);
        result
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap();
    }
}

#[test]
fn fillet_chamfer_rim_after_partial_end_fillets() {
    let mut body = partial_chamfer_fixture();
    for (axis, coordinate, radius) in [(0, 50.430018992481585, 3.0), (2, -24.348125762908833, 1.0)]
    {
        let edge = body
            .edges()
            .find(|e| {
                e.endpoints().unwrap().iter().all(|p| {
                    (p.y - 26.0).abs() < 1e-7
                        && ((if axis == 0 { p.x } else { p.z }) - coordinate).abs() < 1e-7
                })
            })
            .unwrap()
            .clone();
        body = body.fillet(&[edge], radius).unwrap();
    }
    let short = body
        .edges()
        .find(|e| {
            let p = e.endpoints().unwrap();
            p.iter().all(|p| (p.z + 3.908513365021342).abs() < 1e-7)
                && (p[0].y - p[1].y).abs() > 2.9
                && (p[0].x - p[1].x).abs() > 2.9
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[short], 0.75).unwrap();
    let rim = body
        .edges()
        .find(|e| {
            e.connected_faces()
                .unwrap()
                .iter()
                .any(|f| matches!(f.surface(), Surface3::Cone(_)))
                && e.endpoints()
                    .unwrap()
                    .iter()
                    .all(|p| (p.y - 26.0).abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let result = body.fillet(&[rim], 0.2).unwrap();
    assert_eq!(result.solids().len(), 1);
    assert!(result.volume() < body.volume());
    for edge in result.edges() {
        assert_eq!(edge.connected_faces().unwrap().len(), 2);
        let points = edge.endpoints().unwrap();
        assert!(points[0].distance(points[1]) > 1e-7);
    }
    result
        .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
        .unwrap();
}

#[test]
fn blend_oblique_partial_torus_rim() {
    let tol = ModelTolerance::default();
    let wire = Wire2::new(
        [TrimmedCurve2::circle(
            Point2::new(-43.19342433885193, 7.0858322653666725),
            29.84371852126014,
        )
        .unwrap()],
        tol,
    )
    .unwrap();
    let profile = Profile::new(wire, [], tol).unwrap();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        -Vec3::unit_z(),
        tol,
    )
    .unwrap();
    let axis = Axis3::new(
        Point3::new(7.161213459321003, 0.0, -21.48364037903721),
        Vec3::new(
            32.187769969790175 - 7.161213459321003,
            0.0,
            34.67534938370912 + 21.48364037903721,
        ),
    )
    .unwrap();
    let body = profile
        .revolve(plane, axis, 1.5 * std::f64::consts::PI)
        .unwrap();
    let rims = body
        .edges()
        .filter(|edge| {
            edge.connected_faces()
                .unwrap()
                .iter()
                .any(|f| matches!(f.surface(), Surface3::Plane(_)))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(rims.len(), 2);
    for size in [1.0, 5.0] {
        for edge in &rims {
            for result in [
                body.fillet(&[edge.clone()], size).unwrap(),
                body.chamfer(&[edge.clone()], size).unwrap(),
            ] {
                assert_eq!(result.solids().len(), 1);
                assert_eq!(
                    result.faces().count(),
                    4,
                    "one continuous blend face per rim"
                );
                assert!(result.volume() < body.volume());
                result
                    .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                    .unwrap();
            }
        }
    }
    let both = body.fillet(&rims, 5.0).unwrap();
    assert_eq!(both.faces().count(), 5, "two continuous blend faces");
    let mesh = both
        .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
        .unwrap();
    let blends = both
        .faces()
        .filter(|face| matches!(face.surface(), Surface3::RollingBall(_)))
        .map(|face| face.topology_id())
        .collect::<Vec<_>>();
    assert!(
        mesh.triangle_faces
            .iter()
            .filter(|face| blends.contains(face))
            .count()
            < 30_000,
        "band meshing should avoid excessive refinement of the two fillets"
    );
    // The saved follow-up sketch cuts a 4 mm pocket into the starting cap.
    let pocket_plane = both
        .faces()
        .find_map(|face| match face.surface() {
            Surface3::Plane(plane) => Some(*plane),
            _ => None,
        })
        .unwrap();
    let pocket_profile = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(
                Point2::new(52.6543864532136, -6.740777335198338),
                12.163742788970556,
            )
            .unwrap()],
            tol,
        )
        .unwrap(),
        [],
        tol,
    )
    .unwrap();
    let cutter = pocket_profile
        .extrude(pocket_plane, pocket_plane.normal() * -4.0)
        .unwrap();
    let pocket = both
        .difference(&cutter)
        .expect("pocket after continuous rim fillets");
    assert_eq!(pocket.solids().len(), 1);
    assert_eq!(
        pocket.faces().count(),
        7,
        "pocket preserves continuous fillets"
    );
    assert!(pocket.volume() < both.volume());
    pocket
        .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
        .unwrap();
}

#[test]
fn shell_opening_edge_blends_at_small_sizes() {
    let tolerance = ModelTolerance::default();
    let circle = |center, radius| {
        Profile::new(
            Wire2::new([TrimmedCurve2::circle(center, radius).unwrap()], tolerance).unwrap(),
            [],
            tolerance,
        )
        .unwrap()
    };
    let plane = |x| {
        Plane3::new(
            Point3::new(x, 0.0, 0.0),
            -Vec3::unit_z(),
            Vec3::unit_y(),
            tolerance,
        )
        .unwrap()
    };
    let base = circle(
        Point2::new(-17.094721967326727, 3.621763128670917),
        38.40462700511438,
    )
    .extrude(plane(0.0), Vec3::new(33.0, 0.0, 0.0))
    .unwrap();
    let bottom = base
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Circle(c) if c.center().x.abs() < tolerance.linear)
        })
        .unwrap()
        .clone();
    let rounded = base.fillet(&[bottom], 12.0).unwrap();
    let opening = rounded
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
                if (p.origin().x - 33.0).abs() < tolerance.linear)
        })
        .unwrap()
        .clone();
    let shell = rounded.shell(&[opening], -4.0).unwrap();
    let cutter = circle(
        Point2::new(26.107773279218943, 7.6318958166238975),
        26.954243397522823,
    )
    .extrude(plane(33.0), Vec3::new(-44.0, 0.0, 0.0))
    .unwrap();
    let cut = shell.difference(&cutter).unwrap();
    let edges = cut
        .edges()
        .filter(|edge| {
            matches!(edge.curve(), Curve3::Line(_))
                && edge
                    .connected_faces()
                    .unwrap()
                    .iter()
                    .all(|face| matches!(face.surface(), Surface3::Cylinder(_)))
        })
        .cloned()
        .collect::<Vec<_>>();
    let edges = edges.into_iter().filter(|edge| edge.connected_faces().unwrap().iter().any(|face| matches!(face.surface(), Surface3::Cylinder(c) if (c.radius() - 38.40462700511438).abs() < tolerance.linear))).collect::<Vec<_>>();
    assert_eq!(edges.len(), 2);
    for edge in edges {
        for size in [1.0, 0.5] {
            for result in [
                cut.fillet(&[edge.clone()], size)
                    .expect("shell opening fillet"),
                cut.chamfer_with_tangent_propagation(&[edge.clone()], size, true)
                    .expect("shell opening chamfer"),
            ] {
                assert_eq!(result.solids().len(), 1);
                assert!(result.volume() > 0.0 && result.volume() < cut.volume());
                result
                    .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                    .expect("shell opening blend display mesh");
            }
        }
    }
}
