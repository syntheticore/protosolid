use super::*;
use crate::sew::publish;

#[test]
fn revolved_sector_blend_followups() {
    let body = rectangle(12.0, -4.0, 8.0, 8.0)
        .revolve(
            Plane3::xz(),
            Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_z()).unwrap(),
            std::f64::consts::PI * 1.5,
        )
        .unwrap();
    let cap = body
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().x.abs() > 0.99)
        })
        .unwrap();
    let edges = cap.boundary_edges().unwrap();
    let chamfered = body.chamfer(&edges, 1.0).unwrap();
    for edge in chamfered.edges() {
        assert!(
            chamfered.is_semantic_edge(edge),
            "hidden edge {:?}",
            edge.persistent_id()
        );
    }
    let filleted = body.fillet(&edges, 2.0).unwrap();
    for face in body.faces() {
        let resolved = match filleted.resolve(face) {
            super::super::Resolution::Preserved(face)
            | super::super::Resolution::Modified(face) => face,
            other => panic!("sector support lost during fillet: {other:?}"),
        };
        assert_eq!(
            std::mem::discriminant(face.surface()),
            std::mem::discriminant(resolved.surface())
        );
    }
    for distance in [-1.0, 1.0] {
        filleted.shell_with_join(&[], distance, ShellJoin::Sharp).unwrap();
    }
    let shell = filleted.shell(&[], -1.0).unwrap();
    assert!(shell.mass_properties().volume > 0.0);
    assert!(shell.mass_properties().volume < filleted.mass_properties().volume);
    shell
        .tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap())
        .unwrap();
    let opening = body
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().y.abs() > 0.99)
        })
        .unwrap();
    let opening = match filleted.resolve(opening) {
        super::super::Resolution::Preserved(face)
        | super::super::Resolution::Modified(face) => face,
        other => panic!("opening lost: {other:?}"),
    };
    for openings in [vec![], vec![opening]] {
        let shell = filleted.shell(&openings, -2.0).unwrap();
        assert!(shell.volume() > 0.0 && shell.volume() < filleted.volume());
        shell
            .tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap())
            .unwrap();
    }
    let mut expanded = edges;
    expanded.extend(
        body.edges()
            .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
            .cloned(),
    );
    assert_eq!(expanded.len(), 8);
    let fully_rounded = body.fillet(&expanded, 2.0).unwrap();
    let opening = fully_rounded
        .faces()
        .find(|face| {
            matches!(face.surface(), Surface3::Plane(p)
        if p.normal().y.abs() > 0.99)
        })
        .unwrap()
        .clone();
    for thickness in [1.0, 2.0] {
        fully_rounded.shell(&[opening.clone()], -thickness).unwrap();
    }
    let shell = fully_rounded.shell(&[opening], -1.0).unwrap();
    let rim_edges = [shell.edges().find(|edge| {
        edge.endpoints().unwrap().iter().all(|p| p.y.abs() < 1e-7 && (p.z + 4.0).abs() < 1e-7)
    }).unwrap().clone()];
    let rounded = shell.fillet_with_options(&rim_edges,
        FilletOptions::new(0.3).with_tangent_propagation(true)).unwrap();
    let rim_edges = [rounded.edges().find(|edge| {
        edge.endpoints().unwrap().iter().all(|p| p.y.abs() < 1e-7 && (p.x - 13.0).abs() < 1e-7)
    }).unwrap().clone()];
    let chamfered = rounded.chamfer_with_tangent_propagation(&rim_edges, 0.2, true).unwrap();
    let section = Plane3::from_origin_normal(Point3::new(0.0, 0.0, -3.0), Vec3::unit_z()).unwrap();
    chamfered.split(section, SplitKeep::Right).unwrap();
}
fn rectangle(x: f64, y: f64, w: f64, h: f64) -> Profile {
    let t = ModelTolerance::default();
    Profile::new(
        Wire2::polygon(
            [
                Point2::new(x, y),
                Point2::new(x + w, y),
                Point2::new(x + w, y + h),
                Point2::new(x, y + h),
            ],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap()
}
fn circle(r: f64) -> Profile {
    let t = ModelTolerance::default();
    Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), r).unwrap()],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap()
}
fn upper(z: f64) -> Plane3 {
    Plane3::new(
        Point3::new(0.0, 0.0, z),
        Vec3::unit_x(),
        Vec3::unit_y(),
        ModelTolerance::default(),
    )
    .unwrap()
}
fn mesh(body: &Compound) {
    assert!(!body
        .tessellate(TessellationOptions::new(0.05, 0.15, 24).unwrap())
        .unwrap()
        .is_empty());
}
#[test]
fn feature_revolution_crosses_half_turn() {
    let profile = rectangle(2.0, 0.0, 2.0, 3.0);
    let axis = Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).unwrap();
    for degrees in [
        179.0_f64,
        179.999999,
        180.0,
        180.000001,
        181.0,
        270.0,
        359.0,
        -179.0,
        -179.999999,
        -180.0,
        -180.000001,
        -181.0,
        -270.0,
        -359.0,
    ] {
        let angle = degrees.to_radians();
        let body = profile.revolve(Plane3::xy(), axis, angle).unwrap();
        assert!(
            (body.volume() - 18.0 * angle.abs()).abs() < 1e-5,
            "{degrees}: volume {}",
            body.volume()
        );
        let display = body
            .tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap())
            .unwrap();
        let volume = display
            .triangles
            .iter()
            .map(|t| {
                display.positions[t[0]].to_vec().dot(
                    display.positions[t[1]]
                        .to_vec()
                        .cross(display.positions[t[2]].to_vec()),
                ) / 6.0
            })
            .sum::<f64>();
        assert!(
            (volume - body.volume()).abs() < body.volume() * 0.01,
            "{degrees}: display volume {volume}, body volume {}",
            body.volume()
        );
        assert!(body.centroid().z * angle < 0.0);
        let round = Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(5.0, 0.0), 1.0).unwrap()],
                ModelTolerance::default(),
            )
            .unwrap(),
            [],
            ModelTolerance::default(),
        )
        .unwrap();
        let torus = round.revolve(Plane3::xy(), axis, angle).unwrap();
        assert!(
            torus.centroid().z * angle < 0.0,
            "{degrees}: torus centroid {:?}",
            torus.centroid()
        );
        let display = torus
            .tessellate(TessellationOptions::new(0.02, 0.1, 24).unwrap())
            .unwrap();
        let z_moment = display
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| display.positions[i]);
                a.to_vec().dot(b.to_vec().cross(c.to_vec())) / 6.0 * (a.z + b.z + c.z) / 4.0
            })
            .sum::<f64>();
        assert!(
            z_moment * angle < 0.0,
            "{degrees}: torus display z moment {z_moment}"
        );
    }
}
#[test]
fn feature_sewing_reconciles_boundary_subdivisions() {
    let bodies = [
        rectangle(0.0, 0.0, 10.0, 8.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))
            .unwrap(),
        circle(3.0)
            .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0))
            .unwrap(),
        circle(3.0)
            .loft(Plane3::xy(), [(upper(5.0), &circle(2.0))])
            .unwrap(),
    ];
    for body in bodies {
        let topology = body.topology();
        let mut patches = body
            .faces()
            .map(|handle| {
                let face = topology.face(handle.topology_id()).unwrap();
                BrepPatch {
                    preserved: Vec::new(),
                    sources: Vec::new(),
                    surface: face.surface().clone(),
                    orientation: face.orientation(),
                    loops: face
                        .loops()
                        .iter()
                        .map(|id| BrepPatchLoop {
                            kind: topology.loop_(*id).unwrap().kind(),
                            segments: topology
                                .loop_coedges(*id)
                                .unwrap()
                                .iter()
                                .map(|id| {
                                    let c = topology.coedge(*id).unwrap();
                                    BrepPatchSegment {
                                        provenance: Default::default(),
                                        curve: topology.edge(c.edge()).unwrap().curve().clone(),
                                        pcurve: c.pcurve().clone(),
                                        range: c.range(),
                                    }
                                })
                                .collect(),
                        })
                        .collect(),
                }
            })
            .collect::<Vec<_>>();
        let patch = patches
            .iter_mut()
            .find(|p| matches!(p.surface, Surface3::Plane(_)))
            .unwrap();
        let segment = patch.loops[0].segments.remove(0);
        for (index, pair) in [0.0, 0.2, 0.65, 1.0].windows(2).enumerate() {
            patch.loops[0].segments.insert(
                index,
                BrepPatchSegment {
                    range: range(
                        segment.range.start + segment.range.span() * pair[0],
                        segment.range.start + segment.range.span() * pair[1],
                    )
                    .unwrap(),
                    ..segment.clone()
                },
            );
        }
        let result = publish(&patches, body.tolerance()).unwrap();
        assert!((result.volume() - body.volume()).abs() < 1e-5);
        assert_eq!(result.edges().count(), body.edges().count() + 2);
        mesh(&result);
    }
}
#[test]
fn feature_chamfer_after_two_fillets() {
    let body = rectangle(30.0, 17.0, 100.0, 60.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 21.0))
        .unwrap();
    let edge = body
        .edges()
        .find(|e| {
            e.endpoints().is_ok_and(|p| {
                p.iter()
                    .all(|p| (p.x - 30.0).abs() < 1e-7 && (p.y - 17.0).abs() < 1e-7)
            })
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[edge], 20.0).unwrap();
    let edge = body
        .edges()
        .find(|e| {
            matches!(e.curve(), Curve3::Circle(_))
                && e.endpoints()
                    .is_ok_and(|p| p.iter().all(|p| p.z.abs() < 1e-7))
        })
        .unwrap()
        .clone();
    for radius in [2.0, 5.0] {
        let filleted = body.fillet(&[edge.clone()], radius).unwrap();
        for rotation in [
            crate::Matrix3::identity(),
            crate::Matrix3::from_angle_x(cgmath::Deg(90.0)),
        ] {
            let transform = RigidTransform3::new(
                rotation,
                Vec3::new(12.430018992481585, 0.0, -24.348125762908836),
                body.tolerance(),
            )
            .unwrap();
            let body = filleted.transformed(transform).unwrap();
            let edges = body
                .edges()
                .filter(|e| matches!(e.curve(), Curve3::Line(_)))
                .filter(|e| {
                    e.endpoints().is_ok_and(|p| {
                        let p = p.map(|p| transform.inverse().transform_point(p));
                        p.iter().all(|p| p.z.abs() < 1e-7)
                            && (p.iter().all(|p| (p.y - 77.0).abs() < 1e-7)
                                || p.iter().all(|p| (p.x - 130.0).abs() < 1e-7))
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(edges.len(), 2);
            for edge in edges {
                for follow in [false, true] {
                    let result = body
                        .chamfer_with_tangent_propagation(&[edge.clone()], 1.0, follow)
                        .unwrap_or_else(|e| panic!("radius {radius}, follow {follow}: {e}"));
                    assert!(result.volume() < body.volume());
                    assert_eq!(result.solids().len(), 1);
                    mesh(&result);
                }
            }
        }
    }
}
#[test]
fn feature_chamfer_multiple_tangent_edges() {
    let body = rectangle(30.0, 17.0, 100.0, 60.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 21.0))
        .unwrap();
    let edge = body
        .edges()
        .find(|e| {
            e.endpoints().is_ok_and(|p| {
                p.iter()
                    .all(|p| (p.x - 30.0).abs() < 1e-7 && (p.y - 17.0).abs() < 1e-7)
            })
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[edge], 20.0).unwrap();
    let edges = body
        .edges()
        .filter(|e| {
            e.endpoints().is_ok_and(|p| {
                p.iter().all(|p| (p.z - 21.0).abs() < 1e-7)
                    && (matches!(e.curve(), Curve3::Circle(_))
                        || p.iter().all(|p| (p.x - 30.0).abs() < 1e-7)
                        || p.iter().all(|p| (p.y - 17.0).abs() < 1e-7))
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 3);
    for mode in [
        ChamferMode::TwoDistances(2.0),
        ChamferMode::DistanceAngle(0.6),
    ] {
        for flip in [false, true] {
            let references = edges.iter().map(|edge| edge.connected_faces().unwrap().into_iter()
                .find(|face| matches!(face.surface(), Surface3::Plane(plane) if plane.normal().z.abs() > 0.9) != flip)
                .unwrap()).collect::<Vec<_>>();
            let all = body
                .chamfer_with_options(&edges, &references, 1.0, mode, true)
                .unwrap();
            for (edge, face) in edges.iter().zip(&references) {
                let following = body
                    .chamfer_with_options(&[edge.clone()], &[face.clone()], 1.0, mode, true)
                    .unwrap();
                assert!((following.volume() - all.volume()).abs() < 1e-5);
                mesh(&following);
                let selected = body
                    .chamfer_with_options(&[edge.clone()], &[face.clone()], 1.0, mode, false)
                    .unwrap();
                assert!(selected.volume() > following.volume());
            }
        }
    }
    for distance in [1.0, 11.0] {
        for selection in [
            vec![edges[0].clone(), edges[1].clone()],
            vec![edges[0].clone(), edges[2].clone()],
            vec![edges[1].clone(), edges[2].clone()],
            edges.clone(),
        ] {
            let result = body.chamfer(&selection, distance).unwrap_or_else(|e| {
                panic!("{} edges, distance {distance}: {e}", selection.len())
            });
            assert!(result.volume() < body.volume());
            mesh(&result);
        }
    }
    let all = body.chamfer(&edges, 11.0).unwrap();
    for edge in &edges {
        let following = body
            .chamfer_with_tangent_propagation(&[edge.clone()], 11.0, true)
            .unwrap();
        assert!((following.volume() - all.volume()).abs() < 1e-5);
        let selected = body
            .chamfer_with_tangent_propagation(&[edge.clone()], 11.0, false)
            .unwrap();
        assert!(selected.volume() > following.volume());
        mesh(&selected);
    }
    let reversed = body
        .chamfer(&edges.into_iter().rev().collect::<Vec<_>>(), 11.0)
        .unwrap();
    assert!((reversed.volume() - all.volume()).abs() < 1e-5);
}
#[test]
fn tilted_round_ended_profile_revolution() {
    let tolerance = ModelTolerance::default();
    let center = Point2::new(-52.88068565071396, -86.18951841790492);
    let arc = TrimmedCurve2::new(
        Circle2::new(center, 15.0).unwrap(),
        range(-0.4014257279586958, -3.543018381548489).unwrap(),
    )
    .unwrap();
    let upper = Point2::new(-27.61514560357321, 11.721933854678209);
    let origin = Point2::new(0.0, 0.0);
    let profile = Profile::new(
        Wire2::new(
            [
                TrimmedCurve2::line(arc.end().unwrap(), upper).unwrap(),
                TrimmedCurve2::line(upper, origin).unwrap(),
                TrimmedCurve2::line(origin, arc.start().unwrap()).unwrap(),
                arc,
            ],
            tolerance,
        ).unwrap(),
        [],
        tolerance,
    ).unwrap();
    // The long-edge axis makes the arc tangent to the axis. Its mapped
    // circle center rounds just below its radius, reversing du at the pole.
    for direction in [
        Vec3::unit_z(),
        Vec3::new(-39.07311284892735, 0.0, -92.05048534524403),
        Vec3::new(upper.x, 0.0, upper.y),
    ] {
        for angle in [std::f64::consts::PI, -std::f64::consts::PI] {
            let body = profile.revolve(
                Plane3::xz(),
                Axis3::new(Point3::new(0.0, 0.0, 0.0), direction).unwrap(),
                angle,
            ).unwrap();
            assert!(!body
                .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                .unwrap()
                .is_empty());
        }
    }
}
#[test]
fn feature_partial_revolution() {
    let p = rectangle(2.0, 0.0, 2.0, 3.0);
    let axis = Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).unwrap();
    for angle in [std::f64::consts::PI, -std::f64::consts::PI] {
        let body = p.revolve(Plane3::xy(), axis, angle).unwrap();
        assert!((body.volume() - 18.0 * std::f64::consts::PI).abs() < 1.0e-5);
        mesh(&body);
    }
    let body = rectangle(0.0, 0.0, 2.0, 3.0)
        .revolve(Plane3::xy(), axis, std::f64::consts::PI)
        .unwrap();
    assert!((body.volume() - 6.0 * std::f64::consts::PI).abs() < 1.0e-5);
    mesh(&body);
}
#[test]
fn feature_conical_revolution() {
    let t = ModelTolerance::default();
    let p = Profile::new(
        Wire2::polygon(
            [
                Point2::new(2.0, 0.0),
                Point2::new(4.0, 0.0),
                Point2::new(2.0, 2.0),
            ],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap();
    let body = p
        .revolve(
            Plane3::xy(),
            Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).unwrap(),
            std::f64::consts::TAU,
        )
        .unwrap();
    assert!((body.volume() - 32.0 / 3.0 * std::f64::consts::PI).abs() < 1.0e-5);
    mesh(&body);
}
#[test]
fn draft_preserves_neutral_section_and_sets_angle() {
    let a = circle(10.0);
    let b = circle(6.0);
    let plane = Plane3::new(Point3::new(4.0, 0.0, 20.0), Vec3::unit_x(), Vec3::unit_y(), ModelTolerance::default()).unwrap();
    let body = a.loft(Plane3::xy(), [(plane, &b)]).unwrap();
    let side = body.faces().find(|face| matches!(face.surface(), Surface3::Nurbs(_))).unwrap().clone();
    for angle in [-3.0_f64, 3.0].map(f64::to_radians) {
        let drafted = body.draft(&[side.clone()], Plane3::xy(), angle).unwrap();
        let side = drafted.faces().find(|face| matches!(face.surface(), Surface3::Offset(_))).unwrap();
        for u in [0.0, 0.17, 0.4, 0.8, 1.0] {
            let foot = side.surface().evaluate(u, 0.0).unwrap().point;
            assert!((foot.to_vec().magnitude() - 10.0).abs() < 1e-7);
            for v in [0.0, 0.3, 0.7, 1.0] {
                assert!((side.surface().normal(u, v).unwrap().z - angle.sin()).abs() < 1e-7);
            }
        }
        let r = 10.0 - 20.0 * angle.tan();
        let volume = std::f64::consts::PI * 20.0 / 3.0 * (100.0 + 10.0 * r + r * r);
        assert!((drafted.volume() - volume).abs() < volume * 1e-8);
        mesh(&drafted);
    }
}
#[test]
fn spline_sector_end_fillet() {
    let t = ModelTolerance::default();
    let spline = NurbsCurve2::new(3, vec![Point2::new(20.0, -4.0), Point2::new(22.0, -1.0), Point2::new(22.0, 1.0), Point2::new(20.0, 4.0)], vec![1.0;4], vec![0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0]).unwrap();
    let profile = Profile::new(Wire2::new([
        TrimmedCurve2::line(Point2::new(12.0,-4.0), Point2::new(20.0,-4.0)).unwrap(),
        TrimmedCurve2::new(spline, range(0.0,1.0).unwrap()).unwrap(),
        TrimmedCurve2::line(Point2::new(20.0,4.0), Point2::new(12.0,4.0)).unwrap(),
        TrimmedCurve2::line(Point2::new(12.0,4.0), Point2::new(12.0,-4.0)).unwrap(),
    ],t).unwrap(),[],t).unwrap();
    let body = profile.revolve(Plane3::xz(), Axis3::new(Point3::new(0.0,0.0,0.0),Vec3::unit_z()).unwrap(),std::f64::consts::PI*1.5).unwrap();
    let cap = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(p) if p.normal().y.abs()>0.99)).unwrap();
    let result = body.fillet(&cap.boundary_edges().unwrap(),0.5).unwrap();
    mesh(&result);
}
#[test]
fn intersecting_bores_rim_fillet() {
    let t = ModelTolerance::default();
    let body = rectangle(-15.0,-15.0,30.0,30.0).extrude(Plane3::xy(), Vec3::unit_z()*20.0).unwrap();
    let first = circle(5.0).extrude(Plane3::xy(),Vec3::unit_z()*20.0).unwrap();
    let second = first.transformed(RigidTransform3::new(crate::Matrix3::identity(),Vec3::unit_x()*6.0,t).unwrap()).unwrap();
    let body = body.difference(&first).unwrap().difference(&second).unwrap();
    let cap = body.faces().find(|face| matches!(face.surface(), Surface3::Plane(p) if p.origin().z>19.0)).unwrap();
    let result = body.fillet(&cap.boundary_edges().unwrap(),0.5).unwrap();
    mesh(&result);
}
#[test]
fn cross_bore_move() {
    let t = ModelTolerance::default();
    let body = circle(10.0).extrude(Plane3::xy(), Vec3::unit_z() * 20.0).unwrap();
    let plane = Plane3::new(Point3::new(0.0, -15.0, 10.0), Vec3::unit_x(), -Vec3::unit_z(), t).unwrap();
    let tool = circle(3.0).extrude(plane, Vec3::unit_y() * 30.0).unwrap();
    let original = body.clone();
    let body = body.difference(&tool).unwrap();
    let bore = body.faces().find(|face| matches!(face.surface(), Surface3::Cylinder(c) if c.radius() < 4.0)).unwrap().clone();
    let transform = RigidTransform3::new(crate::Matrix3::identity(), Vec3::unit_x(), t).unwrap();
    let moved = body.move_faces(&[bore], transform).unwrap();
    let expected = original.difference(&tool.transformed(transform).unwrap()).unwrap();
    assert!((moved.volume() - expected.volume()).abs() < expected.volume() * 1e-7);
    let bore = moved.faces().find(|face| matches!(face.surface(), Surface3::Cylinder(c) if c.radius() < 4.0)).unwrap();
    let Surface3::Cylinder(cylinder) = bore.surface() else { unreachable!() };
    assert!((cylinder.origin().x - 1.0).abs() < t.linear);
    assert!((cylinder.radius() - 3.0).abs() < t.linear);
    mesh(&moved);
}
#[test]
fn tapered_planar_loft_followups() {
    let a = rectangle(-12.0, -12.0, 24.0, 24.0);
    let b = rectangle(-6.0, -8.0, 16.0, 16.0);
    let body = a.loft(Plane3::xy(), [(upper(20.0), &b)]).unwrap();
    assert!(body.faces().all(|face| matches!(face.surface(), Surface3::Plane(_))));
    for face in body.faces() {
        body.offset_faces(&[face.clone()], 0.5).unwrap();
    }
    body.fillet(&body.edges().cloned().collect::<Vec<_>>(), 0.5).unwrap();
}
#[test]
fn feature_loft_sections() {
    let a = rectangle(-2.0, -2.0, 4.0, 4.0);
    let b = rectangle(-1.0, -1.0, 2.0, 2.0);
    let body = a.loft(Plane3::xy(), [(upper(3.0), &b)]).unwrap();
    assert!((body.volume() - 28.0).abs() < 1.0e-5);
    mesh(&body);
    let a = circle(2.0);
    let b = circle(1.0);
    let body = a.loft(Plane3::xy(), [(upper(3.0), &b)]).unwrap();
    assert!((body.volume() - 7.0 * std::f64::consts::PI).abs() < 1.0e-5);
    mesh(&body);
    let body = a
        .loft(Plane3::xy(), [(upper(3.0), &b), (upper(6.0), &a)])
        .unwrap();
    assert!(body.volume() > 0.0);
    mesh(&body);
}
#[test]
fn feature_loft_compatibility() {
    let t = ModelTolerance::default();
    let a = rectangle(-2.0, -2.0, 4.0, 4.0);
    let b = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-1.0, -1.0),
                Point2::new(1.0, -1.0),
                Point2::new(0.0, 1.0),
            ],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap();
    let body = a.loft(Plane3::xy(), [(upper(3.0), &b)]).unwrap();
    assert!(body.volume() > 0.0);
    mesh(&body);
    let round = circle(1.0);
    let body = a.loft(Plane3::xy(), [(upper(3.0), &round)]).unwrap();
    assert!(body.volume() > 0.0);
    mesh(&body);
    let inverted = Plane3::new(
        Point3::new(0.0, 0.0, 3.0),
        Vec3::unit_x(),
        -Vec3::unit_y(),
        t,
    )
    .unwrap();
    let body = a.loft(Plane3::xy(), [(inverted, &a)]).unwrap();
    assert!((body.volume() - 48.0).abs() < 1.0e-5);
    mesh(&body);
}
#[test]
fn bulging_square_loft_rim() {
    let a = rectangle(-10.0, -10.0, 20.0, 20.0);
    let b = rectangle(-10.0, -12.0, 24.0, 24.0);
    let c = rectangle(-7.0, -8.0, 16.0, 16.0);
    let body = a.loft(Plane3::xy(), [(upper(10.0), &b), (upper(20.0), &c)]).unwrap();
    let edges = body.edges().filter(|edge| {
        edge.endpoints().unwrap().into_iter().all(|p| (p.z - 20.0).abs() < 1e-6)
    }).cloned().collect::<Vec<_>>();
    assert_eq!(edges.len(), 4);
    let result = body.fillet(&edges, 0.5).unwrap();
    assert!(result.volume() < body.volume());
    mesh(&result);
}
#[test]
fn equal_orthogonal_bores() {
    let block = rectangle(-15.0, -15.0, 30.0, 30.0).extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0)).unwrap();
    let bore = circle(5.0).extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0)).unwrap();
    let plane = Plane3::new(Point3::new(0.0, -15.0, 10.0), Vec3::unit_x(), Vec3::unit_z(), block.tolerance()).unwrap();
    let cross = circle(5.0).extrude(plane, Vec3::new(0.0, 30.0, 0.0)).unwrap();
    let result = block.difference(&bore).unwrap().difference(&cross).unwrap();
    let expected = 18000.0 - 50.0 * std::f64::consts::PI * 25.0 + 16.0 * 125.0 / 3.0;
    assert!((result.volume() - expected).abs() < 1e-3);
    mesh(&result);
}
#[test]
fn torus_face_limited_pocket() {
    let t = ModelTolerance::default();
    let section = Profile::new(Wire2::new([TrimmedCurve2::circle(Point2::new(12.0, 0.0), 3.0).unwrap()], t).unwrap(), [], t).unwrap();
    let body = section.revolve(Plane3::xz(), Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_z()).unwrap(), std::f64::consts::TAU).unwrap();
    let pocket = Profile::new(Wire2::new([TrimmedCurve2::circle(Point2::new(12.0, 0.0), 1.0).unwrap()], t).unwrap(), [], t).unwrap();
    let limit = body.faces().find(|face| matches!(face.surface(), Surface3::Torus(_))).unwrap();
    let tool = pocket.extrude_up_to_face(Plane3::xy(), &body, limit, 0.0, 0.0).unwrap();
    let result = body.difference(&tool).unwrap();
    assert!((result.volume() + tool.volume() - body.volume()).abs() < body.volume() * 1e-6,
        "volumes {} + {} != {}", result.volume(), tool.volume(), body.volume());
    mesh(&result);
}
#[test]
fn feature_split() {
    let body = rectangle(-2.0, -2.0, 4.0, 4.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 4.0))
        .unwrap();
    assert!((body.split(upper(1.0), SplitKeep::Left).unwrap().volume() - 16.0).abs() < 1.0e-6);
    assert!((body.split(upper(1.0), SplitKeep::Right).unwrap().volume() - 48.0).abs() < 1.0e-6);
    let both = body.split(upper(1.0), SplitKeep::Both).unwrap();
    assert_eq!(both.solids().len(), 2);
    assert!((both.volume() - 64.0).abs() < 1.0e-6);
    let cylinder = circle(2.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 4.0))
        .unwrap();
    assert!(
        (cylinder.split(upper(1.0), SplitKeep::Left).unwrap().volume() - 4.0 * std::f64::consts::PI)
            .abs()
            < 1.0e-6
    );
}
#[test]
fn feature_chamfer() {
    let body = rectangle(0.0, 0.0, 10.0, 10.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .unwrap();
    let edge = body.edges().next().unwrap().clone();
    let result = body.chamfer(&[edge], 1.0).unwrap();
    assert!((result.volume() - 995.0).abs() < 1.0e-5);
    mesh(&result);
    let cylinder = circle(5.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .unwrap();
    let edge = cylinder
        .edges()
        .find(|e| matches!(e.curve(), Curve3::Circle(_)))
        .unwrap()
        .clone();
    let result = cylinder.chamfer(&[edge], 1.0).unwrap();
    assert!(result.volume() < cylinder.volume());
    mesh(&result);
}
fn filleted_asymmetric_chamfer() -> Compound {
    let body = rectangle(
        -37.04944282031129,
        -21.764692712619542,
        78.94647629000909,
        47.585532703500046,
    )
    .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 34.0))
    .unwrap();
    let edge = body
        .edges()
        .find(|e| {
            e.endpoints()
                .unwrap()
                .iter()
                .all(|p| p.x > 41.0 && p.y < -21.0)
        })
        .unwrap()
        .clone();
    let reference = edge
        .connected_faces()
        .unwrap()
        .into_iter()
        .find(|f| {
            let p = f
                .surface()
                .project(Point3::new(0.0, -21.764692712619542, 0.0), body.tolerance())
                .unwrap();
            p.distance < 1e-6
        })
        .unwrap();
    let body = body
        .chamfer_with_options(
            &[edge],
            &[reference],
            11.0,
            ChamferMode::DistanceAngle(55.0_f64.to_radians()),
            true,
        )
        .unwrap();
    let edges = body
        .edges()
        .filter(|e| {
            let p = e.endpoints().unwrap();
            let top = p.iter().all(|p| (p.z - 34.0).abs() < 1e-6);
            (top && p.iter().all(|p| p.x > 25.0 || p.y < -20.0))
                || (p[0].z - p[1].z).abs() > 33.0
                    && (p[0].x > 25.0 && p[0].y < -5.0 || p[0].x < -37.0 && p[0].y > 25.0)
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 6);
    body.fillet(&edges, 6.0).unwrap()
}

fn shell_rim_document_23() -> (Compound, super::super::EdgeHandle) {
    // untitled(23).cad: 70 x 50 x 34, an 11 mm / 55 degree chamfer,
    // six 6 mm fillets, a rounded 6 mm outward shell, then its bottom rim.
    let x0 = -28.352029411716757;
    let y0 = -22.437515961363786;
    let x1 = x0 + 70.0;
    let y1 = y0 + 50.0;
    let body = rectangle(x0, y0, 70.0, 50.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 34.0))
        .unwrap();
    let edge = body
        .edges()
        .find(|e| {
            e.endpoints()
                .unwrap()
                .iter()
                .all(|p| (p.x - x1).abs() < 1e-6 && (p.y - y0).abs() < 1e-6)
        })
        .unwrap()
        .clone();
    let reference = edge
        .connected_faces()
        .unwrap()
        .into_iter()
        .find(|f| {
            f.surface()
                .project(Point3::new(0.0, y0, 0.0), body.tolerance())
                .unwrap()
                .distance
                < 1e-6
        })
        .unwrap();
    let body = body
        .chamfer_with_options(
            &[edge],
            &[reference],
            11.0,
            ChamferMode::DistanceAngle(55.0_f64.to_radians()),
            true,
        )
        .unwrap();
    let edges = body
        .edges()
        .filter(|e| {
            let p = e.endpoints().unwrap();
            let top = p.iter().all(|p| (p.z - 34.0).abs() < 1e-6);
            (top && p.iter().all(|p| p.x > x1 - 17.0 || p.y < y0 + 1.0))
                || (p[0].z - p[1].z).abs() > 33.0
                    && (p[0].x > x1 - 17.0 && p[0].y < y0 + 17.0
                        || p[0].x < x0 + 1e-6 && p[0].y > y1 - 1e-6)
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 6);
    let body = body.fillet(&edges, 6.0).unwrap();
    let opening = body
        .faces()
        .find(|f| {
            matches!(f.surface(), Surface3::Plane(p)
        if p.normal().z.abs() > 0.99 && p.origin().z.abs() < 1e-6)
        })
        .unwrap()
        .clone();
    let body = body.shell_with_join(&[opening], 6.0, ShellJoin::Rounded).unwrap();
    let edge = body
        .edges()
        .find(|e| {
            let p = e.endpoints().unwrap();
            p.iter()
                .all(|p| p.z.abs() < 1e-6 && p.x > x1 - 17.0 && p.y < y0 + 17.0)
                && matches!(e.curve(), Curve3::Line(_))
                && (p[0].x - p[1].x).abs() > 1.0
                && (p[0].y - p[1].y).abs() > 1.0
        })
        .unwrap()
        .clone();
    (body, edge)
}

#[test]
fn second_shell_rim_fillet_document_23() {
    let (body, edge) = shell_rim_document_23();
    let body = body.fillet(&[edge], 3.0).unwrap();
    // The remaining inner rim is a 6 mm corner arc and two straight
    // edges. At either end it meets a concave edge of the first fillet.
    let edges = body.edges().filter(|edge| {
        let points = edge.endpoints().unwrap();
        points.iter().all(|p| p.z.abs() < 1e-6)
            && (points.iter().all(|p| (p.x + 28.352029411716757).abs() < 1e-6)
                || points.iter().all(|p| (p.y - 27.562484038636214).abs() < 1e-6)
                || matches!(edge.curve(), Curve3::Circle(c)
                    if c.center().distance(Point3::new(-22.352029411716757, 21.562484038636214, 0.0)) < 1e-6
                        && (c.radius() - 6.0).abs() < 1e-6))
    }).cloned().collect::<Vec<_>>();
    assert_eq!(edges.len(), 3);
    // At 3 mm the two terminating arcs are consumed completely.
    for radius in [0.25, 0.5, 1.0, 2.0, 2.999, 3.0] {
        let mut expected_volume: Option<f64> = None;
        for edge in &edges {
            let result = body
                .fillet(&[edge.clone()], radius)
                .unwrap_or_else(|error| panic!("second rim fillet {radius}: {error}"));
            assert!(result.volume() < body.volume());
            if let Some(volume) = expected_volume {
                assert!((result.volume() - volume).abs() < 1e-6);
            }
            expected_volume = Some(result.volume());
            mesh(&result);
        }
    }
}

#[test]
fn oversized_shell_rim_fillet_document_23() {
    let (body, edge) = shell_rim_document_23();
    let original_volume = body.volume();
    for radius in [1.0, 4.0, 5.99, 6.0, 6.01, 7.0, 11.0, 12.0] {
        let result = body.fillet(&[edge.clone()], radius);
        if radius < 6.0 {
            mesh(&result.unwrap());
        } else {
            let error =
                result.expect_err(&format!("radius {radius} published invalid geometry"));
            assert!(error
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.kind
                    == super::super::DiagnosticKind::GeometryInvariant));
        }
        assert_eq!(body.volume(), original_volume);
    }
}

#[test]
fn shell_filleted_asymmetric_chamfer() {
    let body = filleted_asymmetric_chamfer();
    for face in body
        .faces()
        .filter(|f| matches!(f.surface(), Surface3::Sphere(_)))
    {
        assert_eq!(
            body.topology()
                .face(face.topology_id())
                .unwrap()
                .orientation(),
            super::super::FaceOrientation::Forward
        );
    }
    let opening = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.normal().z.abs() > 0.99 && p.origin().z.abs() < 1e-6)).unwrap().clone();
    for openings in [vec![], vec![opening]] {
        for distance in [-2.0, -1.0, -0.5, 0.5, 1.0, 2.0] {
            for join in [ShellJoin::Sharp, ShellJoin::Rounded] {
                let result = body
                    .shell_with_join(&openings, distance, join)
                    .unwrap_or_else(|error| {
                        panic!(
                            "openings {:?}, distance {distance}, join {join:?}: {error}",
                            openings.iter().map(|f| f.topology_id()).collect::<Vec<_>>()
                        )
                    });
                mesh(&result);
            }
        }
    }
}
#[test]
fn cut_shelled_asymmetric_chamfer() {
    for thickness in [5.0, 6.0] {
        let body = filleted_asymmetric_chamfer();
        let opening = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.normal().z.abs() > 0.99 && p.origin().z.abs() < 1e-6)).unwrap().clone();
        let body = body.shell(&[opening], thickness).unwrap();
        let tol = body.tolerance();
        let profile = Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(
                    Point2::new(-21.08780917145651, 5.736491377054718),
                    32.892404807198155,
                )
                .unwrap()],
                tol,
            )
            .unwrap(),
            [],
            tol,
        )
        .unwrap();
        let mut previous_volume = body.volume();
        let distances: &[f64] = if thickness == 5.0 {
            &[5.0, 10.0, 10.99, 11.0, 11.01, 12.0, 15.0, 20.0, 30.0, 40.0]
        } else {
            &[14.0]
        };
        for &distance in distances {
            let tool = profile
                .extrude(upper(34.0 + thickness), Vec3::new(0.0, 0.0, -distance))
                .unwrap();
            let cut = body
                .difference(&tool)
                .unwrap_or_else(|e| panic!("distance {distance}: {e}"));
            assert_eq!(
                cut.solids().len(),
                if distance < (34.0 + thickness) { 1 } else { 2 }
            );
            assert!(cut.volume() > 0.0 && cut.volume() < body.volume());
            assert!(cut.volume() <= previous_volume + 1e-5);
            previous_volume = cut.volume();
            mesh(&cut);
            if thickness == 5.0 && [10.0, 12.0, 20.0].contains(&distance)
                || thickness == 6.0 && distance == 14.0
            {
                for radius in if thickness == 5.0 {
                    vec![1.0]
                } else {
                    vec![0.05, 0.1, 0.25]
                } {
                    let edge = cut
                        .edges()
                        .find(|e| {
                            matches!(e.curve(), Curve3::Circle(c)
                    if (c.center().z - (34.0 + thickness)).abs() < 1e-6
                        && (c.radius() - 32.892404807198155).abs() < 1e-6)
                        })
                        .unwrap()
                        .clone();
                    let rounded = cut
                        .fillet(&[edge], radius)
                        .unwrap_or_else(|e| panic!("shell {thickness}, cut {distance}, fillet {radius}: {e}"));
                    assert_eq!(rounded.solids().len(), 1);
                    assert!(rounded.volume() > 0.0 && rounded.volume() < cut.volume());
                    mesh(&rounded);
                }
            }
        }
    }
}
#[test]
fn feature_asymmetric_chamfer() {
    let body = rectangle(0.0, 0.0, 20.0, 20.0)
        .extrude(Plane3::xy(), Vec3::new(3.0, 2.0, 10.0))
        .unwrap();
    for edge in body.edges() {
        for reference in edge.connected_faces().unwrap() {
            for mode in [
                ChamferMode::TwoDistances(2.0),
                ChamferMode::DistanceAngle(0.6),
            ] {
                let result = body
                    .chamfer_with_options(
                        &[edge.clone()],
                        &[reference.clone()],
                        1.0,
                        mode,
                        false,
                    )
                    .unwrap();
                let bevel = result
                    .faces()
                    .find(|face| {
                        body.faces()
                            .all(|old| old.persistent_id() != face.persistent_id())
                    })
                    .unwrap();
                let mut setbacks = Vec::new();
                for boundary in bevel.boundary_edges().unwrap() {
                    let points = boundary.endpoints().unwrap();
                    let midpoint =
                        Point3::from_vec((points[0].to_vec() + points[1].to_vec()) * 0.5);
                    if (points[1] - points[0])
                        .normalize()
                        .cross(edge.endpoints().unwrap()[1] - edge.endpoints().unwrap()[0])
                        .magnitude()
                        < 1e-6
                    {
                        let distance = edge
                            .curve()
                            .project(midpoint, body.tolerance())
                            .unwrap()
                            .distance;
                        if reference
                            .surface()
                            .project(midpoint, body.tolerance())
                            .unwrap()
                            .distance
                            < 1e-6
                        {
                            assert!((distance - 1.0).abs() < 1e-6);
                        }
                        setbacks.push(distance);
                    }
                }
                assert_eq!(setbacks.len(), 2);
                if let ChamferMode::TwoDistances(_) = mode {
                    setbacks.sort_by(f64::total_cmp);
                    assert!(
                        (setbacks[0] - 1.0).abs() < 1e-6 && (setbacks[1] - 2.0).abs() < 1e-6
                    );
                } else {
                    let n0 = reference.surface().normal(0.0, 0.0).unwrap();
                    let n1 = bevel.surface().normal(0.0, 0.0).unwrap();
                    assert!((n0.dot(n1).abs().clamp(-1.0, 1.0).acos() - 0.6).abs() < 1e-6);
                }
                mesh(&result);
            }
        }
    }
    let cylinder = circle(10.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 20.0))
        .unwrap();
    let edge = cylinder
        .edges()
        .find(|edge| matches!(edge.curve(), Curve3::Circle(_)))
        .unwrap();
    for reference in edge.connected_faces().unwrap() {
        for mode in [
            ChamferMode::TwoDistances(2.0),
            ChamferMode::DistanceAngle(0.6),
        ] {
            let second = match mode {
                ChamferMode::TwoDistances(d) => d,
                _ => 0.6_f64.tan(),
            };
            let (radial, axial) = if matches!(reference.surface(), Surface3::Plane(_)) {
                (1.0, second)
            } else {
                (second, 1.0)
            };
            let result = cylinder
                .chamfer_with_options(&[edge.clone()], &[reference.clone()], 1.0, mode, true)
                .unwrap();
            let removed =
                std::f64::consts::PI * axial * (10.0 * radial - radial * radial / 3.0);
            assert!((cylinder.volume() - result.volume() - removed).abs() < 1e-5);
            mesh(&result);
        }
    }
}

#[test]
fn feature_concave_chamfer() {
    let tolerance = ModelTolerance::default();
    let outer = rectangle(0.0, 0.0, 10.0, 10.0);
    let hole = rectangle(3.0, 3.0, 4.0, 4.0);
    let body = Profile::new(outer.outer().clone(), [hole.outer().clone()], tolerance)
        .unwrap()
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .unwrap();
    let edge = body
        .edges()
        .find(|edge| {
            edge.endpoints().is_ok_and(|points| {
                points
                    .iter()
                    .all(|p| (p.x - 3.0).abs() < 1.0e-7 && (p.y - 3.0).abs() < 1.0e-7)
            })
        })
        .unwrap()
        .clone();
    for reference in edge.connected_faces().unwrap() {
        for mode in [ChamferMode::TwoDistances(2.0), ChamferMode::DistanceAngle(0.6)] {
            let second = match mode { ChamferMode::TwoDistances(d) => d, _ => 0.6_f64.tan() };
            let result = body.chamfer_with_options(&[edge.clone()], &[reference.clone()], 1.0, mode, false).unwrap();
            assert!((result.volume() - body.volume() - 5.0 * second).abs() < 1e-5);
            mesh(&result);
        }
    }
    let result = body.chamfer(&[edge], 1.0).unwrap();
    assert!((result.volume() - 845.0).abs() < 1.0e-5);
    mesh(&result);
}
#[test]
fn feature_edge_cases() {
    let t = ModelTolerance::default();
    let axis = Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_y()).unwrap();
    let cone = Profile::new(
        Wire2::polygon(
            [
                Point2::new(0.0, 0.0),
                Point2::new(3.0, 0.0),
                Point2::new(0.0, 4.0),
            ],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap()
    .revolve(Plane3::xy(), axis, std::f64::consts::TAU)
    .unwrap();
    assert!((cone.volume() - 12.0 * std::f64::consts::PI).abs() < 1.0e-5);
    mesh(&cone);
    let p = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(Point2::new(5.0, 0.0), 1.0).unwrap()],
            t,
        )
        .unwrap(),
        [],
        t,
    )
    .unwrap();
    let torus = p.revolve(Plane3::xy(), axis, std::f64::consts::PI).unwrap();
    assert!((torus.volume() - 5.0 * std::f64::consts::PI.powi(2)).abs() < 1.0e-5);
    mesh(&torus);
    let body = rectangle(0.0, 0.0, 10.0, 10.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .unwrap();
    let result = body
        .chamfer(&body.edges().cloned().collect::<Vec<_>>(), 1.0)
        .unwrap();
    assert!(result.volume() < 1000.0);
    mesh(&result);
    let cylinder = circle(5.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .unwrap();
    let rims = cylinder
        .edges()
        .filter(|e| matches!(e.curve(), Curve3::Circle(_)))
        .cloned()
        .collect::<Vec<_>>();
    let result = cylinder.chamfer(&rims, 1.0).unwrap();
    assert!(result.volume() < cylinder.volume());
    mesh(&result);
    // Contact with the end rims requires a topology change; the retained
    // cylinder trim must not publish an unrepresented pinch point.
    let touching = Plane3::from_origin_normal(
        Point3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 1.0, 1.0),
    ).unwrap();
    assert!(cylinder.split(touching, SplitKeep::Both).is_err());
    let slanted =
        Plane3::from_origin_normal(Point3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.8, 1.0))
            .unwrap();
    let both = cylinder.split(slanted, SplitKeep::Both).unwrap();
    assert_eq!(both.solids().len(), 2);
    assert!((both.volume() - cylinder.volume()).abs() < 1.0e-5);
    assert!((body.split(upper(20.0), SplitKeep::Left).unwrap().volume() - body.volume()).abs() < 1.0e-7);
    assert_eq!(body.split(upper(20.0), SplitKeep::Right).unwrap().solids().len(), 0);
    let face = cylinder
        .faces()
        .find(|f| matches!(f.surface(), Surface3::Cylinder(_)))
        .unwrap()
        .clone();
    let wider = cylinder
        .draft(&[face], Plane3::xy(), -0.1_f64.atan())
        .unwrap();
    assert!(wider.volume() > cylinder.volume());
    mesh(&wider);
}
#[test]
fn feature_rounded_corner_draft() {
    let body = rounded_corner_block();
    let faces = body
        .faces()
        .filter(|f| match f.surface() {
            Surface3::Cylinder(_) => true,
            Surface3::Plane(p) => {
                (p.normal().x.abs() > 0.9 && (p.origin().x - 30.0).abs() < 1e-7)
                    || (p.normal().y.abs() > 0.9 && (p.origin().y - 17.0).abs() < 1e-7)
            }
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(faces.len(), 3);
    for angle in [5_f64.to_radians(), -5_f64.to_radians()] {
        let result = body.draft(&faces, upper(10.0), angle).unwrap();
        assert_eq!(result.faces().count(), body.faces().count());
        let corner_loss = 1.0 - std::f64::consts::FRAC_PI_4;
        let expected = body.volume()
            + (160.0 - 20.0 * corner_loss) * angle.tan() * 50.0
            + (1.0 - corner_loss) * angle.tan().powi(2) * 1000.0 / 3.0;
        // The general BREP mass integrator uses dimensional quadrature
        // targets; check its volume separately from the exact section radii.
        assert!(
            (result.volume() - expected).abs() < expected * 1e-6,
            "draft angle {angle}: volume {}, expected {expected}",
            result.volume()
        );
        let cone = result
            .faces()
            .find_map(|face| match face.surface() {
                Surface3::Cone(cone) => Some(*cone),
                _ => None,
            })
            .unwrap();
        for z in [0.0, 10.0] {
            let v = (z - cone.apex().z) / cone.axis().z;
            let radius = cone.reference_radius() * v;
            assert!((radius - (10.0 + (10.0 - z) * angle.tan())).abs() < 1e-7);
        }
        mesh(&result);
    }
}
#[test]
fn feature_chamfer_application_coordinates() {
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        -Vec3::unit_z(),
        ModelTolerance::default(),
    )
    .unwrap();
    let body = rectangle(-72.2352286216668, -32.617235, 144.4704572433336, 65.23447)
        .extrude(plane, Vec3::new(0.0, 21.0, 0.0))
        .unwrap();
    let edge = body
        .edges()
        .find(|e| {
            e.endpoints().is_ok_and(|p| {
                p.iter().all(|p| {
                    (p.x + 72.2352286216668).abs() < 1e-7 && (p.z - 32.617235).abs() < 1e-7
                })
            })
        })
        .unwrap()
        .clone();
    let body = body.fillet(&[edge], 20.0).unwrap();
    let edge = body
        .edges()
        .find(|e| {
            matches!(e.curve(), Curve3::Line(_))
                && e.endpoints().is_ok_and(|p| {
                    p.iter()
                        .all(|p| (p.y - 21.0).abs() < 1e-7 && (p.z - 32.617235).abs() < 1e-7)
                })
        })
        .unwrap()
        .clone();
    let result = body.chamfer(&[edge], 1.0).unwrap();
    assert_eq!(result.solids().len(), 1);
    assert!(result.volume() < body.volume());
    mesh(&result);
}
#[test]
fn feature_rounded_corner_chamfer() {
    for corner in [(30.0, 17.0), (130.0, 17.0), (130.0, 77.0), (30.0, 77.0)] {
        for rotation in [
            crate::Matrix3::identity(),
            crate::Matrix3::from_angle_x(cgmath::Deg(-90.0)),
            crate::Matrix3::from_angle_y(cgmath::Deg(37.0)),
        ] {
            let transform = super::super::RigidTransform3::new(
                rotation,
                Vec3::new(2.2352286216668, 11.0, 32.617235),
                ModelTolerance::default(),
            )
            .unwrap();
            let body = rounded_corner_block_at(corner)
                .transformed(transform)
                .unwrap();
            let edges = body
                .edges()
                .filter(|e| matches!(e.curve(), Curve3::Line(_)))
                .filter(|e| {
                    e.endpoints().is_ok_and(|p| {
                        p.iter().all(|p| {
                            (transform.inverse().transform_point(*p).z - 10.0).abs() < 1e-7
                        })
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            for edge in edges {
                let endpoints = edge.endpoints().unwrap();
                let result = body
                    .chamfer(&[edge], 1.0)
                    .unwrap_or_else(|e| panic!("chamfer at {endpoints:?}: {e:?}"));
                assert!(result.volume() < body.volume());
                mesh(&result);
            }
        }
    }
}
fn rounded_corner_block() -> Compound {
    rounded_corner_block_at((30.0, 17.0))
}
fn rounded_corner_block_at(corner: (f64, f64)) -> Compound {
    let body = rectangle(30.0, 17.0, 100.0, 60.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .unwrap();
    let edge = body
        .edges()
        .find(|e| {
            e.endpoints().is_ok_and(|p| {
                p.iter()
                    .all(|p| (p.x - corner.0).abs() < 1e-7 && (p.y - corner.1).abs() < 1e-7)
            })
        })
        .unwrap()
        .clone();
    body.fillet(&[edge], 10.0).unwrap()
}
#[test]
fn feature_draft_reversed_axis_and_cone() {
    let cylinder = circle(5.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, -10.0))
        .unwrap();
    let face = cylinder
        .faces()
        .find(|f| matches!(f.surface(), Surface3::Cylinder(_)))
        .unwrap()
        .clone();
    let result = cylinder
        .draft(&[face], Plane3::xy(), 0.1_f64.atan())
        .unwrap();
    assert!((result.volume() - 910.0 / 3.0 * std::f64::consts::PI).abs() < 1.0e-5);
    mesh(&result);
    let face = result
        .faces()
        .find(|f| matches!(f.surface(), Surface3::Cone(_)))
        .unwrap()
        .clone();
    let changed = result.draft(&[face], Plane3::xy(), 0.2_f64.atan()).unwrap();
    assert!(changed.volume() > result.volume());
    mesh(&changed);
}
#[test]
fn feature_draft() {
    let body = rectangle(-5.0, -5.0, 10.0, 10.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .unwrap();
    let faces = body
        .faces()
        .filter(|f| matches!(f.surface(),Surface3::Plane(p) if p.normal().z.abs()<0.1))
        .cloned()
        .collect::<Vec<_>>();
    let angle = 0.1_f64.atan();
    let result = body.draft(&faces, Plane3::xy(), angle).unwrap();
    assert!((result.volume() - 2440.0 / 3.0).abs() < 1.0e-5);
    mesh(&result);
    let cylinder = circle(5.0)
        .extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 10.0))
        .unwrap();
    let face = cylinder
        .faces()
        .find(|f| matches!(f.surface(), Surface3::Cylinder(_)))
        .unwrap()
        .clone();
    let result = cylinder.draft(&[face], Plane3::xy(), angle).unwrap();
    assert!((result.volume() - 610.0 / 3.0 * std::f64::consts::PI).abs() < 1.0e-5);
    mesh(&result);
}
/// A guide drawn in the XZ plane, lifted into space
fn xz_guide(curves: impl IntoIterator<Item = TrimmedCurve2>) -> Wire3 {
    let t = ModelTolerance::default();
    let plane = Plane3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x(), Vec3::unit_z(), t).unwrap();
    Wire2::new(curves, t).unwrap().lifted(plane).unwrap()
}
#[test]
fn loft_along_straight_guides_matches_prism() {
    let a = rectangle(-2.0, -2.0, 4.0, 4.0);
    let guide = |x: f64, y: f64| {
        let line = Line3::new(Point3::new(x, y, 0.0), Vec3::unit_z() * 10.0).unwrap();
        Wire3::new([TrimmedCurve3::new(line, range(0.0, 1.0).unwrap()).unwrap()], ModelTolerance::default())
            .unwrap()
    };
    let body = a.loft_with_guides(Plane3::xy(), [(upper(10.0), &a)], true, &[guide(2.0, 2.0)]).unwrap();
    assert!((body.volume() - 160.0).abs() < 1.0e-6, "{}", body.volume());
    let body = a
        .loft_with_guides(Plane3::xy(), [(upper(10.0), &a)], true, &[guide(2.0, -2.0), guide(-2.0, 2.0)])
        .unwrap();
    assert!((body.volume() - 160.0).abs() < 1.0e-6, "{}", body.volume());
    mesh(&body);
}
#[test]
fn loft_follows_curved_guide() {
    let a = circle(2.0);
    let arc = TrimmedCurve2::arc_through(Point2::new(2.0, 0.0), Point2::new(4.0, 5.0), Point2::new(2.0, 10.0)).unwrap();
    let body = a.loft_with_guides(Plane3::xy(), [(upper(10.0), &a)], true, &[xz_guide([arc])]).unwrap();
    // A single guide carries every section along unchanged and level, so
    // Cavalieri keeps the cylinder volume.
    assert!((body.volume() - 40.0 * std::f64::consts::PI).abs() < 1.0e-4, "{}", body.volume());
    let triangles = body.tessellate(TessellationOptions::new(0.01, 0.15, 24).unwrap()).unwrap();
    let reach = triangles.positions.iter().map(|p| p.x).fold(f64::MIN, f64::max);
    assert!((reach - 4.0).abs() < 1.0e-3, "{reach}");
}
#[test]
fn loft_guide_between_section_vertices_splits_sections() {
    let a = rectangle(-2.0, -2.0, 4.0, 4.0);
    let b = circle(2.0);
    let arc = TrimmedCurve2::arc_through(Point2::new(2.0, 0.0), Point2::new(3.0, 5.0), Point2::new(2.0, 10.0)).unwrap();
    let body = a.loft_with_guides(Plane3::xy(), [(upper(10.0), &b)], true, &[xz_guide([arc])]).unwrap();
    assert!(body.volume() > 0.0);
    mesh(&body);
    let miss = TrimmedCurve2::line(Point2::new(5.0, 0.0), Point2::new(5.0, 10.0)).unwrap();
    assert!(a.loft_with_guides(Plane3::xy(), [(upper(10.0), &b)], true, &[xz_guide([miss])]).is_err());
}
