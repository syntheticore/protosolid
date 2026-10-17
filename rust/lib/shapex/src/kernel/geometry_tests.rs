use super::*;
use super::super::{IntersectionKind, IntersectionSet};

#[test]
fn spline_support_coincidence_uses_rational_polynomials_across_knots_and_degrees() {
    let tolerance = ModelTolerance::default();
    let surface = NurbsSurface3::new((2, 1), vec![
        vec![Point3::new(-3.0, 0.0, 0.0), Point3::new(0.0, 2.0, 1.0), Point3::new(4.0, 0.0, 2.0)],
        vec![Point3::new(-3.0, 0.0, 3.0), Point3::new(0.0, 2.0, 4.0), Point3::new(4.0, 0.0, 5.0)],
    ], vec![vec![1.0, 0.8, 1.2]; 2], (
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0],
    )).unwrap();
    let extended = surface.extended_bezier([0.2, 0.3]).unwrap();

    // Multiplying the homogeneous polynomial by a positive linear factor
    // changes its degree and rational weights without changing any point.
    let multiply = |controls: &[Vec4]| {
        let degree = controls.len();
        (0..=degree).map(|index| {
            let t = index as f64 / degree as f64;
            let mut point = Vec4::new(0.0, 0.0, 0.0, 0.0);
            if index < degree { point += controls[index] * (1.0 - t); }
            if index > 0 { point += controls[index - 1] * (2.0 * t); }
            point
        }).collect::<Vec<_>>()
    };
    let mut net = surface.homogeneous_rows().iter().map(|row| multiply(row)).collect::<Vec<_>>();
    let columns = (0..net[0].len()).map(|column| {
        multiply(&net.iter().map(|row| row[column]).collect::<Vec<_>>())
    }).collect::<Vec<_>>();
    net = (0..columns[0].len()).map(|row| columns.iter().map(|column| column[row]).collect()).collect();
    let mut u_knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    for row in &mut net {
        let mut knots = u_knots.clone();
        insert_homogeneous_knot4(3, 0.4, &mut knots, row).unwrap();
    }
    u_knots.insert(4, 0.4);
    let mut v_knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let columns = (0..net[0].len()).map(|column| {
        let mut controls = net.iter().map(|row| row[column]).collect::<Vec<_>>();
        let mut knots = v_knots.clone();
        insert_homogeneous_knot4(2, 0.6, &mut knots, &mut controls).unwrap();
        controls
    }).collect::<Vec<_>>();
    net = (0..columns[0].len()).map(|row| columns.iter().map(|column| column[row]).collect()).collect();
    v_knots.insert(3, 0.6);
    let refined = NurbsSurface3::new((3, 2), net.iter().map(|row| row.iter().map(|p| {
        Point3::new(p.x / p.w, p.y / p.w, p.z / p.w)
    }).collect()).collect(), net.iter().map(|row| row.iter().map(|p| p.w).collect()).collect(),
        (u_knots, v_knots)).unwrap();
    for other in [&extended, &refined] {
        assert!(surface.coincident_support(other, tolerance).unwrap());
        assert!(other.coincident_support(&surface, tolerance).unwrap());
        let a = Surface3::Nurbs(surface.clone());
        let b = Surface3::Nurbs(other.clone());
        assert!(matches!(a.intersect_surface(&b, tolerance).unwrap(), IntersectionSet::Coincident));
        assert!(super::super::trim::coincident_supports(&a, &b, tolerance).unwrap());
    }
    assert!(extended.coincident_support(&refined, tolerance).unwrap());
    let mut controls = refined.control_points().to_vec();
    controls[1][1].z += 0.001;
    let different = NurbsSurface3::new(refined.degrees(), controls, refined.weights().to_vec(),
        (refined.knots().0.to_vec(), refined.knots().1.to_vec())).unwrap();
    assert!(!surface.coincident_support(&different, tolerance).unwrap());
}

#[test]
fn joins_independent_intersection_charts_with_reversed_parameters_and_supports() {
    let tol = ModelTolerance::default();
    let xy: Surface3 = Plane3::xy().into();
    let xz: Surface3 = Plane3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_x(), Vec3::unit_z(), tol).unwrap().into();
    let guide: Curve2 = Line2::new(Point2::new(0.0, 0.0), Vec2::unit_x()).unwrap().into();
    let first_range = ParameterRange::new(2.0, 4.0).unwrap();
    let first = SurfaceIntersectionCurve::new(xy.clone(), xz.clone(), guide.clone(), guide,
        first_range, tol, 0.0).unwrap();
    let guide: Curve2 = Line2::new(Point2::new(10.0, 0.0), Vec2::new(-2.0, 0.0)).unwrap().into();
    let second = SurfaceIntersectionCurve::new(xz, xy, guide.clone(), guide,
        ParameterRange::new(2.0, 3.0).unwrap(), tol, 0.0).unwrap();
    let joined = first.joined(first_range, &second, ParameterRange::new(3.0, 2.0).unwrap())
        .unwrap().unwrap();
    for i in 0..=32 {
        let t = i as f64 / 32.0;
        let value = joined.evaluate(t).unwrap();
        assert!(value.point.distance(Point3::new(2.0 + 4.0 * t, 0.0, 0.0)) < tol.linear);
        assert!((value.first_derivative - Vec3::unit_x() * 4.0).magnitude() < tol.linear);
    }
}

#[test]
fn spliced_rational_guides_preserve_trims_lifts_and_extension() {
    let first: Curve2 = NurbsCurve2::new(2,
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 2.0), Point2::new(3.0, 1.0)],
        vec![1.0, 0.75, 2.0], vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap().into();
    let second: Curve2 = Line2::new(Point2::new(2.0, 3.0), Vec2::new(4.0, 1.0)).unwrap().into();
    let first_range = ParameterRange::new(0.8, 0.2).unwrap();
    let second_range = ParameterRange::new(5.0, 7.0).unwrap();
    let offset = Vec2::new(std::f64::consts::TAU, 0.0);
    let joined = join_intersection_guides(&first, first_range, &second, second_range, 0.4, offset)
        .unwrap().unwrap();
    let extended = joined.extended(ParameterRange::new(-0.1, 1.1).unwrap()).unwrap();
    for i in 0..=32 {
        let fraction = i as f64 / 32.0;
        let a = first.evaluate(first_range.start + first_range.span() * fraction).unwrap().point;
        let b = second.evaluate(second_range.start + second_range.span() * fraction).unwrap().point + offset;
        if i < 32 {
            assert!(joined.evaluate(fraction * 0.4).unwrap().point.distance(a) < 1e-12);
            assert!(extended.evaluate(fraction * 0.4).unwrap().point.distance(a) < 1e-12);
        }
        assert!(joined.evaluate(0.4 + fraction * 0.6).unwrap().point.distance(b) < 1e-12);
        assert!(extended.evaluate(0.4 + fraction * 0.6).unwrap().point.distance(b) < 1e-12);
    }
}

#[test]
fn distant_line_origin_preserves_circle_tangency() {
    let tolerance = ModelTolerance::default();
    // Coordinates from the extrusion that split one tangent into two nearby hits.
    let center = Point3::new(15.318275486591771, 12.199999999999998, 19.485467329507593);
    let circle: Curve3 = Circle3::new(
        center,
        Vec3::new(0.0, 0.0, -0.3),
        Vec3::new(0.0, 0.3, 0.0),
        tolerance,
    ).unwrap().into();
    let line: Curve3 = Line3::new(
        Point3::new(15.318275486591771, 12.499999999999998, -30.303258588388346),
        Vec3::new(0.0, 0.0, 57.777220306116355),
    ).unwrap().into();

    let IntersectionSet::Isolated(hits) = line.intersect_curve(&circle, tolerance).unwrap() else {
        panic!("expected one tangent hit");
    };
    assert_eq!(hits.len(), 1);
    let hit = hits[0];
    assert_eq!(hit.kind, IntersectionKind::Tangent);
    assert_eq!(hit.multiplicity, 2);
    assert!(hit.point.distance(Point3::new(center.x, center.y + 0.3, center.z)) < tolerance.linear);
    assert!(line.evaluate(hit.first_parameter).unwrap().point
        .distance(circle.evaluate(hit.second_parameter).unwrap().point) < tolerance.linear);
}

#[test]
fn periodic_spatial_projection_preserves_uv_winding() {
    let tolerance = ModelTolerance::default();
    let surface: Surface3 = Cylinder3::new(Point3::new(0.0, 0.0, 0.0),
        Vec3::unit_z(), Vec3::unit_x() * 3.0, Vec3::unit_y() * 3.0, tolerance).unwrap().into();
    let tau = std::f64::consts::TAU;
    for direction in [-1.0, 1.0] {
        let guide: Curve2 = Line2::new(Point2::new(0.3, 2.0), Vec2::new(direction, 0.0)).unwrap().into();
        for range in [ParameterRange::new(0.0, tau).unwrap(), ParameterRange::new(tau, 0.0).unwrap()] {
            let curve = surface.boundary_curve(guide.clone(), range, tolerance).unwrap();
            let projection: Curve2 = SurfaceProjectionPcurve2::new(curve.clone(), surface.clone(),
                guide.clone(), range, tolerance).unwrap().into();
            assert_eq!(projection.domain().period, None);
            for parameter in [-2.0 * tau - 0.1, -0.1, 0.0, tau, tau + 0.1, 3.0 * tau + 0.1] {
                let uv = projection.evaluate(parameter).unwrap();
                assert!((uv.point.x - (0.3 + direction * parameter)).abs() < tolerance.parameter);
                assert!(surface.evaluate(uv.point.x, uv.point.y).unwrap().point
                    .distance(curve.evaluate(parameter).unwrap().point) < tolerance.linear);
                assert!((uv.first_derivative.x - direction).abs() < tolerance.parameter);
            }
        }
    }
}

#[test]
fn analytic_boundary_construction_preserves_pcurve_parameters() {
    let tolerance = ModelTolerance::default();
    let origin = Point3::new(3.0, -2.0, 5.0);
    let plane: Surface3 = Plane3::new(origin, Vec3::unit_x() * 2.0, Vec3::unit_y() * 3.0, tolerance).unwrap().into();
    let cylinder: Surface3 = Cylinder3::new(origin, Vec3::unit_z() * 2.0, Vec3::unit_x() * 3.0, Vec3::unit_y() * 3.0, tolerance).unwrap().into();
    let cone: Surface3 = Cone3::new(origin, Vec3::unit_z() * 2.0, Vec3::unit_x(), Vec3::unit_y(), tolerance).unwrap().into();
    let torus: Surface3 = Torus3::new(origin, Vec3::unit_x() * 5.0, Vec3::unit_y() * 5.0, 2.0, tolerance).unwrap().into();
    let range = ParameterRange::new(-2.0, 7.0).unwrap();
    for (surface, uv, direction, circular) in [
        (&plane, Point2::new(0.3, 0.7), Vec2::new(2.0, -3.0), false),
        (&cylinder, Point2::new(0.3, 0.7), Vec2::new(0.0, -2.0), false),
        (&cone, Point2::new(0.3, 0.7), Vec2::new(0.0, 2.0), false),
        (&cylinder, Point2::new(0.3, 0.7), Vec2::new(1.0, 0.0), true),
        (&cone, Point2::new(0.3, -0.7), Vec2::new(-1.0, 0.0), true),
        (&torus, Point2::new(0.3, 0.7), Vec2::new(-1.0, 0.0), true),
        (&torus, Point2::new(0.3, 0.7), Vec2::new(0.0, 1.0), true),
    ] {
        let pcurve: Curve2 = Line2::new(uv, direction).unwrap().into();
        let curve = surface.boundary_curve(pcurve, range, tolerance).unwrap();
        assert!(if circular { matches!(curve, Curve3::Circle(_)) } else { matches!(curve, Curve3::Line(_)) });
        for t in [-2.0, 0.1, 4.5, 7.0] {
            let uv = uv + direction * t;
            let expected = surface.evaluate(uv.x, uv.y).unwrap();
            let actual = curve.evaluate(t).unwrap();
            assert!(actual.point.distance(expected.point) < 1.0e-12);
            assert!((actual.first_derivative - expected.du * direction.x - expected.dv * direction.y).magnitude() < 1.0e-12);
        }
    }
    // Even a nearly circular helix must remain generic: no tolerance-based
    // recognition is allowed to erase its axial motion.
    for direction in [Vec2::new(1.0, 1.0e-10), Vec2::new(2.0, 0.0)] {
        let pcurve: Curve2 = Line2::new(Point2::new(0.0, 0.0), direction).unwrap().into();
        assert!(matches!(cylinder.boundary_curve(pcurve, range, tolerance).unwrap(), Curve3::SurfaceCurve(_)));
    }
}

#[test]
fn parameter_cache_is_bounded_exact_and_shared_with_local_eviction() {
    let cache = ParameterEvaluationCache::<usize>::default();
    assert!(cache.0.lock().unwrap().sets.is_empty());
    cache.insert(0.0, 1);
    cache.insert(-0.0, 2);
    assert_eq!(cache.get(0.0), Some(1));
    assert_eq!(cache.get(-0.0), Some(2));
    cache.insert(0.0, 3);
    assert_eq!(cache.clone().get(0.0), Some(3));

    let set = parameter_cache_set(123.0_f64.to_bits());
    let protected = (1..1000)
        .map(|i| i as f64)
        .find(|p| parameter_cache_set(p.to_bits()) != set)
        .unwrap();
    cache.insert(protected, 99);
    let colliding = (1..10000)
        .map(|i| i as f64)
        .filter(|p| parameter_cache_set(p.to_bits()) == set)
        .take(PARAMETER_CACHE_WAYS * 3)
        .collect::<Vec<_>>();
    assert_eq!(colliding.len(), PARAMETER_CACHE_WAYS * 3);
    for (index, parameter) in colliding.into_iter().enumerate() {
        cache.insert(parameter, index);
        assert_eq!(cache.get(parameter), Some(index));
        assert_eq!(
            cache.get(protected),
            Some(99),
            "eviction must not flush unrelated sets"
        );
    }
    let entries = cache.0.lock().unwrap();
    assert_eq!(entries.sets.len(), PARAMETER_CACHE_SETS);
    assert!(
        entries.sets.iter().flatten().count() <= 4,
        "allocate only the sets used by these samples"
    );
}

#[test]
fn prepared_blend_sections_preserve_circular_and_ruled_differentials() {
    let tolerance = ModelTolerance::default();
    let guide: Curve2 = Line2::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))
        .unwrap()
        .into();
    let surface = EdgeBlendSurface3::new(
        Line3::new(Point3::new(0.0, 0.0, 0.0), Vec3::unit_z())
            .unwrap()
            .into(),
        Plane3::new(
            Point3::new(2.0, 0.0, 0.0),
            Vec3::unit_z(),
            Vec3::unit_y(),
            tolerance,
        )
        .unwrap()
        .into(),
        Plane3::new(
            Point3::new(0.0, 2.0, 0.0),
            Vec3::unit_z(),
            Vec3::unit_x(),
            tolerance,
        )
        .unwrap()
        .into(),
        guide.clone(),
        guide,
        ParameterRange::new(0.0, 1.0).unwrap(),
        2.0,
        tolerance,
    )
    .unwrap();
    let ruled = EdgeBlendSurface3::with_section(
        surface.center_curve().clone(),
        surface.first_surface().clone(),
        Plane3::new(Point3::new(0.0, 3.0, 0.0), Vec3::unit_z(), Vec3::unit_x(), tolerance)
            .unwrap().into(),
        surface.first_pcurve().clone(),
        surface.second_pcurve().clone(),
        surface.center_range(),
        surface.radius(),
        tolerance,
        true,
    )
    .unwrap();
    let angle = std::f64::consts::FRAC_PI_2;
    for u in [0.0, 0.3, 1.0] {
        for v in [-0.2, 0.0, 0.25, 0.5, 1.0, 1.2] {
            let value = surface.evaluate(u, v).unwrap();
            let (sin, cos) = (v * angle).sin_cos();
            assert!(value.point.distance(Point3::new(2.0 * cos, 2.0 * sin, u)) < 1e-12);
            assert!((value.du - Vec3::unit_z()).magnitude() < 1e-12);
            assert!(
                (value.dv - Vec3::new(-2.0 * angle * sin, 2.0 * angle * cos, 0.0)).magnitude()
                    < 1e-12
            );
            assert!(value.duu.magnitude() < 1e-8 && value.duv.magnitude() < 1e-8);
            assert!(
                (value.dvv + Vec3::new(cos, sin, 0.0) * (2.0 * angle * angle)).magnitude() < 1e-8
            );
            let (point, du, dv) = ruled.first_order(u, v).unwrap();
            assert!(point.distance(Point3::new(2.0 * (1.0 - v), 3.0 * v, u)) < 1e-12);
            assert_eq!(du, Vec3::unit_z());
            assert_eq!(dv, Vec3::new(-2.0, 3.0, 0.0));
        }
    }
}

#[test]
fn local_spline_jets_match_full_derivative_curves() {
    for degree in [1, 2, 3, 5, 16, 20] {
        for multiplicity in [1, degree, degree + 1] {
            let mut knots = vec![0.0; degree + 1];
            for knot in [0.07, 0.31, 0.52, 0.9] {
                knots.extend(std::iter::repeat_n(knot, multiplicity));
            }
            knots.extend(std::iter::repeat_n(1.0, degree + 1));
            check_jet(degree, knots);
        }
        // Unclamped endpoints have a different active span from the knot ends.
        check_jet(
            degree,
            (0..degree * 3 + 8)
                .map(|i| (i as f64 * 0.1).powi(2))
                .collect(),
        );
    }
}

fn check_jet(degree: usize, knots: Vec<f64>) {
    let count = knots.len() - degree - 1;
    let controls = (0..count)
        .map(|i| {
            let weight = 0.5 + (i % 7) as f64 * 0.2;
            Vec4::new(
                (i as f64).sin() * weight,
                (i as f64).cos() * weight,
                i as f64 * weight,
                weight,
            )
        })
        .collect::<Vec<_>>();
    let first = derivative_controls(degree, &knots, &controls).unwrap();
    let second = derivative_controls(first.0, &first.1, &first.2);
    let third = second.as_ref().and_then(|(d, k, c)| derivative_controls(*d, k, c));
    let start = knots[degree];
    let end = knots[count];
    let parameters = knots
        .iter()
        .copied()
        .filter(|t| *t >= start && *t <= end)
        .chain((0..=100).map(|i| start + (end - start) * i as f64 / 100.0));
    for t in parameters {
        let mut accessed = 0;
        let actual = homogeneous_curve_jet::<_, 4>(degree, &knots, count, t, |i| {
            accessed += 1;
            controls[i]
        });
        assert_eq!(
            accessed,
            degree + 1,
            "work must be independent of the control count"
        );
        let expected = [
            de_boor_vec4(degree, &knots, &controls, t).unwrap(),
            de_boor_vec4(first.0, &first.1, &first.2, t).unwrap(),
            second
                .as_ref()
                .map(|(d, k, c)| de_boor_vec4(*d, k, c, t).unwrap())
                .unwrap_or(Vec4::zero()),
            third
                .as_ref()
                .map(|(d, k, c)| de_boor_vec4(*d, k, c, t).unwrap())
                .unwrap_or(Vec4::zero()),
        ];
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (actual - expected).magnitude() <= expected.magnitude().max(1.0) * 1e-11,
                "degree {degree}, parameter {t}: {actual:?} != {expected:?}"
            );
        }
        let actual2 = homogeneous_curve_jet::<_, 4>(degree, &knots, count, t, |i| {
            Vec3::new(controls[i].x, controls[i].y, controls[i].w)
        });
        for (actual, expected) in actual2.into_iter().zip(expected) {
            let expected = Vec3::new(expected.x, expected.y, expected.w);
            assert!((actual - expected).magnitude() <= expected.magnitude().max(1.0) * 1e-11);
        }
    }
}

// Full-control-net reference evaluators for the local spline evaluation tests.
fn de_boor_vec4(
    degree: usize,
    knots: &[f64],
    controls: &[Vec4],
    parameter: f64,
) -> KernelResult<Vec4> {
    if degree == 0 {
        let span = find_span(0, knots, controls.len(), parameter);
        return Ok(controls[span]);
    }
    let span = find_span(degree, knots, controls.len(), parameter);
    let mut values: Vec<Vec4> = (0..=degree).map(|j| controls[span - degree + j]).collect();
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let index = span - degree + j;
            let denominator = knots[index + degree + 1 - level] - knots[index];
            if denominator == 0.0 {
                values[j] = values[j - 1];
            } else {
                let alpha = (parameter - knots[index]) / denominator;
                values[j] = values[j - 1] * (1.0 - alpha) + values[j] * alpha;
            }
        }
    }
    let result = values[degree];
    if !result.x.is_finite()
        || !result.y.is_finite()
        || !result.z.is_finite()
        || !result.w.is_finite()
    {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "NURBS evaluation produced a non-finite value",
        ));
    }
    Ok(result)
}

fn derivative_controls(
    degree: usize,
    knots: &[f64],
    controls: &[Vec4],
) -> Option<(usize, Vec<f64>, Vec<Vec4>)> {
    if degree == 0 || controls.len() < 2 {
        return None;
    }
    let derivative = (0..controls.len() - 1)
        .map(|index| {
            let denominator = knots[index + degree + 1] - knots[index + 1];
            if denominator == 0.0 {
                Vec4::new(0.0, 0.0, 0.0, 0.0)
            } else {
                (controls[index + 1] - controls[index]) * (degree as f64 / denominator)
            }
        })
        .collect();
    Some((degree - 1, knots[1..knots.len() - 1].to_vec(), derivative))
}

fn evaluate_derivative_vec4(
    degree: usize,
    knots: &[f64],
    controls: &[Vec4],
    parameter: f64,
    order: usize,
) -> KernelResult<Vec4> {
    if order == 0 {
        return de_boor_vec4(degree, knots, controls, parameter);
    }
    let mut current_degree = degree;
    let mut current_knots = knots.to_vec();
    let mut current_controls = controls.to_vec();
    for _ in 0..order {
        let Some((next_degree, next_knots, next_controls)) =
            derivative_controls(current_degree, &current_knots, &current_controls)
        else {
            return Ok(Vec4::new(0.0, 0.0, 0.0, 0.0));
        };
        current_degree = next_degree;
        current_knots = next_knots;
        current_controls = next_controls;
    }
    de_boor_vec4(current_degree, &current_knots, &current_controls, parameter)
}

#[test]
fn local_surface_jet_matches_full_rational_derivative_nets() {
    for degrees in [(1, 1), (3, 2), (2, 3), (17, 2)] {
        let knots = |degree| {
            let mut values = vec![0.0; degree + 1];
            values.extend([0.3, 0.3, 0.7]);
            values.extend(vec![1.0; degree + 1]);
            values
        };
        let points = (0..degrees.1 + 4)
            .map(|row| {
                (0..degrees.0 + 4)
                    .map(|column| {
                        Point3::new(
                            column as f64,
                            row as f64,
                            ((column * 7 + row * 3) as f64).sin(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let weights = points
            .iter()
            .enumerate()
            .map(|(row, points)| {
                points
                    .iter()
                    .enumerate()
                    .map(|(column, _)| 0.5 + ((row + column * 3) % 7) as f64 / 5.0)
                    .collect()
            })
            .collect();
        let surface = NurbsSurface3::new(
            degrees,
            points,
            weights,
            (knots(degrees.0), knots(degrees.1)),
        )
        .unwrap();
        let rows = surface.homogeneous_rows();
        for u in [0.0, 0.1, 0.3 - 1e-9, 0.3, 0.3 + 1e-9, 0.7, 1.0] {
            for v in [0.0, 0.1, 0.3 - 1e-9, 0.3, 0.3 + 1e-9, 0.7, 1.0] {
                let jet = surface.homogeneous_jet(u, v);
                for u_order in 0..3 {
                    let along_u = rows
                        .iter()
                        .map(|row| {
                            evaluate_derivative_vec4(degrees.0, &surface.knots.0, row, u, u_order)
                                .unwrap()
                        })
                        .collect::<Vec<_>>();
                    for v_order in 0..3 {
                        let expected = evaluate_derivative_vec4(
                            degrees.1,
                            &surface.knots.1,
                            &along_u,
                            v,
                            v_order,
                        )
                        .unwrap();
                        assert!((jet[u_order][v_order] - expected).magnitude()
                            <= 1e-10 * expected.magnitude().max(1.0),
                            "degrees {degrees:?}, parameter ({u}, {v}), derivative ({u_order}, {v_order})");
                    }
                }
            }
        }
    }
}
