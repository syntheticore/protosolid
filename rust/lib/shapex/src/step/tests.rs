use std::f64::consts::{PI, TAU};

use cgmath::prelude::*;

use crate::{
    Axis3, BooleanKind, Compound, ModelTolerance, NurbsCurve2, ParameterRange, Plane3, Point2,
    Point3, Profile, StepAssembly, StepLengthUnit, StepOccurrence, StepProduct, TessellationOptions, TrimmedCurve2,
    Vec3, Wire2,
};

fn tolerance() -> ModelTolerance {
    ModelTolerance::default()
}

fn polygon(points: &[(f64, f64)]) -> Profile {
    let wire = Wire2::polygon(points.iter().map(|(x, y)| Point2::new(*x, *y)), tolerance()).unwrap();
    Profile::new(wire, [], tolerance()).unwrap()
}

fn circle(center: (f64, f64), radius: f64) -> Profile {
    let wire = Wire2::new([TrimmedCurve2::circle(Point2::new(center.0, center.1), radius).unwrap()], tolerance()).unwrap();
    Profile::new(wire, [], tolerance()).unwrap()
}

fn cuboid(minimum: Point3, size: Vec3) -> Compound {
    let plane = Plane3::new(minimum, Vec3::unit_x(), Vec3::unit_y(), tolerance()).unwrap();
    polygon(&[(0.0, 0.0), (size.x, 0.0), (size.x, size.y), (0.0, size.y)])
        .extrude(plane, Vec3::new(0.0, 0.0, size.z))
        .unwrap()
}

fn round_trip(compound: &Compound) -> Compound {
    let text = compound.to_step("part", "2026-01-01T00:00:00").expect("export");
    let imported = Compound::from_step(&text, tolerance()).unwrap_or_else(|error| panic!("import: {error}\n{text}"));
    assert_eq!(imported.solids().len(), compound.solids().len());
    assert_eq!(imported.faces().count(), compound.faces().count());
    let error = (imported.volume() - compound.volume()).abs();
    assert!(error <= 1.0e-6 * compound.volume().abs().max(1.0), "volume {} != {}", imported.volume(), compound.volume());
    let centroid = mesh_centroid(compound);
    assert!(imported.centroid().distance(centroid) <= 1.0e-3, "centroid {:?} != {:?}", imported.centroid(), centroid);
    imported
}

/// The centroid of a fine tessellation, independent of certified properties.
fn mesh_centroid(compound: &Compound) -> Point3 {
    let mesh = compound.tessellate(TessellationOptions::new(0.005, 0.02, 24).unwrap()).expect("tessellate");
    let (mut volume, mut moment) = (0.0, Vec3::new(0.0, 0.0, 0.0));
    for triangle in &mesh.triangles {
        let [a, b, c] = triangle.map(|index| mesh.positions[index].to_vec());
        let signed = a.dot(b.cross(c)) / 6.0;
        volume += signed;
        moment += (a + b + c) * (signed / 4.0);
    }
    Point3::from_vec(moment / volume)
}

#[test]
fn cuboid_round_trip() {
    let text = cuboid(Point3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0))
        .to_step("box", "2026-01-01T00:00:00")
        .unwrap();
    assert!(text.contains("PLANE(") && text.contains("MANIFOLD_SOLID_BREP"));
    round_trip(&cuboid(Point3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0)));
}

#[test]
fn cylinder_round_trip() {
    let compound = circle((1.0, -2.0), 3.0).extrude(Plane3::xy(), Vec3::new(0.0, 0.0, 5.0)).unwrap();
    let text = compound.to_step("cylinder", "").unwrap();
    assert!(text.contains("CYLINDRICAL_SURFACE"));
    round_trip(&compound);
}

#[test]
fn drilled_block_round_trip() {
    let block = cuboid(Point3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 4.0));
    let tool = circle((5.0, 5.0), 2.0).extrude(
        Plane3::new(Point3::new(0.0, 0.0, -1.0), Vec3::unit_x(), Vec3::unit_y(), tolerance()).unwrap(),
        Vec3::new(0.0, 0.0, 6.0),
    ).unwrap();
    round_trip(&block.boolean(&tool, BooleanKind::Difference).unwrap());
}

#[test]
fn cavity_round_trip() {
    let outer = cuboid(Point3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0));
    let inner = cuboid(Point3::new(3.0, 3.0, 3.0), Vec3::new(4.0, 4.0, 4.0));
    let hollow = outer.boolean(&inner, BooleanKind::Difference).unwrap();
    let text = hollow.to_step("hollow", "").unwrap();
    assert!(text.contains("BREP_WITH_VOIDS"));
    round_trip(&hollow);
}

#[test]
fn torus_round_trip() {
    let axis = Axis3::new(Point3::origin(), Vec3::unit_y()).unwrap();
    round_trip(&circle((5.0, 0.0), 2.0).revolve(Plane3::xy(), axis, TAU).unwrap());
}

#[test]
fn sphere_round_trip() {
    let axis = Axis3::new(Point3::origin(), Vec3::unit_y()).unwrap();
    let half = Wire2::new(
        [
            TrimmedCurve2::new(crate::Circle2::new(Point2::new(0.0, 0.0), 3.0).unwrap(), ParameterRange::new(-PI / 2.0, PI / 2.0).unwrap()).unwrap(),
            TrimmedCurve2::line(Point2::new(0.0, 3.0), Point2::new(0.0, -3.0)).unwrap(),
        ],
        tolerance(),
    ).unwrap();
    round_trip(&Profile::new(half, [], tolerance()).unwrap().revolve(Plane3::xy(), axis, TAU).unwrap());
}

#[test]
fn partial_revolution_round_trip() {
    let axis = Axis3::new(Point3::origin(), Vec3::unit_y()).unwrap();
    let profile = polygon(&[(2.0, 0.0), (5.0, 0.0), (4.0, 3.0), (2.0, 3.0)]);
    round_trip(&profile.revolve(Plane3::xy(), axis, PI * 0.75).unwrap());
}

#[test]
fn spline_revolution_round_trip() {
    let spline = NurbsCurve2::new(
        3,
        vec![Point2::new(2.0, 0.0), Point2::new(4.0, 1.0), Point2::new(3.0, 2.0), Point2::new(2.0, 3.0)],
        vec![1.0; 4],
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    ).unwrap();
    let wire = Wire2::new(
        [
            TrimmedCurve2::new(spline, ParameterRange::new(0.0, 1.0).unwrap()).unwrap(),
            TrimmedCurve2::line(Point2::new(2.0, 3.0), Point2::new(2.0, 0.0)).unwrap(),
        ],
        tolerance(),
    ).unwrap();
    let axis = Axis3::new(Point3::origin(), Vec3::unit_y()).unwrap();
    round_trip(&Profile::new(wire, [], tolerance()).unwrap().revolve(Plane3::xy(), axis, TAU).unwrap());
}

#[test]
fn filleted_block_round_trip() {
    let block = cuboid(Point3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 8.0, 6.0));
    let edges = block.edges().take(3).cloned().collect::<Vec<_>>();
    round_trip(&block.fillet(&edges, 1.0).unwrap());
}

#[test]
fn assembly_placement_is_applied() {
    let text = r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION((''),'2;1');
FILE_NAME('','',(''),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));
ENDSEC;
DATA;
"#;
    // Place the exported cuboid 10 mm along x through a mapped item.
    let part = cuboid(Point3::origin(), Vec3::new(1.0, 1.0, 1.0)).to_step("part", "").unwrap();
    let data = part.split("DATA;\n").nth(1).unwrap().split("ENDSEC;").next().unwrap();
    let representation = data
        .lines()
        .find(|line| line.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"))
        .and_then(|line| line.split('=').next())
        .unwrap();
    let mapped = format!(
        "{data}#9001=CARTESIAN_POINT('',(0.,0.,0.));\n#9002=CARTESIAN_POINT('',(10.,0.,0.));\n\
         #9003=AXIS2_PLACEMENT_3D('',#9001,$,$);\n#9004=AXIS2_PLACEMENT_3D('',#9002,$,$);\n\
         #9005=REPRESENTATION_MAP(#9003,{representation});\n#9006=MAPPED_ITEM('',#9005,#9004);\n\
         #9007=SHAPE_REPRESENTATION('',(#9006),#9999);\nENDSEC;\nEND-ISO-10303-21;\n"
    );
    let imported = Compound::from_step(&format!("{text}{mapped}"), tolerance()).unwrap();
    assert_eq!(imported.solids().len(), 1);
    assert!(imported.centroid().distance(Point3::new(10.5, 0.5, 0.5)) < 1.0e-9);
}

#[test]
fn assembly_structure_round_trip() {
    let placement = |angle: f64, offset: Vec3| {
        let rotation = crate::Matrix3::from_angle_z(cgmath::Rad(angle));
        crate::RigidTransform3::new(rotation, offset, tolerance()).unwrap()
    };
    let product = |name: &str, bodies: Option<Compound>, occurrences: Vec<StepOccurrence>| StepProduct {
        name: name.into(),
        colors: bodies.iter().map(|_| Some([0.25, 0.5, 1.0])).collect(),
        bodies,
        occurrences,
    };
    let block = cuboid(Point3::origin(), Vec3::new(2.0, 3.0, 4.0));
    let pin = circle((0.0, 0.0), 0.5)
        .extrude(Plane3::new(Point3::origin(), Vec3::unit_x(), Vec3::unit_y(), tolerance()).unwrap(), Vec3::new(0.0, 0.0, 6.0))
        .unwrap();
    // A subassembly placing the block and the pin, placed twice in the root
    // with one more block, written in inches.
    let assembly = StepAssembly {
        products: vec![
            product("Grundbaugruppe", None, vec![
                StepOccurrence { product: 1, placement: placement(0.0, Vec3::new(10.0, 0.0, 0.0)) },
                StepOccurrence { product: 1, placement: placement(PI * 0.5, Vec3::new(-10.0, 0.0, 0.0)) },
                StepOccurrence { product: 2, placement: placement(0.3, Vec3::new(0.0, 20.0, 0.0)) },
            ]),
            product("Unterbaugruppe ä", None, vec![
                StepOccurrence { product: 2, placement: placement(0.0, Vec3::new(0.0, 0.0, 1.0)) },
                StepOccurrence { product: 3, placement: placement(0.0, Vec3::new(1.0, 1.5, 0.0)) },
            ]),
            product("Block", Some(block.clone()), Vec::new()),
            product("Pin", Some(pin.clone()), Vec::new()),
        ],
        roots: vec![0],
        unit: StepLengthUnit::Inch,
    };
    let text = assembly.to_step("2026-01-01T00:00:00").unwrap();
    let imported = StepAssembly::from_step(&text, tolerance()).unwrap_or_else(|error| panic!("{error}\n{text}"));
    assert_eq!(imported.products.len(), 4);
    assert_eq!(imported.roots.len(), 1);
    let names = imported.products.iter().map(|product| product.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, ["Grundbaugruppe", "Unterbaugruppe ä", "Block", "Pin"]);
    let root = &imported.products[imported.roots[0]];
    assert!(root.bodies.is_none());
    assert_eq!(root.occurrences.iter().map(|occurrence| occurrence.product).collect::<Vec<_>>(), [1, 1, 2]);
    let second = root.occurrences[1].placement;
    assert!(second.translation().distance(Vec3::new(-10.0, 0.0, 0.0)) < 1.0e-12);
    assert!((second.rotation().x - Vec3::unit_y()).magnitude() < 1.0e-12);
    assert!((imported.products[2].bodies.as_ref().unwrap().volume() - block.volume()).abs() < 1.0e-9);
    assert!((imported.products[3].bodies.as_ref().unwrap().volume() - pin.volume()).abs() < 1.0e-9);
    assert_eq!(imported.products[2].colors, [Some([0.25, 0.5, 1.0])]);
    assert!(imported.products[0].colors.is_empty());

    // Flattened, every occurrence is one solid in world coordinates.
    let flattened = Compound::from_step(&text, tolerance()).unwrap();
    assert_eq!(flattened.solids().len(), 5);
    assert!((flattened.volume() - 3.0 * block.volume() - 2.0 * pin.volume()).abs() < 1.0e-8);
}

/// Offset every coordinate by a deterministic pseudo-random amount up to
/// `amount`, as files written by kernels with a coarser precision are.
fn perturbed(text: &str, amount: f64) -> String {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut noise = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state as f64 / u64::MAX as f64 * 2.0 - 1.0) * amount
    };
    text.lines()
        .map(|line| {
            let Some((head, rest)) = line.split_once("CARTESIAN_POINT('',(") else { return line.to_owned() };
            let (coordinates, tail) = rest.split_once("))").unwrap();
            let coordinates = coordinates
                .split(',')
                .map(|value| super::writer::real(value.parse::<f64>().unwrap() + noise()))
                .collect::<Vec<_>>()
                .join(",");
            format!("{head}CARTESIAN_POINT('',({coordinates})){tail}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn drilled_block() -> Compound {
    let block = cuboid(Point3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 4.0));
    let tool = circle((5.0, 5.0), 2.0).extrude(
        Plane3::new(Point3::new(0.0, 0.0, -1.0), Vec3::unit_x(), Vec3::unit_y(), tolerance()).unwrap(),
        Vec3::new(0.0, 0.0, 6.0),
    ).unwrap();
    block.boolean(&tool, BooleanKind::Difference).unwrap()
}

#[test]
fn coarse_files_import_with_entity_tolerances() {
    let drilled = drilled_block();
    let exported = drilled.to_step("part", "").unwrap();
    for amount in [1.0e-10, 1.0e-8, 1.0e-6, 1.0e-5] {
        let imported = Compound::from_step(&perturbed(&exported, amount), tolerance())
            .unwrap_or_else(|error| panic!("{amount}: {error}"));
        assert!((imported.volume() - drilled.volume()).abs() <= amount * 1000.0, "{amount}: {}", imported.volume());
        // Within Boolean precision the import shares the native tolerance
        // policy and remains a modeling operand.
        if amount <= 1.0e-8 {
            let cut = imported
                .boolean(&cuboid(Point3::new(-1.0, -1.0, 2.0), Vec3::new(12.0, 6.0, 4.0)), BooleanKind::Difference)
                .unwrap_or_else(|error| panic!("{amount} cut: {error}"));
            assert!(cut.volume() < imported.volume());
        }
    }
}

#[test]
fn units_and_unmarked_outer_bounds_are_read() {
    let exported = cuboid(Point3::origin(), Vec3::new(1.0, 2.0, 3.0)).to_step("part", "").unwrap();
    let text = exported
        .replace("FACE_OUTER_BOUND", "FACE_BOUND")
        .replace("SI_UNIT(.MILLI.,.METRE.)", "SI_UNIT(.CENTI.,.METRE.)");
    let imported = Compound::from_step(&text, tolerance()).unwrap();
    assert!((imported.volume() - 6000.0).abs() < 1.0e-6);
}

