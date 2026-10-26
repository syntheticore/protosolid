//! Fillets, chamfers and their edge-network transitions.

use super::*;

#[test]
fn fillet_modes_build_and_tessellate_an_isolated_box_edge() {
    let body = owned_cuboid(Point3::origin(), Point3::new(20.0, 10.0, 5.0));
    let edge = body.edges().find(|edge| {
        let points = edge.endpoints().unwrap();
        (points[1].z - points[0].z).abs() > 4.9
    }).unwrap().clone();
    for width in [false, true] {
        for curvature in [false, true] {
            let result = body.fillet_with_options(&[edge.clone()], FilletOptions::new(1.0)
                .with_constant_width(width).with_curvature_continuity(curvature)).unwrap();
            assert_eq!(result.faces().count(), 7);
            assert!(result.volume() > 0.0 && result.volume() < body.volume());
            result.tessellate(TessellationOptions::new(0.05, 0.1, 24).unwrap()).unwrap();
            if !width && !curvature { continue; }
            let blend = result.faces().find_map(|face| match face.surface() {
                Surface3::Blend(blend) => Some(blend), _ => None,
            }).unwrap();
            let parameter = blend.center_range().middle();
            let first = result.faces().find(|face| face.surface() == blend.first_surface()).unwrap();
            let second = result.faces().find(|face| face.surface() == blend.second_surface()).unwrap();
            let contacts = [blend.first_pcurve(), blend.second_pcurve()].map(|pcurve| pcurve.evaluate(parameter).unwrap().point);
            let points = [first, second].into_iter().zip(contacts).map(|(face, uv)| {
                face.surface().evaluate(uv.x, uv.y).unwrap().point
            }).collect::<Vec<_>>();
            let expected_width = if width { 1.0 } else { 2.0_f64.sqrt() };
            assert!((points[0].distance(points[1]) - expected_width).abs() < 1.0e-8);
            if curvature {
                let surface = Surface3::Blend(blend.clone());
                for v in [0.0, 1.0] {
                    let value = surface.evaluate(parameter, v).unwrap();
                    let normal = surface.normal(parameter, v).unwrap();
                    assert!(normal.dot(value.dvv).abs() < 1.0e-8, "planar rail must have zero transverse curvature");
                }
            }
        }
    }
}

#[test]
fn fillet_modes_build_and_tessellate_a_closed_cylinder_rim() {
    let tolerance = ModelTolerance::default();
    let body = Profile::new(Wire2::new([TrimmedCurve2::circle(Point2::origin(), 5.0).unwrap()], tolerance).unwrap(),
        [], tolerance).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 10.0).unwrap();
    let edge = body.edges().find(|edge| matches!(edge.curve(), Curve3::Circle(_))
        && edge.endpoints().unwrap().iter().all(|point| point.z > 9.9)).unwrap().clone();
    for width in [false, true] {
        for curvature in [false, true] {
            let result = body.fillet_with_options(&[edge.clone()], FilletOptions::new(0.5)
                .with_constant_width(width).with_curvature_continuity(curvature))
                .unwrap_or_else(|error| panic!("width {width}, G2 {curvature}: {error}"));
            assert!(result.volume() > 0.0 && result.volume() < body.volume());
            result.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
        }
    }
}

#[test]
fn fillet_modes_build_a_constant_width_on_a_tilted_cylinder_rim() {
    let tolerance = ModelTolerance::default();
    let cylinder = Profile::new(Wire2::new([TrimmedCurve2::circle(Point2::origin(), 5.0).unwrap()], tolerance).unwrap(),
        [], tolerance).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 15.0).unwrap();
    let cap = Plane3::new(Point3::new(0.0, 0.0, 10.0), Vec3::new(1.0, 0.0, 0.4), Vec3::unit_y(), tolerance).unwrap();
    let body = cylinder.split(cap, SplitKeep::Left).unwrap();
    let rim = body.edges().find(|edge| !matches!(edge.curve(), Curve3::Line(_))
        && edge.endpoints().unwrap().iter().all(|point| point.z > 7.0)).unwrap().clone();
    for curvature in [false, true] {
        let result = body.fillet_with_options(&[rim.clone()], FilletOptions::new(0.5)
            .with_constant_width(true).with_curvature_continuity(curvature)).unwrap();
        assert!(result.volume() > 0.0 && result.volume() < body.volume());
        result.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
        for face in result.faces() {
            let Surface3::Blend(blend) = face.surface() else { continue; };
            let range = blend.center_range();
            for index in 0..=16 {
                let u = range.start + range.span() * index as f64 / 16.0;
                let points = [(&blend.first_surface(), blend.first_pcurve()),
                    (&blend.second_surface(), blend.second_pcurve())].map(|(surface, pcurve)| {
                    let uv = pcurve.evaluate(u).unwrap().point;
                    surface.evaluate(uv.x, uv.y).unwrap().point
                });
                assert!((points[0].distance(points[1]) - 0.5).abs() < 1.0e-8);
                for (v, parent) in [(0.0, blend.first_surface()), (1.0, blend.second_surface())] {
                    let point = face.surface().evaluate(u, v).unwrap().point;
                    let uv = parent.project(point, tolerance).unwrap().parameters;
                    let normal = parent.normal(uv.0, uv.1).unwrap();
                    assert!(normal.cross(face.surface().normal(u, v).unwrap()).magnitude() < 1.0e-7);
                }
            }
        }
    }
}

#[test]
fn g2_fillet_follows_a_rim_across_existing_g2_corner_blends() {
    let body = owned_cuboid(
        Point3::new(-29.827522056939532, 0.0, -22.259344819724575),
        Point3::new(33.88366933499765, 37.5, 26.810144205090438),
    );
    let vertical = body.edges().filter(|edge| {
        let points = edge.endpoints().unwrap();
        (points[0].y - points[1].y).abs() > 37.4
    }).cloned().collect::<Vec<_>>();
    assert_eq!(vertical.len(), 4);
    for first_g2 in [false, true] {
        let rounded = body.fillet_with_options(&vertical,
            FilletOptions::new(12.5).with_curvature_continuity(first_g2)).unwrap();
        let rim = rounded.edges().find(|edge| matches!(edge.curve(), Curve3::Line(_))
            && edge.endpoints().unwrap().iter().all(|point| point.y > 37.4 && point.z > 26.8))
            .unwrap().clone();
        let result = rounded.fillet_with_options(&[rim],
            FilletOptions::new(5.0).with_curvature_continuity(true)).unwrap();
        assert_eq!(result.solids().len(), 1);
        assert_eq!(result.faces().count(), rounded.faces().count() + 8,
            "the fillet must propagate around all eight straight and rounded rim segments");
        assert!(result.volume() > 0.0 && result.volume() < rounded.volume());
        result.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
    }
}

#[test]
fn width_fillet_follows_a_rim_across_existing_corner_blends() {
    let body = owned_cuboid(
        Point3::new(0.0, -27.898378837931627, -52.23526250506349),
        Point3::new(45.0, 20.37966681068764, 21.962553553265337),
    );
    let vertical = body.edges().filter(|edge| {
        let points = edge.endpoints().unwrap();
        (points[0].y - points[1].y).abs() > 48.0
    }).cloned().collect::<Vec<_>>();
    for (first_g2, curvature, width) in [(true, false, 10.0), (true, true, 10.0), (false, false, 6.0)] {
        let rounded = body.fillet_with_options(&vertical,
            FilletOptions::new(12.5).with_curvature_continuity(first_g2)).unwrap();
        let rim = rounded.edges().find(|edge| matches!(edge.curve(), Curve3::Line(_))
            && edge.endpoints().unwrap().iter().all(|point|
                (point.x - 45.0).abs() < 1.0e-7 && point.y < -27.8)).unwrap().clone();
        let result = rounded.fillet_with_options(std::slice::from_ref(&rim),
            FilletOptions::new(width).with_constant_width(true).with_curvature_continuity(curvature)).unwrap();
        assert_eq!(result.faces().count(), 18);
        assert!(result.volume() > 0.0 && result.volume() < rounded.volume());
        result.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
        for face in result.faces() {
            let Surface3::Blend(blend) = face.surface() else { continue; };
            if !matches!(blend.center_curve(), Curve3::SurfaceIntersection(curve) if curve.constant_width.is_some()) {
                continue;
            }
            for fraction in [0.0, 0.5, 1.0] {
                let u = blend.center_range().start + blend.center_range().span() * fraction;
                let contacts = [(blend.first_surface(), blend.first_pcurve()),
                    (blend.second_surface(), blend.second_pcurve())].map(|(surface, pcurve)| {
                    let uv = pcurve.evaluate(u).unwrap().point;
                    surface.evaluate(uv.x, uv.y).unwrap().point
                });
                assert!((contacts[0].distance(contacts[1]) - width).abs() < 1.0e-7);
            }
        }
        let opening = result.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
            if (plane.origin().y - 20.37966681068764).abs() < 1.0e-7)).unwrap().clone();
        let shell = result.shell(std::slice::from_ref(&opening), 1.0).unwrap();
        assert!(shell.volume() > 0.0);
        shell.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
        let error = result.shell(&[opening], -8.0).unwrap_err();
        assert!(error.diagnostics().iter().all(|diagnostic|
            diagnostic.kind == super::super::DiagnosticKind::UnsupportedGeometry));
    }
}

#[test]
fn width_fillet_keeps_its_chord_across_smooth_supports_on_an_inclined_rim() {
    let tolerance = ModelTolerance::default();
    let centers = [(16.0, 4.0), (16.0, 16.0), (4.0, 16.0), (4.0, 4.0)];
    let corners = centers.iter().enumerate().map(|(index, center)| {
        TrimmedCurve2::new(crate::Circle2::new(Point2::new(center.0, center.1), 4.0).unwrap(),
            ParameterRange::new((index as f64 - 1.0) * std::f64::consts::FRAC_PI_2,
                index as f64 * std::f64::consts::FRAC_PI_2).unwrap()).unwrap()
    }).collect::<Vec<_>>();
    let mut segments = Vec::new();
    for index in 0..4 {
        segments.push(corners[index].clone());
        segments.push(TrimmedCurve2::line(corners[index].end().unwrap(), corners[(index + 1) % 4].start().unwrap()).unwrap());
    }
    let body = Profile::new(Wire2::new(segments, tolerance).unwrap(), [], tolerance).unwrap()
        .extrude(Plane3::xy(), Vec3::unit_z() * 20.0).unwrap();
    let cap = Plane3::new(Point3::new(0.0, 0.0, 7.0), Vec3::new(1.0, 0.0, 0.1),
        Vec3::new(0.0, 1.0, 0.2), tolerance).unwrap();
    let body = body.split(cap, SplitKeep::Left).unwrap();
    let rim = body.edges().find(|edge| matches!(edge.curve(), Curve3::Line(_))
        && edge.endpoints().unwrap().iter().all(|point|
            (*point - cap.origin()).dot(cap.normal()).abs() < tolerance.linear)).unwrap().clone();
    let result = body.fillet_with_options(&[rim], FilletOptions::new(1.0).with_constant_width(true)).unwrap();
    assert_eq!(result.faces().count(), 18);
    result.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
    let mut minimum_radius = f64::INFINITY;
    let mut maximum_radius = 0.0_f64;
    for face in result.faces() {
        let Surface3::Blend(blend) = face.surface() else { continue; };
        for fraction in [0.0, 0.5, 1.0] {
            let u = blend.center_range().start + blend.center_range().span() * fraction;
            let contacts = [(blend.first_surface(), blend.first_pcurve()),
                (blend.second_surface(), blend.second_pcurve())].map(|(surface, pcurve)| {
                let uv = pcurve.evaluate(u).unwrap().point;
                surface.evaluate(uv.x, uv.y).unwrap().point
            });
            assert!((contacts[0].distance(contacts[1]) - 1.0).abs() < 1.0e-7);
            let radius = contacts[0].distance(blend.center_curve().evaluate(u).unwrap().point);
            minimum_radius = minimum_radius.min(radius);
            maximum_radius = maximum_radius.max(radius);
        }
    }
    assert!(maximum_radius - minimum_radius > 0.05);
}

#[test]
fn shell_after_two_g2_fillets_meshes_offset_corner_charts() {
    let body = owned_cuboid(
        Point3::new(0.0, -27.898378837931627, -52.23526250506349),
        Point3::new(45.0, 20.37966681068764, 21.962553553265337),
    );
    let vertical = body.edges().filter(|edge| {
        let points = edge.endpoints().unwrap();
        (points[0].y - points[1].y).abs() > 48.0
    }).cloned().collect::<Vec<_>>();
    assert_eq!(vertical.len(), 4);
    let rounded = body.fillet_with_options(&vertical,
        FilletOptions::new(12.5).with_curvature_continuity(true)).unwrap();
    let rim = rounded.edges().find(|edge| matches!(edge.curve(), Curve3::Line(_))
        && edge.endpoints().unwrap().iter().all(|point|
            (point.x - 45.0).abs() < 1.0e-7 && point.y < -27.8)).unwrap().clone();
    let rounded = rounded.fillet_with_options(&[rim],
        FilletOptions::new(5.0).with_curvature_continuity(true)).unwrap();
    let opening = rounded.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
        if (plane.origin().y - 20.37966681068764).abs() < 1.0e-7)).unwrap().clone();
    let options = TessellationOptions::new(0.1, 0.1, 24).unwrap();
    // Slow inner meshing, noisy outer integration, and the wrong projection branch.
    for distance in [-1.0, 2.0, 4.5] {
        let shell = rounded.shell(std::slice::from_ref(&opening), distance)
            .unwrap_or_else(|error| panic!("shell {distance}: {error}"));
        assert_eq!(shell.solids().len(), 1);
        assert_eq!(shell.faces().count(), 35);
        assert!(shell.volume() > 0.0);
        if distance == -1.0 {
            assert!((shell.volume() - 13101.785164818531).abs() < 0.01);
        }
        let mut corners = 0;
        for face in shell.faces() {
            let mesh = face.tessellate(options).unwrap();
            assert!(!mesh.triangles.is_empty());
            assert!(mesh.positions.iter().all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite()));
            assert!(mesh.normals.iter().all(|normal| normal.x.is_finite() && normal.y.is_finite() && normal.z.is_finite()));
            if let Surface3::Offset(offset) = face.surface() {
                if let Surface3::Blend(blend) = offset.base() {
                    if matches!(blend.first_surface(), Surface3::Blend(_))
                        || matches!(blend.second_surface(), Surface3::Blend(_)) {
                        corners += 1;
                        assert!(mesh.triangles.len() < 2000,
                            "offset corner must use a grid rather than refine a long triangle fan: {} triangles", mesh.triangles.len());
                    }
                }
            }
        }
        assert_eq!(corners, 4);
    }
    // Just past the curvature limit, reject the folded skin before tessellation.
    let error = rounded.shell(std::slice::from_ref(&opening), -4.2).unwrap_err();
    assert!(error.diagnostics().iter().all(|diagnostic|
        diagnostic.kind == super::super::DiagnosticKind::UnsupportedGeometry));
}

#[test]
fn shell_after_g2_rim_over_circular_corner_fillets() {
    let body = owned_cuboid(
        Point3::new(0.0, -27.898378837931627, -52.23526250506349),
        Point3::new(45.0, 20.37966681068764, 21.962553553265337),
    );
    let vertical = body.edges().filter(|edge| {
        let points = edge.endpoints().unwrap();
        (points[0].y - points[1].y).abs() > 48.0
    }).cloned().collect::<Vec<_>>();
    let rounded = body.fillet(&vertical, 12.5).unwrap();
    let rim = rounded.edges().find(|edge| matches!(edge.curve(), Curve3::Line(_))
        && edge.endpoints().unwrap().iter().all(|point|
            (point.x - 45.0).abs() < 1.0e-7 && point.y < -27.8)).unwrap().clone();
    let rounded = rounded.fillet_with_options(&[rim],
        FilletOptions::new(5.0).with_curvature_continuity(true)).unwrap();
    let opening = rounded.faces().find(|face| matches!(face.surface(), Surface3::Plane(plane)
        if (plane.origin().y - 20.37966681068764).abs() < 1.0e-7)).unwrap().clone();
    let shell = rounded.shell(std::slice::from_ref(&opening), -1.0)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(shell.solids().len(), 1);
    assert!(shell.volume() > 0.0);
    for face in shell.faces() {
        assert!(!face.tessellate(TessellationOptions::default()).unwrap().triangles.is_empty());
    }
}

#[test]
fn fillet_modes_support_miters_and_reject_unimplemented_corner_patches() {
    let body = owned_cuboid(Point3::origin(), Point3::new(10.0, 10.0, 10.0));
    let miter = body.edges().filter(|edge| {
        let points = edge.endpoints().unwrap();
        points.iter().all(|p| p.z > 9.9)
            && (points.iter().all(|p| p.x.abs() < 1.0e-8) || points.iter().all(|p| p.y.abs() < 1.0e-8))
    }).cloned().collect::<Vec<_>>();
    assert_eq!(miter.len(), 2);
    for width in [false, true] {
        for curvature in [false, true] {
            let options = FilletOptions::new(0.5).with_constant_width(width).with_curvature_continuity(curvature);
            let result = body.fillet_with_options(&miter, options).unwrap();
            result.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
            if !width && !curvature { continue; }
            let all = body.edges().cloned().collect::<Vec<_>>();
            let error = body.fillet_with_options(&all, options).unwrap_err();
            assert!(error.to_string().contains("three-edge corners"));
        }
    }
}

#[test]
fn fillet_upper_cut_shell_rim_after_lower_corner_fillets() {
    let body = cut_shell_after_lower_corner_fillets();
    let edges = upper_cut_shell_rims(&body);
    assert_eq!(edges.len(), 2, "each upper rim must be one edge across the hidden chart seam");
    let mut result_counts = Vec::new();
    for edge in edges {
        let result = body.fillet(&[edge], 1.0).unwrap();
        result_counts.push((result.faces().count(), result.edges().count()));
        let remaining = upper_cut_shell_rims(&result);
        assert_eq!(remaining.len(), 1, "the unselected rim must remain a whole edge");
        let ends = remaining[0].endpoints().unwrap();
        assert!(ends[0].x * ends[1].x < 0.0);
        assert_eq!(result.solids().len(), 1);
        assert!(result.volume() < body.volume());
        result.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
    }
    assert_eq!(result_counts[0], result_counts[1], "the fillet must not inherit a chart-seam subdivision");
}

#[test]
fn chamfer_cut_shell_after_lower_corner_fillets() {
    let body = cut_shell_after_lower_corner_fillets();
    let tolerance = ModelTolerance::default();
    let half_width = 13.249811649142934;
    let edge = body.edges().find(|edge| {
        matches!(edge.curve(), Curve3::Line(_))
            && edge.endpoints().unwrap().iter().all(|point| {
                (point.x + half_width).abs() < tolerance.linear
                    && (point.z + 34.26863968536679).abs() < tolerance.linear
            })
    }).unwrap().clone();
    let mut previous_volume = body.volume();
    for distance in [0.3, 0.5, 0.6, 1.0, 2.0] {
        let chamfer = body.chamfer_with_tangent_propagation(std::slice::from_ref(&edge), distance, true)
            .unwrap_or_else(|error| panic!("distance {distance}: {error}"));
        assert_eq!(chamfer.solids().len(), 1);
        assert!(chamfer.volume() < previous_volume);
        previous_volume = chamfer.volume();
        chamfer.tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap()).unwrap();
    }
}

#[test]
fn fillet_through_cut_rounded_cylinder_shell() {
    let tol = ModelTolerance::default();
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_x(),
        -Vec3::unit_z(),
        tol,
    )
    .unwrap();
    let body = Profile::new(
        Wire2::new(
            [TrimmedCurve2::circle(
                Point2::new(11.867787165729352, 3.114060304138553),
                15.80864991584739,
            )
            .unwrap()],
            tol,
        )
        .unwrap(),
        [],
        tol,
    )
    .unwrap()
    .extrude(plane, Vec3::unit_y() * 18.0)
    .unwrap();
    let rim = body
        .edges()
        .find(|edge| edge.endpoints().unwrap().iter().all(|p| p.y > 17.9))
        .unwrap()
        .clone();
    let body = body.fillet(&[rim], 3.0).unwrap();
    let opening = body
        .faces()
        .find(
            |face| matches!(face.surface(), Surface3::Plane(p) if p.origin().y.abs() < tol.linear),
        )
        .unwrap()
        .clone();
    let body = body.shell(&[opening], -2.0).unwrap();
    let tool = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-13.472482788884726, 13.26818252144573),
                Point2::new(11.267128061979122, 13.26818252144573),
                Point2::new(11.267128061979122, -2.9986976407578023),
                Point2::new(-13.472482788884726, -2.9986976407578023),
            ],
            tol,
        )
        .unwrap(),
        [],
        tol,
    )
    .unwrap()
    .extrude(
        Plane3::new(
            Point3::new(0.0, 0.0, -40.0),
            Vec3::unit_x(),
            Vec3::unit_y(),
            tol,
        )
        .unwrap(),
        Vec3::unit_z() * 80.0,
    )
    .unwrap();
    let cut = body.difference(&tool).unwrap();
    let (edges, seams): (Vec<_>, Vec<_>) = cut
        .edges()
        .filter(|edge| {
            let points = edge.endpoints().unwrap();
            points
                .iter()
                .all(|p| (p.x - 11.267128061979122).abs() < tol.linear)
                && matches!(edge.curve(), Curve3::Line(_))
        })
        .partition(|edge| edge.connected_faces().unwrap().len() == 2);
    // Excluded chart edges must be two opposite uses on the same face.
    let topology = cut.topology();
    for seam in seams {
        assert_eq!(seam.connected_faces().unwrap().len(), 1);
        let (_, edge) = topology.edges().find(|(_, edge)| edge.name() == seam.persistent_id()).unwrap();
        assert_eq!(edge.coedges().len(), 2);
        let first = topology.coedge(edge.coedges()[0]).unwrap();
        let second = topology.coedge(edge.coedges()[1]).unwrap();
        assert_ne!(first.is_forward(), second.is_forward());
        assert_eq!(topology.loop_(first.loop_id()).unwrap().face(), topology.loop_(second.loop_id()).unwrap().face());
    }
    let edges = edges.into_iter()
        .map(|edge| edge.persistent_id())
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 8);
    let motion = RigidTransform3::new(
        Matrix3::from_angle_x(cgmath::Rad(0.7)) * Matrix3::from_angle_z(cgmath::Rad(0.4)),
        Vec3::new(-7.0, 3.0, 5.0),
        tol,
    )
    .unwrap();
    for cut in [cut.clone(), cut.transformed(motion).unwrap()] {
        for edge in cut
            .edges()
            .filter(|edge| edges.contains(&edge.persistent_id()))
        {
            for size in [0.1, 0.5, 1.0] {
                let rounded = cut
                    .check_edge_blends(std::slice::from_ref(edge), size)
                    .unwrap_or_else(|error| {
                        panic!("edge {:?}, size {size}: {error}", edge.endpoints().unwrap())
                    });
                assert_eq!(rounded.solids().len(), 1);
                rounded
                    .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                    .unwrap();
            }
        }
    }
}

#[test]
fn edge_blends_cross_chart_seams_on_annular_prisms() {
    let tol = ModelTolerance::default();
    for radius in [8.0, 20.0] {
        let wire = |radius| {
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(0.0, 0.0), radius).unwrap()],
                tol,
            )
            .unwrap()
        };
        let plane = Plane3::new(Point3::origin(), Vec3::unit_x(), -Vec3::unit_z(), tol).unwrap();
        let body = Profile::new(wire(radius), [wire(radius - 2.0)], tol)
            .unwrap()
            .extrude(plane, Vec3::unit_y() * 20.0)
            .unwrap();
        for offset in [-0.2, 0.2] {
            let tool = Profile::new(
                Wire2::polygon(
                    [
                        Point2::new(-radius * 2.0, -1.0),
                        Point2::new(offset, -1.0),
                        Point2::new(offset, 12.0),
                        Point2::new(-radius * 2.0, 12.0),
                    ],
                    tol,
                )
                .unwrap(),
                [],
                tol,
            )
            .unwrap()
            .extrude(
                Plane3::new(
                    Point3::new(0.0, 0.0, -radius * 2.0),
                    Vec3::unit_x(),
                    Vec3::unit_y(),
                    tol,
                )
                .unwrap(),
                Vec3::unit_z() * radius * 4.0,
            )
            .unwrap();
            let cut = body.difference(&tool).unwrap();
            let edges = cut
                .edges()
                .filter(|edge| {
                    let points = edge.endpoints().unwrap();
                    matches!(edge.curve(), Curve3::Line(_))
                        && points
                            .iter()
                            .all(|point| (point.x - offset).abs() < tol.linear && point.z > 0.0)
                        && (points[0].y - points[1].y).abs() > 10.0
                })
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(edges.len(), 2);
            for edge in edges {
                for size in [0.25, 1.0] {
                    cut.check_edge_blends(&[edge.clone()], size)
                        .unwrap_or_else(|error| {
                            panic!("radius {radius}, offset {offset}, size {size}: {error}")
                        });
                }
                let wall = edge
                    .connected_faces()
                    .unwrap()
                    .into_iter()
                    .find(|face| matches!(face.surface(), Surface3::Cylinder(_)))
                    .unwrap();
                let rim = cut
                    .edges()
                    .find(|rim| {
                        matches!(rim.curve(), Curve3::Circle(circle) if (circle.center().y - 20.0).abs() < tol.linear)
                            && rim.connected_faces().unwrap().iter().any(|face| {
                                face.persistent_id() == wall.persistent_id()
                            })
                    })
                    .unwrap()
                    .clone();
                cut.check_edge_blends(&[edge.clone(), rim.clone()], 0.25)
                    .expect("a seam relocation must retain a simultaneously selected closed rim");
                cut.chamfer_with_options(
                    &[edge, rim],
                    &[wall.clone(), wall],
                    0.25,
                    crate::ChamferMode::TwoDistances(0.4),
                    true,
                )
                .expect("seam normalization must rebind asymmetric chamfer reference faces");
            }
        }
    }
}

#[test]
fn edge_blends_cross_chart_seams_on_conical_rings() {
    let tol = ModelTolerance::default();
    let ring = Profile::new(
        Wire2::polygon(
            [
                Point2::new(8.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(12.0, 20.0),
                Point2::new(10.0, 20.0),
            ],
            tol,
        )
        .unwrap(),
        [],
        tol,
    )
    .unwrap()
    .revolve(
        Plane3::xy(),
        Axis3::new(Point3::origin(), Vec3::unit_y()).unwrap(),
        std::f64::consts::TAU,
    )
    .unwrap()
    .transformed(
        RigidTransform3::new(
            Matrix3::from_angle_y(cgmath::Rad(-std::f64::consts::FRAC_PI_2)),
            Vec3::new(0.0, 0.0, 0.0),
            tol,
        )
        .unwrap(),
    )
    .unwrap();
    let tool = Profile::new(
        Wire2::polygon(
            [
                Point2::new(-20.0, -1.0),
                Point2::new(-0.2, -1.0),
                Point2::new(-0.2, 12.0),
                Point2::new(-20.0, 12.0),
            ],
            tol,
        )
        .unwrap(),
        [],
        tol,
    )
    .unwrap()
    .extrude(
        Plane3::new(
            Point3::new(0.0, 0.0, -30.0),
            Vec3::unit_x(),
            Vec3::unit_y(),
            tol,
        )
        .unwrap(),
        Vec3::unit_z() * 60.0,
    )
    .unwrap();
    let cut = ring.difference(&tool).unwrap();
    let edges = cut
        .edges()
        .filter(|edge| {
            let points = edge.endpoints().unwrap();
            points
                .iter()
                .all(|point| (point.x + 0.2).abs() < tol.linear && point.z > 0.0)
                && (points[0].y - points[1].y).abs() > 10.0
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 2);
    for edge in edges {
        for size in [0.25, 1.0] {
            cut.check_edge_blends(&[edge.clone()], size)
                .unwrap_or_else(|error| panic!("conical ring, size {size}: {error}"));
        }
    }
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
            crate::FilletOptions::new(0.25).with_tangent_propagation(false),
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
    assert!(first.edges().all(|edge| matches!(edge.curve(), Curve3::Line(_) | Curve3::Circle(_))),
        "the cylindrical fillet must construct analytic contact and end edges");
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
            // Contact rails are now lines too, but run vertically; only the
            // horizontal rim edges continue the end arc tangentially.
            (endpoints[1].z - endpoints[0].z).abs() < 1.0e-7 && endpoints
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
    let top_edge_near = |solid: &crate::Compound, target: Point3, direction: Vec3| {
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
fn compound_tilt_chamfer_document_30() {
    let body = compound_tilt_boss_document_30(-41.051);
    let cap = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.normal().x > 0.5)).unwrap();
    let tolerance = ModelTolerance::default();
    for distance in [2.0, 2.5, 2.99, 3.0, 3.01, 3.5, 4.0] {
        let result = body.chamfer(&cap.boundary_edges().unwrap(), distance)
            .unwrap_or_else(|error| panic!("chamfer {distance}: {error:?}"));
        result.tessellate(TessellationOptions::default()).unwrap();
        assert_eq!(result.faces().count(), body.faces().count() + 1);
        let chamfer = result.faces().find(|face| matches!(face.surface(), Surface3::Ruled(_))).unwrap();
        let rails = chamfer.boundary_edges().unwrap().into_iter()
            .filter(|edge| edge.connected_faces().unwrap().len() == 2).collect::<Vec<_>>();
        assert_eq!(rails.len(), 2);
        for rail in rails {
            let endpoints = rail.endpoints().unwrap();
            assert!(endpoints[0].distance(endpoints[1]) < tolerance.linear);
        }
        assert!(result.volume() > 0.0 && result.volume() < body.volume());
    }
}

#[test]
fn blend_tilted_chamfer_lower_rim_document_30() {
    let body = compound_tilt_boss_document_30(-34.051);
    let cap = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.normal().x > 0.5)).unwrap();
    let body = body.chamfer(&cap.boundary_edges().unwrap(), 3.0).unwrap();
    let rim = body.edges().find(|edge| {
        let faces = edge.connected_faces().unwrap();
        faces.iter().any(|f| matches!(f.surface(), Surface3::Ruled(_)))
            && faces.iter().any(|f| matches!(f.surface(), Surface3::Cylinder(_)))
    }).unwrap().clone();
    for fillet in [true, false] {
        let result = if fillet { body.fillet(&[rim.clone()], 1.0) }
            else { body.chamfer(&[rim.clone()], 1.0) };
        let result = result.unwrap_or_else(|error| panic!("fillet={fillet}: {error:?}"));
        result.tessellate(TessellationOptions::default()).unwrap();
        assert_eq!(result.faces().count(), body.faces().count() + 1);
        assert!(result.volume() > 0.0 && result.volume() < body.volume());
    }
}

#[test]
fn closed_blends_document_29() {
    // Replay untitled(29).cad: a round base, a boss and a shallow pocket,
    // followed by the pocket fillet and a chamfer on the tilted boss cap.
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
    let rim = body.edges().find(|edge| matches!(edge.curve(), Curve3::Circle(c)
        if (c.radius() - 8.639117031005743).abs() < 1e-7 && (c.center().y - 19.0).abs() < 1e-7)).unwrap().clone();
    let body = body.fillet(&[rim], 1.0).unwrap();
    let check_pocket_rims = |shape: &crate::modeling::Compound| {
        for (radius, height) in [(8.639117031005743, 18.0), (9.639117031005743, 19.0)] {
            let edges = shape.edges().filter(|edge| matches!(edge.curve(), Curve3::Circle(c)
                if (c.radius() - radius).abs() < 1e-7 && (c.center().y - height).abs() < 1e-7))
                .collect::<Vec<_>>();
            assert_eq!(edges.len(), 1, "one edge per circular contact rail");
            let endpoints = edges[0].endpoints().unwrap();
            assert!(endpoints[0].distance(endpoints[1]) < tolerance.linear);
        }
    };
    check_pocket_rims(&body);
    let cap = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if (p.origin().y - 34.0).abs() < 1e-7)).unwrap().clone();
    let center = Vec3::new(-17.48305641426953, 34.0, 3.255357738829639);
    let rotation = Matrix3::from_angle_z(cgmath::Deg(-20.0));
    let body = body.move_faces(&[cap], RigidTransform3::new(rotation, center - rotation * center, tolerance).unwrap()).unwrap();
    let cap = body.faces().find(|f| matches!(f.surface(), Surface3::Plane(p) if p.normal().dot(rotation * Vec3::unit_y()) > 0.999)).unwrap();
    let result = body.chamfer(&cap.boundary_edges().unwrap(), 2.0).unwrap();
    for face in result.faces() { face.tessellate(TessellationOptions::default()).unwrap(); }
    assert_eq!(result.faces().count(), body.faces().count() + 1);
    check_pocket_rims(&result);
    let chamfer = result.faces().find(|face| matches!(face.surface(), Surface3::Ruled(_))).unwrap();
    let physical_edges = chamfer.boundary_edges().unwrap().into_iter()
        .filter(|edge| edge.connected_faces().unwrap().len() == 2).collect::<Vec<_>>();
    assert_eq!(physical_edges.len(), 2, "one closed edge per chamfer contact rail");
    for edge in physical_edges {
        let endpoints = edge.endpoints().unwrap();
        assert!(endpoints[0].distance(endpoints[1]) < tolerance.linear);
    }
    assert!(result.volume() > 0.0 && result.volume() < body.volume());
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

#[test]
fn cylinder_end_fillet_stops_at_chamfered_transverse_hole() {
    // invalid-geom.cad: the end blend must not cross the chamfered bore.
    let tolerance = ModelTolerance::default();
    let circle = |radius| {
        Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(0.0, 0.0), radius).unwrap()],
                tolerance,
            ).unwrap(),
            [],
            tolerance,
        ).unwrap()
    };
    let plane = Plane3::new(
        Point3::new(0.0, 0.0, -40.0), Vec3::unit_x(), Vec3::unit_y(), tolerance,
    ).unwrap();
    let cylinder = circle(41.339009777411725)
        .extrude(plane, Vec3::unit_z() * 80.0).unwrap();
    let side = cylinder.faces()
        .find(|face| matches!(face.surface(), Surface3::Cylinder(_))).unwrap().clone();
    let tapered = cylinder.draft(&[side], plane, 0.04).unwrap();
    let plane = Plane3::new(
        Point3::new(0.0, 100.0, 0.0), Vec3::unit_x(), Vec3::unit_z(), tolerance,
    ).unwrap();
    let cutter = circle(13.975006382948514)
        .extrude(plane, -Vec3::unit_y() * 200.0).unwrap();
    for (label, source) in [("cylinder", cylinder), ("cone", tapered)] {
        let cut = source.difference(&cutter).unwrap();
        if label == "cylinder" {
            let rims = cut.edges().filter(|edge| {
                let faces = edge.connected_faces().unwrap();
                faces.len() == 2 && faces.iter().all(|face| matches!(face.surface(), Surface3::Cylinder(_)))
            }).cloned().collect::<Vec<_>>();
            assert_eq!(rims.len(), 2, "each bore opening must have one native edge");
            for rim in &rims {
                let ends = rim.endpoints().unwrap();
                assert!(ends[0].distance(ends[1]) < tolerance.linear);
            }
            cut.chamfer_with_tangent_propagation(&rims, 4.0, true).unwrap();
        }
        let bore_rim = cut.edges().find(|edge| {
            let faces = edge.connected_faces().unwrap();
            faces.len() == 2
                && faces.iter().all(|face| matches!(face.surface(), Surface3::Cylinder(_) | Surface3::Cone(_)))
                && edge.endpoints().unwrap().iter().all(|point| point.y > 0.0)
        }).unwrap().clone();
        let body = cut.chamfer_with_tangent_propagation(&[bore_rim], 4.0, true).unwrap();
        let rim = body.edges().find(|edge| matches!(edge.curve(), Curve3::Circle(c)
            if (c.center().z - 40.0).abs() < tolerance.linear)).unwrap().clone();
        for radius in [1.0, 20.0] {
            body.fillet(std::slice::from_ref(&rim), radius)
                .unwrap_or_else(|error| panic!("{label}, valid radius {radius}: {error}"));
        }
        for radius in [25.0, 35.0] {
            let error = body.fillet(std::slice::from_ref(&rim), radius).err()
                .unwrap_or_else(|| panic!("{label}, radius {radius} must stop before the chamfered hole"));
            assert!(error.to_string().contains("blend contact boundary"), "{label}, radius {radius}: {error}");
        }
    }
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
fn fillet_spline_prism_cap_edges() {
    let body = spline_prism();
    let curved = spline_prism_cap_curves(&body);
    assert_eq!(curved.len(), 4);
    // Where a spline meets a line at about 143 degrees, the cap contact ends
    // on the line beyond the spline's end, outside its support's domain.
    let volume = body.volume();
    for radius in [1.0, 3.0] {
        for edges in curved.iter().map(|edge| vec![edge.clone()]).chain([curved.clone()]) {
            let result = body.check_edge_blends(&edges, radius).unwrap();
            assert_eq!(result.solids().len(), 1);
            assert_eq!(result.faces().count(), body.faces().count() + edges.len());
            assert!(result.volume() < volume && result.volume() > volume * 0.9);
            result
                .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
                .unwrap();
        }
    }
}

#[test]
fn fillet_spline_tube_inner_cap_edges() {
    let body = spline_prism();
    let ends = body
        .faces()
        .filter(|face| match face.surface() {
            Surface3::Plane(plane) => plane.normal().z.abs() < 0.5,
            _ => false,
        })
        .cloned()
        .collect::<Vec<_>>();
    let tube = body.shell(&ends, -2.5).unwrap();
    let inner = spline_prism_cap_curves(&tube)
        .into_iter()
        .filter(|edge| {
            let z = edge.endpoints().unwrap()[0].z;
            z > 1.0 && z < 21.5
        })
        .collect::<Vec<_>>();
    assert_eq!(inner.len(), 4);
    let volume = tube.volume();
    for edges in inner.iter().map(|edge| vec![edge.clone()]).chain([inner.clone()]) {
        let result = tube.check_edge_blends(&edges, 1.0).unwrap();
        assert_eq!(result.solids().len(), 1);
        assert_eq!(result.faces().count(), tube.faces().count() + edges.len());
        assert!(result.volume() > volume && result.volume() < volume * 1.1);
        result
            .tessellate(TessellationOptions::new(0.1, 0.1, 24).unwrap())
            .unwrap();
    }
}

#[test]
fn perimeter_chamfer_of_pocketed_plate_certifies() {
    // All faces are planar and all edges straight, but some planar p-curves
    // are not lines; the boundary integral must still certify the body.
    let tolerance = ModelTolerance::default();
    let mut plate = owned_cuboid(Point3::origin(), Point3::new(66.0, 66.0, 3.0));
    for index in 0..30 {
        let corner = Point2::new(3.0 + (index % 6) as f64 * 10.0, 3.0 + (index / 6) as f64 * 12.0);
        let square = Wire2::polygon(
            [corner, corner + crate::Vec2::new(4.0, 0.0), corner + crate::Vec2::new(4.0, 4.0), corner + crate::Vec2::new(0.0, 4.0)],
            tolerance,
        ).unwrap();
        let tool = Profile::new(square, [], tolerance).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 3.0).unwrap();
        plate = plate.difference(&tool).unwrap();
    }
    let perimeter = plate.edges().filter(|edge| {
        let [a, b] = edge.endpoints().unwrap();
        a.z > 2.9 && b.z > 2.9
            && [a.x, a.y, b.x, b.y].iter().filter(|c| c.abs() < 1e-9 || (*c - 66.0).abs() < 1e-9).count() >= 2
    }).cloned().collect::<Vec<_>>();
    assert_eq!(perimeter.len(), 4);
    let chamfered = plate.chamfer(&perimeter, 0.5).unwrap();
    let removed = 4.0 * 66.0 * 0.125 - 4.0 * 0.5 * 0.5 * 0.5 / 3.0 * 1.0;
    assert!((chamfered.volume() - (plate.volume() - removed)).abs() < 1e-6, "{} {}", chamfered.volume(), plate.volume() - removed);
}
