use shapex::kernel::*;
use shapex::Point2;

// Two adjacent Hermite spans from an offset spline. The intersection solver
// reports their shared endpoint within its root tolerance, not exactly on it.
#[test]
fn adjacent_offset_spline_spans_do_not_self_intersect() {
    let points = vec![
        Point2::new(-59.895748852533245, -2.288395285233081),
        Point2::new(-59.88950508812803, -2.1577922166129677),
        Point2::new(-59.8824140160845, -2.0270830504948147),
        Point2::new(-59.87446311003153, -1.8962802836298014),
        Point2::new(-59.86651220397855, -1.765477516764788),
        Point2::new(-59.85770145936893, -1.6345811489540418),
        Point2::new(-59.8480187391911, -1.5036047876678336),
    ];
    let curve = NurbsCurve2::new(
        3,
        points.clone(),
        vec![1.0; 7],
        vec![
            0.031068781263087906,
            0.031068781263087906,
            0.031068781263087906,
            0.031068781263087906,
            0.03279482466659279,
            0.03279482466659279,
            0.03279482466659279,
            0.034520868070097674,
            0.034520868070097674,
            0.034520868070097674,
            0.034520868070097674,
        ],
    )
    .unwrap();
    let edge = TrimmedCurve2::new(
        curve,
        ParameterRange::new(0.031068781263087906, 0.034520868070097674).unwrap(),
    )
    .unwrap();
    let closing = TrimmedCurve2::line(points[6], points[0]).unwrap();
    Wire2::new(vec![edge, closing], ModelTolerance::default()).unwrap();
}
