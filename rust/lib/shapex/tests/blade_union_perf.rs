use shapex::*;
use std::time::Instant;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64, tolerance: ModelTolerance) -> Profile {
    Profile::new(Wire2::polygon([
        Point2::new(x0, y0), Point2::new(x1, y0), Point2::new(x1, y1), Point2::new(x0, y1),
    ], tolerance).unwrap(), [], tolerance).unwrap()
}

// Joins many blades to a revolved hub one at a time, like a pattern of features.
#[test]
#[ignore]
fn blade_union_perf() {
    let tolerance = ModelTolerance::default();
    let count: usize = std::env::var("BLADES").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    let mut hub = rect(-480.0, 0.0, -200.0, 66.0, tolerance)
        .revolve(Plane3::xy(), Axis3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x()).unwrap(), std::f64::consts::TAU)
        .unwrap();
    let stages: usize = std::env::var("STAGES").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let blades = (0..stages).map(|stage| {
        let (a, x) = (0.7_f64, -230.0 - stage as f64 * 20.0);
        let plane = Plane3::new(Point3::new(x, 64.5, 0.0), Vec3::new(a.cos(), 0.0, a.sin()),
            Vec3::new(-a.sin(), 0.0, a.cos()), tolerance).unwrap();
        rect(-6.0, -0.8, 6.0, 0.8, tolerance).extrude(plane, Vec3::unit_y() * 40.0).unwrap()
    }).collect::<Vec<_>>();
    let rotated = |i: usize| {
        let angle = std::f64::consts::TAU * i as f64 / count as f64;
        RigidTransform3::from_quaternion(
            Quaternion::new((angle / 2.0).cos(), (angle / 2.0).sin(), 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0), tolerance).unwrap()
    };
    if std::env::var("BATCH").is_ok() {
        // Like a pattern feature: the copies are united first, then joined
        // to the component holding the hub and the source blades.
        let copies = (1..count).flat_map(|i| blades.iter().map(move |blade| blade.transformed(rotated(i)).unwrap()))
            .collect::<Vec<_>>();
        let tool = copies[0].combined(&copies[1..]).unwrap();
        let start = Instant::now();
        let body = hub.combined(&blades).unwrap();
        hub = body.union(&tool).unwrap();
        eprintln!("batch solids {} faces {} {:?}", hub.solids().count(), hub.faces().count(), start.elapsed());
        return;
    }
    // ORDER lists "copy:stage" pairs to unite, for narrowing down failures.
    let order = std::env::var("ORDER").ok().map(|order| order.split(',').map(|pair| {
        let (i, stage) = pair.split_once(':').unwrap();
        (i.parse::<usize>().unwrap(), stage.parse::<usize>().unwrap())
    }).collect::<Vec<_>>()).unwrap_or_else(|| (0..count).flat_map(|i| (0..3).map(move |stage| (i, stage))).collect());
    for (i, stage) in order {
        let start = Instant::now();
        hub = match hub.union(&blades[stage].transformed(rotated(i)).unwrap()) {
            Ok(hub) => hub,
            Err(error) => panic!("blade {i}:{stage} {error:?}"),
        };
        eprintln!("blade {i}:{stage} faces {} {:?}", hub.faces().count(), start.elapsed());
    }
}
