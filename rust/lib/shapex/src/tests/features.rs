//! Shells, face offsets, drafts and other face edits.

use super::*;

// Solved profile and the two fillet operations in cannot-shell.cad.
fn twice_filleted_arrow() -> crate::Compound {
    let tolerance = ModelTolerance::default();
    let points = [
        (-2.9312171977861046, 45.81231271970444),
        (-1.890522698465444, 47.79076855888661),
        (27.91205038939795, 63.82269725795327),
        (44.91441132658731, 65.4919389764288),
        (35.21955057735643, 89.33372170506867),
        (-2.0092509300350017, 62.07210292760956),
        (-5.1908645709734795, 63.68572903162131),
        (-5.1908645709734795, 84.97898606569127),
        (-8.021470417833681, 86.79835241889275),
        (-37.05191770274903, 73.54491436921849),
        (-38.20969991342599, 71.94075241337818),
        (-44.69511187143471, 12.018576827575835),
        (-42.28449223921115, 9.84845030968457),
        (-0.0009748927061039581, 18.98100649952064),
    ].map(|(x, y)| Point2::new(x, y));
    let arcs = std::collections::BTreeMap::from([
        (0, (-0.9430383472867704, 46.02944138761163, 3.2503713870558792, 2.06433154001986)),
        (5, (-3.1908645709734778, 63.68572903162131, -0.93873784253222, -std::f64::consts::PI)),
        (7, (-7.190864570973478, 84.97898606569127, 0.0, 1.9990721032924696)),
        (9, (-36.22131185588883, 71.725548016017, 1.999072103292467, 3.0337817249867047)),
        (11, (-42.70672381389755, 11.803372430214656, 3.033781724986704, 4.925105315606756)),
    ]);
    let segments = (0..points.len()).map(|index| {
        let (start, end) = (points[index], points[(index + 1) % points.len()]);
        match arcs.get(&index) {
            Some(&(x, y, a, b)) => TrimmedCurve2::new(
                crate::Circle2::new(Point2::new(x, y), 2.0000000000000013).unwrap(),
                ParameterRange::new(a, b).unwrap(),
            ).unwrap(),
            None => TrimmedCurve2::line(start, end).unwrap(),
        }
    });
    let plane = Plane3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
    let body = Profile::new(Wire2::new(segments, tolerance).unwrap(), [], tolerance).unwrap()
        .extrude(plane, Vec3::new(0.0, 11.0, 0.0)).unwrap();
    let corners = body.edges().filter(|edge| {
        let [a, b] = edge.endpoints().unwrap();
        (a.y - b.y).abs() > 10.0 && [points[3].x, points[4].x, points[13].x]
            .iter().any(|x| (a.x - x).abs() < tolerance.linear)
    }).cloned().collect::<Vec<_>>();
    assert_eq!(corners.len(), 3);
    let body = body.fillet(&corners, 4.0).unwrap();
    let rim = body.edges().find(|edge| {
        let [a, b] = edge.endpoints().unwrap();
        a.y.abs() < tolerance.linear && b.y.abs() < tolerance.linear
            && [a, b].iter().any(|p| (p.x - points[0].x).abs() < tolerance.linear
                && (p.z + points[0].y).abs() < tolerance.linear)
            && a.distance(b) > 20.0
    }).unwrap().clone();
    body.fillet(&[rim], 1.0).unwrap()
}

#[test]
fn shell_twice_filleted_arrow_without_openings() {
    let body = twice_filleted_arrow();
    assert_eq!(body.faces().count(), 36);
    let mut failures = Vec::new();
    for thickness in [0.5, 0.75, 0.982, 0.983, 0.984, 0.985, 0.987, 0.989, 0.99, 0.993,
        0.997, 0.999, 0.9995, 1.0, 1.01, 1.016, 1.1, 1.5, 1.99, 2.0, 2.01, 2.5, 3.0] {
        for join in [ShellJoin::Rounded, ShellJoin::Sharp] {
            let shell = match body.shell_with_join(&[], -thickness, join) {
                Ok(shell) => shell,
                Err(error) => {
                    failures.push(format!("{thickness} mm {join:?}: {error}"));
                    continue;
                }
            };
            assert_eq!(shell.solids().len(), 1);
            assert_eq!(shell.topology().shells().count(), 2);
            assert!(shell.volume() > 0.0 && shell.volume() < body.volume());
            if let Err(error) = shell.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()) {
                failures.push(format!("{thickness} mm {join:?} tessellation: {error}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "targeted shell timing; run in release mode with --ignored --nocapture --test-threads=1"]
fn benchmark_closed_shell_regressions() {
    let arrow = twice_filleted_arrow();
    let slot = filleted_chamfered_slot_document_51();
    for (name, body, thickness, join) in [
        ("arrow", &arrow, 0.5, ShellJoin::Sharp),
        ("slot", &slot, 6.0, ShellJoin::Rounded),
        ("arrow", &arrow, 0.9995, ShellJoin::Rounded),
        ("arrow", &arrow, 1.016, ShellJoin::Rounded),
        ("arrow", &arrow, 3.0, ShellJoin::Rounded),
    ] {
        let mut construction = Vec::new();
        let mut tessellation = Vec::new();
        for iteration in 0..4 {
            let start = std::time::Instant::now();
            let shell = body.shell_with_join(&[], -thickness, join).unwrap();
            let built = start.elapsed().as_secs_f64();
            let start = std::time::Instant::now();
            shell.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
            let meshed = start.elapsed().as_secs_f64();
            assert_eq!(shell.solids().len(), 1);
            assert!(shell.volume() > 0.0 && shell.volume() < body.volume());
            if iteration > 0 { construction.push(built); tessellation.push(meshed); }
        }
        construction.sort_by(f64::total_cmp);
        tessellation.sort_by(f64::total_cmp);
        println!("{name} {join:?} {thickness} mm: construction {:.6}s, tessellation {:.6}s (medians; 1 warmup, 3 measured)",
            construction[1], tessellation[1]);
    }
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
    let rim = joined.edges().find(|edge| matches!(edge.curve(), Curve3::Circle(circle)
        if circle.center().y.abs() < tol.linear && (circle.radius() - 36.40489826969949).abs() < tol.linear))
        .unwrap().clone();
    // A 4 mm fillet would cross the inner boundary of the 3 mm shell wall.
    assert!(joined.fillet(&[rim.clone()], 4.0).is_err());
    let filleted = joined.fillet(&[rim], 0.25).unwrap();
    let bottom = filleted.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
        if plane.origin().y.abs() < tol.linear && plane.normal().y.abs() > 0.99)).unwrap();
    let topology = filleted.topology();
    let inner = topology.face(bottom.topology_id()).unwrap().loops().iter().copied()
        .find(|id| topology.loop_(*id).unwrap().kind() == crate::LoopKind::Inner).unwrap();
    let inner_edges = topology.loop_coedges(inner).unwrap().into_iter()
        .map(|id| topology.edge(topology.coedge(id).unwrap().edge()).unwrap().name()).collect::<Vec<_>>();
    let edges = filleted.edges().filter(|edge| inner_edges.contains(&edge.persistent_id())).cloned().collect::<Vec<_>>();
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

#[test]
fn dissolve_faces_heals_bores_pockets_and_bosses() {
    let base = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 20.0, 10.0));
    let tolerance = ModelTolerance::default();
    let cylinder = |height: f64| Profile::new(
        Wire2::new([TrimmedCurve2::circle(Point2::new(10.0, 10.0), 3.0).unwrap()], tolerance).unwrap(),
        [], tolerance,
    ).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * height).unwrap();
    for (name, edited) in [
        ("bore", base.difference(&cylinder(10.0)).unwrap()),
        ("pocket", base.difference(&cylinder(6.0)).unwrap()),
        ("boss", base.union(&cylinder(14.0)).unwrap()),
    ] {
        let selected = edited.faces().filter(|face| matches!(face.surface(), Surface3::Cylinder(_))
            || matches!(face.surface(), Surface3::Plane(p) if p.origin().z == 6.0 || p.origin().z == 14.0))
            .cloned().collect::<Vec<_>>();
        let result = edited.dissolve_faces(&selected).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(result.faces().count(), 6, "{name}");
        assert!((result.volume() - 4000.0).abs() < 1e-6, "{name}");
        result.tessellate(TessellationOptions::default()).unwrap();
        assert!(selected.iter().all(|face| matches!(result.resolve(face), Resolution::Deleted)));
    }
}

#[test]
fn dissolve_faces_reconstructs_fillet_and_chamfer_supports() {
    let base = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(20.0, 10.0, 5.0));
    let edge = base.edges().find(|edge| {
        let p = edge.endpoints().unwrap(); (p[1].z - p[0].z).abs() > 4.9
    }).unwrap().clone();
    for edited in [base.fillet(&[edge.clone()], 1.0).unwrap(), base.chamfer(&[edge], 1.0).unwrap()] {
        let selected = edited.faces().filter(|face| !matches!(face.surface(), Surface3::Plane(p)
            if p.normal().x.abs() > 0.999 || p.normal().y.abs() > 0.999 || p.normal().z.abs() > 0.999))
            .cloned().collect::<Vec<_>>();
        let result = edited.dissolve_faces(&selected).unwrap();
        assert_eq!(result.faces().count(), 6);
        assert!((result.volume() - 1000.0).abs() < 1e-6);
        result.tessellate(TessellationOptions::default()).unwrap();
    }
}

#[test]
fn dissolve_faces_heals_periodic_blends_and_preserves_other_bodies() {
    let tolerance = ModelTolerance::default();
    let cylinder = Profile::new(
        Wire2::new([TrimmedCurve2::circle(Point2::new(0.0, 0.0), 10.0).unwrap()], tolerance).unwrap(),
        [], tolerance,
    ).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 20.0).unwrap();
    let top = cylinder.edges().find(|edge| edge.endpoints().unwrap().iter().all(|p| p.z > 19.9)).unwrap().clone();
    let other = owned_cuboid(Point3::new(30.0, 0.0, 0.0), Point3::new(40.0, 10.0, 10.0));
    for edited in [cylinder.fillet(&[top.clone()], 1.0).unwrap(), cylinder.chamfer(&[top], 1.0).unwrap()] {
        let combined = edited.combined(&[other.clone()]).unwrap();
        let selected = combined.faces().filter(|face| matches!(face.surface(), Surface3::Torus(_)
            | Surface3::RollingBall(_) | Surface3::Cone(_))).cloned().collect::<Vec<_>>();
        let result = combined.dissolve_faces(&selected).unwrap();
        assert_eq!(result.solids().len(), 2);
        assert!((result.volume() - cylinder.volume() - other.volume()).abs() < 1e-6);
        assert!(other.faces().all(|face| matches!(result.resolve(face), Resolution::Preserved(_))));
        result.tessellate(TessellationOptions::default()).unwrap();
    }
}

#[test]
fn dissolve_faces_heals_short_chamfers_and_rejects_open_results() {
    let base = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 0.5));
    let edge = base.edges().find(|edge| {
        let p = edge.endpoints().unwrap(); (p[1].z - p[0].z).abs() > 0.49
    }).unwrap().clone();
    let edited = base.chamfer(&[edge], 1.0).unwrap();
    let selected = edited.faces().filter(|face| matches!(face.surface(), Surface3::Plane(p)
        if p.normal().x.abs() > 0.1 && p.normal().y.abs() > 0.1)).cloned().collect::<Vec<_>>();
    assert!((edited.dissolve_faces(&selected).unwrap().volume() - 50.0).abs() < 1e-6);
    assert!(base.dissolve_faces(&[base.faces().next().unwrap().clone()]).is_err());
    assert!(base.dissolve_faces(&base.faces().cloned().collect::<Vec<_>>()).is_err());
    assert!(base.dissolve_faces(&[]).is_err());
}

#[test]
fn dissolve_faces_heals_blends_meeting_at_corners() {
    let base = owned_cuboid(Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 10.0, 10.0));
    let edges = base.edges().cloned().collect::<Vec<_>>();
    for edited in [base.fillet(&edges, 1.0).unwrap(), base.chamfer(&edges, 1.0).unwrap()] {
        let selected = edited.faces().filter(|face| !matches!(face.surface(), Surface3::Plane(p)
            if p.normal().x.abs() > 0.999 || p.normal().y.abs() > 0.999 || p.normal().z.abs() > 0.999))
            .cloned().collect::<Vec<_>>();
        let result = edited.dissolve_faces(&selected).unwrap();
        assert_eq!(result.faces().count(), 6);
        assert!((result.volume() - 1000.0).abs() < 1e-6);
        result.tessellate(TessellationOptions::default()).unwrap();
    }
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
        .shell_with_join(&[], 1.0, ShellJoin::Sharp)
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
    for mode in [ShellJoin::Sharp, ShellJoin::Rounded] {
        let result = combined
            .shell_with_join(&[opening.clone()], 1.0, mode)
            .unwrap();
        assert_eq!(result.solids().len(), 2);
        result.tessellate(TessellationOptions::default()).unwrap();
    }
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
        .shell_with_join(&[opening.clone()], -9.0, ShellJoin::Sharp)
        .unwrap();
    for thickness in [1.0, 3.0, 9.0] {
        let outward = source
            .shell(&[opening.clone()], thickness)
            .expect("outward rounded fixture");
        let sharp_outward = source
            .shell_with_join(&[opening.clone()], thickness, ShellJoin::Sharp)
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
fn replace_face_contact_supports_boolean_operations() {
    let tolerance = ModelTolerance::default();
    let spline = |height: f64| TrimmedCurve2::new(
        NurbsCurve2::new(3, vec![
            Point2::new(-5.0, 3.0 + height), Point2::new(-2.0, 6.0 + height),
            Point2::new(2.0, 1.0 + height), Point2::new(5.0, 4.0 + height),
        ], vec![1.0; 4], vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap(),
        ParameterRange::new(0.0, 1.0).unwrap(),
    ).unwrap();
    let upper = Profile::new(Wire2::new([
        spline(0.0),
        TrimmedCurve2::line(Point2::new(5.0, 4.0), Point2::new(5.0, 6.0)).unwrap(),
        spline(2.0).reversed(),
        TrimmedCurve2::line(Point2::new(-5.0, 5.0), Point2::new(-5.0, 3.0)).unwrap(),
    ], tolerance).unwrap(), [], tolerance).unwrap().extrude(
        Plane3::new(Point3::new(0.0, 0.0, -2.0), Vec3::unit_x(), Vec3::unit_y(), tolerance).unwrap(),
        Vec3::unit_z() * 4.0,
    ).unwrap();
    let underside = upper.faces().filter(|face| matches!(face.surface(), Surface3::Nurbs(_)))
        .min_by(|a, b| a.centroid().unwrap().y.total_cmp(&b.centroid().unwrap().y)).unwrap().clone();
    let Surface3::Nurbs(spline) = underside.surface() else { unreachable!() };
    let continuation = Surface3::Nurbs(spline.extended_bezier([0.5, 0.5]).unwrap());
    let outside = continuation.evaluate(1.2, 0.5).unwrap().point;
    let classifier = crate::brep_classification::BrepClassifier::for_compound(&upper).unwrap();
    assert!(!classifier.covers_on_support(&continuation, outside).unwrap());
    assert_eq!(classifier.sides_on_support(&continuation, outside,
        continuation.normal(1.2, 0.5).unwrap()).unwrap(), (false, false));
    for width in [2.0, 3.0] {
        let lower = owned_cuboid(Point3::new(-width, -4.0, -1.0), Point3::new(width, 0.0, 1.0));
        let top = lower.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
            if plane.normal().y.abs() > 0.9 && plane.origin().y.abs() < tolerance.linear)).unwrap().clone();
        check_replace_face_boolean_contact(&upper, &lower, &top, &underside);
    }
}

#[test]
fn replace_face_analytic_contact_supports_boolean_operations() {
    let tolerance = ModelTolerance::default();
    let upper = owned_cuboid(Point3::new(-2.0, -1.0, -2.0), Point3::new(2.0, 1.0, 2.0));
    let underside = upper.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
        if plane.normal().y.abs() > 0.9 && (plane.origin().y + 1.0).abs() < tolerance.linear)).unwrap().clone();
    let lower = owned_cuboid(Point3::new(-0.6, -5.0, -0.5), Point3::new(0.6, -3.0, 0.5));
    let top = lower.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
        if plane.normal().y.abs() > 0.9 && (plane.origin().y + 3.0).abs() < tolerance.linear)).unwrap().clone();
    check_replace_face_boolean_contact(&upper, &lower, &top, &underside);
    for kind in ["cylinder", "sphere", "cone", "torus"] {
        let mut kernel = Kernel::new(tolerance).unwrap();
        let mut transaction = kernel.begin_operation();
        match kind {
            "cylinder" => transaction.cylinder(Point3::new(-3.0, 0.0, 0.0),
                Vec3::unit_y() * 2.0, Vec3::unit_z() * 2.0, Vec3::unit_x() * 6.0),
            "sphere" => transaction.sphere(Point3::new(0.0, 0.0, 0.0),
                Vec3::unit_x() * 2.0, Vec3::unit_y() * 2.0, Vec3::unit_z() * 2.0),
            "cone" => transaction.cone(Point3::new(-4.0, 0.0, 0.0), Vec3::unit_x() * 8.0,
                Vec3::unit_y() * 4.0, Vec3::unit_z() * 4.0),
            "torus" => transaction.torus(Point3::new(0.0, 0.0, -4.0),
                Vec3::unit_x() * 4.0, Vec3::unit_z() * 4.0, 2.0),
            _ => unreachable!(),
        }.unwrap();
        let committed = transaction.finish().unwrap();
        let upper = crate::Compound::from_operation(kernel, committed, []).unwrap();
        let underside = upper.faces().find(|face| !matches!(face.surface(), Surface3::Plane(_))).unwrap().clone();
        let lower = owned_cuboid(Point3::new(-0.6, -5.0, -0.5), Point3::new(0.6, -3.0, 0.5));
        let top = lower.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
            if plane.normal().y.abs() > 0.9 && (plane.origin().y + 3.0).abs() < tolerance.linear)).unwrap().clone();
        check_replace_face_boolean_contact(&upper, &lower, &top, &underside);
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
fn move_cap_of_rounded_closed_shell() {
    let tolerance = ModelTolerance::default();
    let width = 30.266420840254132;
    let length = 88.4682421048154;
    let diagonal = 8.0 / 2.0_f64.sqrt();
    let wire = Wire2::new([
        TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(length, 0.0)).unwrap(),
        TrimmedCurve2::arc_through(Point2::new(length, 0.0),
            Point2::new(length + diagonal, 8.0 - diagonal), Point2::new(length + 8.0, 8.0)).unwrap(),
        TrimmedCurve2::line(Point2::new(length + 8.0, 8.0), Point2::new(length + 8.0, width - 8.0)).unwrap(),
        TrimmedCurve2::arc_through(Point2::new(length + 8.0, width - 8.0),
            Point2::new(length + diagonal, width - 8.0 + diagonal), Point2::new(length, width)).unwrap(),
        TrimmedCurve2::line(Point2::new(length, width), Point2::new(0.0, width)).unwrap(),
        TrimmedCurve2::line(Point2::new(0.0, width), Point2::new(0.0, 0.0)).unwrap(),
    ], tolerance).unwrap();
    let body = Profile::new(wire, [], tolerance).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 13.0).unwrap();
    let rim = body.edges().filter(|edge| edge.endpoints().unwrap().iter()
        .all(|point| point.z.abs() < 1e-7)).cloned().collect::<Vec<_>>();
    let body = body.fillet(&rim, 5.0).unwrap().shell(&[], -3.0).unwrap();
    let cap = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(p)
        if p.normal().z.abs() > 0.9 && (p.origin().z - 13.0).abs() < 1e-7)).unwrap().clone();
    let cap_area = (length + 8.0) * width - 128.0 + 32.0 * std::f64::consts::PI;
    for distance in [1.0, 5.0] {
        let moved = body.move_faces(&[cap.clone()], RigidTransform3::new(
            Matrix3::identity(), Vec3::unit_z() * distance, tolerance,
        ).unwrap()).unwrap();
        let delta = moved.volume() - body.volume();
        assert!((delta - cap_area * distance).abs() < cap_area * distance * 1e-7,
            "distance {distance}: volume change {delta}, expected {}", cap_area * distance);
        assert_eq!(moved.faces().count(), body.faces().count());
        moved.tessellate(TessellationOptions::default()).unwrap();
    }
    assert!(body.move_faces(&[cap], RigidTransform3::new(
        Matrix3::identity(), -Vec3::unit_z() * 4.0, tolerance,
    ).unwrap()).is_err());
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
fn move_faces_preserves_fillet_modes_when_rotating_a_support() {
    let tolerance = ModelTolerance::default();
    let block = owned_cuboid(Point3::origin(), Point3::new(20.0, 10.0, 5.0));
    let edge = block.edges().find(|edge| edge.endpoints().unwrap().iter()
        .all(|point| point.x.abs() < 1e-7 && point.y.abs() < 1e-7)).unwrap().clone();
    for (width, g2) in [(false, true), (true, false), (true, true)] {
        let rounded = block.fillet_with_options(&[edge.clone()], FilletOptions::new(1.0)
            .with_constant_width(width).with_curvature_continuity(g2)).unwrap();
        let side = rounded.faces().find(|face| matches!(face.surface(), Surface3::Plane(p)
            if p.normal().x.abs() > 0.9 && p.origin().x.abs() < 1e-7)).unwrap().clone();
        let motion = RigidTransform3::new(Matrix3::from_angle_z(cgmath::Rad(0.1)),
            Vec3::new(-0.25, 0.0, 0.0), tolerance).unwrap();
        let result = rounded.move_faces(&[side], motion)
            .unwrap_or_else(|error| panic!("width={width} G2={g2}: {error}"));
        assert_eq!(result.faces().count(), rounded.faces().count());
        let blend = result.faces().find_map(|face| match face.surface() {
            Surface3::Blend(blend) => Some(blend), _ => None,
        }).unwrap();
        assert_eq!(blend.is_curvature_continuous(), g2);
        if width {
            let u = blend.center_range().middle();
            let points = [(blend.first_surface(), blend.first_pcurve()),
                (blend.second_surface(), blend.second_pcurve())].map(|(surface, pcurve)| {
                let uv = pcurve.evaluate(u).unwrap().point;
                surface.evaluate(uv.x, uv.y).unwrap().point
            });
            assert!((points[0].distance(points[1]) - 1.0).abs() < 1e-8);
        }
        result.tessellate(TessellationOptions::default()).unwrap();
    }
}

#[test]
fn move_faces_rotates_a_fillet_on_an_inclined_cylinder_cap() {
    let tolerance = ModelTolerance::default();
    let cylinder = Profile::new(Wire2::new([TrimmedCurve2::circle(Point2::origin(), 5.0).unwrap()], tolerance).unwrap(),
        [], tolerance).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 15.0).unwrap();
    let cap = Plane3::new(Point3::new(0.0, 0.0, 10.0), Vec3::new(1.0, 0.0, 0.4), Vec3::unit_y(), tolerance).unwrap();
    let body = cylinder.split(cap, SplitKeep::Left).unwrap();
    let rim = body.edges().find(|edge| !matches!(edge.curve(), Curve3::Line(_))
        && edge.endpoints().unwrap().iter().all(|point| point.z > 7.0)).unwrap().clone();
    for (width, g2) in [(false, false), (false, true), (true, false), (true, true)] {
        let rounded = body.fillet_with_options(&[rim.clone()], FilletOptions::new(0.5)
            .with_constant_width(width).with_curvature_continuity(g2)).unwrap();
        let side = rounded.faces().find(|face| matches!(face.surface(), Surface3::Plane(p)
            if p.normal().z.abs() > 0.9 && p.origin().z > 1.0)).unwrap().clone();
        let rotation = Matrix3::from_angle_y(cgmath::Rad(0.1));
        let center = Vec3::new(0.0, 0.0, 10.0);
        let motion = RigidTransform3::new(rotation,
            center - rotation * center + Vec3::new(0.0, 0.0, 0.25), tolerance).unwrap();
        let result = rounded.move_faces(&[side], motion)
            .unwrap_or_else(|error| panic!("width={width} G2={g2}: {error}"));
        assert_eq!(result.faces().count(), rounded.faces().count());
        assert!(result.volume() > 0.0);
        let blend = result.faces().find_map(|face| match face.surface() {
            Surface3::RollingBall(blend) | Surface3::Blend(blend) => Some(blend), _ => None,
        }).unwrap();
        assert_eq!(blend.is_curvature_continuous(), g2);
        let period = blend.center_period().unwrap();
        let u = blend.center_range().middle() - period * 0.25;
        let center = blend.center_curve().evaluate(u).unwrap().point;
        let next_turn = blend.center_curve().evaluate(u + period).unwrap().point;
        assert!(center.distance(next_turn) < 1e-8,
            "the rebuilt guide must close before the surface wraps its parameter");
        if width {
            let points = [(blend.first_surface(), blend.first_pcurve()),
                (blend.second_surface(), blend.second_pcurve())].map(|(surface, pcurve)| {
                let uv = pcurve.evaluate(u).unwrap().point;
                surface.evaluate(uv.x, uv.y).unwrap().point
            });
            assert!((points[0].distance(points[1]) - 0.5).abs() < 1e-8);
        }
        result.tessellate(TessellationOptions::default())
            .unwrap_or_else(|error| panic!("width={width} G2={g2}: {error}"));
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
fn move_boss_rejects_crossing_its_base_document_30() {
    let tolerance = ModelTolerance::default();
    let plane = |height| Plane3::new(Point3::new(0.0, height, 0.0),
        Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
    let cylinder = |x, y, radius, base, height| Profile::new(Wire2::new([
        TrimmedCurve2::circle(Point2::new(x, y), radius).unwrap()
    ], tolerance).unwrap(), [], tolerance).unwrap()
        .extrude(plane(base), Vec3::unit_y() * height).unwrap();
    let radius = 8.840646636818086;
    let body = cylinder(0.0, 0.0, 34.48516397016436, 0.0, 19.0)
        .union(&cylinder(-17.48305641426953, -3.255357738829639, radius, 19.0, 15.0)).unwrap()
        .difference(&cylinder(18.923801555564232, 2.1259479110724135, 8.639117031005743, 19.0, -4.0)).unwrap();
    let cap = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if (p.origin().y - 34.0).abs() < 1e-7)).unwrap().clone();
    let center = Vec3::new(-17.48305641426953, 34.0, 3.255357738829639);
    let contact = (15.0_f64 / radius).atan().to_degrees();
    for angle in [-80.0, -contact, -contact + 0.1, 0.0, contact - 0.1, contact, 80.0] {
        let rotation = Matrix3::from_angle_z(cgmath::Deg(angle));
        let result = body.move_faces(&[cap.clone()], RigidTransform3::new(
            rotation, center - rotation * center, tolerance,
        ).unwrap());
        if angle.abs() >= contact {
            assert!(result.is_err(), "accepted touching/crossing cap at {angle} degrees");
        } else {
            result.unwrap().tessellate(TessellationOptions::default()).unwrap();
        }
    }
}

#[test]
fn move_boss_on_document_28_frame() {
    // Replay the third component's construction from untitled(28).cad.cad.cad.
    // Keep its segment and selection order: they determine the periodic trim
    // charts that exposed the failure on the planar face around the boss.
    let tolerance = ModelTolerance::default();
    let p = |x, y| Point2::new(x, y);
    let outer = Wire2::new(
        [
            TrimmedCurve2::arc_through(
                p(-41.08804749448451, 24.49828364036358),
                p(-63.757392263221234, 1.8289388716268609),
                p(-41.08804749448452, -20.840405897109868),
            ).unwrap(),
            TrimmedCurve2::line(
                p(-41.08804749448451, -20.840405897109868),
                p(39.243454695349335, -20.840405897109868),
            ).unwrap(),
            TrimmedCurve2::arc_through(
                p(39.24345469534932, -20.840405897109868),
                p(61.91279946408605, 1.8289388716268524),
                p(39.243454695349335, 24.49828364036358),
            ).unwrap(),
            TrimmedCurve2::line(
                p(39.24345469534932, 24.49828364036358),
                p(-41.08804749448452, 24.49828364036358),
            ).unwrap(),
        ], tolerance,
    ).unwrap();
    let inner = Wire2::new(
        [
            TrimmedCurve2::arc_through(
                p(-35.09044151977286, 10.270564794554685),
                p(-44.66890053586293, 0.6921057784646129),
                p(-35.09044151977286, -8.886353237625462),
            ).unwrap(),
            TrimmedCurve2::line(
                p(-35.09044151977286, -8.886353237625462),
                p(32.38023301545568, -8.886353237625462),
            ).unwrap(),
            TrimmedCurve2::arc_through(
                p(32.38023301545568, -8.886353237625462),
                p(41.95869203154575, 0.6921057784646093),
                p(32.38023301545568, 10.270564794554685),
            ).unwrap(),
            TrimmedCurve2::line(
                p(32.38023301545568, 10.270564794554685),
                p(-35.09044151977286, 10.270564794554685),
            ).unwrap(),
        ], tolerance,
    ).unwrap();
    let plane = |height| Plane3::new(
        Point3::new(0.0, height, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tolerance,
    ).unwrap();
    let body = Profile::new(outer, [inner], tolerance).unwrap()
        .extrude(plane(0.0), Vec3::unit_y() * 14.0).unwrap();
    let faces = [8, 6].map(|index| body.faces().nth(index).unwrap().clone());
    let body = body.move_faces(&faces, RigidTransform3::new(
        Matrix3::from_angle_y(cgmath::Deg(-5.0)),
        Vec3::new(0.2895224260213478, 0.0, -2.1925285538421133), tolerance,
    ).unwrap()).unwrap();
    let edges = [6, 15, 3, 9].map(|index| body.edges().nth(index).unwrap().clone());
    let body = body.chamfer_with_tangent_propagation(&edges, 1.0, true).unwrap();
    let edges = [12, 16, 30, 28].map(|index| body.edges().nth(index).unwrap().clone());
    let body = body.fillet(&edges, 0.75).unwrap();
    let cylinder = |x, radius, height| Profile::new(Wire2::new([
        TrimmedCurve2::circle(p(x, 0.0), radius).unwrap()
    ], tolerance).unwrap(), [], tolerance).unwrap()
        .extrude(plane(14.0), Vec3::unit_y() * height).unwrap();
    let body = body.difference(&cylinder(52.273464211369564, 5.769615333998077, -2.0)).unwrap();
    let edges = [52, 57].map(|index| body.edges().nth(index).unwrap().clone());
    let body = body.fillet(&edges, 0.5).unwrap();
    let radius = 5.744170132035949;
    let body = body.union(&cylinder(-53.8427277335454, radius, 12.0)).unwrap();
    let options = TessellationOptions::new(0.1, 0.1, 24).unwrap();
    for face in body.faces() {
        face.tessellate(options).unwrap();
    }
    let cap = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(p)
        if p.normal().y.abs() > 0.99 && (p.origin().y - 26.0).abs() < 1e-7)).unwrap().clone();
    for distance in [-1.0, 1.0] {
        let moved = body.move_faces(&[cap.clone()], RigidTransform3::new(
            Matrix3::identity(), Vec3::unit_y() * distance, tolerance,
        ).unwrap()).unwrap();
        for face in moved.faces() {
            face.tessellate(options).unwrap();
        }
        // Curved BREP mass properties use adaptive quadrature at body scale.
        let expected = std::f64::consts::PI * radius * radius * distance;
        assert!((moved.volume() - body.volume() - expected).abs() < body.volume() * 1e-7,
            "volume change {} expected {}", moved.volume() - body.volume(), expected);
    }
    let center = Vec3::new(-53.8427277335454, 26.0, 0.0);
    for angle in [-60.0, -40.0, -5.0, 5.0, 40.0, 60.0] {
        for rotation in [
            Matrix3::from_angle_x(cgmath::Deg(angle)),
            Matrix3::from_angle_z(cgmath::Deg(angle)),
        ] {
            let rotated = body.move_faces(&[cap.clone()], RigidTransform3::new(
                rotation, center - rotation * center, tolerance,
            ).unwrap()).unwrap();
            assert_eq!(rotated.faces().count(), body.faces().count());
            let normal = rotation * Vec3::unit_y();
            assert!(rotated.faces().any(|face| matches!(face.surface(), Surface3::Plane(plane)
                if plane.normal().dot(normal) > 1.0 - tolerance.angular
                    && plane.project(Point3::from_vec(center)).unwrap().distance < tolerance.linear)));
            for face in rotated.faces() {
                face.tessellate(options).unwrap();
            }
            // Tilting about the circular cap's center preserves its mean height
            // and volume; allow the body-scale boundary quadrature error.
            assert!((rotated.volume() - body.volume()).abs() < body.volume() * 1e-6,
                "rotation {angle}: volume change {}", rotated.volume() - body.volume());
            let tilted_cap = rotated.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
                if plane.normal().dot(normal) > 1.0 - tolerance.angular
                    && plane.project(Point3::from_vec(center)).unwrap().distance < tolerance.linear)).unwrap();
            let rim = tilted_cap.boundary_edges().unwrap();
            for blended in [
                rotated.fillet(&rim, 0.3).unwrap(),
                rotated.chamfer(&rim, 0.3).unwrap(),
            ] {
                assert_eq!(blended.faces().count(), rotated.faces().count() + 1);
                assert!(blended.volume() > 0.0 && blended.volume() < rotated.volume());
                for face in blended.faces() {
                    assert!(!face.tessellate(options).unwrap().is_empty());
                }
            }
        }
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
    // Moving the hole changes the cap trims without changing their area.
    for cap in body.faces().filter(|face| {
        matches!(face.surface(), Surface3::Plane(p)
        if p.normal().z.abs() > 0.9)
    }) {
        assert!(matches!(moved.resolve(cap), Resolution::Modified(_)));
    }
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
        .shell_with_join(&[], 1.0, ShellJoin::Sharp)
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
    let boss = crate::Compound::from_operation(kernel, committed, []).expect("boss");
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
    let check_partitions = |compound: &crate::Compound| {
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
fn moved_support_reports_edge_and_vertex_changes_at_construction() {
    let tolerance = ModelTolerance::default();
    let source = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let top = source.faces().find(|face| {
        matches!(face.surface(), Surface3::Plane(plane) if plane.origin().z == 10.0 && plane.normal().z.abs() > 0.9)
    }).unwrap();
    let movement =
        RigidTransform3::new(Matrix3::identity(), Vec3::new(0.0, 0.0, 0.125), tolerance).unwrap();
    let result = source.move_faces(&[top.clone()], movement).unwrap();
    for vertex in source.vertices() {
        if vertex.point().z == 0.0 {
            assert!(matches!(result.resolve(vertex), Resolution::Preserved(_)));
        } else {
            assert!(matches!(result.resolve(vertex), Resolution::Modified(_)));
        }
    }
    for edge in source.edges() {
        if edge.endpoints().unwrap().iter().all(|point| point.z == 0.0) {
            assert!(matches!(result.resolve(edge), Resolution::Preserved(_)));
        } else {
            assert!(matches!(result.resolve(edge), Resolution::Modified(_)));
        }
    }
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
    // Select the cut boundary, not the tangent contact with the base cylinder;
    // topology normalization can change which of those edges is listed first.
    let rim = cut.edges().find(|edge| edge.connected_faces().unwrap().iter().any(|face| matches!(face.surface(), Surface3::Torus(t) if (t.minor_radius()-12.0).abs()<tolerance.linear)) && edge.connected_faces().unwrap().iter().any(|face| matches!(face.surface(), Surface3::Cylinder(c) if (c.radius() - 26.954243397522823).abs() < tolerance.linear))).unwrap().clone();
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
                .shell_with_join(&openings, -thickness, ShellJoin::Sharp)
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
fn shell_filleted_curved_pocket_document_44() {
    // The 1 mm pocket fillet disappears; larger rounded offset joins also
    // intersect the dome and remove portions of the cylindrical pocket wall.
    let tol = ModelTolerance::default();
    let circle = |center, radius| Profile::new(
        Wire2::new([TrimmedCurve2::circle(center, radius).unwrap()], tol).unwrap(), [], tol,
    ).unwrap();
    let plane = Plane3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tol).unwrap();
    let cylinder = circle(Point2::new(0.0, 0.0), 46.54400344616973)
        .extrude(plane, Vec3::new(0.0, 68.0, 0.0)).unwrap();
    let rim = cylinder.edges().find(|edge| {
        matches!(edge.curve(), Curve3::Circle(circle) if circle.center().y > 67.0)
    }).unwrap().clone();
    let rounded = cylinder.fillet(&[rim], 30.0).unwrap();
    let limit = rounded.faces().find(|face| !matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_))).unwrap().clone();
    let top = Plane3::new(Point3::new(0.0, 68.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tol).unwrap();
    let cutter = circle(Point2::new(0.0, 30.413996263282527), 9.684232064779334)
        .extrude_up_to_face(top, &rounded, &limit, -2.0, 4.0).unwrap();
    let pocket = rounded.difference(&cutter).unwrap();
    let rim = pocket.edges().find(|edge| {
        let faces = edge.connected_faces().unwrap();
        faces.iter().any(|face| !matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_)))
            && faces.iter().any(|face| matches!(face.surface(), Surface3::Cylinder(cylinder) if cylinder.radius() < 10.0))
            && !matches!(edge.curve(), Curve3::Circle(_))
    }).unwrap().clone();
    let body = pocket.fillet(&[rim], 1.0).unwrap();
    let bottom = body.faces().find(|face| {
        matches!(face.surface(), Surface3::Plane(plane) if plane.origin().y < 1.0)
    }).unwrap().clone();
    let top = body.faces().find(|face| {
        matches!(face.surface(), Surface3::Plane(plane) if plane.origin().y > 67.0)
    }).unwrap().clone();
    for thickness in [1.0, 2.0, 3.0] {
        assert!(body.shell(&[top.clone()], -thickness).is_err());
        for openings in [vec![], vec![bottom.clone()]] {
            let shell = body.shell(&openings, -thickness)
                .unwrap_or_else(|error| panic!("{thickness} mm, {} openings: {error}", openings.len()));
            assert_eq!(shell.solids().len(), 1);
            assert!(shell.volume() > 0.0 && shell.volume() < body.volume());
            assert!(shell.faces().any(|face| {
                matches!(face.surface(), Surface3::RollingBall(blend) if (blend.radius() - thickness).abs() < tol.linear)
            }));
            shell.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
        }
    }
}

#[test]
fn shell_filleted_slot_with_floor_chamfer() {
    let tol = ModelTolerance::default();
    let plane = Plane3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tol).unwrap();
    let cylinder = Profile::new(Wire2::new([
        TrimmedCurve2::circle(Point2::new(0.0, 0.0), 37.32788704942782).unwrap(),
    ], tol).unwrap(), [], tol).unwrap().extrude(plane, Vec3::new(0.0, 63.0, 0.0)).unwrap();
    let rim = cylinder.edges().find(|edge| matches!(edge.curve(), Curve3::Circle(c) if c.center().y > 62.0)).unwrap().clone();
    let rounded = cylinder.fillet(&[rim], 13.0).unwrap();
    let points = [
        Point2::new(-8.976935751099653, 7.6854630100868855),
        Point2::new(64.84272456939797, 7.6854630100868855),
        Point2::new(64.84272456939797, -9.720435845300738),
        Point2::new(-8.976935751099653, -9.720435845300738),
    ];
    let slot = Profile::new(Wire2::new((0..4).map(|i| TrimmedCurve2::line(points[i], points[(i + 1) % 4]).unwrap()), tol).unwrap(), [], tol).unwrap();
    let top = Plane3::new(Point3::new(0.0, 63.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tol).unwrap();
    let mut body = rounded.difference(&slot.extrude(top, Vec3::new(0.0, -37.0, 0.0)).unwrap()).unwrap();
    let corners = body.edges().filter(|edge| {
        let [a, b] = edge.endpoints().unwrap();
        (a.x + 8.976935751099653).abs() < tol.linear && (b.x - a.x).abs() < tol.linear && (b.y - a.y).abs() > 36.0
    }).cloned().collect::<Vec<_>>();
    assert_eq!(corners.len(), 2);
    body = body.fillet(&corners, 4.0).unwrap();
    let lip = body.edges().find(|edge| {
        let [a, b] = edge.endpoints().unwrap();
        (a.z + 7.685463010086885).abs() < tol.linear && (b.z + 7.685463010086885).abs() < tol.linear
            && (a.y - b.y).abs() > 12.0 && matches!(edge.curve(), Curve3::SurfaceIntersection(_))
    }).unwrap().clone();
    body = body.fillet(&[lip], 5.0).unwrap();
    let rim = body.edges().find(|edge| matches!(edge.curve(), Curve3::Circle(c) if (c.center().y - 26.0).abs() < tol.linear)).unwrap().clone();
    body = body.chamfer_with_tangent_propagation(&[rim], 5.0, true).unwrap();
    let bottom = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(p) if p.origin().y.abs() < tol.linear)).unwrap().clone();
    for thickness in [3.0, 4.0, 5.0, 6.0] {
        let shell = body.shell(&[bottom.clone()], -thickness)
            .unwrap_or_else(|error| panic!("{thickness} mm shell: {error}"));
        assert_eq!(shell.solids().len(), 1);
        assert!(shell.volume() > 0.0 && shell.volume() < body.volume());
        shell.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
    }
}

#[test]
fn shell_chamfered_slot_document_51() {
    let body = chamfered_slot_document_51();
    let tol = ModelTolerance::default();
    let bounds = crate::bounds::compound(&body).unwrap();
    for (_, edge) in body.topology().edges() {
        for step in 0..=16 {
            let t = edge.range().start + edge.range().span() * step as f64 / 16.0;
            let point = edge.curve().evaluate(t).unwrap().point;
            assert!((0..3).all(|axis| point[axis] >= bounds.0[axis] && point[axis] <= bounds.1[axis]));
        }
    }
    let bottom = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.origin().y.abs() < tol.linear)).unwrap().clone();
    for thickness in [1.0, 2.0] {
        for openings in [vec![], vec![bottom.clone()]] {
            let shell = body.shell(&openings, -thickness)
                .unwrap_or_else(|error| panic!("{thickness} mm, {} openings: {error}", openings.len()));
            assert_eq!(shell.solids().len(), 1);
            assert!(shell.volume() > 0.0 && shell.volume() < body.volume());
            shell.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
        }
    }
}

#[test]
fn shell_filleted_chamfered_slot_document_51() {
    let tol = ModelTolerance::default();
    let body = filleted_chamfered_slot_document_51();
    let bottom = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.origin().y.abs() < tol.linear)).unwrap().clone();
    let mut failures = Vec::new();
    for join in [ShellJoin::Sharp, ShellJoin::Rounded] {
        for thickness in (1..=12).map(f64::from) {
            for openings in [vec![], vec![bottom.clone()]] {
                let shell = match body.shell_with_join(&openings, -thickness, join) {
                    Ok(shell) => shell,
                    Err(error) => {
                        failures.push(format!("{join:?} {thickness} mm, {} openings: {error}", openings.len()));
                        continue;
                    }
                };
                assert_eq!(shell.solids().len(), 1);
                assert!(shell.volume() > 0.0 && shell.volume() < body.volume());
                shell.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                    .unwrap_or_else(|error| panic!("{join:?} {thickness} mm, {} openings: {error}", openings.len()));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn shell_past_circular_fillet_document_45() {
    let tol = ModelTolerance::default();
    let plane = Plane3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tol).unwrap();
    let cylinder = Profile::new(
        Wire2::new([TrimmedCurve2::circle(Point2::new(0.0, 0.0), 33.22795844564359).unwrap()], tol).unwrap(),
        [], tol,
    ).unwrap().extrude(plane, Vec3::new(0.0, 39.0, 0.0)).unwrap();
    let rim = cylinder.edges().find(|edge| {
        matches!(edge.curve(), Curve3::Circle(circle) if circle.center().y > 38.0)
    }).unwrap().clone();
    let body = cylinder.fillet(&[rim], 9.0).unwrap();
    let bottom = body.faces().find(|face| {
        matches!(face.surface(), Surface3::Plane(plane) if plane.origin().y < 1.0)
    }).unwrap().clone();
    for thickness in [8.0, 9.0, 10.0, 11.0] {
        let shell = body.shell(&[bottom.clone()], -thickness)
            .unwrap_or_else(|error| panic!("{thickness} mm: {error}"));
        assert_eq!(shell.solids().len(), 1);
        assert!(shell.volume() > 0.0 && shell.volume() < body.volume());
        if thickness >= 9.0 {
            let cavity = std::f64::consts::PI * (33.22795844564359 - thickness).powi(2) * (39.0 - thickness);
            assert!((shell.volume() - (body.volume() - cavity)).abs() < 1.0e-5);
            assert_eq!(shell.faces().filter(|face| matches!(face.surface(), Surface3::Torus(_))).count(), 1);
        }
        shell.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
    }
}

#[test]
fn shell_through_top_chamfer_document_46() {
    // The 0.5 mm chamfer vanishes before the 1 mm vertical fillet collapses.
    let tol = ModelTolerance::default();
    let arc = |start, middle, end| TrimmedCurve2::arc_through(start, middle, end).unwrap();
    let wire = Wire2::new([
        arc(Point2::new(9.444171418893058, -9.381765000711214), Point2::new(-9.381765000711212, -9.44417141889306), Point2::new(-9.44417141889306, 9.38176500071121)),
        TrimmedCurve2::line(Point2::new(-9.44417141889306, 9.38176500071121), Point2::new(6.398874075080942, 25.33019660511283)).unwrap(),
        TrimmedCurve2::line(Point2::new(6.398874075080936, 25.33019660511284), Point2::new(-18.465793285138364, 73.60545721113832)).unwrap(),
        arc(Point2::new(-18.465793285138375, 73.60545721113832), Point2::new(-41.74986151465153, 81.0582106603938), Point2::new(-49.20261496390702, 57.774142430880666)),
        TrimmedCurve2::line(Point2::new(-49.20261496390702, 57.774142430880666), Point2::new(-15.36841083938433, -7.915657390128813)).unwrap(),
        arc(Point2::new(-15.368410839384332, -7.915657390128809), Point2::new(3.3107605450329225, -16.967160877986043), Point2::new(17.216860111955928, -1.557373279386644)),
        TrimmedCurve2::line(Point2::new(17.216860111955896, -1.557373279386681), Point2::new(9.444171418893063, -9.381765000711212)).unwrap(),
    ], tol).unwrap();
    let plane = Plane3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tol).unwrap();
    let mut body = Profile::new(wire, [], tol).unwrap().extrude(plane, Vec3::new(0.0, 8.0, 0.0)).unwrap();
    for (point, radius) in [
        (Point3::new(17.216860111955896, 4.0, 1.557373279386681), 1.0),
        (Point3::new(6.398874075080936, 4.0, -25.33019660511284), 16.0),
    ] {
        let edge = body.edges().find(|edge| {
            let [start, end] = edge.endpoints().unwrap();
            (start + (end - start) * 0.5).distance(point) < tol.linear
        }).unwrap().clone();
        body = body.fillet(&[edge], radius).unwrap();
    }
    let top = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.origin().y > 7.9)).unwrap();
    body = body.chamfer(&top.boundary_edges().unwrap(), 0.5).unwrap();
    let bottom = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.origin().y < 0.1)).unwrap().clone();
    for thickness in [0.5, 0.8, 0.9, 1.0] {
        let shell = body.shell(&[bottom.clone()], -thickness)
            .unwrap_or_else(|error| panic!("{thickness} mm: {error}"));
        assert_eq!(shell.solids().len(), 1);
        assert!(shell.volume() > 0.0 && shell.volume() < body.volume());
        shell.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
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

#[test]
fn shell_filleted_spline_prism_through_flat_end() {
    let body = spline_prism();
    let body = body.fillet(&spline_prism_cap_curves(&body), 5.0).unwrap();
    let volume = body.volume();
    let ends = body
        .faces()
        .filter(|face| match face.surface() {
            Surface3::Plane(plane) => plane.normal().z.abs() < 0.5,
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(ends.len(), 2);
    for opening in ends {
        let result = body.shell(&[opening.clone()], -1.0).unwrap();
        assert_eq!(result.solids().len(), 1);
        assert_eq!(result.faces().count(), 2 * body.faces().count() - 1);
        assert!(result.volume() > 0.0 && result.volume() < volume * 0.2);
        result
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap();
    }
}
