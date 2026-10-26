use super::*;

#[derive(Debug, Clone)]
pub(super) struct OffsetEdge {
    pub(super) curve: Curve3,
    pub(super) pcurves: BTreeMap<CoedgeId, Curve2>,
    pub(super) range: ParameterRange,
}

pub(crate) fn face_parameter_bounds(
    topology: &Topology,
    face: FaceId,
    tolerance: crate::ModelTolerance,
) -> KernelResult<[ParameterRange; 2]> {
    let mut bounds = crate::trim::face_parameter_bounds(
        &crate::trim::face_boundary_curves(topology, face, tolerance)?,
        tolerance,
    )?;
    // Trims carry an achieved tolerance. Their bounding box is intersected
    // with the support domain before it is used to evaluate that support.
    for (bound, domain) in bounds.iter_mut().zip(topology.face(face)?.surface().domains()) {
        if domain.period.is_none() {
            if let crate::DomainBounds::Bounded(range) = domain.bounds {
                bound.start = bound.start.max(range.start.min(range.end));
                bound.end = bound.end.min(range.start.max(range.end));
            }
        }
    }
    Ok(bounds)
}

pub(crate) fn exact_pcurve(
    curve: &Curve3,
    surface: &Surface3,
    fallback: &Curve2,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Curve2> {
    if let Surface3::Plane(plane) = surface {
        if let Some(pcurve) = plane.flatten_curve(curve, tolerance)? {
            return Ok(pcurve);
        }
    }
    Ok(match curve {
        Curve3::SurfaceIntersection(intersection) => {
            let exact = intersection.exact_pcurves();
            if intersection.first_surface() == surface {
                exact[0].clone()
            } else if intersection.second_surface() == surface {
                exact[1].clone()
            } else {
                fallback.clone()
            }
        }
        Curve3::SurfaceCurve(curve) if curve.surface() == surface => curve.pcurve().clone(),
        _ => fallback.clone(),
    })
}

pub(super) fn continued_range(
    surface: &Surface3,
    pcurve: &Curve2,
    bounds: [ParameterRange; 2],
    range: ParameterRange,
    tolerance: crate::ModelTolerance,
) -> KernelResult<ParameterRange> {
    let Curve2::Line(line) = pcurve else {
        return Ok(range);
    };
    let direction = line.direction();
    if surface.domains().iter().enumerate().any(|(axis, domain)| {
        direction[axis].abs() > tolerance.parameter && domain.period.is_some()
    }) {
        return Ok(range);
    }
    let mut lower = f64::NEG_INFINITY;
    let mut upper = f64::INFINITY;
    for axis in 0..2 {
        if direction[axis].abs() <= tolerance.parameter {
            continue;
        }
        let a = (bounds[axis].start - line.origin()[axis]) / direction[axis];
        let b = (bounds[axis].end - line.origin()[axis]) / direction[axis];
        lower = lower.max(a.min(b));
        upper = upper.min(a.max(b));
    }
    if lower.is_finite() && upper.is_finite() && lower < upper {
        ParameterRange::new(lower, upper)
    } else {
        Ok(range)
    }
}

pub(super) fn offset_edge(
    topology: &Topology,
    edge: EdgeId,
    supports: &BTreeMap<FaceId, Surface3>,
    bounds: &BTreeMap<FaceId, [ParameterRange; 2]>,
    tolerance: crate::ModelTolerance,
    retain: bool,
    reparameterized: &BTreeSet<FaceId>,
) -> KernelResult<OffsetEdge> {
    let faces = topology.edge_faces(edge)?;
    if faces.len() == 1 {
        // A one-face edge is a periodic parameter seam. Its existing p-curve
        // remains the exact seam on the normally offset support.
        let face = faces[0];
        if reparameterized.contains(&face) {
            return Err(not_implemented("replacement of a periodic seam face"));
        }
        let coedge_id = topology.edge(edge)?.coedges()[0];
        let coedge = topology.coedge(coedge_id)?;
        let range = coedge.range();
        let pcurve = coedge.pcurve().clone();
        let range = continued_range(&supports[&face], &pcurve, bounds[&face], range, tolerance)?;
        let curve = supports[&face].boundary_curve(pcurve.clone(), range, tolerance)?;
        return Ok(OffsetEdge {
            curve,
            pcurves: topology
                .edge(edge)?
                .coedges()
                .iter()
                .map(|coedge| Ok((*coedge, topology.coedge(*coedge)?.pcurve().clone())))
                .collect::<KernelResult<_>>()?,
            range,
        });
    }
    if faces.len() != 2 {
        return Err(topology_error("shelling requires every physical edge to have two incident faces"));
    }
    let first = faces[0];
    let second = faces[1];
    let source = topology.edge(edge)?;
    let source_range = source.range();
    // Keep the original parameterization when neither support moved. Recasting
    // a periodic edge as a bounded surface curve can lose its full-turn trim.
    if retain
        && faces.iter().all(|face| {
            topology
                .face(*face)
                .is_ok_and(|value| value.surface() == &supports[face])
        })
    {
        return Ok(OffsetEdge {
            curve: if let Curve3::SurfaceCurve(curve) = source.curve() {
                curve.surface().boundary_curve(curve.pcurve().clone(), source_range, tolerance)?
            } else {
                source.curve().clone()
            },
            pcurves: source
                .coedges()
                .iter()
                .map(|id| Ok((*id, topology.coedge(*id)?.pcurve().clone())))
                .collect::<KernelResult<_>>()?,
            range: source_range,
        });
    }
    let carried_curve = |face: FaceId, pcurve: &Curve2| -> KernelResult<Curve3> {
        let range = continued_range(
            &supports[&face],
            pcurve,
            bounds[&face],
            source_range,
            tolerance,
        )?;
        supports[&face].boundary_curve(pcurve.clone(), range, tolerance)
    };
    let mut shared_pcurves = BTreeMap::new();
    let mut shared_curve = None;
    let mut continued = false;
    let mut shared_points = Vec::<Vec<Point3>>::new();
    let same_charts = !faces.iter().any(|face| reparameterized.contains(face));
    if same_charts {
        for coedge_id in source.coedges() {
            let coedge = topology.coedge(*coedge_id)?;
            let face = topology.loop_(coedge.loop_id())?.face();
            shared_pcurves.insert(*coedge_id, coedge.pcurve().clone());
            // Carry the curve in a chart where it is an iso-line, if any, so
            // that offset corners beyond the source edge's ends can reach it.
            if shared_curve.is_none() || matches!(coedge.pcurve(), Curve2::Line(_)) && !continued {
                shared_curve = Some(carried_curve(face, coedge.pcurve())?);
                continued = matches!(coedge.pcurve(), Curve2::Line(_));
            }
            shared_points.push(
                [0.0, 0.25, 0.5, 0.75, 1.0]
                    .into_iter()
                    .map(|fraction| {
                        let parameter = source_range.start + source_range.span() * fraction;
                        let uv = coedge.pcurve().evaluate(parameter)?.point;
                        supports[&face]
                            .evaluate(uv.x, uv.y)
                            .map(|value| value.point)
                    })
                    .collect::<KernelResult<Vec<_>>>()?,
            );
        }
    }
    // A spherical corner meets an equal-radius cylinder on a great circle.
    // Keep its full periodic support: rotating a neighboring face can extend
    // the contact past the old spherical patch's bounded parameter curve.
    let pair = match (&supports[&first], &supports[&second]) {
        (Surface3::Sphere(sphere), Surface3::Cylinder(cylinder))
        | (Surface3::Cylinder(cylinder), Surface3::Sphere(sphere)) => Some((sphere, cylinder)),
        _ => None,
    };
    if let Some((sphere, cylinder)) = pair {
        let axis = cylinder.axis().normalize();
        let delta = sphere.center() - cylinder.origin();
        if (sphere.radius() - cylinder.radius()).abs() <= tolerance.linear
            && (delta - axis * delta.dot(axis)).magnitude() <= tolerance.linear {
            let curve: Curve3 = crate::Circle3::new(sphere.center(), cylinder.x_radius(), cylinder.y_radius(), tolerance)?.into();
            let range = ParameterRange::new(-std::f64::consts::TAU, 2.0 * std::f64::consts::TAU)?;
            let mut pcurves = BTreeMap::new();
            for id in source.coedges() {
                let face = topology.loop_(topology.coedge(*id)?.loop_id())?.face();
                let surface = &supports[&face];
                let mut points = (0..=96).map(|index| {
                    let point = curve.evaluate(range.start + range.span() * index as f64 / 96.0)?.point;
                    let uv = surface.project(point, tolerance)?.parameters;
                    Ok(Point2::new(uv.0, uv.1))
                }).collect::<KernelResult<Vec<_>>>()?;
                crate::intersection::unwrap_periodic_points(&mut points, surface.domains());
                let guide = crate::intersection::smooth_nurbs_guide(&points, range)?.into();
                pcurves.insert(*id, SurfaceProjectionPcurve2::new(curve.clone(), surface.clone(), guide, range, tolerance)?.into());
            }
            return Ok(OffsetEdge { curve, pcurves, range: source_range });
        }
    }
    // Equal normal offsets of tangent faces retain their source contact curve.
    // This curve is a singular surface/surface intersection, so routing it
    // through the regular intersection tracer would either reject it or stop
    // just short of its tangent endpoint. The coincident parameter images are
    // an exact constructive certificate and preserve the original naming.
    if let Some(first_points) = shared_points.first() {
        let shared = shared_points.iter().skip(1).all(|points| {
            first_points.iter().zip(points).all(|(first, second)| {
                let scale = first
                    .to_vec()
                    .magnitude()
                    .max(second.to_vec().magnitude())
                    .max(1.0);
                first.distance(*second) <= tolerance.linear_at_scale(scale) * 8.0
            })
        });
        if shared {
            return Ok(OffsetEdge {
                curve: shared_curve.expect("an edge with coedges has a carried curve"),
                pcurves: shared_pcurves,
                range: source_range,
            });
        }
    }
    // Tangent contacts may move along an invariant neighbor, changing its UV
    // coordinates while retaining an exact rail on the other support. Such a
    // rail cannot be recovered by a transverse surface-intersection solver.
    if same_charts && !tangent_contact(topology, edge, first, second, tolerance)?.is_empty() {
        for coedge_id in source.coedges() {
            let coedge = topology.coedge(*coedge_id)?;
            let face = topology.loop_(coedge.loop_id())?.face();
            let other = if face == first { second } else { first };
            let curve = carried_curve(face, coedge.pcurve())?;
            let mut points = Vec::new();
            for index in 0..=32 {
                let parameter = source_range.start + source_range.span() * index as f64 / 32.0;
                let point = curve.evaluate(parameter)?.point;
                let projection = supports[&other].project(point, tolerance)?;
                if projection.distance > tolerance.linear_at_scale(point.to_vec().magnitude()) * 8.0 {
                    points.clear();
                    break;
                }
                points.push(Point2::new(projection.parameters.0, projection.parameters.1));
            }
            if points.is_empty() { continue; }
            crate::intersection::unwrap_periodic_points(&mut points, supports[&other].domains());
            let guide = crate::intersection::smooth_nurbs_guide(&points, source_range)?.into();
            let projected: Curve2 = SurfaceProjectionPcurve2::new(
                curve.clone(), supports[&other].clone(), guide, source_range, tolerance,
            )?.into();
            let pcurves = source.coedges().iter().map(|id| {
                (*id, if id == coedge_id { coedge.pcurve().clone() } else { projected.clone() })
            }).collect();
            return Ok(OffsetEdge { curve, pcurves, range: source_range });
        }
    }
    let intersections = supports[&first]
        .intersect_support(&supports[&second], tolerance)
        .or_else(|error| {
            if !error
                .diagnostics()
                .iter()
                .all(|d| d.kind == DiagnosticKind::UnsupportedGeometry)
            {
                return Err(error);
            }
            // The existing coedges identify the desired local branch, including
            // on unbounded blend supports. Retain exact surface intersection
            // evaluation; these samples only provide its parameter guides.
            let mut points = [Vec::new(), Vec::new()];
            let uses = [first, second].map(|face| {
                *source
                    .coedges()
                    .iter()
                    .find(|id| {
                        topology
                            .coedge(**id)
                            .and_then(|c| topology.loop_(c.loop_id()))
                            .is_ok_and(|l| l.face() == face)
                    })
                    .unwrap()
            });
            for index in 0..=32 {
                let parameter = source_range.start + source_range.span() * index as f64 / 32.0;
                let seed = |side: usize, face: FaceId| -> KernelResult<Point2> {
                    if reparameterized.contains(&face) {
                        let point = source.curve().evaluate(parameter)?.point;
                        let uv = supports[&face].project(point, tolerance)?.parameters;
                        Ok(Point2::new(uv.0, uv.1))
                    } else {
                        Ok(topology.coedge(uses[side])?.pcurve().evaluate(parameter)?.point)
                    }
                };
                let first_uv = seed(0, first)?;
                let second_uv = seed(1, second)?;
                let corrected = if supports[&first]
                    .evaluate(first_uv.x, first_uv.y)?
                    .point
                    .distance(supports[&second].evaluate(second_uv.x, second_uv.y)?.point)
                    <= tolerance.linear
                {
                    [first_uv.x, first_uv.y, second_uv.x, second_uv.y]
                } else {
                    crate::intersection::correct_surface_pair_near(
                        &supports[&first],
                        &supports[&second],
                        [first_uv.x, first_uv.y, second_uv.x, second_uv.y],
                        tolerance,
                    )?
                };
                points[0].push(Point2::new(corrected[0], corrected[1]));
                points[1].push(Point2::new(corrected[2], corrected[3]));
            }
            for (side, face) in [first, second].into_iter().enumerate() {
                crate::intersection::unwrap_periodic_points(
                    &mut points[side],
                    supports[&face].domains(),
                );
            }
            let curve = SurfaceIntersectionCurve::new(
                supports[&first].clone(),
                supports[&second].clone(),
                crate::intersection::smooth_nurbs_guide(
                    &points[0],
                    ParameterRange::new(0.0, 1.0)?,
                )?
                .into(),
                crate::intersection::smooth_nurbs_guide(
                    &points[1],
                    ParameterRange::new(0.0, 1.0)?,
                )?
                .into(),
                ParameterRange::new(0.0, 1.0)?,
                tolerance,
                tolerance.linear,
            )?;
            let pcurves = curve.exact_pcurves();
            Ok(IntersectionSet::Isolated(vec![
                crate::SurfaceSurfaceCurve {
                    curve: curve.into(),
                    first_pcurve: pcurves[0].clone(),
                    second_pcurve: pcurves[1].clone(),
                    achieved_error: tolerance.linear,
                },
            ]))
        })?;
    let intersections = match intersections {
        IntersectionSet::Isolated(intersections) => intersections,
        IntersectionSet::Coincident => {
            if !same_charts {
                return Err(not_implemented("replacement coincident with an adjacent face"));
            }
            // Boolean imprinting may retain an exact split edge between two
            // patches on the same analytical support. Their offsets remain
            // coincident, so carry that split through the shared parameters.
            let mut pcurves = BTreeMap::new();
            let mut curve = None;
            for coedge_id in source.coedges() {
                let coedge = topology.coedge(*coedge_id)?;
                let face = topology.loop_(coedge.loop_id())?.face();
                pcurves.insert(*coedge_id, coedge.pcurve().clone());
                if curve.is_none() {
                    curve = Some(carried_curve(face, coedge.pcurve())?);
                }
            }
            return Ok(OffsetEdge {
                curve: curve.ok_or_else(|| {
                    topology_error("a coincident-support split edge has no coedge")
                })?,
                pcurves,
                range: source.range(),
            });
        }
        IntersectionSet::Empty => {
            return Err(unsupported("offsets of adjacent shell faces do not intersect"));
        }
    };
    let source_edge = source;
    let source_samples = [0.0, 0.25, 0.5, 0.75, 1.0]
        .into_iter()
        .map(|fraction| {
            source_edge
                .curve()
                .evaluate(source_edge.range().start + source_edge.range().span() * fraction)
                .map(|value| value.point)
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let chosen = intersections
        .into_iter()
        .filter_map(|candidate| {
            source_samples
                .iter()
                .map(|point| candidate.curve.project(*point, tolerance))
                .collect::<KernelResult<Vec<_>>>()
                .ok()
                .map(|projections| {
                    let worst = projections
                        .iter()
                        .map(|projection| projection.distance)
                        .fold(0.0_f64, f64::max);
                    let total = projections
                        .iter()
                        .map(|projection| projection.distance)
                        .sum::<f64>();
                    ((worst, total), candidate)
                })
        })
        .min_by(|first, second| {
            let ((first_worst, first_total), _) = first;
            let ((second_worst, second_total), _) = second;
            first_worst
                .total_cmp(second_worst)
                .then_with(|| first_total.total_cmp(second_total))
        })
        .map(|(_, candidate)| candidate)
        .ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::NumericalFailure,
                "could not identify the offset intersection belonging to a source edge",
            )
        })?;
    let mut pcurves = BTreeMap::new();
    for coedge_id in topology.edge(edge)?.coedges() {
        let face = topology
            .loop_(topology.coedge(*coedge_id)?.loop_id())?
            .face();
        let fallback = if face == first {
            &chosen.first_pcurve
        } else {
            &chosen.second_pcurve
        };
        pcurves.insert(
            *coedge_id,
            exact_pcurve(&chosen.curve, &supports[&face], fallback, tolerance)?,
        );
    }
    Ok(OffsetEdge {
        curve: chosen.curve,
        pcurves,
        range: source_edge.range(),
    })
}

pub(super) fn offset_vertex(
    topology: &Topology,
    vertex: VertexId,
    incident: &[EdgeId],
    edges: &BTreeMap<EdgeId, OffsetEdge>,
    supports: &BTreeMap<FaceId, Surface3>,
    reparameterized: &BTreeSet<FaceId>,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Point3> {
    let source_point = topology.vertex(vertex)?.point();
    let mut incident = incident
        .iter()
        .copied()
        .filter(|edge| edges.contains_key(edge))
        .collect::<Vec<_>>();
    let faces = incident
        .iter()
        .map(|edge| topology.edge_faces(*edge))
        .collect::<KernelResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    // Parameter seams do not constrain the shape. When seams from two
    // supports meet, retain one chart anchor and let the other seam move.
    let seams = incident.iter().copied().filter(|edge| {
        topology.edge_faces(*edge).is_ok_and(|faces| faces.len() == 1)
    }).collect::<Vec<_>>();
    if faces.len() > 1 && seams.len() > 1 {
        let anchor = seams.iter().copied().find(|edge| {
            let face = topology.edge_faces(*edge).unwrap()[0];
            topology.face(face).unwrap().surface() == &supports[&face]
        }).unwrap_or(seams[0]);
        incident.retain(|edge| !seams.contains(edge) || *edge == anchor);
    }
    // Equal offsets of tangent faces carry their shared corner to the same
    // point in every retained chart. Seed there, rather than projecting the
    // old corner onto a distant branch of an extended blend.
    if faces.iter().all(|face| !reparameterized.contains(face)) {
        let mut points = Vec::new();
        for edge in &incident {
            for id in topology.edge(*edge)?.coedges() {
                let coedge = topology.coedge(*id)?;
                let face = topology.loop_(coedge.loop_id())?.face();
                // An opening stays on its original support, but its corner
                // slides along that support as the neighboring skin offsets.
                if topology.face(face)?.surface() == &supports[&face] { continue; }
                let vertices = topology.coedge_vertices(*id)?;
                let parameter = if vertices[0] == vertex { coedge.range().start } else { coedge.range().end };
                let uv = coedge.pcurve().evaluate(parameter)?.point;
                points.push(supports[&face].first_order(uv.x, uv.y)?.0);
            }
        }
        if let Some(point) = points.first().copied() {
            if points.iter().all(|other| point.distance(*other)
                <= tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0)) * 8.0) {
                // Carried rails preserve their endpoint parameters. If every
                // rebuilt edge also passes through this point, the corner is
                // already certified; a global curve projection can otherwise
                // choose another minimum on the extended blend.
                let on_edges = incident.iter().all(|id| {
                    let source = topology.edge(*id).unwrap();
                    let parameter = if source.vertices()[0] == vertex { source.range().start } else { source.range().end };
                    edges[id].curve.first_order(parameter).is_ok_and(|value| value.0.distance(point)
                        <= tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0)) * 8.0)
                });
                if on_edges { return Ok(point); }
                if let Ok(point) = refine_offset_corner(topology, vertex, point, &faces, &incident, edges, supports, tolerance) {
                    return Ok(point);
                }
            }
        }
    }
    // Analytic rails give an exact seed even when the old vertex is an
    // antipode of a moved sphere. Refine the same support system from it.
    for (index, first) in incident.iter().enumerate() {
        for second in &incident[index + 1..] {
            let (a, b) = (&edges[first].curve, &edges[second].curve);
            if !matches!(a, Curve3::Line(_) | Curve3::Circle(_))
                || !matches!(b, Curve3::Line(_) | Curve3::Circle(_)) {
                continue;
            }
            let pa = a.project(source_point, tolerance)?.parameter;
            let pb = b.project(source_point, tolerance)?.parameter;
            if let Ok(hit) = a.intersect_curve_near(b, pa, pb, tolerance) {
                if let Ok(point) = refine_offset_corner(
                    topology, vertex, hit.point, &faces, &incident, edges, supports, tolerance,
                ) {
                    return Ok(point);
                }
            }
        }
    }
    refine_offset_corner(topology, vertex, source_point, &faces, &incident, edges, supports, tolerance)
}

#[derive(Clone, Copy)]
pub(super) enum CornerConstraint<'a> {
    Surface(&'a Surface3, Option<Point2>),
    Curve(&'a Curve3),
    Point(Point3),
}

impl CornerConstraint<'_> {
    fn project(
        self,
        point: Point3,
        tolerance: crate::ModelTolerance,
    ) -> KernelResult<(Point3, crate::Matrix3)> {
        Ok(match self {
            Self::Surface(surface, seed) => {
                let projected = match seed {
                    Some(seed) => surface.project_near(point, seed, tolerance)?,
                    None => surface.project(point, tolerance)?,
                };
                let normal = surface.normal(projected.parameters.0, projected.parameters.1)?;
                (
                    projected.point,
                    crate::Matrix3::from_cols(
                        normal * normal.x,
                        normal * normal.y,
                        normal * normal.z,
                    ),
                )
            }
            Self::Curve(curve) => {
                let projected = curve.project(point, tolerance)?;
                let tangent = curve
                    .evaluate(projected.parameter)?
                    .first_derivative
                    .normalize();
                (
                    projected.point,
                    crate::Matrix3::identity()
                        - crate::Matrix3::from_cols(
                            tangent * tangent.x,
                            tangent * tangent.y,
                            tangent * tangent.z,
                        ),
                )
            }
            Self::Point(center) => (center, crate::Matrix3::identity()),
        })
    }
}

// Solve supports of every dimension with one local projection corrector.
// Tangent rails supply constraints that coincident surface normals cannot.
pub(super) fn refine_offset_corner(
    topology: &Topology,
    vertex: VertexId,
    point: Point3,
    faces: &BTreeSet<FaceId>,
    incident: &[EdgeId],
    edges: &BTreeMap<EdgeId, OffsetEdge>,
    supports: &BTreeMap<FaceId, Surface3>,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Point3> {
    let mut constraints = faces
        .iter()
        .map(|face| {
            let surface = &supports[face];
            if !matches!(surface, Surface3::Offset(_) | Surface3::RollingBall(_) | Surface3::Blend(_) | Surface3::Ruled(_)) {
                return Ok(CornerConstraint::Surface(surface, None));
            }
            // Extended guides can reach another projection minimum. Retain
            // the corner's original face chart when refining its offset.
            for loop_id in topology.face(*face)?.loops() {
                for id in topology.loop_coedges(*loop_id)? {
                    let vertices = topology.coedge_vertices(id)?;
                    if vertices.contains(&vertex) {
                        let coedge = topology.coedge(id)?;
                        let parameter = if vertices[0] == vertex { coedge.range().start } else { coedge.range().end };
                        let seed = coedge.pcurve().evaluate(parameter)?.point;
                        return Ok(CornerConstraint::Surface(surface, Some(seed)));
                    }
                }
            }
            Err(topology_error("offset corner has no incident coedge on its support face"))
        })
        .collect::<KernelResult<Vec<_>>>()?;
    for edge in incident {
        if matches!(edges[edge].curve, Curve3::SurfaceIntersection(_)) {
            continue;
        }
        let adjacent = topology.edge_faces(*edge)?;
        if adjacent.len() == 1
            || !tangent_contact(topology, *edge, adjacent[0], adjacent[1], tolerance)?.is_empty()
        {
            constraints.push(CornerConstraint::Curve(&edges[edge].curve));
        }
    }
    refine_support_corner(point, &constraints, tolerance)
}

pub(super) fn refine_support_corner(
    mut point: Point3,
    constraints: &[CornerConstraint<'_>],
    tolerance: crate::ModelTolerance,
) -> KernelResult<Point3> {
    for _ in 0..96 {
        let mut matrix = [[0.0; 3]; 3];
        let mut rhs = [0.0; 3];
        let mut worst = 0.0_f64;
        let mut cost = 0.0;
        for constraint in constraints {
            let (projected, jacobian) = constraint.project(point, tolerance)?;
            let residual = point - projected;
            worst = worst.max(residual.magnitude());
            cost += residual.magnitude2();
            let correction = jacobian * residual;
            for row in 0..3 {
                rhs[row] -= correction[row];
                for col in 0..3 {
                    matrix[row][col] += jacobian[col][row];
                }
            }
        }
        for axis in 0..3 {
            matrix[axis][axis] += 1e-12;
        }
        let Some(step) = crate::linalg::solve_linear(matrix, rhs) else {
            break;
        };
        let step = crate::Vec3::new(step[0], step[1], step[2]);
        // Near tangency a small normal residual can hide a large movement.
        // Require positional convergence as well, within evaluation roundoff.
        let roundoff = f64::EPSILON * point.to_vec().magnitude().max(1.0) * 64.0;
        if worst <= (tolerance.linear * 0.1).max(roundoff)
            && step.magnitude() <= (tolerance.linear * 0.01).max(roundoff)
        {
            return Ok(point);
        }
        if step.magnitude() <= tolerance.linear * 0.001 {
            break;
        }
        let mut damping = 1.0;
        let mut accepted = false;
        for _ in 0..12 {
            let candidate = point + step * damping;
            let candidate_cost = constraints.iter().try_fold(0.0, |cost, constraint| {
                constraint
                    .project(candidate, tolerance)
                    .map(|(projected, _)| cost + candidate.distance2(projected))
            });
            if candidate_cost.is_ok_and(|candidate| candidate < cost) {
                point = candidate;
                accepted = true;
                break;
            }
            damping *= 0.5;
        }
        if !accepted {
            break;
        }
    }
    Err(KernelError::new(
        DiagnosticKind::NumericalFailure,
        "offset corner did not converge",
    ))
}

/// The parameter of a closed, nonperiodic `curve` at which to restart it when
/// the open source trim between its new `vertices` would cross the curve's
/// chart seam: the middle of the complementary arc.
pub(super) fn crossed_seam(
    source: &crate::Edge,
    curve: &Curve3,
    range: ParameterRange,
    vertices: &BTreeMap<VertexId, Point3>,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Option<f64>> {
    let start = curve.evaluate(range.start)?.point;
    let linear = tolerance.linear_at_scale(start.to_vec().magnitude());
    // A trim ending at the seam itself needs no restart; trimming chooses the
    // matching end of the chart there.
    if curve == source.curve()
        || start.distance(curve.evaluate(range.end)?.point) > linear
        || source.vertices().iter().any(|vertex| vertices[vertex].distance(start) <= linear * 8.0)
    {
        return Ok(None);
    }
    let [first, second] = source.vertices().map(|vertex| curve.project(vertices[&vertex], tolerance));
    let (first, second) = (first?.parameter, second?.parameter);
    let (low, high) = (first.min(second), first.max(second));
    if high - low <= tolerance.parameter {
        return Ok(None);
    }
    // The source edge's middle identifies which of the two arcs it became:
    // the one whose own middle lies nearer around the closed curve. Moving
    // supports can carry it just outside a short arc, and tangents are
    // unreliable where a trim ends at a singular crossing.
    let middle = source.curve().evaluate((source.range().start + source.range().end) * 0.5)?.point;
    let middle = curve.project(middle, tolerance)?.parameter;
    let period = range.span().abs();
    let distance = |parameter: f64| {
        let offset = (middle - parameter).rem_euclid(period);
        offset.min(period - offset)
    };
    let inner = (low + high) * 0.5;
    Ok((distance(inner + period * 0.5) < distance(inner)).then_some(inner))
}

pub(super) fn retarget_offset_intersection_endpoints(
    topology: &Topology,
    edge_id: EdgeId,
    edge: &mut OffsetEdge,
    vertices: &BTreeMap<VertexId, Point3>,
    supports: &BTreeMap<FaceId, Surface3>,
    tolerance: crate::ModelTolerance,
) -> KernelResult<()> {
    // Collapsed blend rails share the same exact center curve. Retargeting each
    // rail independently would give the two surviving faces different seams.
    // Their p-curves are constructed on the surviving supports after trimming.
    if edge.pcurves.is_empty() {
        return Ok(());
    }
    let Curve3::SurfaceIntersection(intersection) = &edge.curve else {
        return Ok(());
    };
    let source = topology.edge(edge_id)?;
    let closed = source.vertices()[0] == source.vertices()[1];
    let curve_range = intersection.range();
    let exact = intersection.exact_pcurves();
    let seam = if closed {
        Some(edge.curve.project(vertices[&source.vertices()[0]], tolerance)?.parameter)
    } else {
        crossed_seam(source, &edge.curve, curve_range, vertices, tolerance)?
    };
    // An open trim of a closed section (for example a tilted plane through a
    // cylinder) may run across the curve's bounded chart seam. Its guide then
    // restarts in the middle of the complementary arc.
    let rotated = !closed && seam.is_some();
    let mut first_points = Vec::new();
    let mut second_points = Vec::new();
    for index in 0..=32 {
        let fraction = index as f64 / 32.0;
        // A closed intersection may start at a different seam from the source
        // edge. Rotate its entire guide to that seam; moving only the two end
        // samples folds the guide back over the rest of the intersection.
        let fraction = seam.map_or(fraction, |seam| {
            ((seam - curve_range.start) / curve_range.span() + fraction).rem_euclid(1.0)
        });
        let parameter = curve_range.start + curve_range.span() * fraction;
        first_points.push(exact[0].evaluate(parameter)?.point);
        second_points.push(exact[1].evaluate(parameter)?.point);
    }
    if closed || rotated {
        crate::intersection::unwrap_periodic_points(
            &mut first_points,
            intersection.first_surface().domains(),
        );
        crate::intersection::unwrap_periodic_points(
            &mut second_points,
            intersection.second_surface().domains(),
        );
    }
    let source_tangent = source
        .curve()
        .evaluate((source.range().start + source.range().end) * 0.5)?
        .first_derivative
        * source.range().span();
    let guide_tangent = edge
        .curve
        .evaluate((curve_range.start + curve_range.end) * 0.5)?
        .first_derivative
        * curve_range.span();
    let forward = source_tangent.dot(guide_tangent) >= 0.0;
    let [first, second] = source
        .vertices()
        .map(|vertex| edge.curve.project(vertices[&vertex], tolerance));
    let projections = [first?, second?];
    let collapsed =
        (projections[0].parameter - projections[1].parameter).abs() <= tolerance.parameter;
    let mut changed = rotated;
    for (side, vertex) in source.vertices().into_iter().enumerate() {
        let point = vertices[&vertex];
        let projection = projections[side];
        // A rotated guide has no samples at the trim ends; the trim projects
        // its vertices onto the exact section instead.
        if rotated || !closed && projection.distance <= tolerance.linear {
            continue;
        }
        // Projection can send both new vertices to the same old endpoint.
        // The source traversal establishes their distinct correspondence.
        let endpoint = if closed || forward {
            side * 32
        } else {
            (1 - side) * 32
        };
        changed = true;
        let first_projection = intersection.first_surface().project(point, tolerance)?;
        let second_projection = intersection.second_surface().project(point, tolerance)?;
        first_points[endpoint] =
            Point2::new(first_projection.parameters.0, first_projection.parameters.1);
        second_points[endpoint] = Point2::new(
            second_projection.parameters.0,
            second_projection.parameters.1,
        );
        let neighbor = if endpoint == 0 { 1 } else { 31 };
        for (points, surface) in [
            (&mut first_points, intersection.first_surface()),
            (&mut second_points, intersection.second_surface()),
        ] {
            points[endpoint] = nearest_uv(surface.domains(), points[endpoint], points[neighbor]);
        }
    }
    if !changed {
        return Ok(());
    }
    let folded = [&first_points, &second_points].into_iter().any(|points| {
        (points[1] - points[0]).dot(points[2] - points[1]) < 0.0
            || (points[32] - points[31]).dot(points[31] - points[30]) < 0.0
    });
    if !closed && !rotated && (collapsed || folded) {
        // Moving just an endpoint can fold the old guide back along the same
        // intersection. Continue the entire retained branch between its new,
        // topologically ordered vertices.
        for index in 1..32 {
            let fraction = index as f64 / 32.0;
            let a = first_points[0] + (first_points[32] - first_points[0]) * fraction;
            let b = second_points[0] + (second_points[32] - second_points[0]) * fraction;
            let uv = crate::intersection::correct_surface_pair_near(
                intersection.first_surface(),
                intersection.second_surface(),
                [a.x, a.y, b.x, b.y],
                tolerance,
            )?;
            first_points[index] = Point2::new(uv[0], uv[1]);
            second_points[index] = Point2::new(uv[2], uv[3]);
        }
        crate::intersection::unwrap_periodic_points(
            &mut first_points,
            intersection.first_surface().domains(),
        );
        crate::intersection::unwrap_periodic_points(
            &mut second_points,
            intersection.second_surface().domains(),
        );
    }
    let rebuilt = SurfaceIntersectionCurve::new(
        intersection.first_surface().clone(),
        intersection.second_surface().clone(),
        crate::intersection::smooth_nurbs_guide(&first_points, curve_range)?.into(),
        crate::intersection::smooth_nurbs_guide(&second_points, curve_range)?.into(),
        curve_range,
        tolerance,
        intersection.achieved_error(),
    )?;
    edge.curve = rebuilt.clone().into();
    let exact = rebuilt.exact_pcurves();
    for coedge_id in source.coedges() {
        let coedge = topology.coedge(*coedge_id)?;
        let face = topology.loop_(coedge.loop_id())?.face();
        edge.pcurves.insert(
            *coedge_id,
            if rebuilt.first_surface() == &supports[&face] {
                exact[0].clone()
            } else {
                exact[1].clone()
            },
        );
    }
    Ok(())
}

pub(super) fn trim_offset_edge(
    topology: &Topology,
    edge_id: EdgeId,
    edge: &OffsetEdge,
    vertices: &BTreeMap<VertexId, Point3>,
    tolerance: crate::ModelTolerance,
) -> KernelResult<ParameterRange> {
    let source = topology.edge(edge_id)?;
    let [first_vertex, second_vertex] = source.vertices();
    if first_vertex == second_vertex {
        let domain = edge.curve.domain();
        let crate::DomainBounds::Bounded(bounds) = domain.bounds else {
            return Err(unsupported("a closed offset edge needs a bounded analytical parameterization"));
        };
        let reference = vertices
            .get(&first_vertex)
            .copied()
            .unwrap_or(source.curve().evaluate(source.range().start)?.point);
        let source_tangent = source
            .curve()
            .evaluate(source.range().start)?
            .first_derivative
            * source.range().span().signum();
        let projection = edge.curve.project(reference, tolerance)?;
        let candidate_tangent = edge.curve.evaluate(projection.parameter)?.first_derivative;
        let direction = if source_tangent.dot(candidate_tangent) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        if edge.pcurves.is_empty() && domain.period.is_none() {
            // The blend center support is extended beyond its rails. A closed
            // rail still traverses only its source turn, starting at its seam.
            let projection = edge.curve.project_on_trim(reference, source.range(), tolerance)?;
            let period = source.range().span().abs();
            let start = projection.parameter
                + ((bounds.middle() - projection.parameter) / period).round() * period;
            return ParameterRange::new(
                start,
                start + direction * period,
            );
        }
        return if let Some(period) = domain.period {
            // Projection identifies a point modulo a full turn. Keep the
            // source chart's turn so its p-curves remain continuous at the
            // seam; an equivalent 3D range can cross their UV branch cut.
            let start = domain.nearest(projection.parameter, source.range().start);
            ParameterRange::new(
                start,
                start + direction * period,
            )
        // Bounded surface curves can carry a descending domain. Match the
        // traversal direction, rather than assuming every domain increases.
        } else if bounds.span().signum() == direction {
            Ok(bounds)
        } else {
            Ok(bounds.reversed())
        };
    }
    let project_endpoint = |point: Point3, source_parameter: f64| -> KernelResult<crate::Projection1> {
        // Tangent offsets carry their rails in the original parameter chart.
        // Keep an endpoint that still lies exactly on that curve; searching its
        // extended domain can select a different local projection minimum.
        if let Ok((candidate, _)) = edge.curve.first_order(source_parameter) {
            let distance = candidate.distance(point);
            if distance <= tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0)) {
                return Ok(crate::Projection1 { parameter: source_parameter, point: candidate, distance });
            }
        }
        let mut projection = edge.curve.project(point, tolerance)?;
        if let crate::DomainBounds::Bounded(range) = edge.curve.domain().bounds {
            for parameter in [range.start, range.end] {
                let candidate = edge.curve.evaluate(parameter)?.point;
                let distance = candidate.distance(point);
                if distance < projection.distance {
                    projection = crate::Projection1 {
                        parameter,
                        point: candidate,
                        distance,
                    };
                }
            }
        }
        Ok(projection)
    };
    let mut first = project_endpoint(vertices[&first_vertex], source.range().start)?;
    let mut second = project_endpoint(vertices[&second_vertex], source.range().end)?;
    // A bounded intersection curve can close at its chart seam without
    // declaring a periodic domain. Projection may choose either coincident
    // endpoint due to roundoff. Choose the endpoint that lets the trim follow
    // the source edge, instead of traversing the complementary arc.
    let domain = edge.curve.domain();
    if domain.period.is_none() {
        if let crate::DomainBounds::Bounded(bounds) = domain.bounds {
            let start = edge.curve.evaluate(bounds.start)?;
            let end = edge.curve.evaluate(bounds.end)?;
            if start.point.distance(end.point) <= tolerance.linear {
                for (projection, source_parameter, is_first) in [
                    (&mut first, source.range().start, true),
                    (&mut second, source.range().end, false),
                ] {
                    if projection.point.distance(start.point) > tolerance.linear {
                        continue;
                    }
                    let tangent = source.curve().evaluate(source_parameter)?.first_derivative
                        * source.range().span().signum();
                    let forward =
                        tangent.dot(start.first_derivative) * bounds.span().signum() >= 0.0;
                    projection.parameter = if forward == is_first {
                        bounds.start
                    } else {
                        bounds.end
                    };
                }
            }
        }
    }
    let mut range = ParameterRange::new(first.parameter, second.parameter)?;
    if let Some(period) = edge.curve.domain().period {
        let source_midpoint = source
            .curve()
            .evaluate(source.range().start + source.range().span() * 0.5)?
            .point;
        let source_tangent = source
            .curve()
            .evaluate(source.range().start)?
            .first_derivative
            * source.range().span().signum();
        let candidate_tangent = edge.curve.evaluate(first.parameter)?.first_derivative;
        let desired_sign = if source_tangent.dot(candidate_tangent) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let score = |candidate: ParameterRange| {
            let wrong_direction = candidate.span().signum() != desired_sign;
            // Normally offsetting a face preserves its UV chart. Prefer the
            // periodic branch whose coedge endpoints stay in that same chart;
            // spatial midpoint tests alone cannot distinguish, for example,
            // `pi..2*pi` from the multi-turn alias `pi..-2*pi`.
            let chart_error = source
                .coedges()
                .iter()
                .map(|coedge_id| -> KernelResult<f64> {
                    // A collapsed surface contributes a spatial curve; its
                    // new UV charts are constructed after choosing the trim.
                    let Some(candidate_pcurve) = edge.pcurves.get(coedge_id) else {
                        return Ok(0.0);
                    };
                    let source_coedge = topology.coedge(*coedge_id)?;
                    let source_range = source_coedge.range();
                    let candidate_range = if source_coedge.is_forward() {
                        candidate
                    } else {
                        candidate.reversed()
                    };
                    let source_start = source_coedge.pcurve().evaluate(source_range.start)?.point;
                    let source_end = source_coedge.pcurve().evaluate(source_range.end)?.point;
                    let candidate_start = candidate_pcurve.evaluate(candidate_range.start)?.point;
                    let candidate_end = candidate_pcurve.evaluate(candidate_range.end)?.point;
                    Ok(source_start.distance2(candidate_start)
                        + source_end.distance2(candidate_end))
                })
                .collect::<KernelResult<Vec<_>>>()
                .map(|errors| errors.into_iter().sum::<f64>())
                .unwrap_or(f64::INFINITY);
            let midpoint_distance = edge
                .curve
                .evaluate((candidate.start + candidate.end) * 0.5)
                .map(|value| value.point.distance(source_midpoint))
                .unwrap_or(f64::INFINITY);
            (
                wrong_direction,
                chart_error,
                midpoint_distance,
                candidate.span().abs(),
            )
        };
        range = (-2..=2)
            .flat_map(|start_turn| {
                (-2..=2).filter_map(move |end_turn| {
                    let candidate = ParameterRange::new(
                        first.parameter + period * start_turn as f64,
                        second.parameter + period * end_turn as f64,
                    )
                    .ok()?;
                    (candidate.span().abs() > tolerance.parameter
                        && candidate.span().abs() <= period + tolerance.parameter)
                        .then_some(candidate)
                })
            })
            .min_by(|first_range, second_range| {
                let first_score = score(*first_range);
                let second_score = score(*second_range);
                first_score
                    .0
                    .cmp(&second_score.0)
                    .then_with(|| first_score.1.total_cmp(&second_score.1))
                    .then_with(|| first_score.2.total_cmp(&second_score.2))
                    .then_with(|| first_score.3.total_cmp(&second_score.3))
            })
            .unwrap_or(range);
    }
    Ok(range)
}

pub(super) fn projected_pcurve(
    curve: &Curve3,
    surface: &Surface3,
    guide: &Curve2,
    range: ParameterRange,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Curve2> {
    if guide.domain().contains_range(range, f64::EPSILON * 64.0) {
        return Ok(guide.clone());
    }
    let mut points = Vec::new();
    for index in 0..=32 {
        let parameter = range.start + range.span() * index as f64 / 32.0;
        let point = curve.evaluate(parameter)?.point;
        let projection = surface.project(point, tolerance)?;
        points.push(Point2::new(
            projection.parameters.0,
            projection.parameters.1,
        ));
    }
    crate::intersection::unwrap_periodic_points(&mut points, surface.domains());
    let guide: Curve2 = crate::intersection::smooth_nurbs_guide(&points, range)?.into();
    Ok(
        SurfaceProjectionPcurve2::new(curve.clone(), surface.clone(), guide, range, tolerance)?
            .into(),
    )
}

pub(super) fn offset_parameter_padding(
    surface: &Surface3,
    bounds: [ParameterRange; 2],
    distance: f64,
    tolerance: crate::ModelTolerance,
) -> KernelResult<[f64; 2]> {
    let mut minimum_rates = [f64::INFINITY; 2];
    for first_index in 0..=2 {
        for second_index in 0..=2 {
            let u = bounds[0].start + bounds[0].span() * first_index as f64 / 2.0;
            let v = bounds[1].start + bounds[1].span() * second_index as f64 / 2.0;
            let value = surface.evaluate(u, v)?;
            for (axis, rate) in [value.du.magnitude(), value.dv.magnitude()]
                .into_iter()
                .enumerate()
            {
                if rate.is_finite() && rate > tolerance.linear {
                    minimum_rates[axis] = minimum_rates[axis].min(rate);
                }
            }
        }
    }
    Ok(minimum_rates.map(|rate| {
        if rate.is_finite() {
            distance.abs() * 2.0 / rate + tolerance.parameter * 64.0
        } else {
            0.0
        }
    }))
}

pub(crate) fn extended_support(
    surface: &Surface3,
    bounds: [ParameterRange; 2],
    distance: f64,
    tolerance: crate::ModelTolerance,
) -> KernelResult<Surface3> {
    match surface {
        Surface3::Swept(sweep) => {
            let padding = offset_parameter_padding(surface, bounds, distance, tolerance)?;
            sweep.extended(padding).map(Surface3::Swept)
        }
        Surface3::Nurbs(spline) => {
            let padding = offset_parameter_padding(surface, bounds, distance, tolerance)?;
            spline.extended_bezier(padding).map(Into::into)
        }
        Surface3::Revolved(revolved) if matches!(revolved.profile(), Curve2::Nurbs(_)) => {
            let padding = offset_parameter_padding(surface, bounds, distance, tolerance)?;
            let range = revolved.profile_range();
            let range = ParameterRange::new(range.start - padding[1], range.end + padding[1])?;
            crate::RevolvedSurface3::new(
                revolved.frame(), revolved.profile().extended(range)?, range, tolerance,
            ).map(Into::into)
        }
        // An offset shares its base's chart, so continue that base instead.
        Surface3::Offset(offset) if offset.v_slope() == 0.0 => {
            let base = extended_support(offset.base(), bounds, distance, tolerance)?;
            if &base == offset.base() {
                return Ok(surface.clone());
            }
            crate::OffsetSurface3::new(base, offset.distance(), tolerance).map(Surface3::Offset)
        }
        _ => Ok(surface.clone()),
    }
}

// Contact rails delimit the material sector of a blend. Miter end curves can
// continue beyond those rails and must not enlarge its half-space constraint.
pub(super) fn contact_sector(
    topology: &Topology,
    face: FaceId,
    tolerance: crate::ModelTolerance,
) -> KernelResult<[ParameterRange; 2]> {
    let boundary = crate::trim::face_boundary_curves(topology, face, tolerance)?;
    let mut bounds = crate::trim::face_parameter_bounds(&boundary, tolerance)?;
    let mut rails = [Vec::new(), Vec::new()];
    for curve in boundary {
        let edge = topology.coedge(curve.provenance)?.edge();
        let faces = topology.edge_faces(edge)?;
        if faces.len() != 2 || tangent_contact(topology, edge, faces[0], faces[1], tolerance)?.is_empty() {
            continue;
        }
        let points = [curve.range.start, (curve.range.start + curve.range.end) * 0.5, curve.range.end]
            .map(|parameter| curve.pcurve.evaluate(parameter).map(|v| v.point));
        let [a, b, c] = points;
        let (a, b, c) = (a?, b?, c?);
        for axis in 0..2 {
            if (a[axis] - b[axis]).abs().max((a[axis] - c[axis]).abs()) <= tolerance.parameter {
                rails[axis].push(a[axis]);
            }
        }
    }
    for axis in 0..2 {
        let low = rails[axis].iter().copied().fold(f64::INFINITY, f64::min);
        let high = rails[axis].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if high - low > tolerance.parameter {
            bounds[axis] = ParameterRange::new(low, high)?;
        }
    }
    Ok(bounds)
}

pub(crate) fn expanded_offset_bounds(
    surface: &Surface3,
    bounds: [ParameterRange; 2],
    distance: f64,
    tolerance: crate::ModelTolerance,
) -> KernelResult<[ParameterRange; 2]> {
    let padding = offset_parameter_padding(surface, bounds, distance, tolerance)?;
    let domains = surface.domains();
    let mut expanded = bounds;
    for axis in 0..2 {
        if padding[axis] == 0.0 {
            continue;
        }
        let minimum = bounds[axis].start.min(bounds[axis].end) - padding[axis];
        let maximum = bounds[axis].start.max(bounds[axis].end) + padding[axis];
        expanded[axis] = if let Some(period) = domains[axis].period {
            if maximum - minimum >= period {
                let center = (bounds[axis].start + bounds[axis].end) * 0.5;
                ParameterRange::new(center - period * 0.5, center + period * 0.5)?
            } else {
                ParameterRange::new(minimum, maximum)?
            }
        } else if let crate::DomainBounds::Bounded(domain) = domains[axis].bounds {
            ParameterRange::new(
                minimum.max(domain.start.min(domain.end)),
                maximum.min(domain.start.max(domain.end)),
            )?
        } else {
            ParameterRange::new(minimum, maximum)?
        };
    }
    Ok(expanded)
}
