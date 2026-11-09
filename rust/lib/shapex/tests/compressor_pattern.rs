use shapex::*;
use std::time::Instant;

fn spline(points: &[[f64; 2]]) -> TrimmedCurve2 {
    let degree = points.len() - 1;
    let knots = [vec![0.0; degree + 1], vec![1.0; degree + 1]].concat();
    TrimmedCurve2::new(
        NurbsCurve2::new(degree, points.iter().map(|p| Point2::new(p[0], p[1])).collect(), vec![1.0; points.len()], knots).unwrap(),
        ParameterRange::new(0.0, 1.0).unwrap(),
    ).unwrap()
}

// The HPC blade at s = 341 sits on the drum's cone. Its trailing edge is a
// 0.03 mm line.
#[test]
#[ignore]
fn cone_blade_union() {
    let tolerance = ModelTolerance::default();
    let drum = Profile::new(Wire2::polygon([
        [-200.0, 70.0], [-215.0, 66.0], [-305.0, 66.0], [-320.0, 56.0], [-405.0, 70.0],
        [-415.0, 62.0], [-487.0, 62.0], [-487.0, 0.0], [-200.0, 0.0],
    ].map(|p| Point2::new(p[0], p[1])), tolerance).unwrap(), [], tolerance).unwrap()
        .revolve(Plane3::xy(), Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x()).unwrap(), std::f64::consts::TAU)
        .unwrap();
    let a = [[-337.536, 2.0], [-337.962, 1.459], [-339.522, 0.641], [-341.688, -0.409], [-344.456, -2.014]];
    let b = [[-337.536, 2.0], [-338.336, 2.105], [-340.042, 1.541], [-342.082, 0.274], [-344.472, -1.986]];
    let wire = Wire2::new([
        spline(&a),
        TrimmedCurve2::line(Point2::new(-344.456, -2.014), Point2::new(-344.472, -1.986)).unwrap(),
        spline(&b).reversed(),
    ], tolerance).unwrap();
    let plane = Plane3::new(Point3::new(0.0, 57.39, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
    let blade = Profile::new(wire, [], tolerance).unwrap().extrude(plane, Vec3::new(0.0, 32.87, 0.0)).unwrap();
    let united = drum.union(&blade).unwrap();
    eprintln!("solids {} faces {}", united.solids().count(), united.faces().count());
}

// The first compressor stage of the jet engine model, joined as a radial
// pattern into the drum like the app's pattern feature does.
#[test]
#[ignore]
fn compressor_pattern() {
    let tolerance = ModelTolerance::default();
    let count: usize = std::env::var("BLADES").ok().and_then(|v| v.parse().ok()).unwrap_or(28);
    let drum = Profile::new(Wire2::polygon([
        [-200.0, 70.0], [-215.0, 66.0], [-305.0, 66.0], [-320.0, 56.0], [-405.0, 70.0],
        [-415.0, 62.0], [-487.0, 62.0], [-487.0, 0.0], [-200.0, 0.0],
    ].map(|p| Point2::new(p[0], p[1])), tolerance).unwrap(), [], tolerance).unwrap()
        .revolve(Plane3::xy(), Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x()).unwrap(), std::f64::consts::TAU)
        .unwrap();
    let a = [[-218.106, -5.785], [-219.794, -5.557], [-223.307, -3.583], [-227.373, -0.027], [-231.918, 5.758]];
    let b = [[-218.106, -5.785], [-218.964, -4.568], [-222.15, -2.204], [-226.496, 1.018], [-231.871, 5.813]];
    let wire = Wire2::new([
        spline(&a),
        TrimmedCurve2::line(Point2::new(-231.918, 5.758), Point2::new(-231.871, 5.813)).unwrap(),
        spline(&b).reversed(),
    ], tolerance).unwrap();
    let plane = Plane3::new(Point3::new(0.0, 64.5, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
    let blade = Profile::new(wire, [], tolerance).unwrap().extrude(plane, Vec3::new(0.0, 45.19, 0.0)).unwrap();
    let rotated = |i: usize| {
        let angle = std::f64::consts::TAU * i as f64 / count as f64;
        RigidTransform3::from_quaternion(
            Quaternion::new((angle / 2.0).cos(), (angle / 2.0).sin(), 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0), tolerance).unwrap()
    };
    let copies = (1..count).map(|i| blade.transformed(rotated(i)).unwrap()).collect::<Vec<_>>();
    let tool = copies[0].combined(&copies[1..]).unwrap();
    let body = drum.combined(&[blade]).unwrap();
    let start = Instant::now();
    match body.union(&tool) {
        Ok(result) => eprintln!("solids {} faces {} {:?}", result.solids().count(), result.faces().count(), start.elapsed()),
        Err(error) => panic!("{error:?} after {:?}", start.elapsed()),
    }
}

const BLADES: [([[f64; 2]; 5], [[f64; 2]; 5], f64, f64); 13] = [
    ([[-218.106, -5.785], [-219.794, -5.557], [-223.307, -3.583], [-227.373, -0.027], [-231.918, 5.758]], [[-218.106, -5.785], [-218.964, -4.568], [-222.15, -2.204], [-226.496, 1.018], [-231.871, 5.813]], 64.5, 45.19),
    ([[-248.106, -5.785], [-249.794, -5.557], [-253.307, -3.583], [-257.373, -0.027], [-261.918, 5.758]], [[-248.106, -5.785], [-248.964, -4.568], [-252.15, -2.204], [-256.496, 1.018], [-261.871, 5.813]], 64.5, 41.08),
    ([[-278.106, -5.785], [-279.794, -5.557], [-283.307, -3.583], [-287.373, -0.027], [-291.918, 5.758]], [[-278.106, -5.785], [-278.964, -4.568], [-282.15, -2.204], [-286.496, 1.018], [-291.871, 5.813]], 64.5, 36.98),
    ([[-233.938, 3.5], [-234.76, 2.683], [-237.519, 1.303], [-241.283, -0.577], [-246.048, -3.524]], [[-233.938, 3.5], [-235.262, 3.554], [-238.219, 2.516], [-241.814, 0.341], [-246.076, -3.476]], 64.5, 43.25),
    ([[-263.938, 3.5], [-264.76, 2.683], [-267.519, 1.303], [-271.283, -0.577], [-276.048, -3.524]], [[-263.938, 3.5], [-265.262, 3.554], [-268.219, 2.516], [-271.814, 0.341], [-276.076, -3.476]], 64.5, 39.14),
    ([[-293.938, 3.5], [-294.76, 2.683], [-297.519, 1.303], [-301.283, -0.577], [-306.048, -3.524]], [[-293.938, 3.5], [-295.262, 3.554], [-298.219, 2.516], [-301.814, 0.341], [-306.076, -3.476]], 63.79, 35.75),
    ([[-326.553, -2.893], [-327.459, -2.853], [-329.24, -1.895], [-331.252, -0.092], [-333.459, 2.879]], [[-326.553, -2.893], [-326.92, -2.21], [-328.488, -0.998], [-330.682, 0.587], [-333.436, 2.906]], 55.58, 36.36),
    ([[-348.553, -2.893], [-349.459, -2.853], [-351.24, -1.895], [-353.252, -0.092], [-355.459, 2.879]], [[-348.553, -2.893], [-348.92, -2.21], [-350.488, -0.998], [-352.682, 0.587], [-355.436, 2.906]], 59.2, 29.38),
    ([[-370.553, -2.893], [-371.459, -2.853], [-373.24, -1.895], [-375.252, -0.092], [-377.459, 2.879]], [[-370.553, -2.893], [-370.92, -2.21], [-372.488, -0.998], [-374.682, 0.587], [-377.436, 2.906]], 62.83, 22.39),
    ([[-392.553, -2.893], [-393.459, -2.853], [-395.24, -1.895], [-397.252, -0.092], [-399.459, 2.879]], [[-392.553, -2.893], [-392.92, -2.21], [-394.488, -0.998], [-396.682, 0.587], [-399.436, 2.906]], 66.45, 15.4),
    ([[-337.536, 2.0], [-337.962, 1.459], [-339.522, 0.641], [-341.688, -0.409], [-344.456, -2.014]], [[-337.536, 2.0], [-338.336, 2.105], [-340.042, 1.541], [-342.082, 0.274], [-344.472, -1.986]], 57.39, 32.87),
    ([[-359.536, 2.0], [-359.962, 1.459], [-361.522, 0.641], [-363.688, -0.409], [-366.456, -2.014]], [[-359.536, 2.0], [-360.336, 2.105], [-362.042, 1.541], [-364.082, 0.274], [-366.472, -1.986]], 61.01, 25.88),
    ([[-381.536, 2.0], [-381.962, 1.459], [-383.522, 0.641], [-385.688, -0.409], [-388.456, -2.014]], [[-381.536, 2.0], [-382.336, 2.105], [-384.042, 1.541], [-386.082, 0.274], [-388.472, -1.986]], 64.64, 18.89),
];

fn drum(tolerance: ModelTolerance) -> Compound {
    Profile::new(Wire2::polygon([
        [-200.0, 70.0], [-215.0, 66.0], [-305.0, 66.0], [-320.0, 56.0], [-405.0, 70.0],
        [-415.0, 62.0], [-487.0, 62.0], [-487.0, 0.0], [-200.0, 0.0],
    ].map(|p| Point2::new(p[0], p[1])), tolerance).unwrap(), [], tolerance).unwrap()
        .revolve(Plane3::xy(), Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x()).unwrap(), std::f64::consts::TAU)
        .unwrap()
}

fn blade((a, b, offset, distance): ([[f64; 2]; 5], [[f64; 2]; 5], f64, f64), tolerance: ModelTolerance) -> Compound {
    let wire = Wire2::new([
        spline(&a),
        TrimmedCurve2::line(Point2::new(a[4][0], a[4][1]), Point2::new(b[4][0], b[4][1])).unwrap(),
        spline(&b).reversed(),
    ], tolerance).unwrap();
    let plane = Plane3::new(Point3::new(0.0, offset, 0.0), Vec3::unit_x(), -Vec3::unit_z(), tolerance).unwrap();
    Profile::new(wire, [], tolerance).unwrap().extrude(plane, Vec3::new(0.0, distance, 0.0)).unwrap()
}

// The drum and source blades of the compressor are united in body order
// before pattern copies join them.
#[test]
#[ignore]
fn compressor_bodies_union() {
    let tolerance = ModelTolerance::default();
    let mut body = drum(tolerance);
    for (index, row) in BLADES.into_iter().enumerate() {
        body = match body.union(&blade(row, tolerance)) {
            Ok(body) => body,
            Err(error) => panic!("blade {index}: {error:?}"),
        };
    }
    eprintln!("solids {} faces {}", body.solids().count(), body.faces().count());
}

// The full compressor pattern: drum and source blades joined with 27 copies
// of every blade, as one Boolean like the pattern feature.
#[test]
#[ignore]
fn compressor_full_pattern() {
    let tolerance = ModelTolerance::default();
    let count: usize = std::env::var("BLADES").ok().and_then(|v| v.parse().ok()).unwrap_or(28);
    let blades = BLADES.into_iter().map(|row| blade(row, tolerance)).collect::<Vec<_>>();
    let rotated = |i: usize| {
        let angle = std::f64::consts::TAU * i as f64 / count as f64;
        RigidTransform3::from_quaternion(
            Quaternion::new((angle / 2.0).cos(), (angle / 2.0).sin(), 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0), tolerance).unwrap()
    };
    let copies = (1..count).flat_map(|i| blades.iter().map(move |blade| blade.transformed(rotated(i)).unwrap()))
        .collect::<Vec<_>>();
    let tool = copies[0].combined(&copies[1..]).unwrap();
    let body = drum(tolerance).combined(&blades).unwrap();
    let start = Instant::now();
    match body.union(&tool) {
        Ok(result) => eprintln!("solids {} faces {} {:?}", result.solids().count(), result.faces().count(), start.elapsed()),
        Err(error) => panic!("{error:?} after {:?}", start.elapsed()),
    }
}
