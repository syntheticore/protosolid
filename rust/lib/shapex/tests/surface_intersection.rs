use shapex::*;

fn graph(heights: &[f64]) -> Surface3 {
    let degree = heights.len() - 1;
    NurbsSurface3::new(
        (degree, 1),
        [-1.0, 1.0].into_iter().map(|y| heights.iter().enumerate().map(|(i, z)| {
            Point3::new(-1.0 + 2.0 * i as f64 / degree as f64, y, *z)
        }).collect()).collect(),
        vec![vec![1.0; heights.len()]; 2],
        ([vec![-1.0; degree + 1], vec![1.0; degree + 1]].concat(), vec![-1.0, -1.0, 1.0, 1.0]),
    ).unwrap().into()
}

#[test]
fn shallow_transverse_planes_retain_their_intersection() {
    let result = graph(&[0.0, 0.0]).intersect_surface(&graph(&[-1.0e-6, 1.0e-6]), ModelTolerance::default()).unwrap();
    let IntersectionSet::Isolated(curves) = result else { panic!("missing shallow crossing: {result:?}") };
    assert_eq!(curves.len(), 1);
}

#[test]
fn parallel_center_does_not_hide_two_shallow_branches() {
    let result = graph(&[0.0, 0.0]).intersect_surface(&graph(&[7.5e-5, -1.25e-4, 7.5e-5]), ModelTolerance::default()).unwrap();
    let IntersectionSet::Isolated(curves) = result else { panic!("missing quadratic crossings: {result:?}") };
    assert_eq!(curves.len(), 2);
    for curve in curves {
        let DomainBounds::Bounded(range) = curve.curve.domain().bounds else { panic!("unbounded crossing") };
        let x = curve.curve.evaluate(range.middle()).unwrap().point.x;
        assert!((x.abs() - 0.5).abs() < 1.0e-7);
    }
}

#[test]
fn shallow_curved_crossing_is_not_a_double_root() {
    let result = graph(&[0.0, 0.0]).intersect_surface(&graph(&[-9.9e-7, -1.0e-8, 1.01e-6]), ModelTolerance::default()).unwrap();
    let IntersectionSet::Isolated(curves) = result else { panic!("missing curved crossing: {result:?}") };
    assert_eq!(curves.len(), 1);
}

#[test]
fn touching_parabolic_patch_does_not_create_crossing_curves() {
    let result = graph(&[0.0, 0.0]).intersect_surface(&graph(&[1.0e-4, -1.0e-4, 1.0e-4]), ModelTolerance::default()).unwrap();
    assert!(matches!(result, IntersectionSet::Empty), "spurious tangent crossing: {result:?}");
}

fn curved_contact_pair(inset: f64) -> (Surface3, Surface3) {
    let meridian = [(2.0, 0.0), (1.0, 1.0), (3.0, 3.0)];
    let quarter = NurbsSurface3::new((2, 2), meridian.iter().map(|(r, z)| {
        vec![Point3::new(*r, 0.0, *z), Point3::new(*r, *r, *z), Point3::new(0.0, *r, *z)]
    }).collect(), vec![vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]; 3],
        (vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0])).unwrap().into();
    let strip = NurbsSurface3::new((2, 1), [0.0, 1.0].into_iter().map(|y| meridian.iter()
        .map(|(x, z)| Point3::new(x - inset, y, *z)).collect()).collect(), vec![vec![1.0; 3]; 2],
        (vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0])).unwrap().into();
    (quarter, strip)
}

#[test]
fn curved_boundary_contact_does_not_exhaust_subdivision() {
    let (quarter, strip) = curved_contact_pair(0.0);
    for (first, second) in [(&quarter, &strip), (&strip, &quarter)] {
        assert!(matches!(first.intersect_surface(second, ModelTolerance::default()).unwrap(), IntersectionSet::Empty));
    }
}

#[test]
fn inset_strip_still_crosses_the_curved_patch() {
    let (quarter, strip) = curved_contact_pair(0.01);
    for (first, second) in [(&quarter, &strip), (&strip, &quarter)] {
        let result = first.intersect_surface(second, ModelTolerance::default()).unwrap();
        let IntersectionSet::Isolated(curves) = result else { panic!("missing inset crossing: {result:?}") };
        assert_eq!(curves.len(), 1);
    }
}
