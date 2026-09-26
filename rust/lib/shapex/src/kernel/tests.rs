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

#[test]
fn shell_support_extension_preserves_rational_surface_chart() {
    let surface = NurbsSurface3::new(
        (2, 1),
        vec![
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 2.0, 0.0),
                Point3::new(3.0, 0.0, 0.0),
            ],
            vec![
                Point3::new(0.0, 0.0, 4.0),
                Point3::new(1.0, 2.0, 4.0),
                Point3::new(3.0, 0.0, 4.0),
            ],
        ],
        vec![vec![1.0, 0.8, 1.0]; 2],
        (
            vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
            vec![-1.0, -1.0, 1.0, 1.0],
        ),
    )
    .unwrap();
    let extended: Surface3 = surface.extended_bezier([0.2, 0.3]).unwrap().into();
    let original: Surface3 = surface.into();
    for u in [2.0, 2.1, 3.0, 4.9, 5.0] {
        for v in [-1.0, 0.0, 1.0] {
            let a = original.evaluate(u, v).unwrap();
            let b = extended.evaluate(u, v).unwrap();
            assert!(a.point.distance(b.point) < 1e-12);
            for delta in [
                a.du - b.du,
                a.dv - b.dv,
                a.duu - b.duu,
                a.duv - b.duv,
                a.dvv - b.dvv,
            ] {
                assert!(delta.magnitude() < 1e-12);
            }
        }
    }
    // Independently evaluate the rational quadratic outside its former range.
    for u in [1.8, 5.2] {
        let t: f64 = (u - 2.0) / 3.0;
        let weights = [(1.0 - t).powi(2), 1.6 * t * (1.0 - t), t * t];
        let denominator: f64 = weights.iter().sum();
        for v in [-1.3, 1.3] {
            let expected = Point3::new(
                (weights[1] + 3.0 * weights[2]) / denominator,
                2.0 * weights[1] / denominator,
                2.0 * (v + 1.0),
            );
            assert!(extended.evaluate(u, v).unwrap().point.distance(expected) < 1e-12);
        }
    }
}

#[test]
fn shell_twice_filleted_sector() {
    let tol = ModelTolerance::default();
    let a = Point2::new(100.0, 0.0);
    let b = Point2::new(100.0, 28.86751397086674);
    let c = Point2::new(84.1506348326489, 38.01814937882426);
    let d = Point2::new(75.0, 43.3012709563001);
    let e = Point2::new(52.754440335806734, 16.36634224401021);
    let wire = Wire2::new(
        [
            TrimmedCurve2::line(a, b).unwrap(),
            TrimmedCurve2::line(b, c).unwrap(),
            TrimmedCurve2::line(c, d).unwrap(),
            TrimmedCurve2::arc_through(d, Point2::new(61.448374625936175, 31.839790317286276), e)
                .unwrap(),
            TrimmedCurve2::line(e, a).unwrap(),
        ],
        tol,
    )
    .unwrap();
    let body = Profile::new(wire, [], tol)
        .unwrap()
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 38.0))
        .unwrap();
    let rim = body
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Circle(_)) && edge.endpoints().unwrap()[0].z > 37.0
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[rim], 8.0).unwrap();
    let corner = body
        .edges()
        .find(|edge| {
            let p = edge.endpoints().unwrap();
            matches!(edge.curve(), Curve3::Line(_))
                && (p[0].x - 75.0).abs() < 1e-5
                && (p[1].x - 75.0).abs() < 1e-5
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[corner], 5.0).unwrap();
    let opening = body
        .faces()
        .find(|face| match face.surface() {
            Surface3::Plane(p) => {
                p.normal().z.abs() < 0.1 && p.normal().x.abs() > 0.1 && p.normal().x.abs() < 0.4
            }
            _ => false,
        })
        .unwrap()
        .clone();
    for thickness in [1.0, 2.0] {
        let shell = body.shell(&[opening.clone()], -thickness).unwrap();
        assert!(shell.volume() > 0.0 && shell.volume() < body.volume());
        shell
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap();
    }
}

#[test]
fn join_offset_cylinder_shell_from_open_end() {
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
    let body = circle(
        Point2::new(7.071384583473251, 8.184680835253655),
        31.40489826969949,
    )
    .extrude(plane, Vec3::unit_y() * 37.0)
    .unwrap();
    let rim = body
        .edges()
        .find(|edge| edge.endpoints().unwrap().iter().all(|p| p.y > 36.9))
        .unwrap()
        .clone();
    let body = body.fillet(&[rim], 6.0).unwrap();
    let side = body
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Cylinder(_)))
        .unwrap()
        .clone();
    let body = body.offset_faces(&[side], 5.0).unwrap();
    let opening = body
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(p) if p.origin().y.abs() < 1e-7))
        .unwrap()
        .clone();
    let body = body.shell(&[opening], -3.0).unwrap();
    let tool = circle(
        Point2::new(13.481018952106044, 11.006242526665803),
        35.35671545346585,
    )
    .extrude(plane, Vec3::unit_y() * 14.0)
    .unwrap();
    let joined = body
        .union(&tool)
        .expect("join final extrusion in untitled(17).cad");
    assert_eq!(joined.solids().len(), 1);
    assert!(joined.volume() > body.volume());
    let reversed = tool.union(&body).expect("union in reverse operand order");
    assert!((joined.volume() - reversed.volume()).abs() < joined.volume() * 1e-8);
    let cut = body.difference(&tool).expect("original cut operation");
    assert!((joined.volume() - tool.volume() - cut.volume()).abs() < joined.volume() * 1e-8);
    joined
        .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
        .unwrap();
    // Round the outer bottom rim, then chamfer the two inner rim edges.
    // Their junction creates a revolved corner with a collapsed boundary.
    let rim = joined.edges().next().unwrap().clone();
    // A 4 mm fillet would cross the inner boundary of the 3 mm shell wall.
    assert!(joined.fillet(&[rim.clone()], 4.0).is_err());
    let filleted = joined.fillet(&[rim], 0.25).unwrap();
    let edges: Vec<_> = filleted.edges().skip(1).take(2).cloned().collect();
    for distance in [1.0, 2.5] {
        let chamfered = filleted
            .chamfer_with_tangent_propagation(&edges, distance, true)
            .unwrap();
        assert_eq!(
            chamfered
                .faces()
                .filter(|face| matches!(face.surface(), Surface3::Cone(_)))
                .count(),
            2,
            "each circular chamfer must have one cone face despite coefficient roundoff"
        );
        chamfered
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .expect("two-edge chamfer must mesh across its collapsed corner");
    }
}

#[test]
fn shelled_cylinder_deeper_offset_circle_cut() {
    // Reproduce untitled(7).cad: a 60 mm cylinder, a 14 mm top fillet,
    // a 5 mm inward shell, and an offset circular cut from the open end.
    // Just beyond 48 mm the outer fillet intersection crosses the cutter's
    // parameter seam, leaving a short branch between the seam and the cap.
    let tol = ModelTolerance::default();
    let circle_profile = |center, radius| {
        Profile::new(
            Wire2::new([TrimmedCurve2::circle(center, radius).unwrap()], tol).unwrap(),
            [],
            tol,
        )
        .unwrap()
    };
    let plane = Plane3::new(
        Point3::new(-30.0, 0.0, 0.0),
        -Vec3::unit_z(),
        Vec3::unit_y(),
        tol,
    )
    .unwrap();
    let body = circle_profile(
        Point2::new(12.076566694315764, 15.0800651467881),
        31.27066987074541,
    )
    .extrude(plane, Vec3::new(60.0, 0.0, 0.0))
    .unwrap();
    let rim = body
        .edges()
        .find(|edge| edge.endpoints().unwrap().iter().all(|p| p.x > 29.9))
        .unwrap()
        .clone();
    let body = body.fillet(&[rim], 14.0).unwrap();
    let opening = body
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(p) if p.origin().x < -29.9))
        .unwrap()
        .clone();
    let body = body.shell(&[opening], -5.0).unwrap();
    let cut_plane = *body
        .faces()
        .find_map(|face| match face.surface() {
            Surface3::Plane(p) if p.origin().x < -29.9 => Some(p),
            _ => None,
        })
        .unwrap();
    let mut previous_volume = body.volume();
    for distance in [48.0, 48.05, 49.0, 50.0] {
        let tool = circle_profile(
            Point2::new(35.878129354641324, 17.976990818287497),
            22.946161555129095,
        )
        .extrude(cut_plane, cut_plane.normal() * distance)
        .unwrap();
        let result = body
            .difference(&tool)
            .unwrap_or_else(|error| panic!("cut {distance}: {error}"));
        assert_eq!(result.solids().len(), 1);
        assert!(result.volume() > 0.0 && result.volume() < previous_volume);
        previous_volume = result.volume();
        result
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap_or_else(|error| panic!("cut {distance} mesh: {error}"));
    }
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

    let outward = cube
        .shell_with_join(&[], 1.0, "sharp")
        .expect("outward sharp shell");
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
fn rounded_shell_joins_outward_box_edges() {
    let cube = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0));
    let shell = cube.shell(&[], 1.0).expect("rounded outward shell");
    let expected = 600.0 + 30.0 * std::f64::consts::PI + 4.0 * std::f64::consts::PI / 3.0;
    // Curved patch mass properties use numerical quadrature.
    assert!((shell.volume() - expected).abs() < expected * 1.0e-5);
    assert_eq!(
        shell
            .faces()
            .filter(|face| matches!(face.surface(), Surface3::Sphere(_)))
            .count(),
        8
    );
    shell.tessellate(TessellationOptions::default()).unwrap();
}

#[test]
fn rounded_shell_joins_open_and_disconnected_boxes() {
    let cube = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0));
    let opening = cube
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Plane(p) if p.origin().z > 9.9))
        .unwrap()
        .clone();
    let open = cube.shell(&[opening.clone()], 1.0).unwrap();
    assert_eq!(open.solids().len(), 1);
    open.tessellate(TessellationOptions::default()).unwrap();
    let other = owned_cuboid(Point3::new(30.0, 0.0, 0.0), Point3::new(40.0, 10.0, 10.0));
    let combined = cube.combined(&[other]).unwrap();
    for mode in ["sharp", "rounded"] {
        let result = combined
            .shell_with_join(&[opening.clone()], 1.0, mode)
            .unwrap();
        assert_eq!(result.solids().len(), 2);
        result.tessellate(TessellationOptions::default()).unwrap();
    }
    assert!(cube.shell_with_join(&[], 1.0, "invalid").is_err());
}

#[test]
fn rounded_shell_joins_circle_and_plane_in_both_directions() {
    let tolerance = ModelTolerance::default();
    let polygon = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-110.53131546434899, 0.0),
                Point2::new(45.77629428845431, 0.0),
                Point2::new(262.1306769872058, 187.68455648216442),
                Point2::new(-110.53131546434899, 187.68455648216442),
            ],
            tolerance,
        )
        .unwrap(),
        [],
        tolerance,
    )
    .unwrap();
    let circle = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(
                Point2::new(-1.8302127961054384, 6.925129498777356),
                48.10755590722806,
            )
            .unwrap()],
            tolerance,
        )
        .unwrap(),
        [],
        tolerance,
    )
    .unwrap();
    let extrusion = Vec3::new(0.0, 0.0, 60.0);
    let source = polygon
        .extrude(Plane3::xy(), extrusion)
        .unwrap()
        .union(&circle.extrude(Plane3::xy(), extrusion).unwrap())
        .unwrap();
    let corner = source
        .edges()
        .find(|edge| {
            edge.endpoints().is_ok_and(|points| {
                points
                    .iter()
                    .all(|p| (p.x - 262.1306769872058).abs() < 1.0e-6)
            })
        })
        .unwrap()
        .clone();
    let source = source.fillet(&[corner], 56.0).unwrap();
    let opening = source.faces().find(|face|
        matches!(face.surface(), Surface3::Plane(p) if p.normal().z.abs() > 0.9 && p.origin().z.abs() < 1.0e-6)
    ).unwrap().clone();
    let sharp = source
        .shell_with_join(&[opening.clone()], -9.0, "sharp")
        .unwrap();
    for thickness in [1.0, 3.0, 9.0] {
        let outward = source
            .shell(&[opening.clone()], thickness)
            .expect("outward rounded fixture");
        let sharp_outward = source
            .shell_with_join(&[opening.clone()], thickness, "sharp")
            .unwrap();
        assert_eq!(outward.solids().len(), 1);
        assert!(outward.volume() > 0.0 && outward.volume() < sharp_outward.volume());
        outward.tessellate(TessellationOptions::default()).unwrap();
    }
    source
        .shell(&[], 9.0)
        .expect("closed outward rounded fixture")
        .tessellate(TessellationOptions::default())
        .unwrap();
    let rounded = source.shell(&[opening], -9.0).unwrap();
    assert!(rounded.faces().count() > sharp.faces().count());
    assert!(rounded.volume() < sharp.volume());
    assert!(rounded.faces().any(|face| match face.surface() {
        Surface3::Cylinder(c) => (c.radius() - 9.0).abs() < 1.0e-6,
        Surface3::RollingBall(s) => (s.radius() - 9.0).abs() < 1.0e-6,
        _ => false,
    }));
    for face in rounded.faces() {
        let Surface3::RollingBall(join) = face.surface() else {
            continue;
        };
        if (join.radius() - 9.0).abs() > 1.0e-6 {
            continue;
        }
        for fraction in [0.25, 0.5, 0.75] {
            let range = join.center_range();
            let center = join
                .center_curve()
                .evaluate(range.start + fraction * range.span())
                .unwrap()
                .point;
            assert!(
                source.edges().any(|edge| edge
                    .curve()
                    .project(center, tolerance)
                    .is_ok_and(|projection| projection.distance < 1.0e-6)),
                "join center must lie on the original edge"
            );
        }
    }
    rounded.tessellate(TessellationOptions::default()).unwrap();
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
fn face_offset_adjusts_a_cylinder_fillet_for_selected_supports() {
    let tolerance = ModelTolerance::default();
    let profile = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 10.0).unwrap()],
            tolerance,
        )
        .unwrap(),
        [],
        tolerance,
    )
    .unwrap();
    let oblique =
        Plane3::from_origin_normal(Point3::new(4.0, -2.0, 7.0), Vec3::new(1.0, 2.0, 3.0)).unwrap();
    for (plane, axis) in [
        (Plane3::xy(), Vec3::unit_z()),
        (Plane3::xy(), -Vec3::unit_z()),
        (oblique, oblique.normal()),
    ] {
        let height = |point: Point3| (point - plane.origin()).dot(axis);
        let cylinder = profile.extrude(plane, axis * 20.0).unwrap();
        let top_edge = cylinder
            .edges()
            .find(|edge| {
                edge.endpoints()
                    .unwrap()
                    .iter()
                    .all(|point| height(*point) > 19.9)
            })
            .unwrap()
            .clone();
        let rounded = cylinder.fillet(&[top_edge], 2.0).unwrap();
        for (body, is_rounded) in [(&cylinder, false), (&rounded, true)] {
            let side = body
                .faces()
                .find(|face| matches!(face.surface(), Surface3::Cylinder(_)))
                .unwrap()
                .clone();
            let top = body
                .faces()
                .find(|face| matches!(face.surface(), Surface3::Plane(cap) if height(cap.origin()) > 19.9))
                .unwrap()
                .clone();
            for (move_side, move_top, move_fillet) in [
                (true, false, false),
                (false, true, false),
                (true, true, false),
                (true, true, true),
            ] {
                for distance in [-1.0, 1.0] {
                    let mut selected = Vec::new();
                    if move_side {
                        selected.push(side.clone());
                    }
                    if move_top {
                        selected.push(top.clone());
                    }
                    if move_fillet {
                        selected.extend(
                            body.faces()
                                .filter(|face| matches!(face.surface(), Surface3::Torus(_)))
                                .cloned(),
                        );
                    }
                    let side_offset = if move_side { distance } else { 0.0 };
                    let top_offset = if move_top { distance } else { 0.0 };
                    let result = body.offset_faces(&selected, distance).unwrap_or_else(|error| panic!(
                    "axis={axis:?}, rounded={is_rounded}, side={move_side}, top={move_top}, fillet={move_fillet}, distance={distance}: {error}"
                ));
                    assert_eq!(result.solids().len(), 1);
                    assert_eq!(result.faces().count(), body.faces().count());
                    let mut cap_heights = result
                        .faces()
                        .filter_map(|face| match face.surface() {
                            Surface3::Plane(plane) => Some(height(plane.origin())),
                            Surface3::Cylinder(cylinder) => {
                                assert!((cylinder.radius() - (10.0 + side_offset)).abs() < 1e-7);
                                None
                            }
                            Surface3::Torus(torus) => {
                                assert!((torus.minor_radius() - (2.0 + side_offset)).abs() < 1e-7);
                                assert!((torus.major_radius() - 8.0).abs() < 1e-7);
                                assert!(
                                    (height(torus.center()) - (18.0 + top_offset - side_offset))
                                        .abs()
                                        < 1e-7
                                );
                                None
                            }
                            _ => panic!("unexpected offset support"),
                        })
                        .collect::<Vec<_>>();
                    cap_heights.sort_by(f64::total_cmp);
                    // The bottom stays fixed and the top moves only when selected.
                    assert!((cap_heights[0]).abs() < 1e-7);
                    assert!((cap_heights[1] - (20.0 + top_offset)).abs() < 1e-7);
                    let radius = 10.0 + side_offset;
                    let fillet = 2.0 + side_offset;
                    let pi = std::f64::consts::PI;
                    let expected = if is_rounded {
                        pi * (radius.powi(2) * (20.0 + top_offset - fillet)
                            + 64.0 * fillet
                            + 4.0 * pi * fillet.powi(2)
                            + 2.0 * fillet.powi(3) / 3.0)
                    } else {
                        pi * radius.powi(2) * (20.0 + top_offset)
                    };
                    assert!(
                    (result.volume() - expected).abs() < expected * 1e-8,
                    "axis={axis:?}, rounded={is_rounded}, offset={distance}: volume={}, expected={expected}",
                    result.volume()
                );
                    result.tessellate(TessellationOptions::default()).unwrap();
                }
            }
            if is_rounded {
                assert!(body.offset_faces(&[side.clone()], -2.0).is_err());
                assert!(body.offset_faces(&[side], -3.0).is_err());
            }
        }
    }
}

#[test]
fn face_offset_adjusts_a_fillet_between_planar_supports() {
    let block = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let edge = block
        .edges()
        .find(|edge| {
            edge.endpoints()
                .unwrap()
                .iter()
                .all(|point| point.x.abs() < 1e-7 && point.y.abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let rounded = block.fillet(&[edge], 1.0).unwrap();
    let x_face = rounded
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(plane)
        if plane.normal().x.abs() > 0.9 && plane.origin().x.abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let y_face = rounded
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(plane)
        if plane.normal().y.abs() > 0.9 && plane.origin().y.abs() < 1e-7)
        })
        .unwrap()
        .clone();
    for both in [false, true] {
        for distance in [-0.25, 0.25] {
            let selected = if both {
                vec![x_face.clone(), y_face.clone()]
            } else {
                vec![x_face.clone()]
            };
            let result = rounded.offset_faces(&selected, distance).unwrap();
            assert_eq!(result.solids().len(), 1);
            assert_eq!(result.faces().count(), rounded.faces().count());
            let expected = (20.0 + distance) * (10.0 + if both { distance } else { 0.0 }) * 5.0
                - (1.0 - std::f64::consts::PI / 4.0) * 5.0;
            assert!((result.volume() - expected).abs() < expected * 1e-8);
            result.tessellate(TessellationOptions::default()).unwrap();
        }
    }
}

#[test]
fn move_faces_retrims_planar_neighbors_and_preserves_history() {
    let tolerance = ModelTolerance::default();
    let body = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let top = body
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().z.abs() > 0.9 && (p.origin().z - 5.0).abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let top_edge = body
        .edges()
        .find(|edge| {
            edge.endpoints()
                .unwrap()
                .iter()
                .all(|p| (p.z - 5.0).abs() < 1e-7)
        })
        .unwrap()
        .clone();
    for angle in [0.0, 0.1, -0.1] {
        let rotation = Matrix3::from_angle_y(cgmath::Rad(angle));
        let pivot = Vec3::new(10.0, 5.0, 5.0);
        let motion = RigidTransform3::new(
            rotation,
            pivot - rotation * pivot + Vec3::unit_z() * 2.0,
            tolerance,
        )
        .unwrap();
        let result = body.move_faces(&[top.clone()], motion).unwrap();
        assert_eq!(result.faces().count(), 6);
        assert!((result.volume() - 1400.0).abs() < 1e-5);
        assert!(matches!(
            result.resolve(&top),
            Resolution::Preserved(_) | Resolution::Modified(_)
        ));
        assert!(matches!(
            result.resolve(&top_edge),
            Resolution::Preserved(_) | Resolution::Modified(_)
        ));
        result.tessellate(TessellationOptions::default()).unwrap();
    }
    assert!((body.volume() - 1000.0).abs() < 1e-7);
    let side = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(p)
        if p.normal().x.abs() > 0.9 && p.origin().x.abs() < 1e-7)).unwrap().clone();
    let together = body.move_faces(&[top.clone(), side], RigidTransform3::new(
        Matrix3::identity(), Vec3::new(-2.0, 0.0, 2.0), tolerance,
    ).unwrap()).unwrap();
    assert!((together.volume() - 1540.0).abs() < 1e-6);
    assert!(body
        .move_faces(
            &[top.clone()],
            RigidTransform3::new(Matrix3::identity(), -Vec3::unit_z() * 6.0, tolerance).unwrap()
        )
        .is_err());
    assert!(body.move_faces(&[], RigidTransform3::identity()).is_err());
    assert!(body
        .move_faces(&[top.clone(), top.clone()], RigidTransform3::identity())
        .is_err());
    let other = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    assert!(other
        .move_faces(&[top], RigidTransform3::identity())
        .is_err());
}

#[test]
fn move_faces_preserves_fillet_radius_when_translating_and_rotating_a_support() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let edge = block
        .edges()
        .find(|edge| {
            edge.endpoints()
                .unwrap()
                .iter()
                .all(|point| point.x.abs() < 1e-7 && point.y.abs() < 1e-7)
        })
        .unwrap()
        .clone();
    let rounded = block.fillet(&[edge], 1.0).unwrap();
    let side = rounded
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().x.abs() > 0.9 && p.origin().x.abs() < 1e-7)
        })
        .unwrap()
        .clone();
    for angle in [0.0, 0.1, -0.1] {
        let motion = RigidTransform3::new(
            Matrix3::from_angle_z(cgmath::Rad(angle)),
            Vec3::new(-0.25, 0.0, 0.0),
            tolerance,
        )
        .unwrap();
        let result = rounded
            .move_faces(&[side.clone()], motion)
            .unwrap_or_else(|e| panic!("angle {angle}: {e}"));
        assert_eq!(result.faces().count(), rounded.faces().count());
        let blend = result
            .faces()
            .find_map(|face| match face.surface() {
                Surface3::Cylinder(cylinder) => Some(*cylinder),
                _ => None,
            })
            .unwrap();
        assert!((blend.radius() - 1.0).abs() < 1e-9);
        if angle == 0.0 {
            let expected = 20.25 * 10.0 * 5.0 - (1.0 - std::f64::consts::PI / 4.0) * 5.0;
            assert!((result.volume() - expected).abs() < 1e-5);
        }
        result.tessellate(TessellationOptions::default()).unwrap();
    }
}

#[test]
fn move_faces_resizes_round_slot_ends_between_parallel_walls() {
    let tolerance = ModelTolerance::default();
    let capsule = |radius: f64| {
        Wire2::new([
            TrimmedCurve2::line(Point2::new(0.0, -radius), Point2::new(20.0, -radius)).unwrap(),
            TrimmedCurve2::arc_through(
                Point2::new(20.0, -radius), Point2::new(20.0 + radius, 0.0),
                Point2::new(20.0, radius),
            ).unwrap(),
            TrimmedCurve2::line(Point2::new(20.0, radius), Point2::new(0.0, radius)).unwrap(),
            TrimmedCurve2::arc_through(
                Point2::new(0.0, radius), Point2::new(-radius, 0.0),
                Point2::new(0.0, -radius),
            ).unwrap(),
        ], tolerance).unwrap()
    };
    let body = Profile::new(capsule(5.0), [capsule(2.0)], tolerance).unwrap()
        .extrude(Plane3::xz(), -Vec3::unit_y() * 4.0).unwrap();
    let wall = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
        if plane.normal().z.abs() > 0.9 && (plane.origin().z - 2.0).abs() < 1e-7))
        .unwrap().clone();
    for distance in [1.305, -1.305] {
        let motion = RigidTransform3::new(Matrix3::identity(), Vec3::unit_z() * distance, tolerance).unwrap();
        let result = body.move_faces(&[wall.clone()], motion).unwrap();
        let radius = 2.0 + distance / 2.0;
        let expected = (200.0 + std::f64::consts::PI * 25.0
            - 40.0 * radius - std::f64::consts::PI * radius * radius) * 4.0;
        assert!((result.volume() - expected).abs() < 1e-5);
        assert_eq!(result.faces().count(), body.faces().count());
        assert_eq!(result.faces().filter(|face| matches!(face.surface(), Surface3::Cylinder(cylinder)
            if (cylinder.radius() - radius).abs() < 1e-7
                && (cylinder.origin().z - distance / 2.0).abs() < 1e-7)).count(), 2);
        result.tessellate(TessellationOptions::default()).unwrap();
    }
    assert!(body.move_faces(&[wall], RigidTransform3::new(
        Matrix3::identity(), -Vec3::unit_z() * 4.1, tolerance,
    ).unwrap()).is_err());
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
    let at_height = |edge: &super::EdgeHandle, heights: [f64; 2]| {
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
    let bounds = super::bounds::compound(&rounded).unwrap();
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
    let retained = rounded.split(section, "left").unwrap();
    assert_eq!(retained.solids().len(), 1);
    assert!((retained.volume() - rounded.volume()).abs() < rounded.volume() * 1e-8);
    assert_eq!(rounded.split(section, "right").unwrap().solids().len(), 0);
}

#[test]
fn move_faces_extends_cylinder_and_its_rounded_cap() {
    let tolerance = ModelTolerance::default();
    let cylinder = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 5.0).unwrap()],
            tolerance,
        )
        .unwrap(),
        [],
        tolerance,
    )
    .unwrap()
    .extrude(Plane3::xy(), Vec3::unit_z() * 10.0)
    .unwrap();
    for rounded in [0, 1, 2] {
        let body = if rounded > 0 {
            let edges = cylinder
                .edges()
                .filter(|edge| {
                    matches!(edge.curve(), Curve3::Circle(_))
                        && (rounded == 2
                            || edge
                                .endpoints()
                                .unwrap()
                                .iter()
                                .all(|p| (p.z - 10.0).abs() < 1e-7))
                })
                .cloned()
                .collect::<Vec<_>>();
            cylinder.fillet(&edges, 1.0).unwrap()
        } else {
            cylinder.clone()
        };
        let cap = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(p) if (p.origin().z - 10.0).abs() < 1e-7)).unwrap().clone();
        let result = body
            .move_faces(
                &[cap],
                RigidTransform3::new(Matrix3::identity(), Vec3::unit_z() * 3.0, tolerance).unwrap(),
            )
            .unwrap();
        assert_eq!(result.faces().count(), body.faces().count());
        assert!((result.volume() - body.volume() - 75.0 * std::f64::consts::PI).abs() < 1e-5);
        result.tessellate(TessellationOptions::default()).unwrap();
    }
}

#[test]
fn move_faces_updates_connected_corner_fillets() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let edges = block.edges().cloned().collect::<Vec<_>>();
    let rounded = block.fillet(&edges, 0.5).unwrap();
    let side = rounded
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().x.abs() > 0.9 && p.origin().x.abs() < 1e-7)
        })
        .unwrap()
        .clone();
    for angle in [0.0, 0.08, -0.08] {
        let motion = RigidTransform3::new(
            Matrix3::from_angle_z(cgmath::Rad(angle)),
            -Vec3::unit_x(),
            tolerance,
        )
        .unwrap();
        let result = rounded
            .move_faces(&[side.clone()], motion)
            .unwrap_or_else(|e| panic!("angle {angle}: {e}"));
        assert_eq!(result.faces().count(), rounded.faces().count());
        assert!(result.volume() > rounded.volume());
        result.tessellate(TessellationOptions::default()).unwrap();
    }
}

#[test]
fn move_faces_handles_whole_bodies_and_multiple_solids() {
    let tolerance = ModelTolerance::default();
    let first = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 2.0, 2.0));
    let second = owned_cuboid(Point3::new(10.0, 0.0, 0.0), Point3::new(12.0, 2.0, 2.0));
    let body = first.combined(&[second]).unwrap();
    let selected = body
        .solids()
        .next()
        .unwrap()
        .faces()
        .cloned()
        .collect::<Vec<_>>();
    let motion = RigidTransform3::new(
        Matrix3::from_angle_z(cgmath::Rad(2.0)),
        Vec3::new(0.0, 5.0, 0.0),
        tolerance,
    )
    .unwrap();
    let moved = body.move_faces(&selected, motion).unwrap();
    assert_eq!(moved.solids().len(), 2);
    assert!((moved.volume() - body.volume()).abs() < 1e-7);
    moved.tessellate(TessellationOptions::default()).unwrap();
}

#[test]
fn move_faces_relocates_a_hole_and_rejects_crossing_the_boundary() {
    let tolerance = ModelTolerance::default();
    let profile = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-10.0, -10.0),
                Point2::new(10.0, -10.0),
                Point2::new(10.0, 10.0),
                Point2::new(-10.0, 10.0),
            ],
            tolerance,
        )
        .unwrap(),
        [Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 2.0).unwrap()],
            tolerance,
        )
        .unwrap()],
        tolerance,
    )
    .unwrap();
    let body = profile.extrude(Plane3::xy(), Vec3::unit_z() * 5.0).unwrap();
    let wall = body
        .faces()
        .find(|face| matches!(face.surface(), Surface3::Cylinder(_)))
        .unwrap()
        .clone();
    let motion =
        |x| RigidTransform3::new(Matrix3::identity(), Vec3::new(x, 0.0, 0.0), tolerance).unwrap();
    let moved = body.move_faces(&[wall.clone()], motion(3.0)).unwrap();
    assert!((moved.volume() - body.volume()).abs() < 1e-6);
    assert!(moved.faces().any(|face| matches!(face.surface(), Surface3::Cylinder(cylinder) if (cylinder.origin().x - 3.0).abs() < 1e-7)));
    moved.tessellate(TessellationOptions::default()).unwrap();
    assert!(body.move_faces(&[wall], motion(9.0)).is_err());
    assert!((body.volume() - (400.0 - 4.0 * std::f64::consts::PI) * 5.0).abs() < 1e-6);
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
    let outward = cylinder
        .shell_with_join(&[], 1.0, "sharp")
        .expect("outward sharp cylinder shell");
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
    // References were checked by integrating in both UV directions with
    // quadrature targets reduced by 1e6 (agreement within 6e-6 in volume).
    // Production integration uses scale-aware error budgets, so compare to
    // those converged values at 1e-7 relative accuracy rather than requiring
    // the rounding of one particular integration direction.
    for (radius, volume, area) in [
        (0.1, 94578.79209313872, 13670.747348368352),
        (0.7, 94579.5977752258, 13668.850180018275),
    ] {
        let result = body.fillet(&[short.clone()], radius).unwrap();
        assert_eq!(result.solids().len(), 1);
        assert_eq!(result.faces().count(), body.faces().count() + 1);
        let solid = result.solids().next().unwrap();
        assert!((solid.volume() - volume).abs() < volume * 1e-7);
        assert!((solid.surface_area() - area).abs() < area * 1e-7);
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

#[test]
fn join_after_shell_opening_blends() {
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
    let rim = cut.edges().find(|edge| edge.connected_faces().unwrap().iter().any(|face| matches!(face.surface(), Surface3::Torus(t) if (t.minor_radius()-12.0).abs()<tolerance.linear)) && edge.connected_faces().unwrap().iter().any(|face| matches!(face.surface(), Surface3::Cylinder(_)))).unwrap().clone();
    let body = cut
        .chamfer_with_tangent_propagation(&[rim], 0.5, true)
        .unwrap();
    let inner = body
        .edges()
        .find(|edge| {
            matches!(edge.curve(), Curve3::Line(_))
                && edge
                    .connected_faces()
                    .unwrap()
                    .iter()
                    .all(|face| matches!(face.surface(), Surface3::Cylinder(_)))
        })
        .unwrap()
        .clone();
    let rounded = body.fillet(&[inner], 0.5).unwrap();
    for distance in [
        0.5, 1.0, 5.0, 20.0, 33.0, 40.0, -0.5, -1.0, -5.0, -20.0, -33.0, -40.0,
    ] {
        let tool = circle(
            Point2::new(21.27550542650958, -8.266006903113285),
            28.817039202349168,
        )
        .extrude(plane(33.0), Vec3::new(distance, 0.0, 0.0))
        .unwrap();
        let joined = rounded
            .union(&tool)
            .unwrap_or_else(|error| panic!("join at {distance} mm: {error}"));
        assert_eq!(joined.solids().len(), 1);
        assert!(joined.volume() > rounded.volume());
        if distance > 0.0 {
            // Curved-face quadrature uses a scale-dependent error target;
            // this is a volume sanity check, not an exact integration test.
            let expected = rounded.volume() + tool.volume();
            assert!(
                (joined.volume() - expected).abs() < expected * 1e-5,
                "distance {distance}: volume delta {}",
                joined.volume() - expected
            );
        } else {
            assert!(joined.volume() < rounded.volume() + tool.volume());
        }
        joined
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap();
    }
}

#[test]
fn rounded_partial_torus_rim_blends() {
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
        .filter(|e| matches!(e.curve(), Curve3::Line(_)))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 2);
    let body = body.fillet(&edges, 13.0).unwrap();
    let rims = body
        .edges()
        .filter(|e| {
            matches!(e.curve(), Curve3::Circle(_))
                && e.connected_faces()
                    .unwrap()
                    .iter()
                    .any(|f| matches!(f.surface(), Surface3::Plane(_)))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(rims.len(), 2);
    for edge in rims {
        for size in [0.5, 1.0, 5.0] {
            for result in [
                body.fillet(&[edge.clone()], size)
                    .expect("rounded torus fillet"),
                body.chamfer_with_tangent_propagation(&[edge.clone()], size, true)
                    .expect("rounded torus chamfer"),
            ] {
                assert_eq!(result.solids().len(), 1);
                assert!(result.volume() > 0.0 && result.volume() < body.volume());
                result
                    .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                    .unwrap();
            }
        }
    }
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

#[test]
fn shell_rounded_chamfered_torus() {
    let body = rounded_chamfered_torus(false);
    let planar = body
        .faces()
        .filter(|face| matches!(face.surface(), Surface3::Plane(_)))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(planar.len(), 2);
    for thickness in [1.0, 0.5, 2.0] {
        for openings in [vec![], vec![planar[0].clone()], vec![planar[1].clone()]] {
            let result = body
                .shell_with_join(&openings, -thickness, "sharp")
                .unwrap_or_else(|error| {
                    panic!("shell {thickness}, {} openings: {error:?}", openings.len())
                });
            assert_eq!(result.solids().len(), 1);
            assert!(result.volume() > 0.0 && result.volume() < body.volume());
            result
                .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                .unwrap();
        }
    }
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

#[test]
fn torus_sections_preserve_exact_circles_in_scaled_and_skewed_plane_charts() {
    let tol = ModelTolerance::default();
    let torus: Surface3 = super::Torus3::new(
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
            let sections = super::intersection::parallel_plane_torus_section(*p, *t, tol).unwrap();
            let sections = if matches!(first, Surface3::Plane(_)) {
                sections
            } else {
                super::intersection::swap_surface_curves(sections)
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
                                super::euler::BrepPatchSegment {
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
                    segments.push(super::euler::BrepPatchSegment {
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

#[test]
fn fillet_circular_cut_on_offset_rounded_shell() {
    let (cut, rim) = circular_cut_on_offset_rounded_shell();
    let volume = cut.volume();
    for radius in [0.01, 0.05, 0.1, 0.3, 0.5, 0.6, 0.8, 1.0, 1.2, 1.4, 1.49] {
        let result = cut
            .fillet(std::slice::from_ref(&rim), radius)
            .unwrap_or_else(|error| panic!("radius {radius}: {error}"));
        assert_eq!(result.solids().len(), 1);
        assert_eq!(result.faces().count(), cut.faces().count() + 1);
        assert!(result.volume() > 0.0 && result.volume() < volume * (1.0 + 1e-5));
        let mesh = result
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap_or_else(|error| panic!("radius {radius} mesh: {error}"));
        assert!(!mesh.triangles.is_empty());
    }
    for radius in [1.5, 1.6, 2.0] {
        assert!(cut.fillet(std::slice::from_ref(&rim), radius).is_err());
    }
}

#[test]
fn chamfer_circular_cut_on_offset_rounded_shell() {
    let (cut, rim) = circular_cut_on_offset_rounded_shell();
    let volume = cut.volume();
    for distance in [
        0.01, 0.05, 0.1, 0.3, 0.5, 0.6, 0.8, 1.0, 1.5, 2.0, 3.0, 3.5, 3.9,
    ] {
        let result = cut
            .chamfer_with_tangent_propagation(std::slice::from_ref(&rim), distance, true)
            .unwrap_or_else(|error| panic!("distance {distance}: {error}"));
        assert_eq!(result.solids().len(), 1);
        assert_eq!(result.faces().count(), cut.faces().count() + 1);
        // The volume integrator's error can exceed the material removed by
        // the smallest setbacks; allow that small relative integration error.
        assert!(result.volume() > 0.0 && result.volume() < volume * (1.0 + 1e-5));
        let mesh = result
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap_or_else(|error| panic!("distance {distance} mesh: {error}"));
        assert!(!mesh.triangles.is_empty());
    }
    for distance in [4.0, 5.0] {
        assert!(cut
            .chamfer_with_tangent_propagation(std::slice::from_ref(&rim), distance, true)
            .is_err());
    }
}

#[test]
fn small_fillet_on_rounded_torus_cutout() {
    let tol = ModelTolerance::default();
    let body = rounded_chamfered_torus(true);
    let opening = body
        .faces()
        .find(|f| matches!(f.surface(), Surface3::Plane(_)))
        .unwrap()
        .clone();
    let shell = body.shell(&[opening], -4.0).unwrap();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        -Vec3::unit_z(),
        Vec3::unit_y(),
        tol,
    )
    .unwrap();
    let tool = Profile::new(
        Wire2::polygon(
            [
                Point2::new(42.11427381659291, -12.144015041766547),
                Point2::new(66.12781618720352, -12.144015041766547),
                Point2::new(66.12781618720352, 12.361608115127035),
                Point2::new(42.11427381659291, 12.361608115127035),
            ],
            tol,
        )
        .unwrap(),
        [],
        tol,
    )
    .unwrap()
    .extrude(plane, Vec3::new(-60.0, 0.0, 0.0))
    .unwrap();
    let cut = shell.difference(&tool).unwrap();
    let corners = cut
        .edges()
        .filter(|edge| {
            let points = edge.endpoints().unwrap();
            let p = points[0] + (points[1] - points[0]) * 0.5;
            p.x < -35.0
                && edge
                    .connected_faces()
                    .unwrap()
                    .iter()
                    .all(|face| matches!(face.surface(), Surface3::Plane(_)))
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(corners.len(), 4);
    let rounded = cut.fillet(&corners, 4.0).unwrap();
    let rim = rounded
        .edges()
        .find(|edge| {
            let faces = edge.connected_faces().unwrap();
            let points = edge.endpoints().unwrap();
            let p = points[0] + (points[1] - points[0]) * 0.5;
            p.x < -59.9
                && p.z < -55.0
                && faces
                    .iter()
                    .any(|f| matches!(f.surface(), Surface3::Torus(_)))
        })
        .unwrap()
        .clone();
    for radius in [0.05, 0.1, 0.15, 0.2, 0.25] {
        let result = rounded
            .fillet(std::slice::from_ref(&rim), radius)
            .unwrap_or_else(|e| panic!("radius {radius}: {e}"));
        assert_eq!(result.solids().len(), 1);
        assert!((result.volume() / rounded.volume() - 1.0).abs() < 0.001);
        assert_eq!(result.faces().count(), rounded.faces().count() + 8);
        let mesh = result
            .tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap())
            .unwrap_or_else(|e| panic!("radius {radius} tessellation: {e}"));
        assert!(!mesh.triangles.is_empty());
    }
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

#[test]
fn transverse_circle_on_open_torus_shell() {
    let tol = ModelTolerance::default();
    let body = rounded_chamfered_torus(true);
    let opening = body
        .faces()
        .find(|f| matches!(f.surface(), Surface3::Plane(_)))
        .unwrap()
        .clone();
    let shell = body.shell(&[opening], -3.0).unwrap();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        -Vec3::unit_z(),
        tol,
    )
    .unwrap();
    let circle = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(
                Point2::new(12.754345842186886, 9.817011016391483),
                43.44134694370669,
            )
            .unwrap()],
            tol,
        )
        .unwrap(),
        [],
        tol,
    )
    .unwrap();
    for distance in [1.0, -1.0, 5.0, -5.0, 20.0, -20.0] {
        let tool = circle.extrude(plane, Vec3::unit_y() * distance).unwrap();
        for cut in [false, true] {
            let result = if cut {
                shell.difference(&tool)
            } else {
                shell.union(&tool)
            }
            .unwrap_or_else(|error| panic!("distance {distance}, cut {cut}: {error}"));
            assert_eq!(result.solids().len(), 1);
            assert!(if cut {
                result.volume() < shell.volume()
            } else {
                result.volume() > shell.volume()
            });
            result
                .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                .unwrap();
        }
    }
}
