//! Fillets whose contact rails leave a support face: across a neighboring
//! edge of a face narrower than the blend, or through a hole in the band.

use super::*;

fn prism(points: &[(f64, f64)], height: f64) -> crate::Compound {
    let tolerance = ModelTolerance::default();
    let wire = Wire2::polygon(points.iter().map(|(x, y)| Point2::new(*x, *y)), tolerance).unwrap();
    Profile::new(wire, [], tolerance).unwrap()
        .extrude(Plane3::xy(), Vec3::unit_z() * height).unwrap()
}

/// A cylinder of `radius` about the line from `start` along `axis`.
fn rod(start: Point3, axis: Vec3, radius: f64) -> crate::Compound {
    let tolerance = ModelTolerance::default();
    let wire = Wire2::new([TrimmedCurve2::circle(Point2::origin(), radius).unwrap()], tolerance).unwrap();
    let x = if axis.x.abs() < 0.5 { Vec3::unit_x() } else { Vec3::unit_y() };
    let x = (x - axis * x.dot(axis) / axis.magnitude2()).normalize();
    let plane = Plane3::new(start, x, axis.cross(x).normalize(), tolerance).unwrap();
    Profile::new(wire, [], tolerance).unwrap().extrude(plane, axis).unwrap()
}

/// The edge along z through `(x, y)`.
fn z_edge(body: &crate::Compound, x: f64, y: f64) -> crate::EdgeHandle {
    body.edges().find(|edge| edge.endpoints().is_ok_and(|points| {
        points.iter().all(|point| (point.x - x).abs() < 1.0e-9 && (point.y - y).abs() < 1.0e-9)
            && (points[0].z - points[1].z).abs() > 1.0e-9
    })).unwrap().clone()
}

fn mesh(body: &crate::Compound) {
    body.tessellate(TessellationOptions::new(0.05, 0.1, 24).unwrap()).unwrap();
}

/// Area under the trimmed section `y = c - sqrt(r² - (x - c)²)` over `0..width`.
fn trimmed_section_area(radius: f64, width: f64) -> f64 {
    let steps = 200_000;
    (0..steps).map(|index| {
        let x = (index as f64 + 0.5) * width / steps as f64;
        (radius - (radius * radius - (x - radius).powi(2)).sqrt()) * width / steps as f64
    }).sum()
}

#[test]
fn concave_fillet_overflows_a_narrow_ledge() {
    // The ledge at y = 4 is 2 wide; a radius of 3 lands beyond its outer edge.
    let body = prism(&[(0.0, 0.0), (7.0, 0.0), (7.0, 4.0), (5.0, 4.0), (5.0, 10.0), (0.0, 10.0)], 10.0);
    let edge = z_edge(&body, 5.0, 4.0);
    let result = body.fillet(&[edge.clone()], 3.0).unwrap();
    let expected = body.volume() + trimmed_section_area(3.0, 2.0) * 10.0;
    assert!((result.volume() - expected).abs() < 1.0e-5, "{} vs {expected}", result.volume());
    // The blend consumes the ledge, and the drop below it rises to meet it.
    assert_eq!(result.faces().count(), 8);
    assert_eq!(result.faces_generated_from(&edge).len(), 1);
    assert!(matches!(result.resolve(&edge), Resolution::Deleted));
    mesh(&result);
}

#[test]
fn convex_fillet_overflows_a_narrow_face() {
    let body = prism(&[(0.0, 0.0), (2.0, 0.0), (2.0, 10.0), (0.0, 10.0)], 10.0);
    let result = body.fillet(&[z_edge(&body, 0.0, 0.0)], 3.0).unwrap();
    let expected = body.volume() - trimmed_section_area(3.0, 2.0) * 10.0;
    assert!((result.volume() - expected).abs() < 1.0e-5, "{} vs {expected}", result.volume());
    assert_eq!(result.faces().count(), 6);
    mesh(&result);
}

#[test]
fn convex_fillet_crosses_a_hole_in_its_band() {
    let block = prism(&[(0.0, 0.0), (10.0, 0.0), (10.0, 20.0), (0.0, 20.0)], 20.0);
    let corner = prism(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)], 20.0);
    let ball = rod(Point3::new(2.0, 2.0, -1.0), Vec3::unit_z() * 22.0, 2.0);
    let wedge = corner.difference(&ball).unwrap();
    // The larger hole crosses the contact rail. The smaller hole sits
    // wholly between the rail and the original edge.
    for bore_radius in [1.0, 0.25] {
        let hole = rod(Point3::new(-1.0, 1.5, 10.0), Vec3::unit_x() * 12.0, bore_radius);
        let body = block.difference(&hole).unwrap();
        let result = body.fillet(&[z_edge(&body, 0.0, 0.0)], 2.0)
            .unwrap_or_else(|error| panic!("bore radius {bore_radius}: {error}"));
        let expected = body.difference(&wedge).unwrap();
        assert!((result.volume() - expected.volume()).abs() < 1.0e-5, "{} vs {}", result.volume(), expected.volume());
        mesh(&result);
    }
}

#[test]
fn concave_fillet_crosses_a_hole_in_its_band() {
    let step = prism(&[(0.0, 0.0), (20.0, 0.0), (20.0, 4.0), (5.0, 4.0), (5.0, 10.0), (0.0, 10.0)], 10.0);
    // A bore through the ledge, 1.5 from the wall.
    let hole = rod(Point3::new(6.5, -1.0, 5.0), Vec3::unit_y() * 6.0, 1.0);
    let body = step.difference(&hole).unwrap();
    let result = body.fillet(&[z_edge(&body, 5.0, 4.0)], 2.0).unwrap();
    // The bore continues up through the added material.
    let filled = step.fillet(&[z_edge(&step, 5.0, 4.0)], 2.0).unwrap();
    let bore = rod(Point3::new(6.5, -1.0, 5.0), Vec3::unit_y() * 12.0, 1.0);
    let expected = filled.difference(&bore).unwrap();
    assert!((result.volume() - expected.volume()).abs() < 1.0e-5, "{} vs {}", result.volume(), expected.volume());
    mesh(&result);
}

#[test]
fn convex_fillet_overflows_a_narrow_annulus() {
    let tube = rod(Point3::new(0.0, 0.0, 0.0), Vec3::unit_z() * 10.0, 6.0)
        .difference(&rod(Point3::new(0.0, 0.0, -1.0), Vec3::unit_z() * 12.0, 5.0)).unwrap();
    let edge = rim(&tube, 10.0).into_iter()
        .find(|edge| matches!(edge.curve(), Curve3::Circle(circle) if (circle.radius() - 6.0).abs() < 1.0e-9)).unwrap();
    let result = tube.fillet(&[edge], 2.0).unwrap();
    let solid = rod(Point3::new(0.0, 0.0, 0.0), Vec3::unit_z() * 10.0, 6.0);
    let expected = solid.fillet(&rim(&solid, 10.0), 2.0).unwrap()
        .difference(&rod(Point3::new(0.0, 0.0, -1.0), Vec3::unit_z() * 12.0, 5.0)).unwrap();
    assert!((result.volume() - expected.volume()).abs() < 1.0e-4, "{} vs {}", result.volume(), expected.volume());
    mesh(&result);
}

#[test]
fn overflowing_fillet_follows_a_tangent_rim() {
    let t = ModelTolerance::default();
    let (low, high, corner) = (0.0, 20.0, 4.0);
    let bend = corner * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
    let p = Point2::new;
    let wire = Wire2::new([
        TrimmedCurve2::line(p(low + corner, low), p(high - corner, low)).unwrap(),
        TrimmedCurve2::arc_through(p(high - corner, low), p(high - bend, low + bend), p(high, low + corner)).unwrap(),
        TrimmedCurve2::line(p(high, low + corner), p(high, high - corner)).unwrap(),
        TrimmedCurve2::arc_through(p(high, high - corner), p(high - bend, high - bend), p(high - corner, high)).unwrap(),
        TrimmedCurve2::line(p(high - corner, high), p(low + corner, high)).unwrap(),
        TrimmedCurve2::arc_through(p(low + corner, high), p(low + bend, high - bend), p(low, high - corner)).unwrap(),
        TrimmedCurve2::line(p(low, high - corner), p(low, low + corner)).unwrap(),
        TrimmedCurve2::arc_through(p(low, low + corner), p(low + bend, low + bend), p(low + corner, low)).unwrap(),
    ], t).unwrap();
    let plate = Profile::new(wire, [], t).unwrap().extrude(Plane3::xy(), Vec3::unit_z() * 5.0).unwrap();
    let hole = rod(Point3::new(10.0, 1.5, -1.0), Vec3::unit_z() * 7.0, 1.0);
    let body = plate.difference(&hole).unwrap();
    let outline = |body: &crate::Compound| rim(body, 5.0).into_iter().filter(|edge| {
        !matches!(edge.curve(), Curve3::Circle(circle) if (circle.radius() - 1.0).abs() < 1.0e-9)
    }).collect::<Vec<_>>();
    let result = body.fillet(&outline(&body), 2.0).unwrap();
    let expected = plate.fillet(&outline(&plate), 2.0).unwrap().difference(&hole).unwrap();
    assert!((result.volume() - expected.volume()).abs() < 1.0e-4, "{} vs {}", result.volume(), expected.volume());
    mesh(&result);
}

/// The edges of `body` lying in the plane `z = height`.
fn rim(body: &crate::Compound, height: f64) -> Vec<crate::EdgeHandle> {
    body.edges().filter(|edge| edge.endpoints().is_ok_and(|points| {
        points.iter().all(|point| (point.z - height).abs() < 1.0e-9)
    })).cloned().collect()
}

#[test]
fn overflowing_fillets_meet_at_miters() {
    let plate = prism(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)], 5.0);
    let hole = rod(Point3::new(10.0, 1.5, -1.0), Vec3::unit_z() * 7.0, 1.0);
    let body = plate.difference(&hole).unwrap();
    let edges = rim(&body, 5.0).into_iter().filter(|edge| matches!(edge.curve(), Curve3::Line(_))).collect::<Vec<_>>();
    assert_eq!(edges.len(), 4);
    let result = body.fillet(&edges, 2.0).unwrap();
    let expected = plate.fillet(&rim(&plate, 5.0), 2.0).unwrap().difference(&hole).unwrap();
    assert!((result.volume() - expected.volume()).abs() < 1.0e-4, "{} vs {}", result.volume(), expected.volume());
    mesh(&result);
}

#[test]
fn concave_fillet_overflows_the_rim_beside_an_eccentric_boss() {
    // 163-eccentric-boss-fillet-grazing-rim.cad: the narrowest ledge is
    // about 14.4696 mm, just under the saved radius of 14.47 mm.
    // Wider blends run off the disc there, whose wall rises to meet them.
    let base = rod(Point3::new(3.060509769811371, 12.47389588061244, 0.0), Vec3::unit_z() * 30.0, 56.72595428250884);
    let boss = rod(Point3::new(27.91555880949149, 7.65097442523162, 30.0), Vec3::unit_z() * 40.0, 16.937674889783747);
    let body = base.union(&boss).unwrap();
    let foot = rim(&body, 30.0).into_iter()
        .find(|edge| matches!(edge.curve(), Curve3::Circle(circle) if (circle.radius() - 16.937674889783747).abs() < 1.0e-9)).unwrap();
    let volume = |radius| {
        let result = body.fillet(std::slice::from_ref(&foot), radius)
            .unwrap_or_else(|error| panic!("radius {radius}: {error}"));
        assert_eq!(result.faces().count(), 6, "radius {radius}");
        mesh(&result);
        result.volume()
    };
    let grazing = [14.46, 14.47, 14.48].map(volume);
    assert!(grazing[0] < grazing[1] && grazing[1] < grazing[2]);
    let volumes = [13.5, 14.5, 15.5, 16.5, 20.5, 25.5].map(volume);
    // The added material keeps growing steadily across the first overflow.
    let steps = volumes[..4].windows(2).map(|pair| pair[1] - pair[0]).collect::<Vec<_>>();
    for pair in steps.windows(2) {
        assert!(pair[1] > pair[0] && pair[1] < pair[0] * 1.25, "{steps:?}");
    }
    assert!(volumes[5] > volumes[4] && volumes[4] > volumes[3]);
}
