//! Persistent names, operation history and entity resolution.

use super::*;

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
fn history_does_not_accumulate_tolerance_as_unchanged_motion() {
    let tolerance = ModelTolerance::default();
    let cube = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let motion = RigidTransform3::new(
        Matrix3::identity(),
        Vec3::unit_z() * tolerance.linear * 0.75,
        tolerance,
    )
    .unwrap();
    let once = cube.transformed(motion).unwrap();
    let twice = once.transformed(motion).unwrap();
    for face in cube.faces() {
        assert!(matches!(once.resolve(face), Resolution::Modified(_)));
        assert!(matches!(twice.resolve(face), Resolution::Modified(_)));
    }
    let identity = twice.transformed(RigidTransform3::identity()).unwrap();
    for face in twice.faces() {
        assert!(matches!(identity.resolve(face), Resolution::Preserved(_)));
    }
    for face in cube.faces() {
        assert!(matches!(identity.resolve(face), Resolution::Modified(_)));
    }
}

#[test]
fn unchanged_history_distinguishes_section_caps_from_existing_faces() {
    let cube = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let pocket = owned_cuboid(Point3::new(3.0, 3.0, 5.0), Point3::new(7.0, 7.0, 12.0));
    let pocketed = cube.difference(&pocket).unwrap();
    let floor = pocketed
        .faces()
        .find(|f| {
            matches!(f.surface(), Surface3::Plane(p)
        if p.normal().z.abs() > 0.9 && (p.origin().z - 5.0).abs() < 1e-7)
        })
        .unwrap();
    let bottom = pocketed
        .faces()
        .find(|f| {
            matches!(f.surface(), Surface3::Plane(p)
        if p.normal().z.abs() > 0.9 && p.origin().z.abs() < 1e-7)
        })
        .unwrap();
    let section = Plane3::from_origin_normal(Point3::new(0.0, 0.0, 5.0), Vec3::unit_z()).unwrap();
    let cut = pocketed.split(section, SplitKeep::Left).unwrap();
    assert!(
        matches!(cut.resolve(floor), Resolution::Preserved(_)),
        "coincident floor remains separate from the cut cap"
    );
    assert!(
        matches!(cut.resolve(bottom), Resolution::Preserved(_)),
        "bottom is untouched"
    );
    for face in cut.faces() {
        assert!(
            matches!(cut.resolve(face), Resolution::Preserved(_)),
            "current revision resolves unchanged"
        );
    }
    let untouched = cut.split(section, SplitKeep::Left).unwrap();
    for face in cut.faces() {
        assert!(
            matches!(untouched.resolve(face), Resolution::Preserved(_)),
            "repeated section is unchanged"
        );
    }
    assert!(
        matches!(untouched.resolve(floor), Resolution::Preserved(_)),
        "older floor stays preserved"
    );
    assert!(
        matches!(untouched.resolve(bottom), Resolution::Preserved(_)),
        "unchanged ancestry is transitive"
    );
}

#[test]
fn construction_history_does_not_invent_ancestry_for_coincident_geometry() {
    let source = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let independently_built = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let result = independently_built
        .with_operation_parents(&[&source], crate::DerivationRole::BrepFace)
        .unwrap();
    for face in source.faces() {
        assert!(matches!(result.resolve(face), Resolution::Deleted));
    }
    for edge in source.edges() {
        assert!(matches!(result.resolve(edge), Resolution::Deleted));
    }
    for vertex in source.vertices() {
        assert!(matches!(result.resolve(vertex), Resolution::Deleted));
    }
}

#[test]
fn construction_history_preserves_reparameterized_boundaries() {
    let source = owned_cuboid(Point3::origin(), Point3::new(10.0, 12.0, 14.0));
    let mut patches = source
        .faces()
        .map(|face| {
            crate::features::face_patch(source.topology(), face.topology_id(), false).unwrap()
        })
        .collect::<Vec<_>>();
    for segment in patches
        .iter_mut()
        .flat_map(|p| &mut p.loops)
        .flat_map(|l| &mut l.segments)
    {
        let Curve3::Line(line) = &segment.curve else {
            unreachable!()
        };
        segment.curve = Line3::new(line.origin(), line.direction() / 3.0)
            .unwrap()
            .into();
        let Curve2::Line(line) = &segment.pcurve else {
            unreachable!()
        };
        segment.pcurve = crate::Line2::new(line.origin(), line.direction() / 3.0)
            .unwrap()
            .into();
        segment.range =
            ParameterRange::new(segment.range.start * 3.0, segment.range.end * 3.0).unwrap();
    }
    let mut kernel = Kernel::new(source.tolerance()).unwrap();
    let mut operation = kernel.begin_operation();
    operation.brep_shell(&patches).unwrap();
    let committed = operation.finish().unwrap();
    let result = crate::Compound::from_operation_with_parents(kernel, committed, &[&source], crate::DerivationRole::BrepFace)
        .unwrap();
    for face in source.faces() {
        assert!(matches!(result.resolve(face), Resolution::Preserved(_)));
    }
    for edge in source.edges() {
        assert!(matches!(result.resolve(edge), Resolution::Preserved(_)));
    }
    for vertex in source.vertices() {
        assert!(matches!(result.resolve(vertex), Resolution::Preserved(_)));
    }
}

#[test]
fn unchanged_history_covers_modeling_operations() {
    let tolerance = ModelTolerance::default();
    let cube = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let top = cube
        .faces()
        .find(|f| {
            matches!(f.surface(), Surface3::Plane(p)
        if p.normal().z.abs() > 0.9 && (p.origin().z - 10.0).abs() < 1e-7)
        })
        .unwrap();
    let bottom = cube
        .faces()
        .find(|f| {
            matches!(f.surface(), Surface3::Plane(p)
        if p.normal().z.abs() > 0.9 && p.origin().z.abs() < 1e-7)
        })
        .unwrap();
    let edge = cube
        .edges()
        .find(|edge| {
            edge.endpoints()
                .unwrap()
                .iter()
                .all(|point| (point.z - 10.0).abs() < 1e-7)
        })
        .unwrap();
    let translation = RigidTransform3::new(Matrix3::identity(), Vec3::unit_z(), tolerance).unwrap();
    let results = [
        ("fillet", cube.fillet(&[edge.clone()], 1.0).unwrap()),
        ("chamfer", cube.chamfer(&[edge.clone()], 1.0).unwrap()),
        ("offset", cube.offset_faces(&[top.clone()], 1.0).unwrap()),
        (
            "move",
            cube.move_faces(&[top.clone()], translation).unwrap(),
        ),
        (
            "draft",
            cube.draft(&[top.clone()], Plane3::yz(), 0.1).unwrap(),
        ),
    ];
    for (operation, result) in results {
        assert!(
            matches!(result.resolve(bottom), Resolution::Preserved(_)),
            "{operation}: bottom"
        );
        assert!(
            !matches!(result.resolve(top), Resolution::Preserved(_)),
            "{operation}: changed top"
        );
    }
    let distant = owned_cuboid(Point3::new(30.0, 0.0, 0.0), Point3::new(40.0, 10.0, 10.0));
    let joined = cube.union(&distant).unwrap();
    for face in cube.faces().chain(distant.faces()) {
        assert!(
            matches!(joined.resolve(face), Resolution::Preserved(_)),
            "disjoint union"
        );
    }
    for (operation, result) in [
        (
            "zero offset",
            cube.offset_faces(&[top.clone()], 0.0).unwrap(),
        ),
        (
            "zero move",
            cube.move_faces(&[top.clone()], RigidTransform3::identity())
                .unwrap(),
        ),
        (
            "zero draft",
            cube.draft(&[top.clone()], Plane3::yz(), 0.0).unwrap(),
        ),
        ("combine", cube.combined(&[]).unwrap()),
    ] {
        for face in cube.faces() {
            assert_eq!(
                result
                    .topology_history()
                    .change(face.persistent_id())
                    .unwrap()
                    .0,
                crate::ChangeKind::Unchanged,
                "{operation}: explicit unchanged record"
            );
            assert!(
                matches!(result.resolve(face), Resolution::Preserved(_)),
                "{operation}"
            );
        }
    }
    let removed = joined.solids().nth(1).unwrap().clone();
    let kept = joined.solids().next().unwrap();
    let reduced = joined.without_solids(&[removed.clone()]).unwrap();
    for face in kept.faces() {
        assert_eq!(
            reduced
                .topology_history()
                .change(face.persistent_id())
                .unwrap()
                .0,
            crate::ChangeKind::Unchanged
        );
        assert!(matches!(reduced.resolve(face), Resolution::Preserved(_)));
    }
    for face in removed.faces() {
        assert_eq!(
            reduced
                .topology_history()
                .change(face.persistent_id())
                .unwrap()
                .0,
            crate::ChangeKind::Deleted
        );
        assert!(matches!(reduced.resolve(face), Resolution::Deleted));
    }
}

#[test]
fn unchanged_history_is_reported_by_low_level_operations() {
    let tolerance = ModelTolerance::default();
    let mut kernel = Kernel::new(tolerance).unwrap();
    let mut operation = kernel.begin_operation();
    let body = operation
        .sphere(
            Point3::origin(),
            Vec3::unit_x(),
            Vec3::unit_y(),
            Vec3::unit_z(),
        )
        .unwrap();
    operation.commit().unwrap();
    let names = kernel.topology().body_entity_names(body).unwrap();
    let history = kernel
        .transform_body(body, RigidTransform3::identity())
        .unwrap();
    for name in &names {
        assert_eq!(
            history.change(*name).unwrap().0,
            crate::ChangeKind::Unchanged
        );
    }
    let history = kernel.begin_operation().commit().unwrap();
    for name in names {
        assert_eq!(
            history.change(name).unwrap().0,
            crate::ChangeKind::Unchanged
        );
    }
}
