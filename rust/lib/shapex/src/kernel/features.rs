use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3};

use super::sew::{BrepPatch, BrepPatchLoop, BrepPatchSegment, SegmentProvenance};
use super::{
    Axis3, CoedgeId, Compound, Curve2, Curve3, DiagnosticKind, EdgeId, FaceHandle, FaceId,
    FaceOrientation, IntersectionSet, Kernel, KernelError, KernelResult, Line3, ParameterRange,
    PersistentId, Plane3, Profile, RigidTransform3, Surface3,
    SurfaceIntersectionCurve, SurfaceProjectionPcurve2, Topology,
    SplitKeep, TrimmedCurve2, VertexId, Wire2, box_center, invalid, nearest_uv, unsupported,
    whole_periods,
};

fn not_implemented(operation: &str) -> KernelError {
    unsupported(format!(
        "exact analytical {operation} is not implemented for the supplied geometry"
    ))
}

fn publish_torus(
    center: Point3,
    x_major: crate::Vec3,
    y_major: crate::Vec3,
    minor_radius: f64,
    tolerance: super::ModelTolerance,
) -> KernelResult<Compound> {
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    transaction.torus(center, x_major, y_major, minor_radius)?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [])
}

fn distinct_values(values: impl IntoIterator<Item = f64>, tolerance: f64) -> Vec<f64> {
    let mut result = Vec::<f64>::new();
    for value in values {
        if !result
            .iter()
            .any(|candidate| (candidate - value).abs() <= tolerance)
        {
            result.push(value);
        }
    }
    result.sort_by(f64::total_cmp);
    result
}

/// Recognize full revolutions that have a closed-form analytical BREP.
fn exact_full_revolution(
    profile: &Profile,
    plane: Plane3,
    axis: Axis3,
) -> KernelResult<Option<Compound>> {
    if !profile.holes().is_empty() {
        return Ok(None);
    }
    let tolerance = profile.tolerance();
    let axis_direction = axis.direction();
    if axis_direction.dot(plane.normal()).abs() > tolerance.angular
        || plane.project(axis.origin())?.distance.abs()
            > tolerance.linear_at_scale(axis.origin().to_vec().magnitude())
    {
        return Ok(None);
    }

    if let [segment] = profile.outer().segments() {
        if let Curve2::Circle(circle) = segment.curve() {
            let section_center = plane.point_at(circle.center());
            let center = axis.origin()
                + axis_direction * (section_center - axis.origin()).dot(axis_direction);
            let x_major = section_center - center;
            if x_major.magnitude() > circle.radius() + tolerance.linear {
                let y_major = axis_direction.cross(x_major);
                return publish_torus(center, x_major, y_major, circle.radius(), tolerance)
                    .map(Some);
            }
        }
    }

    let segments = profile.outer().segments();
    if segments.len() != 4
        || segments
            .iter()
            .any(|segment| !matches!(segment.curve(), Curve2::Line(_)))
    {
        return Ok(None);
    }
    let points = segments
        .iter()
        .map(TrimmedCurve2::start)
        .collect::<KernelResult<Vec<_>>>()?
        .into_iter()
        .map(|point| plane.point_at(point))
        .collect::<Vec<_>>();
    let axial = points
        .iter()
        .map(|point| (*point - axis.origin()).dot(axis_direction))
        .collect::<Vec<_>>();
    let radial_vectors = points
        .iter()
        .zip(&axial)
        .map(|(point, axial)| *point - (axis.origin() + axis_direction * *axial))
        .collect::<Vec<_>>();
    let Some(radial_direction) = radial_vectors
        .iter()
        .find(|vector| vector.magnitude() > tolerance.linear)
        .map(|vector| vector.normalize())
    else {
        return Ok(None);
    };
    if radial_vectors.iter().any(|vector| {
        vector.cross(radial_direction).magnitude() > tolerance.linear
            || vector.dot(radial_direction) < -tolerance.linear
    }) {
        return Ok(None);
    }
    let radial = radial_vectors
        .iter()
        .map(|vector| vector.dot(radial_direction))
        .collect::<Vec<_>>();
    let axial_values = distinct_values(axial.iter().copied(), tolerance.linear);
    let radial_values = distinct_values(radial.iter().copied(), tolerance.linear);
    if axial_values.len() != 2 || radial_values.len() != 2 {
        return Ok(None);
    }
    for index in 0..points.len() {
        let next = (index + 1) % points.len();
        let changes_axial = (axial[index] - axial[next]).abs() > tolerance.linear;
        let changes_radial = (radial[index] - radial[next]).abs() > tolerance.linear;
        if changes_axial == changes_radial {
            return Ok(None);
        }
    }

    let inner_radius = radial_values[0];
    let outer_radius = radial_values[1];
    if outer_radius <= tolerance.linear || inner_radius < -tolerance.linear {
        return Ok(None);
    }
    let outer = Wire2::new(
        [TrimmedCurve2::circle(Point2::new(0.0, 0.0), outer_radius)?],
        tolerance,
    )?;
    let holes = if inner_radius > tolerance.linear {
        vec![Wire2::new(
            [TrimmedCurve2::circle(Point2::new(0.0, 0.0), inner_radius)?],
            tolerance,
        )?]
    } else {
        Vec::new()
    };
    let annulus = Profile::new(outer, holes, tolerance)?;
    let section_plane = Plane3::new(
        axis.origin() + axis_direction * axial_values[0],
        radial_direction,
        axis_direction.cross(radial_direction),
        tolerance,
    )?;
    let result = annulus.extrude(
            section_plane,
            axis_direction * (axial_values[1] - axial_values[0]),
        )?;
    let roles = result.faces().map(|face| {
        let (values, value) = match result.origin(face) {
            Some(super::Origin::Created { role: Some(super::CreationRole::ExtrusionStart) }) => (&axial, axial_values[0]),
            Some(super::Origin::Created { role: Some(super::CreationRole::ExtrusionEnd) }) => (&axial, axial_values[1]),
            Some(super::Origin::Created { role: Some(super::CreationRole::ExtrusionSide { boundary, .. }) }) =>
                (&radial, if matches!(boundary, super::ProfileBoundary::Outer) { outer_radius } else { inner_radius }),
            _ => unreachable!(),
        };
        let segment = (0..4).find(|i| (values[*i] - value).abs() <= tolerance.linear &&
            (values[(*i + 1) % 4] - value).abs() <= tolerance.linear).unwrap();
        (face.persistent_id(), super::CreationRole::ProfileSide { generators: vec![[0, 0, profile.generator(0, segment)]], chart: None })
    }).collect::<Vec<_>>();
    Ok(Some(result.with_creation_roles(roles)))
}

#[derive(Debug, Clone)]
struct OffsetEdge {
    curve: Curve3,
    pcurves: BTreeMap<CoedgeId, Curve2>,
    range: ParameterRange,
}

pub(super) fn face_parameter_bounds(
    topology: &Topology,
    face: FaceId,
    tolerance: super::ModelTolerance,
) -> KernelResult<[ParameterRange; 2]> {
    let mut bounds = super::trim::face_parameter_bounds(
        &super::trim::face_boundary_curves(topology, face, tolerance)?,
        tolerance,
    )?;
    // Trims carry an achieved tolerance. Their bounding box is intersected
    // with the support domain before it is used to evaluate that support.
    for (bound, domain) in bounds.iter_mut().zip(topology.face(face)?.surface().domains()) {
        if domain.period.is_none() {
            if let super::DomainBounds::Bounded(range) = domain.bounds {
                bound.start = bound.start.max(range.start.min(range.end));
                bound.end = bound.end.min(range.start.max(range.end));
            }
        }
    }
    Ok(bounds)
}

pub(super) fn exact_pcurve(
    curve: &Curve3,
    surface: &Surface3,
    fallback: &Curve2,
    tolerance: super::ModelTolerance,
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

fn continued_range(
    surface: &Surface3,
    pcurve: &Curve2,
    bounds: [ParameterRange; 2],
    range: ParameterRange,
    tolerance: super::ModelTolerance,
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

fn offset_edge(
    topology: &Topology,
    edge: EdgeId,
    supports: &BTreeMap<FaceId, Surface3>,
    bounds: &BTreeMap<FaceId, [ParameterRange; 2]>,
    tolerance: super::ModelTolerance,
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
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "shelling requires every physical edge to have two incident faces",
        ));
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
    let mut shared_points = Vec::<Vec<Point3>>::new();
    let same_charts = !faces.iter().any(|face| reparameterized.contains(face));
    if same_charts {
        for coedge_id in source.coedges() {
            let coedge = topology.coedge(*coedge_id)?;
            let face = topology.loop_(coedge.loop_id())?.face();
            shared_pcurves.insert(*coedge_id, coedge.pcurve().clone());
            if shared_curve.is_none() {
                shared_curve = Some(carried_curve(face, coedge.pcurve())?);
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
            let curve: Curve3 = super::Circle3::new(sphere.center(), cylinder.x_radius(), cylinder.y_radius(), tolerance)?.into();
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
                super::intersection::unwrap_periodic_points(&mut points, surface.domains());
                let guide = super::intersection::smooth_nurbs_guide(&points, range)?.into();
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
            super::intersection::unwrap_periodic_points(&mut points, supports[&other].domains());
            let guide = super::intersection::smooth_nurbs_guide(&points, source_range)?.into();
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
                    super::intersection::correct_surface_pair_near(
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
                super::intersection::unwrap_periodic_points(
                    &mut points[side],
                    supports[&face].domains(),
                );
            }
            let curve = SurfaceIntersectionCurve::new(
                supports[&first].clone(),
                supports[&second].clone(),
                super::intersection::smooth_nurbs_guide(
                    &points[0],
                    ParameterRange::new(0.0, 1.0)?,
                )?
                .into(),
                super::intersection::smooth_nurbs_guide(
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
                super::SurfaceSurfaceCurve {
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
                    KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "a coincident-support split edge has no coedge",
                    )
                })?,
                pcurves,
                range: source.range(),
            });
        }
        IntersectionSet::Empty => {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "offsets of adjacent shell faces do not intersect",
            ));
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

fn offset_vertex(
    topology: &Topology,
    vertex: VertexId,
    edges: &BTreeMap<EdgeId, OffsetEdge>,
    supports: &BTreeMap<FaceId, Surface3>,
    tolerance: super::ModelTolerance,
) -> KernelResult<Point3> {
    let source_point = topology.vertex(vertex)?.point();
    let mut incident = topology
        .vertex_edges(vertex)?
        .into_iter()
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
enum CornerConstraint<'a> {
    Surface(&'a Surface3, Option<Point2>),
    Curve(&'a Curve3),
    Point(Point3),
}

impl CornerConstraint<'_> {
    fn project(
        self,
        point: Point3,
        tolerance: super::ModelTolerance,
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
fn refine_offset_corner(
    topology: &Topology,
    vertex: VertexId,
    point: Point3,
    faces: &BTreeSet<FaceId>,
    incident: &[EdgeId],
    edges: &BTreeMap<EdgeId, OffsetEdge>,
    supports: &BTreeMap<FaceId, Surface3>,
    tolerance: super::ModelTolerance,
) -> KernelResult<Point3> {
    let mut constraints = faces
        .iter()
        .map(|face| {
            let surface = &supports[face];
            if !matches!(surface, Surface3::Offset(_) | Surface3::RollingBall(_) | Surface3::Ruled(_)) {
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
            Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "offset corner has no incident coedge on its support face",
            ))
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

fn refine_support_corner(
    mut point: Point3,
    constraints: &[CornerConstraint<'_>],
    tolerance: super::ModelTolerance,
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
        let Some(step) = super::linalg::solve_linear(matrix, rhs) else {
            break;
        };
        let step = crate::Vec3::new(step[0], step[1], step[2]);
        // Near tangency a small normal residual can hide a large movement.
        // Require positional convergence as well.
        if worst <= tolerance.linear * 0.1 && step.magnitude() <= tolerance.linear * 0.01 {
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

fn retarget_offset_intersection_endpoints(
    topology: &Topology,
    edge_id: EdgeId,
    edge: &mut OffsetEdge,
    vertices: &BTreeMap<VertexId, Point3>,
    supports: &BTreeMap<FaceId, Surface3>,
    tolerance: super::ModelTolerance,
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
        None
    };
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
    if closed {
        super::intersection::unwrap_periodic_points(
            &mut first_points,
            intersection.first_surface().domains(),
        );
        super::intersection::unwrap_periodic_points(
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
    let mut changed = false;
    for (side, vertex) in source.vertices().into_iter().enumerate() {
        let point = vertices[&vertex];
        let projection = projections[side];
        if !closed && projection.distance <= tolerance.linear {
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
    if !closed && (collapsed || folded) {
        // Moving just an endpoint can fold the old guide back along the same
        // intersection. Continue the entire retained branch between its new,
        // topologically ordered vertices.
        for index in 1..32 {
            let fraction = index as f64 / 32.0;
            let a = first_points[0] + (first_points[32] - first_points[0]) * fraction;
            let b = second_points[0] + (second_points[32] - second_points[0]) * fraction;
            let uv = super::intersection::correct_surface_pair_near(
                intersection.first_surface(),
                intersection.second_surface(),
                [a.x, a.y, b.x, b.y],
                tolerance,
            )?;
            first_points[index] = Point2::new(uv[0], uv[1]);
            second_points[index] = Point2::new(uv[2], uv[3]);
        }
        super::intersection::unwrap_periodic_points(
            &mut first_points,
            intersection.first_surface().domains(),
        );
        super::intersection::unwrap_periodic_points(
            &mut second_points,
            intersection.second_surface().domains(),
        );
    }
    let rebuilt = SurfaceIntersectionCurve::new(
        intersection.first_surface().clone(),
        intersection.second_surface().clone(),
        super::intersection::smooth_nurbs_guide(&first_points, curve_range)?.into(),
        super::intersection::smooth_nurbs_guide(&second_points, curve_range)?.into(),
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

fn trim_offset_edge(
    topology: &Topology,
    edge_id: EdgeId,
    edge: &OffsetEdge,
    vertices: &BTreeMap<VertexId, Point3>,
    tolerance: super::ModelTolerance,
) -> KernelResult<ParameterRange> {
    let source = topology.edge(edge_id)?;
    let [first_vertex, second_vertex] = source.vertices();
    if first_vertex == second_vertex {
        let domain = edge.curve.domain();
        let super::DomainBounds::Bounded(bounds) = domain.bounds else {
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "a closed offset edge needs a bounded analytical parameterization",
            ));
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
    let project_endpoint = |point: Point3| -> KernelResult<super::Projection1> {
        let mut projection = edge.curve.project(point, tolerance)?;
        if let super::DomainBounds::Bounded(range) = edge.curve.domain().bounds {
            for parameter in [range.start, range.end] {
                let candidate = edge.curve.evaluate(parameter)?.point;
                let distance = candidate.distance(point);
                if distance < projection.distance {
                    projection = super::Projection1 {
                        parameter,
                        point: candidate,
                        distance,
                    };
                }
            }
        }
        Ok(projection)
    };
    let mut first = project_endpoint(vertices[&first_vertex])?;
    let mut second = project_endpoint(vertices[&second_vertex])?;
    // A bounded intersection curve can close at its chart seam without
    // declaring a periodic domain. Projection may choose either coincident
    // endpoint due to roundoff. Choose the endpoint that lets the trim follow
    // the source edge, instead of traversing the complementary arc.
    let domain = edge.curve.domain();
    if domain.period.is_none() {
        if let super::DomainBounds::Bounded(bounds) = domain.bounds {
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

fn projected_pcurve(
    curve: &Curve3,
    surface: &Surface3,
    guide: &Curve2,
    range: ParameterRange,
    tolerance: super::ModelTolerance,
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
    super::intersection::unwrap_periodic_points(&mut points, surface.domains());
    let guide: Curve2 = super::intersection::smooth_nurbs_guide(&points, range)?.into();
    Ok(
        SurfaceProjectionPcurve2::new(curve.clone(), surface.clone(), guide, range, tolerance)?
            .into(),
    )
}

fn offset_parameter_padding(
    surface: &Surface3,
    bounds: [ParameterRange; 2],
    distance: f64,
    tolerance: super::ModelTolerance,
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

pub(super) fn extended_support(
    surface: &Surface3,
    bounds: [ParameterRange; 2],
    distance: f64,
    tolerance: super::ModelTolerance,
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
            super::RevolvedSurface3::new(
                revolved.frame(), revolved.profile().extended(range)?, range, tolerance,
            ).map(Into::into)
        }
        _ => Ok(surface.clone()),
    }
}

// Contact rails delimit the material sector of a blend. Miter end curves can
// continue beyond those rails and must not enlarge its half-space constraint.
fn contact_sector(
    topology: &Topology,
    face: FaceId,
    tolerance: super::ModelTolerance,
) -> KernelResult<[ParameterRange; 2]> {
    let boundary = super::trim::face_boundary_curves(topology, face, tolerance)?;
    let mut bounds = super::trim::face_parameter_bounds(&boundary, tolerance)?;
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

pub(super) fn expanded_offset_bounds(
    surface: &Surface3,
    bounds: [ParameterRange; 2],
    distance: f64,
    tolerance: super::ModelTolerance,
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
        } else if let super::DomainBounds::Bounded(domain) = domains[axis].bounds {
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

fn offset_body(
    source: &Compound,
    openings: &BTreeSet<FaceId>,
    distance: f64,
    adjust_blends: bool,
) -> KernelResult<(Compound, bool)> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let source_faces = source
        .faces()
        .map(|face| face.topology_id())
        .collect::<BTreeSet<_>>();
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    let mut collapsed = BTreeMap::new();
    let mut vanished_blends = BTreeSet::new();
    for face_id in &source_faces {
        let face = topology.face(*face_id)?;
        let face_bounds = face_parameter_bounds(topology, *face_id, tolerance)?;
        let base = extended_support(face.surface(), face_bounds, distance, tolerance)?;
        let surface = if openings.contains(face_id) {
            base
        } else {
            let orientation = match face.orientation() {
                FaceOrientation::Forward => 1.0,
                FaceOrientation::Reversed => -1.0,
            };
            if let Some((support, past_radius)) = collapsed_support(&base, distance * orientation, tolerance)? {
                collapsed.insert(*face_id, support);
                if past_radius {
                    vanished_blends.insert(*face_id);
                }
                continue;
            }
            base.offset(distance * orientation, tolerance)?
        };
        let expanded = expanded_offset_bounds(&surface, face_bounds, distance, tolerance)?;
        supports.insert(*face_id, surface);
        bounds.insert(*face_id, expanded);
    }

    let has_vanished_blends = !vanished_blends.is_empty();
    let mut corner_blends = Vec::new();
    for face_id in vanished_blends {
        let neighbors = tangent_neighbors(topology, face_id, tolerance, |face| supports.contains_key(&face))?
            .into_iter().collect::<Vec<_>>();
        let (first, second) = match neighbors.as_slice() {
            [first, second] => (first, second),
            [face] => {
                corner_blends.push((face_id, *face));
                continue;
            },
            _ => return Err(not_implemented("removal of a blend without surviving contact supports")),
        };
        let CollapsedSupport::Curve(guide) = &collapsed[&face_id] else {
            unreachable!()
        };
        let mut period = None;
        for edge_id in topology.face_edges(face_id)? {
            let edge = topology.edge(edge_id)?;
            if edge.vertices()[0] == edge.vertices()[1]
                && topology.edge_faces(edge_id)?.len() == 2 {
                period = Some(edge.range().span().abs());
                break;
            }
        }
        let range = match guide.domain().bounds {
            super::DomainBounds::Bounded(range) => range,
            _ => match topology.face(face_id)?.surface() {
                Surface3::Cylinder(_) => face_parameter_bounds(topology, face_id, tolerance)?[1],
                Surface3::RollingBall(blend) => blend.center_range(),
                _ => return Err(not_implemented("unbounded collapsed blend guide")),
            },
        };
        let curve = intersect_collapsed_blend(
            &supports[first], &supports[second], guide, range, period, tolerance,
        )?;
        collapsed.insert(face_id, CollapsedSupport::Curve(curve));
    }
    for (face_id, contact) in corner_blends {
        let range = face_parameter_bounds(topology, face_id, tolerance)?;
        let point = topology.face(face_id)?.surface().evaluate(
            (range[0].start + range[0].end) * 0.5, (range[1].start + range[1].end) * 0.5,
        )?.point;
        let mut best: Option<(f64, &Curve3, f64, (f64, f64))> = None;
        for edge in topology.face_edges(face_id)? {
            for neighbor in topology.edge_faces(edge)? {
                if neighbor == face_id { continue; }
                let Some(CollapsedSupport::Curve(curve)) = collapsed.get(&neighbor) else { continue; };
                let projection = curve.project(point, tolerance)?;
                let value = curve.evaluate(projection.parameter)?;
                let uv = supports[&contact].project(value.point, tolerance)?.parameters;
                let transverse = value.first_derivative.normalize()
                    .dot(supports[&contact].normal(uv.0, uv.1)?).abs();
                if transverse > tolerance.angular && best.as_ref().is_none_or(|candidate| transverse > candidate.0) {
                    best = Some((transverse, curve, projection.parameter, uv));
                }
            }
        }
        let (_, guide, parameter, uv) = best
            .ok_or_else(|| not_implemented("collapsed blend corner without a transverse surviving rail"))?;
        let point = guide.intersect_surface_near(&supports[&contact], parameter, uv, tolerance)?.point;
        collapsed.insert(face_id, CollapsedSupport::Point(point));
    }

    let mut changed_supports = source_faces
        .difference(openings)
        .copied()
        .collect::<BTreeSet<_>>();
    if adjust_blends {
        changed_supports.extend(adjust_offset_blends(
            source,
            openings,
            distance,
            &mut supports,
        )?);
    }
    let patches = loop {
        let patches = support_patches(
            topology,
            tolerance,
            SupportEdit {
                supports: supports.clone(),
                bounds: bounds.clone(),
                changed: changed_supports.clone(),
                collapsed: collapsed.clone(),
                ..Default::default()
            },
        )?;
        let mut removed = BTreeSet::new();
        for patch in patches.values().flatten() {
            if offset_patch_collapsed(patch, tolerance)? {
                for face_id in supports.keys() {
                    if patch.sources.contains(&topology.face(*face_id)?.name()) {
                        removed.insert(*face_id);
                    }
                }
            }
        }
        if removed.is_empty() {
            break patches;
        }
        // A thin offset face can reverse after its two boundary rails cross.
        // Replace the entire strip by the intersection of the surviving supports.
        for face_id in &removed {
            supports.remove(face_id);
            bounds.remove(face_id);
        }
        let previous_collapsed = collapsed.clone();
        for face_id in removed {
            let edges = topology.face_edges(face_id)?;
            let neighbors = edges
                .iter()
                .map(|edge| topology.edge_faces(*edge))
                .collect::<KernelResult<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect::<BTreeSet<_>>();
            let mut surviving = neighbors
                .iter()
                .filter(|face| supports.contains_key(face))
                .copied()
                .collect::<Vec<_>>();
            if surviving.len() > 2 && matches!(topology.face(face_id)?.surface(), Surface3::Cone(_)) {
                // A chamfer's end faces clip its angular sector; its two
                // constant-height rails identify the supports that replace
                // the strip when those rails cross.
                let mut rails = BTreeSet::new();
                for edge in &edges {
                    let coedge = topology.edge(*edge)?.coedges().iter().map(|id| topology.coedge(*id))
                        .collect::<KernelResult<Vec<_>>>()?.into_iter().find(|coedge|
                            topology.loop_(coedge.loop_id()).unwrap().face() == face_id).unwrap();
                    let range = coedge.range();
                    let heights = [range.start, (range.start + range.end) * 0.5, range.end]
                        .map(|t| coedge.pcurve().evaluate(t).map(|value| value.point.y));
                    let heights = heights.into_iter().collect::<KernelResult<Vec<_>>>()?;
                    if heights.iter().all(|height| (height - heights[0]).abs() <= tolerance.parameter * 64.0) {
                        rails.extend(topology.edge_faces(*edge)?.into_iter().filter(|face| supports.contains_key(face)));
                    }
                }
                surviving = rails.into_iter().collect();
            }
            let support = match surviving.as_slice() {
                [first, second] => {
                    let edge_id = edges
                        .iter()
                        .find(|edge| {
                            topology.edge_faces(**edge).is_ok_and(|faces| faces.contains(first))
                        })
                        .unwrap();
                    let edge = topology.edge(*edge_id)?;
                    CollapsedSupport::Curve(intersect_collapsed_blend(
                        &supports[first],
                        &supports[second],
                        edge.curve(),
                        edge.range(),
                        None,
                        tolerance,
                    )?)
                }
                [face] => {
                    let guide = neighbors
                        .iter()
                        .find_map(|neighbor| match previous_collapsed.get(neighbor) {
                            Some(CollapsedSupport::Curve(curve)) => Some(curve),
                            _ => None,
                        })
                        .ok_or_else(|| not_implemented("collapsed offset face without a surviving rail"))?;
                    let range = face_parameter_bounds(topology, face_id, tolerance)?;
                    let source_point = topology.face(face_id)?
                        .surface()
                        .evaluate(
                            (range[0].start + range[0].end) * 0.5,
                            (range[1].start + range[1].end) * 0.5,
                        )?
                        .point;
                    let parameter = guide.project(source_point, tolerance)?.parameter;
                    let uv = supports[face]
                        .project(guide.evaluate(parameter)?.point, tolerance)?
                        .parameters;
                    CollapsedSupport::Point(
                        guide
                            .intersect_surface_near(&supports[face], parameter, uv, tolerance)?
                            .point,
                    )
                }
                _ => {
                    return Err(not_implemented(
                        "removal of an offset face without two surviving supports",
                    ));
                }
            };
            collapsed.insert(face_id, support);
        }
    };
    let mut patches = patches;
    for shell in patches.values_mut() {
        regularize_coincident_offset_faces(shell, distance, tolerance)?;
    }
    let offset = commit_support_patches(source, patches, changed_supports)?;
    Ok((offset, has_vanished_blends))
}

fn regularize_coincident_offset_faces(
    patches: &mut Vec<BrepPatch>,
    distance: f64,
    tolerance: super::ModelTolerance,
) -> KernelResult<()> {
    // Move untouched geometry directly to the result. Only planar patches
    // need grouping, with one representative comparison per existing group.
    let mut groups: Vec<Vec<BrepPatch>> = Vec::new();
    let mut result = Vec::new();
    for patch in std::mem::take(patches) {
        if !matches!(patch.surface, Surface3::Plane(_)) {
            result.push(patch);
            continue;
        }
        let mut group = None;
        for (candidate, members) in groups.iter().enumerate() {
            if super::trim::coincident_supports(&patch.surface, &members[0].surface, tolerance)? {
                group = Some(candidate);
                break;
            }
        }
        if let Some(group) = group { groups[group].push(patch); }
        else { groups.push(vec![patch]); }
    }
    let boundary = |patch: &BrepPatch, surface: &Surface3| {
        patch.loops.iter().flat_map(|loop_| loop_.segments.iter().enumerate().map(|(index, segment)| {
            let pcurve = if &patch.surface == surface { segment.pcurve.clone() } else {
                SurfaceProjectionPcurve2::new(segment.curve.clone(), surface.clone(),
                    segment.pcurve.clone(), segment.range, tolerance)?.into()
            };
            Ok(super::trim::FaceCurve {
                provenance: segment.provenance.clone(), curve: segment.curve.clone(),
                pcurve, range: segment.range,
                loop_end: index + 1 == loop_.segments.len(), loop_kind: loop_.kind,
                parameter_seam: false,
            })
        })).collect::<KernelResult<Vec<_>>>()
    };
    for group in groups {
        let patch = &group[0];
        let normal = patch.surface.normal(0.0, 0.0)?;
        let orientations = group.iter().map(|other| {
            Ok(if normal.dot(other.surface.normal(0.0, 0.0)?) < 0.0 {
                other.orientation.reversed()
            } else { other.orientation })
        }).collect::<KernelResult<Vec<_>>>()?;
        if orientations.iter().all(|orientation| *orientation == orientations[0]) {
            result.extend(group);
            continue;
        }
        // Opposite offset caps can meet when a wall reaches zero thickness.
        // Cancel their overlap while retaining the surrounding cap regions.
        let boundaries = group.iter().map(|other| boundary(other, &patch.surface))
            .collect::<KernelResult<Vec<_>>>()?;
        let curves = boundaries.iter().flatten().cloned().collect::<Vec<_>>();
        let bounds = super::trim::face_parameter_bounds(&curves, tolerance)?;
        let mut regions = super::boolean::arrange_surface_regions(&patch.surface, &curves,
            distance.abs() * 1.0e-3, bounds[0].span().abs().min(bounds[1].span().abs()) * 0.05,
            tolerance, |uv| {
                let mut winding = 0;
                for (orientation, boundary) in orientations.iter().zip(&boundaries) {
                    if super::trim::point_in_trim(uv, boundary)? {
                        winding += if *orientation == FaceOrientation::Forward { 1 } else { -1 };
                    }
                }
                Ok(match winding.cmp(&0) {
                    std::cmp::Ordering::Greater => Some(FaceOrientation::Forward),
                    std::cmp::Ordering::Less => Some(FaceOrientation::Reversed),
                    std::cmp::Ordering::Equal => None,
                })
            })?;
        for region in &mut regions {
            region.sources = group.iter().zip(&orientations).filter(|(_, orientation)| **orientation == region.orientation)
                .flat_map(|(patch, _)| patch.sources.iter().copied()).collect();
        }
        result.extend(regions);
    }
    *patches = result;
    Ok(())
}

#[cfg(test)]
mod offset_cap_tests {
    use super::*;

    #[test]
    fn cancel_caps_in_different_plane_charts() {
        let tolerance = super::super::ModelTolerance::default();
        let rectangle = |plane: Plane3, left: f64, orientation| {
            let mut points = [Point3::new(left, 0.0, 0.0), Point3::new(left + 2.0, 0.0, 0.0),
                Point3::new(left + 2.0, 1.0, 0.0), Point3::new(left, 1.0, 0.0)];
            if plane.normal().z < 0.0 { points.reverse(); }
            let segments = (0..4).map(|index| {
                let a = points[index];
                let b = points[(index + 1) % 4];
                let uv = |point| {
                    let (u, v) = plane.project(point).unwrap().parameters;
                    Point2::new(u, v)
                };
                BrepPatchSegment {
                    curve: Line3::new(a, b - a).unwrap().into(),
                    pcurve: super::super::Line2::new(uv(a), uv(b) - uv(a)).unwrap().into(),
                    range: ParameterRange::new(0.0, 1.0).unwrap(),
                    provenance: SegmentProvenance::default(),
                }
            }).collect();
            BrepPatch {
                surface: plane.into(), orientation, sources: Vec::new(), preserved: Vec::new(),
                loops: vec![BrepPatchLoop { kind: super::super::LoopKind::Outer, segments }],
            }
        };
        for plane in [
            Plane3::xy(),
            Plane3::new(Point3::new(11.0, -7.0, 0.0), crate::Vec3::unit_y() * 2.0,
                -crate::Vec3::unit_x() * 3.0, tolerance).unwrap(),
            Plane3::new(Point3::new(11.0, -7.0, 0.0), crate::Vec3::unit_y() * 2.0,
                crate::Vec3::unit_x() * 3.0, tolerance).unwrap(),
        ] {
            let orientation = if plane.normal().z > 0.0 { FaceOrientation::Reversed }
                else { FaceOrientation::Forward };
            let mut patches = vec![rectangle(Plane3::xy(), 0.0, FaceOrientation::Forward),
                rectangle(plane, 1.0, orientation)];
            regularize_coincident_offset_faces(&mut patches, -1.0, tolerance).unwrap();
            assert_eq!(patches.len(), 2);
            for patch in patches {
                assert_eq!(patch.surface, Surface3::Plane(Plane3::xy()));
                let segments = &patch.loops[0].segments;
                let area = segments.iter().map(|segment| segment.pcurve
                    .signed_area_contribution(segment.range, tolerance.parameter).unwrap()).sum::<f64>();
                assert!((area.abs() - 1.0).abs() < tolerance.linear);
                let left = if patch.orientation == FaceOrientation::Forward { 0.0 } else { 2.0 };
                for segment in segments {
                    for t in [segment.range.start, segment.range.end] {
                        let x = segment.curve.evaluate(t).unwrap().point.x;
                        assert!(x >= left - tolerance.linear && x <= left + 1.0 + tolerance.linear);
                    }
                }
            }
        }
    }
}

fn offset_patch_collapsed(
    patch: &BrepPatch,
    tolerance: super::ModelTolerance,
) -> KernelResult<bool> {
    let sign = patch.orientation.sign();
    let domains = patch.surface.domains();
    for loop_ in &patch.loops {
        if loop_.kind != super::LoopKind::Outer {
            continue;
        }
        let mut area = 0.0;
        let mut principal_area = 0.0;
        let mut previous_end: Option<Point2> = None;
        for segment in &loop_.segments {
            let start = segment.pcurve.evaluate(segment.range.start)?.point;
            let end = segment.pcurve.evaluate(segment.range.end)?.point;
            let mut shift = crate::Vec2::new(0.0, 0.0);
            if let Some(previous) = previous_end {
                for axis in 0..2 {
                    if let Some(period) = domains[axis].period {
                        let delta = previous[axis] - start[axis];
                        let candidate = (delta / period).round() * period;
                        if (delta - candidate).abs() <= tolerance.parameter * 64.0 {
                            shift[axis] = candidate;
                        }
                    }
                }
            }
            let contribution = segment
                .pcurve
                .signed_area_contribution(segment.range, tolerance.parameter)?;
            principal_area += contribution;
            area += contribution + 0.5 * (shift.x * (end.y - start.y) - shift.y * (end.x - start.x));
            previous_end = Some(end + shift);
        }
        if principal_area * sign <= tolerance.parameter.powi(2)
            && area * sign <= tolerance.parameter.powi(2)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Clone)]
pub(super) enum CollapsedSupport {
    Curve(Curve3),
    Point(Point3),
}

fn collapsed_support(
    surface: &Surface3,
    distance: f64,
    tolerance: super::ModelTolerance,
) -> KernelResult<Option<(CollapsedSupport, bool)>> {
    let zero = |radius: f64| radius.abs() <= tolerance.linear;
    Ok(match surface {
        Surface3::Cylinder(cylinder) => {
            let sign = cylinder
                .axis()
                .dot(cylinder.x_radius().cross(cylinder.y_radius()))
                .signum();
            let radius = cylinder.radius() + distance * sign;
            if radius <= tolerance.linear {
                Some((CollapsedSupport::Curve(
                    Line3::new(cylinder.origin(), cylinder.axis())?.into(),
                ), radius < -tolerance.linear))
            } else {
                None
            }
        }
        Surface3::Sphere(sphere) if zero(sphere.radius() + distance) => {
            Some((CollapsedSupport::Point(sphere.center()), false))
        }
        Surface3::Torus(torus) if torus.minor_radius() + distance <= tolerance.linear => {
            Some((CollapsedSupport::Curve(
                super::Circle3::new(torus.center(), torus.x_major(), torus.y_major(), tolerance)?
                    .into(),
            ), torus.minor_radius() + distance < -tolerance.linear))
        }
        Surface3::RollingBall(blend) => {
            let range = blend.center_range();
            let parameter = (range.start + range.end) * 0.5;
            let value = surface.evaluate(parameter, 0.5)?;
            let radial = value.point - blend.center_curve().evaluate(parameter)?.point;
            let sign = value.normal()?.dot(radial).signum();
            let radius = blend.radius() + distance * sign;
            (radius <= tolerance.linear)
                .then(|| (CollapsedSupport::Curve(blend.center_curve().clone()), radius < -tolerance.linear))
        }
        _ => None,
    })
}

fn intersect_collapsed_blend(
    first: &Surface3,
    second: &Surface3,
    guide: &Curve3,
    mut range: ParameterRange,
    period: Option<f64>,
    tolerance: super::ModelTolerance,
) -> KernelResult<Curve3> {
    let intersections = first.intersect_surface(second, tolerance).or_else(|error| {
        if !error.diagnostics().iter().all(|d| d.kind == DiagnosticKind::UnsupportedGeometry) {
            return Err(error);
        }
        // The removed blend identifies the local intersection branch of its
        // surviving offset supports. Keep exact surface intersection evaluation;
        // the old center curve supplies only seeds for the parameter guides.
        let mut points = [Vec::new(), Vec::new()];
        let center = (range.start + range.end) * 0.5;
        if let Some(period) = period {
            range = ParameterRange::new(center - 1.5 * period, center + 1.5 * period)?;
        }
        let indices = if period.is_some() { -1..=97 } else { 0..=96 };
        for index in indices {
            let parameter = range.start + range.span() * index as f64 / 96.0;
            let parameter = period.map_or(parameter, |period| {
                center - period * 0.5 + (parameter - center + period * 0.5).rem_euclid(period)
            });
            let point = guide.evaluate(parameter)?.point;
            let a = first.project(point, tolerance)?.parameters;
            let b = second.project(point, tolerance)?.parameters;
            let uv = super::intersection::correct_surface_pair_near(
                first, second, [a.0, a.1, b.0, b.1], tolerance,
            )?;
            points[0].push(Point2::new(uv[0], uv[1]));
            points[1].push(Point2::new(uv[2], uv[3]));
        }
        for (points, surface) in points.iter_mut().zip([first, second]) {
            super::intersection::unwrap_periodic_points(points, surface.domains());
        }
        let pcurve = |points: &[Point2]| {
            if period.is_some() {
                super::intersection::periodic_nurbs_guide(points, range)
            } else {
                super::intersection::smooth_nurbs_guide(points, range)
            }
        };
        let curve = SurfaceIntersectionCurve::new(
            first.clone(), second.clone(),
            pcurve(&points[0])?.into(), pcurve(&points[1])?.into(),
            range, tolerance, tolerance.linear,
        )?;
        let exact = curve.exact_pcurves();
        Ok(IntersectionSet::Isolated(vec![super::SurfaceSurfaceCurve {
            curve: curve.into(), first_pcurve: exact[0].clone(), second_pcurve: exact[1].clone(),
            achieved_error: tolerance.linear,
        }]))
    })?;
    let IntersectionSet::Isolated(candidates) = intersections else {
        return Err(invalid("surviving supports of a removed blend have no distinct intersection"));
    };
    let reference = guide.evaluate((range.start + range.end) * 0.5)?.point;
    candidates.into_iter().map(|candidate| {
        candidate.curve.project(reference, tolerance).map(|projection| (projection.distance, candidate.curve))
    }).collect::<KernelResult<Vec<_>>>()?.into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, curve)| curve)
        .ok_or_else(|| invalid("surviving supports of a removed blend do not intersect"))
}

fn collapsed_offset_vertex(
    topology: &Topology,
    vertex: VertexId,
    edges: &BTreeMap<EdgeId, OffsetEdge>,
    supports: &BTreeMap<FaceId, Surface3>,
    collapsed: &BTreeMap<FaceId, CollapsedSupport>,
    tolerance: super::ModelTolerance,
) -> KernelResult<Point3> {
    let incident = topology.vertex_edges(vertex)?;
    let faces = incident.iter().copied()
        .map(|edge| topology.edge_faces(edge))
        .collect::<KernelResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    let mut support_faces = faces.iter().filter(|face| supports.contains_key(face)).copied().collect::<BTreeSet<_>>();
    let mut constraints = Vec::new();
    for face in &faces {
        let Some(collapsed) = collapsed.get(face) else { continue; };
        let contacts = tangent_neighbors(topology, *face, tolerance, |face| supports.contains_key(&face))?;
        match collapsed {
            CollapsedSupport::Curve(curve) if contacts.is_empty() => constraints.push(CornerConstraint::Curve(curve)),
            CollapsedSupport::Point(center) => constraints.push(CornerConstraint::Point(*center)),
            _ => {},
        }
        support_faces.extend(contacts);
    }
    constraints.extend(support_faces.iter().map(|face| CornerConstraint::Surface(&supports[face], None)));
    // Surviving parameter seams still constrain the vertex after the blend
    // disappears. A nearest point on the new boundary alone can slide away
    // from a neighboring face's seam and leave its loop disconnected.
    let mut contact_edges = BTreeSet::new();
    for face in &support_faces {
        for edge_id in topology.face_edges(*face)? {
            let faces = topology.edge_faces(edge_id)?;
            if faces.len() == 2 && faces.iter().all(|face| support_faces.contains(face))
                && !tangent_contact(topology, edge_id, faces[0], faces[1], tolerance)?.is_empty() {
                contact_edges.insert(edge_id);
            }
        }
    }
    for edge_id in incident {
        let faces = topology.edge_faces(edge_id)?;
        if faces.len() == 1 && !collapsed.contains_key(&faces[0]) {
            constraints.push(CornerConstraint::Curve(&edges[&edge_id].curve));
        }
    }
    constraints.extend(contact_edges.iter().map(|edge| CornerConstraint::Curve(&edges[edge].curve)));
    // At tangent supports, start on the replacement rail rather than correcting
    // from the old corner through a singular surface intersection.
    let mut seed = topology.vertex(vertex)?.point();
    if let Some(support) = faces.iter().find_map(|face| collapsed.get(face)) {
        seed = match support {
            CollapsedSupport::Curve(curve) => curve.project(seed, tolerance)?.point,
            CollapsedSupport::Point(point) => *point,
        };
    }
    refine_support_corner(seed, &constraints, tolerance)
}

/// A body's faces with edited supports, from which support reconstruction
/// rebuilds every affected boundary.
#[derive(Default)]
pub(super) struct SupportEdit {
    /// The (possibly new) support of every face.
    pub supports: BTreeMap<FaceId, Surface3>,
    /// The parameter box each rebuilt face trim may occupy.
    pub bounds: BTreeMap<FaceId, [ParameterRange; 2]>,
    /// Faces whose supports were edited.
    pub changed: BTreeSet<FaceId>,
    /// Keep every face's orientation instead of deriving it from its support.
    pub preserve_orientation: bool,
    /// Recompute every edge from its supports, retaining none unchanged.
    pub reconstruct_edges: bool,
    /// Faces whose parameterization changed although their support did not.
    pub reparameterized: BTreeSet<FaceId>,
    /// Edited supports that collapse to a curve or a point.
    pub collapsed: BTreeMap<FaceId, CollapsedSupport>,
}

fn rebuild_supports(source: &Compound, edit: SupportEdit) -> KernelResult<Compound> {
    let changed = edit.changed.clone();
    let patches_by_shell = support_patches(source.topology(), source.tolerance(), edit)?;
    commit_support_patches(source, patches_by_shell, changed)
}

fn commit_support_patches(
    source: &Compound,
    patches_by_shell: BTreeMap<super::ShellId, Vec<BrepPatch>>,
    changed_supports: BTreeSet<FaceId>,
) -> KernelResult<Compound> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    let mut affected_faces = changed_supports.clone();
    for face in changed_supports {
        for edge in topology.face_edges(face)? {
            affected_faces.extend(topology.edge_faces(edge)?);
        }
    }
    let affected = affected_faces.into_iter().map(|face| topology.face(face).map(|value| value.name()))
        .collect::<KernelResult<BTreeSet<_>>>()?;
    let mut bodies = BTreeMap::new();
    for (shell, patches) in patches_by_shell {
        bodies.insert(shell, transaction.brep_shell(&patches)?);
    }
    transaction.restore_brep_shells(topology, &bodies, &affected)?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [])
}

pub(super) fn support_patches(
    topology: &Topology,
    tolerance: super::ModelTolerance,
    edit: SupportEdit,
) -> KernelResult<BTreeMap<super::ShellId, Vec<BrepPatch>>> {
    let SupportEdit {
        supports,
        bounds,
        changed: changed_supports,
        preserve_orientation,
        reconstruct_edges,
        reparameterized,
        collapsed,
    } = edit;
    let (reparameterized, collapsed) = (&reparameterized, &collapsed);
    let source_faces = supports.keys().copied().collect::<BTreeSet<_>>();
    let mut source_edges = BTreeSet::new();
    for face_id in &source_faces {
        source_edges.extend(topology.face_edges(*face_id)?);
    }
    let mut edges = BTreeMap::new();
    for edge_id in source_edges {
        let faces = topology.edge_faces(edge_id)?;
        if let Some(support) = faces.iter().find_map(|face| collapsed.get(face)) {
            if let CollapsedSupport::Curve(curve) = support {
                edges.insert(
                    edge_id,
                    OffsetEdge {
                        curve: curve.clone(),
                        pcurves: BTreeMap::new(),
                        range: topology.edge(edge_id)?.range(),
                    },
                );
            }
            continue;
        }
        edges.insert(
            edge_id,
            offset_edge(
                topology,
                edge_id,
                &supports,
                &bounds,
                tolerance,
                !reconstruct_edges,
                reparameterized,
            )?,
        );
    }

    let mut required_vertices = BTreeSet::new();
    for (edge_id, edge) in &edges {
        let source_edge = topology.edge(*edge_id)?;
        if source_edge.vertices()[0] != source_edge.vertices()[1] {
            required_vertices.extend(source_edge.vertices());
        } else if edge.curve.domain().period.is_none() {
            required_vertices.extend(source_edge.vertices());
        }
    }
    let mut vertices = BTreeMap::new();
    let mut split_vertices = BTreeMap::<(EdgeId, VertexId), Point3>::new();
    let mut corner_edges = BTreeMap::<(VertexId, FaceId), Curve3>::new();
    for vertex in required_vertices {
        if topology.vertex_edges(vertex)?.into_iter().any(|edge| {
            topology
                .edge_faces(edge)
                .is_ok_and(|faces| faces.iter().any(|face| collapsed.contains_key(face)))
        }) {
            vertices.insert(
                vertex,
                collapsed_offset_vertex(topology, vertex, &edges, &supports, collapsed, tolerance)?,
            );
            continue;
        }
        match offset_vertex(topology, vertex, &edges, &supports, tolerance) {
            Ok(point) => {
                vertices.insert(vertex, point);
            }
            Err(error) => {
                // A four-face corner need not survive an offset as one vertex.
                // Try the two diagonals of its face fan, retaining the split
                // whose triple intersections lie inside the remaining faces.
                let incident = topology.vertex_edges(vertex)?;
                let faces = incident
                    .iter()
                    .map(|edge| topology.edge_faces(*edge))
                    .collect::<KernelResult<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<BTreeSet<_>>();
                if incident.len() != 4 || faces.len() != 4 {
                    return Err(error);
                }
                let source_point = topology.vertex(vertex)?.point();
                let mut solution = None;
                for first in &faces {
                    for second in
                        faces.range((std::ops::Bound::Excluded(first), std::ops::Bound::Unbounded))
                    {
                        if incident.iter().any(|edge| {
                            topology
                                .edge_faces(*edge)
                                .is_ok_and(|f| f.contains(first) && f.contains(second))
                        }) {
                            continue;
                        }
                        let others = faces
                            .iter()
                            .filter(|f| *f != first && *f != second)
                            .copied()
                            .collect::<Vec<_>>();
                        let mut ends = Vec::new();
                        for other in &others {
                            let subset = BTreeSet::from([*first, *second, *other]);
                            let uses = incident
                                .iter()
                                .filter(|edge| {
                                    topology
                                        .edge_faces(**edge)
                                        .is_ok_and(|f| f.iter().all(|face| subset.contains(face)))
                                })
                                .copied()
                                .collect::<Vec<_>>();
                            if let Ok(point) = refine_offset_corner(
                                topology,
                                vertex,
                                source_point,
                                &subset,
                                &uses,
                                &edges,
                                &supports,
                                tolerance,
                            ) {
                                ends.push((point, uses));
                            }
                        }
                        if ends.len() != 2 {
                            continue;
                        }
                        let mut inside = true;
                        for side in 0..2 {
                            let excluded = others[1 - side];
                            let projection =
                                supports[&excluded].project(ends[side].0, tolerance)?;
                            let uv = projection.parameters;
                            let trim = contact_sector(topology, excluded, tolerance)?;
                            let domains = supports[&excluded].domains();
                            // Angular supports constrain only their retained
                            // sector. Their continuation is not a global
                            // half-space beyond that sector.
                            if [uv.0, uv.1].into_iter().enumerate().any(|(axis, value)| {
                                let value = if let Some(period) = domains[axis].period {
                                    value
                                        + (((trim[axis].start + trim[axis].end) * 0.5 - value)
                                            / period)
                                            .round()
                                            * period
                                } else if axis == 1
                                    && matches!(
                                        supports[&excluded],
                                        Surface3::RollingBall(_) | Surface3::Ruled(_)
                                    )
                                {
                                    value
                                } else {
                                    return false;
                                };
                                !trim[axis].contains(value, tolerance)
                            }) {
                                continue;
                            }
                            let sign = topology.face(excluded)?.orientation().sign();
                            if (ends[side].0 - projection.point)
                                .dot(supports[&excluded].normal(uv.0, uv.1)?)
                                * sign
                                > tolerance.linear
                            {
                                inside = false;
                            }
                        }
                        if !inside {
                            continue;
                        }
                        solution = Some((*first, *second, ends));
                        break;
                    }
                    if solution.is_some() {
                        break;
                    }
                }
                let Some((first, second, ends)) = solution else {
                    return Err(error);
                };
                let mut guides = [Vec::new(), Vec::new()];
                for index in 0..=8 {
                    let point = ends[0].0 + (ends[1].0 - ends[0].0) * (index as f64 / 8.0);
                    let a = supports[&first].project(point, tolerance)?.parameters;
                    let b = supports[&second].project(point, tolerance)?.parameters;
                    let corrected = super::intersection::correct_surface_pair_near(
                        &supports[&first],
                        &supports[&second],
                        [a.0, a.1, b.0, b.1],
                        tolerance,
                    )?;
                    guides[0].push(Point2::new(corrected[0], corrected[1]));
                    guides[1].push(Point2::new(corrected[2], corrected[3]));
                }
                let range = ParameterRange::new(0.0, 1.0)?;
                for (side, face) in [first, second].into_iter().enumerate() {
                    super::intersection::unwrap_periodic_points(
                        &mut guides[side],
                        supports[&face].domains(),
                    );
                }
                let curve: Curve3 = SurfaceIntersectionCurve::new(
                    supports[&first].clone(),
                    supports[&second].clone(),
                    super::intersection::smooth_nurbs_guide(&guides[0], range)?.into(),
                    super::intersection::smooth_nurbs_guide(&guides[1], range)?.into(),
                    range,
                    tolerance,
                    tolerance.linear,
                )?
                .into();
                corner_edges.insert((vertex, first), curve.clone());
                corner_edges.insert((vertex, second), curve);
                for (point, uses) in ends {
                    for edge in uses {
                        split_vertices.insert((edge, vertex), point);
                    }
                }
            }
        }
    }
    if !collapsed.is_empty() {
        edges.retain(|id, _| {
            let [first, second] = topology.edge(*id).unwrap().vertices();
            first == second || vertices[&first].distance(vertices[&second]) > tolerance.linear
        });
    }
    for (edge_id, edge) in &mut edges {
        let endpoints: BTreeMap<_, _> = topology
            .edge(*edge_id)?
            .vertices()
            .into_iter()
            .filter_map(|vertex| {
                split_vertices
                    .get(&(*edge_id, vertex))
                    .or_else(|| vertices.get(&vertex))
                    .map(|point| (vertex, *point))
            })
            .collect();
        let faces = topology.edge_faces(*edge_id)?;
        if let [face] = faces.as_slice() {
            let original = topology.edge(*edge_id)?;
            if endpoints.len() == 2 && endpoints.values().any(|point| {
                edge.curve.project(*point, tolerance).is_ok_and(|p| p.distance > tolerance.linear)
            }) {
                let range = original.range();
                let mut uv = original.vertices().map(|vertex| {
                    supports[face].project(endpoints[&vertex], tolerance).map(|p| Point2::new(p.parameters.0, p.parameters.1))
                }).into_iter().collect::<KernelResult<Vec<_>>>()?;
                super::intersection::unwrap_periodic_points(&mut uv, supports[face].domains());
                let direction = (uv[1] - uv[0]) / range.span();
                let pcurve: Curve2 = super::Line2::new(uv[0] - direction * range.start, direction)?.into();
                edge.curve = supports[face].boundary_curve(pcurve.clone(), range, tolerance)?;
                for id in original.coedges() {
                    let old_uv = topology.coedge(*id)?.pcurve().evaluate(range.start)?.point;
                    let shift = whole_periods(supports[face].domains(), old_uv - uv[0]);
                    edge.pcurves.insert(*id, super::Line2::new(uv[0] + shift - direction * range.start, direction)?.into());
                }
            }
        }
        retarget_offset_intersection_endpoints(
            topology, *edge_id, edge, &endpoints, &supports, tolerance,
        )?;
        edge.range = trim_offset_edge(topology, *edge_id, edge, &endpoints, tolerance)?;
        if edge.pcurves.is_empty() {
            for id in topology.edge(*edge_id)?.coedges() {
                let face = topology.loop_(topology.coedge(*id)?.loop_id())?.face();
                let Some(surface) = supports.get(&face) else {
                    continue;
                };
                let mut points = (0..=16)
                    .map(|i| {
                        let point = edge
                            .curve
                            .evaluate(edge.range.start + edge.range.span() * i as f64 / 16.0)?
                            .point;
                        let uv = surface.project(point, tolerance)?.parameters;
                        Ok(Point2::new(uv.0, uv.1))
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                super::intersection::unwrap_periodic_points(&mut points, surface.domains());
                let guide = super::intersection::smooth_nurbs_guide(&points, edge.range)?.into();
                let pcurve = SurfaceProjectionPcurve2::new(
                    edge.curve.clone(),
                    surface.clone(),
                    guide,
                    edge.range,
                    tolerance,
                )?
                .into();
                edge.pcurves.insert(*id, pcurve);
            }
        }
    }

    let mut patches_by_shell = BTreeMap::<super::ShellId, Vec<BrepPatch>>::new();
    let mut changed_vertices = BTreeSet::new();
    for id in &changed_supports {
        for edge in topology.face_edges(*id)? {
            changed_vertices.extend(topology.edge(edge)?.vertices());
        }
    }
    for face_id in source_faces {
        let face = topology.face(face_id)?;
        let surface = &supports[&face_id];
        let mut face_retained = !changed_supports.contains(&face_id);
        let mut loops = Vec::new();
        for loop_id in face.loops() {
            let loop_value = topology.loop_(*loop_id)?;
            let mut segments = Vec::new();
            for coedge_id in topology.loop_coedges(*loop_id)? {
                let coedge = topology.coedge(coedge_id)?;
                let Some(edge) = edges.get(&coedge.edge()) else {
                    continue;
                };
                let range = if coedge.is_forward() {
                    edge.range
                } else {
                    edge.range.reversed()
                };
                let pcurve = projected_pcurve(
                    &edge.curve,
                    surface,
                    &edge.pcurves[&coedge_id],
                    range,
                    tolerance,
                )?;
                let endpoints = topology.coedge_vertices(coedge_id)?;
                let affected = endpoints
                    .iter()
                    .any(|vertex| changed_vertices.contains(vertex))
                    || topology
                        .edge_faces(coedge.edge())?
                        .iter()
                        .any(|face| changed_supports.contains(face));
                face_retained &= !affected;
                let original = SegmentProvenance::from_coedge(topology, coedge_id)?;
                let mut provenance = if affected {
                    original.modified()
                } else {
                    original.clone()
                };
                for end in 0..2 {
                    if !changed_vertices.contains(&endpoints[end]) {
                        provenance.vertices[end] = original.vertices[end].clone();
                    }
                }
                let mut parameters = vec![range.start, range.end];
                for neighbor in topology.edge_faces(coedge.edge())? {
                    if !collapsed.contains_key(&neighbor) {
                        continue;
                    }
                    // Both rails of a collapsed blend become one boundary.
                    // Retain the seams from both neighboring faces by splitting
                    // that boundary at every surviving rail vertex.
                    for rail in topology.face_edges(neighbor)? {
                        for vertex in topology.edge(rail)?.vertices() {
                            if let Some(point) = vertices.get(&vertex) {
                                let projection = edge.curve.project_on_trim(*point, range, tolerance)?;
                                if projection.distance <= tolerance.linear * 8.0 {
                                    parameters.push(projection.parameter);
                                }
                            }
                        }
                    }
                }
                parameters.sort_by(|a, b| ((a - range.start) / range.span())
                    .total_cmp(&((b - range.start) / range.span())));
                parameters.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
                for pair in parameters.windows(2) {
                    let retained = ParameterRange::new(pair[0], pair[1])?;
                    let start = edge.curve.evaluate(retained.start)?.point;
                    let end = edge.curve.evaluate(retained.end)?.point;
                    let middle = edge.curve.evaluate((retained.start + retained.end) * 0.5)?.point;
                    if start.distance(end).max(start.distance(middle)) <= tolerance.linear {
                        continue;
                    }
                    segments.push(BrepPatchSegment {
                        provenance: provenance.trimmed(range, retained),
                        curve: edge.curve.clone(),
                        pcurve: pcurve.clone(),
                        range: retained,
                    });
                }
                let source_edge = topology.edge(coedge.edge())?;
                let vertex = source_edge.vertices()[if coedge.is_forward() { 1 } else { 0 }];
                if let Some(curve) = corner_edges.get(&(vertex, face_id)) {
                    let next = topology.coedge(coedge.next())?;
                    let start = split_vertices[&(coedge.edge(), vertex)];
                    let end = split_vertices[&(next.edge(), vertex)];
                    let range = ParameterRange::new(
                        curve.project(start, tolerance)?.parameter,
                        curve.project(end, tolerance)?.parameter,
                    )?;
                    let Curve3::SurfaceIntersection(intersection) = curve else {
                        unreachable!()
                    };
                    let exact = intersection.exact_pcurves();
                    let pcurve = if intersection.first_surface() == surface {
                        exact[0].clone()
                    } else {
                        exact[1].clone()
                    };
                    segments.push(BrepPatchSegment {
                        provenance: Default::default(),
                        curve: curve.clone(),
                        pcurve,
                        range,
                    });
                }
            }
            loops.push(BrepPatchLoop {
                kind: loop_value.kind(),
                segments,
            });
        }
        patches_by_shell
            .entry(face.shell())
            .or_default()
            .push(BrepPatch {
                preserved: if face_retained {
                    vec![face.name()]
                } else {
                    Vec::new()
                },
                sources: vec![face.name()],
                surface: surface.clone(),
                orientation: if preserve_orientation {
                    face.orientation()
                } else {
                    let sample = topology.face_edges(face_id)?[0];
                    let edge = topology.edge(sample)?;
                    let point = edge
                        .curve()
                        .evaluate((edge.range().start + edge.range().end) * 0.5)?
                        .point;
                    let old_uv = face.surface().project(point, tolerance)?.parameters;
                    let new_uv = surface.project(point, tolerance)?.parameters;
                    let same = face
                        .surface()
                        .normal(old_uv.0, old_uv.1)?
                        .dot(surface.normal(new_uv.0, new_uv.1)?)
                        >= 0.0;
                    if same {
                        face.orientation()
                    } else {
                        face.orientation().reversed()
                    }
                },
                loops,
            });
    }

    Ok(patches_by_shell)
}

pub(crate) fn face_patch(
    topology: &Topology,
    face_id: FaceId,
    reverse: bool,
) -> KernelResult<BrepPatch> {
    let face = topology.face(face_id)?;
    let mut loops = Vec::new();
    for loop_id in face.loops() {
        let loop_value = topology.loop_(*loop_id)?;
        let mut segments = topology
            .loop_coedges(*loop_id)?
            .into_iter()
            .map(|coedge_id| {
                let coedge = topology.coedge(coedge_id)?;
                Ok(BrepPatchSegment {
                    provenance: SegmentProvenance::from_coedge(topology, coedge_id)?,
                    curve: topology.edge(coedge.edge())?.curve().clone(),
                    pcurve: coedge.pcurve().clone(),
                    range: coedge.range(),
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        if reverse {
            segments.reverse();
            for segment in &mut segments {
                segment.range = segment.range.reversed();
                segment.provenance = segment.provenance.reversed();
            }
        }
        loops.push(BrepPatchLoop {
            kind: loop_value.kind(),
            segments,
        });
    }
    Ok(BrepPatch {
        preserved: if reverse {
            Vec::new()
        } else {
            vec![face.name()]
        },
        sources: vec![face.name()],
        surface: face.surface().clone(),
        orientation: if reverse {
            face.orientation().reversed()
        } else {
            face.orientation()
        },
        loops,
    })
}

fn loop_patch(
    topology: &Topology,
    loop_id: super::LoopId,
    output_kind: super::LoopKind,
) -> KernelResult<BrepPatchLoop> {
    let loop_value = topology.loop_(loop_id)?;
    let mut segments = topology
        .loop_coedges(loop_id)?
        .into_iter()
        .map(|coedge_id| {
            let coedge = topology.coedge(coedge_id)?;
            Ok(BrepPatchSegment {
                provenance: SegmentProvenance::from_coedge(topology, coedge_id)?,
                curve: topology.edge(coedge.edge())?.curve().clone(),
                pcurve: coedge.pcurve().clone(),
                range: coedge.range(),
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    if loop_value.kind() != output_kind {
        segments.reverse();
        for segment in &mut segments {
            segment.range = segment.range.reversed();
            segment.provenance = segment.provenance.reversed();
        }
    }
    Ok(BrepPatchLoop {
        kind: output_kind,
        segments,
    })
}

fn rim_segment_points(segment: &BrepPatchSegment) -> KernelResult<[Point3; 3]> {
    Ok([
        segment.curve.evaluate(segment.range.start)?.point,
        segment
            .curve
            .evaluate(segment.range.start + segment.range.span() * 0.5)?
            .point,
        segment.curve.evaluate(segment.range.end)?.point,
    ])
}

fn split_rim_segments(
    segments: impl IntoIterator<Item = BrepPatchSegment>,
    tolerance: super::ModelTolerance,
) -> KernelResult<Vec<BrepPatchSegment>> {
    let segments = segments.into_iter().collect::<Vec<_>>();
    let endpoints = segments
        .iter()
        .map(rim_segment_points)
        .collect::<KernelResult<Vec<_>>>()?;
    let linear = tolerance.linear * 16.0;
    let mut result = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let mut parameters = vec![segment.range.start, segment.range.end];
        for (other_index, points) in endpoints.iter().enumerate() {
            if other_index == index {
                continue;
            }
            for point in [points[0], points[2]] {
                let Ok(projection) = segment.curve.project(point, tolerance) else {
                    continue;
                };
                if projection.distance > linear {
                    continue;
                }
                let parameter = segment
                    .curve
                    .domain()
                    .nearest(projection.parameter, segment.range.middle());
                if segment.range.contains(parameter, tolerance) {
                    parameters.push(parameter);
                }
            }
        }
        parameters.sort_by(|first, second| {
            if segment.range.span() > 0.0 {
                first.total_cmp(second)
            } else {
                second.total_cmp(first)
            }
        });
        parameters.dedup_by(|first, second| (*first - *second).abs() <= tolerance.parameter * 16.0);
        for bounds in parameters.windows(2) {
            if (bounds[1] - bounds[0]).abs() <= tolerance.parameter * 16.0 {
                continue;
            }
            result.push(BrepPatchSegment {
                provenance: segment
                    .provenance
                    .trimmed(segment.range, ParameterRange::new(bounds[0], bounds[1])?),
                curve: segment.curve.clone(),
                pcurve: segment.pcurve.clone(),
                range: ParameterRange::new(bounds[0], bounds[1])?,
            });
        }
    }
    Ok(result)
}

fn rim_piece_matches(
    first: &BrepPatchSegment,
    second: &BrepPatchSegment,
    tolerance: super::ModelTolerance,
) -> KernelResult<bool> {
    let first = rim_segment_points(first)?;
    let second = rim_segment_points(second)?;
    let linear = tolerance.linear * 64.0;
    Ok(first[1].distance(second[1]) <= linear
        && ((first[0].distance(second[0]) <= linear && first[2].distance(second[2]) <= linear)
            || (first[0].distance(second[2]) <= linear && first[2].distance(second[0]) <= linear)))
}

fn rim_face_patch(
    surface: &Surface3,
    orientation: FaceOrientation,
    outer: BrepPatchLoop,
    inner: BrepPatchLoop,
    tolerance: super::ModelTolerance,
) -> KernelResult<Option<BrepPatch>> {
    let original = [outer.clone(), inner.clone()];
    let pieces = split_rim_segments(outer.segments.into_iter().chain(inner.segments), tolerance)?;
    let mut cancelled = vec![false; pieces.len()];
    let mut removed = false;
    for first in 0..pieces.len() {
        if cancelled[first] {
            continue;
        }
        let first_points = rim_segment_points(&pieces[first])?;
        for second in first + 1..pieces.len() {
            if cancelled[second] || !rim_piece_matches(&pieces[first], &pieces[second], tolerance)?
            {
                continue;
            }
            let second_points = rim_segment_points(&pieces[second])?;
            if first_points[0].distance(second_points[2]) <= tolerance.linear * 64.0
                && first_points[2].distance(second_points[0]) <= tolerance.linear * 64.0
            {
                cancelled[first] = true;
                cancelled[second] = true;
                removed = true;
                break;
            }
        }
    }
    if !removed {
        return Ok(Some(BrepPatch {
            preserved: Vec::new(),
            sources: Vec::new(),
            surface: surface.clone(),
            orientation,
            loops: original.into(),
        }));
    }

    let mut unused = pieces
        .into_iter()
        .enumerate()
        .filter_map(|(index, segment)| (!cancelled[index]).then_some(segment))
        .collect::<Vec<_>>();
    if unused.is_empty() {
        return Ok(None);
    }
    let linear = tolerance.linear * 64.0;
    let mut loops = Vec::new();
    while let Some(first) = unused.pop() {
        let start = rim_segment_points(&first)?[0];
        let mut end = rim_segment_points(&first)?[2];
        let mut segments = vec![first];
        while end.distance(start) > linear {
            let Some((index, reverse)) =
                unused.iter().enumerate().find_map(|(index, candidate)| {
                    let points = rim_segment_points(candidate).ok()?;
                    if end.distance(points[0]) <= linear {
                        Some((index, false))
                    } else if end.distance(points[2]) <= linear {
                        Some((index, true))
                    } else {
                        None
                    }
                })
            else {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "an analytical shell opening rim did not form a closed boundary",
                ));
            };
            let mut next = unused.swap_remove(index);
            if reverse {
                next.range = next.range.reversed();
                next.provenance = next.provenance.reversed();
            }
            end = rim_segment_points(&next)?[2];
            segments.push(next);
        }
        let area = segments.iter().try_fold(0.0, |sum, segment| {
            segment
                .pcurve
                .signed_area_contribution(segment.range, tolerance.parameter)
                .map(|area| sum + area)
        })?;
        let orientation_sign = match orientation {
            FaceOrientation::Forward => 1.0,
            FaceOrientation::Reversed => -1.0,
        };
        loops.push(BrepPatchLoop {
            kind: if area * orientation_sign > 0.0 {
                super::LoopKind::Outer
            } else {
                super::LoopKind::Inner
            },
            segments,
        });
    }
    if loops
        .iter()
        .filter(|loop_| loop_.kind == super::LoopKind::Outer)
        .count()
        != 1
    {
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "an analytical shell opening rim must have one outer boundary",
        ));
    }
    Ok(Some(BrepPatch {
        preserved: Vec::new(),
        sources: Vec::new(),
        surface: surface.clone(),
        orientation,
        loops,
    }))
}

/// Build the shell boundary directly from the source skin, the exact offset
/// skin, and one planar/analytical rim for every removed-face boundary.  A
/// generic Boolean is deliberately unnecessary here: these patches are already
/// the complete boundary representation of a shell operation.
fn assemble_shell_boundary(
    source: &Compound,
    raw_offset: &Compound,
    openings: &BTreeSet<FaceId>,
    distance: f64,
) -> KernelResult<Compound> {
    let namespace = super::identity::fresh_uuid();
    let mut tokens = BTreeMap::new();
    let mut tag = |patch: &mut BrepPatch, role: super::CreationRole| {
        let name = super::PersistentId { namespace, local: tokens.len() as u64 };
        tokens.insert(name, role);
        patch.sources.push(name);
    };
    let source_topology = source.topology();
    let offset_topology = raw_offset.topology();
    let source_by_name = source
        .faces()
        .map(|face| (face.persistent_id(), face.topology_id()))
        .collect::<BTreeMap<_, _>>();
    let offset_by_source = raw_offset
        .faces()
        .map(|face| {
            let source_name = raw_offset
                .topology_history()
                .primary_source(face.persistent_id());
            (source_by_name[&source_name], face.topology_id())
        })
        .collect::<BTreeMap<_, _>>();

    let source_shells = source
        .faces()
        .map(|face| {
            source_topology
                .face(face.topology_id())
                .map(|value| value.shell())
        })
        .collect::<KernelResult<BTreeSet<_>>>()?;
    let direct_closed_pair = source_shells.len() == 1 && openings.is_empty();
    let mut groups = Vec::<Vec<BrepPatch>>::new();
    for shell_id in source_shells {
        let shell_faces = source_topology.shell(shell_id)?.faces();
        let shell_openings = shell_faces
            .iter()
            .copied()
            .filter(|face| openings.contains(face))
            .collect::<Vec<_>>();

        let source_skin = shell_faces
            .iter()
            .copied()
            .filter(|face| !openings.contains(face))
            .map(|face| {
                let mut patch = face_patch(source_topology, face, distance > 0.0)?;
                tag(&mut patch, super::CreationRole::ShellSource);
                Ok(patch)
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let offset_skin = raw_offset.faces()
            .filter(|face| {
                let name = raw_offset.topology_history().primary_source(face.persistent_id());
                let source_face = source_by_name[&name];
                shell_faces.contains(&source_face) && !openings.contains(&source_face)
            })
            .map(|face| {
                let mut patch = face_patch(offset_topology, face.topology_id(), distance < 0.0)?;
                tag(&mut patch, super::CreationRole::ShellOffset);
                Ok(patch)
            })
            .collect::<KernelResult<Vec<_>>>()?;

        if shell_openings.is_empty() {
            groups.push(source_skin);
            groups.push(offset_skin);
            continue;
        }

        let mut connected = source_skin;
        connected.extend(offset_skin);
        for source_face_id in shell_openings {
            let source_face = source_topology.face(source_face_id)?;
            let offset_face = offset_topology.face(offset_by_source[&source_face_id])?;
            if source_face.loops().len() != offset_face.loops().len() {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "an opening face and its offset have different loop counts",
                ));
            }
            for (source_loop, offset_loop) in source_face
                .loops()
                .iter()
                .copied()
                .zip(offset_face.loops().iter().copied())
            {
                let kind = source_topology.loop_(source_loop)?.kind();
                let source_is_outer = (distance < 0.0) == (kind == super::LoopKind::Outer);
                let (outer_loop, inner_loop) = if source_is_outer {
                    (
                        loop_patch(source_topology, source_loop, super::LoopKind::Outer)?,
                        loop_patch(offset_topology, offset_loop, super::LoopKind::Inner)?,
                    )
                } else {
                    (
                        loop_patch(offset_topology, offset_loop, super::LoopKind::Outer)?,
                        loop_patch(source_topology, source_loop, super::LoopKind::Inner)?,
                    )
                };
                if let Some(mut rim) = rim_face_patch(
                    source_face.surface(),
                    source_face.orientation(),
                    outer_loop,
                    inner_loop,
                    source.tolerance(),
                )? {
                    rim.sources.push(source_face.name());
                    tag(&mut rim, super::CreationRole::ShellRim);
                    connected.push(rim);
                }
            }
        }
        groups.push(connected);
    }

    let mut kernel = Kernel::new(source.tolerance())?;
    let mut transaction = kernel.begin_operation();
    let mut bodies = Vec::new();
    for patch in groups.iter_mut().flatten() {
        let mut preserved = Vec::new();
        let mut sources = Vec::new();
        for name in &patch.sources {
            if let Some(records) = raw_offset.topology_history().construction.get(name) {
                for record in records {
                    sources.push(record.name);
                    if patch.preserved.contains(name) && record.unchanged {
                        preserved.push(record.name);
                    }
                }
            } else {
                sources.push(*name);
                if patch.preserved.contains(name) {
                    preserved.push(*name);
                }
            }
        }
        patch.sources = sources;
        patch.preserved = preserved;
        for segment in patch.loops.iter_mut().flat_map(|l| &mut l.segments) {
            segment.provenance.through(raw_offset.topology_history());
        }
    }
    if direct_closed_pair {
        let mut groups = groups.into_iter();
        let source_patches = groups
            .next()
            .expect("a closed source shell has a source skin");
        let offset_patches = groups
            .next()
            .expect("a closed source shell has an offset skin");
        debug_assert!(groups.next().is_none());
        let source_body = transaction.brep_shell(&source_patches)?;
        let offset_body = transaction.brep_shell(&offset_patches)?;
        let (outer_body, cavity_body) = if distance < 0.0 {
            (source_body, offset_body)
        } else {
            (offset_body, source_body)
        };
        transaction.merge_shell_into_body(outer_body, cavity_body)?;
        bodies.push(outer_body);
    } else {
        for patches in groups {
            bodies.push(transaction.brep_shell(&patches)?);
        }
        transaction.nest_brep_shells(&bodies)?;
    }
    let committed = transaction.finish()?;
    Ok(Compound::from_operation(kernel, committed, [])?.with_patch_roles(&tokens))
}

// Solve contact-normal constraints with a minimum-length solution in the
// unconstrained directions. Reorthogonalization also handles repeated normals
// along straight contact edges without making the system singular.
fn solve_blend_contacts(
    equations: &[([f64; 4], f64)],
    change_radius: bool,
    tolerance: super::ModelTolerance,
) -> Option<[f64; 4]> {
    let mut basis = Vec::<([f64; 4], f64)>::new();
    for &(mut row, mut rhs) in equations {
        if !change_radius {
            row[3] = 0.0;
        }
        for _ in 0..2 {
            for (normal, value) in &basis {
                let projection: f64 = row.iter().zip(normal).map(|(a, b)| a * b).sum();
                for i in 0..4 {
                    row[i] -= projection * normal[i];
                }
                rhs -= projection * value;
            }
        }
        let length = row.iter().map(|value| value * value).sum::<f64>().sqrt();
        if length <= tolerance.angular * 8.0 {
            if rhs.abs() > tolerance.linear * 8.0 {
                return None;
            }
        } else {
            for value in &mut row {
                *value /= length;
            }
            basis.push((row, rhs / length));
        }
    }
    let mut result = [0.0; 4];
    for (normal, value) in basis {
        for i in 0..4 {
            result[i] += normal[i] * value;
        }
    }
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

fn adjust_offset_blends(
    source: &Compound,
    retained: &BTreeSet<FaceId>,
    distance: f64,
    supports: &mut BTreeMap<FaceId, Surface3>,
) -> KernelResult<BTreeSet<FaceId>> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let mut replacements = Vec::new();
    for face_id in retained {
        let face = topology.face(*face_id)?;
        if matches!(face.surface(), Surface3::Plane(_)) {
            continue;
        }
        let edges = topology.face_edges(*face_id)?;
        let adjacent = edges
            .iter()
            .map(|edge| topology.edge_faces(*edge))
            .collect::<KernelResult<Vec<_>>>()?;
        if adjacent.iter().flatten().all(|id| retained.contains(id)) {
            continue;
        }
        let orientation = face.orientation().sign();
        let mut neighbors = BTreeSet::new();
        let mut contacts = Vec::new();
        for (edge_id, adjacent) in edges.into_iter().zip(adjacent) {
            let Some(&neighbor) = adjacent.iter().find(|id| *id != face_id) else {
                continue;
            };
            let samples = tangent_contact(topology, edge_id, *face_id, neighbor, tolerance)?;
            if !samples.is_empty() {
                neighbors.insert(neighbor);
                contacts.extend(samples);
            }
        }
        if neighbors.len() < 2 || neighbors.iter().all(|id| retained.contains(id)) {
            continue;
        }
        let equations = contacts
            .iter()
            .map(|(neighbor, _, normal)| {
                (
                    [normal.x, normal.y, normal.z, 1.0],
                    if retained.contains(neighbor) {
                        0.0
                    } else {
                        distance
                    },
                )
            })
            .collect::<Vec<_>>();
        // Prefer preserving the blend radius. When varying contact normals
        // prevent a translation alone, also solve for a uniform normal offset.
        let adjustment = solve_blend_contacts(&equations, false, tolerance)
            .or_else(|| solve_blend_contacts(&equations, true, tolerance))
            .ok_or_else(|| not_implemented("blend adjustment for incompatible face offsets"))?;
        let translation = crate::Vec3::new(adjustment[0], adjustment[1], adjustment[2]);
        let offset = adjustment[3];
        let surface = if offset.abs() > tolerance.linear {
            supports[face_id].offset(offset * orientation, tolerance)?
        } else {
            supports[face_id].clone()
        };
        let surface = super::transform::transform_surface(
            surface,
            RigidTransform3::new(crate::Matrix3::identity(), translation, tolerance)?,
            tolerance,
        )?;
        // The normal equations are only a candidate construction on curved
        // supports. Check the actual moved contacts against the exact target
        // surfaces before allowing topology reconstruction to publish a solid.
        for (neighbor, point, normal) in contacts {
            let moved = point + normal * offset + translation;
            let target = supports.get(&neighbor).ok_or_else(|| not_implemented(
                "tangent blend adjustment against a collapsed offset support",
            ))?;
            let projection = target.project(moved, tolerance)?;
            let allowed = tolerance.linear_at_scale(moved.to_vec().magnitude()) * 8.0;
            let sign = topology.face(neighbor)?.orientation().sign();
            let target_normal =
                target.normal(projection.parameters.0, projection.parameters.1)? * sign;
            if projection.distance > allowed
                || (target_normal - normal).magnitude() > tolerance.angular * 8.0
            {
                return Err(not_implemented(
                    "blend adjustment that cannot preserve tangent contacts",
                ));
            }
        }
        replacements.push((*face_id, surface));
    }
    let changed = replacements.iter().map(|(id, _)| *id).collect();
    supports.extend(replacements);
    Ok(changed)
}

pub(crate) fn offset_faces(
    source: &Compound,
    faces: &[FaceHandle],
    distance: f64,
) -> KernelResult<Compound> {
    let selected = faces
        .iter()
        .map(FaceHandle::topology_id)
        .collect::<BTreeSet<_>>();
    let retained = source
        .faces()
        .filter(|face| !selected.contains(&face.topology_id()))
        .map(|face| face.topology_id())
        .collect();
    // Rebuild only the offset skin; shelling additionally adds the source skin
    // and opening rims to create a wall.
    let (result, _) = offset_body(source, &retained, distance, true)?;
    if result.solids().len() != 1 || result.volume() <= source.tolerance().linear.powi(3) {
        return Err(invalid("face offset must preserve a nonempty solid"));
    }
    result.with_feature_parent(source, super::DerivationRole::OffsetFaces)
}

fn rounded_join_center(
    topology: &Topology,
    edge_id: EdgeId,
    tolerance: super::ModelTolerance,
) -> KernelResult<Curve3> {
    let edge = topology.edge(edge_id)?;
    let Curve3::SurfaceIntersection(intersection) = edge.curve() else { return Ok(edge.curve().clone()); };
    if edge.vertices()[0] == edge.vertices()[1] { return Ok(edge.curve().clone()); }
    let [start, end] = edge.vertices().map(|vertex| {
        let faces = topology.vertex_edges(vertex)?.into_iter().map(|edge| topology.edge_faces(edge))
            .collect::<KernelResult<Vec<_>>>()?.into_iter().flatten().collect::<BTreeSet<_>>();
        let constraints = faces.iter().map(|face| Ok(CornerConstraint::Surface(topology.face(*face)?.surface(), None)))
            .collect::<KernelResult<Vec<_>>>()?;
        // Tangential intersections amplify a model-tolerance residual into a
        // visible trim gap. Refine the shared center before constructing joins.
        let mut exact = tolerance;
        exact.linear = 1.0e-12;
        exact.relative = 1.0e-14;
        refine_support_corner(topology.vertex(vertex)?.point(), &constraints, exact)
    });
    let mut corners = [start?, end?];
    let range = edge.range();
    let mut parameters = vec![range.start, range.end];
    parameters.extend(intersection.first_pcurve().integration_breakpoints(range));
    parameters.extend(intersection.second_pcurve().integration_breakpoints(range));
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
    if range.span() < 0.0 { corners.swap(0, 1); }
    let exact = intersection.exact_pcurves();
    let guide = |curve: &Curve2, surface: &Surface3| -> KernelResult<Curve2> {
        let mut points = parameters.iter().map(|parameter| curve.evaluate(*parameter).map(|value| value.point))
            .collect::<KernelResult<Vec<_>>>()?;
        for (index, point) in [(0, corners[0]), (points.len() - 1, corners[1])] {
            let uv = surface.project(point, tolerance)?.parameters;
            points[index] = Point2::new(uv.0, uv.1);
        }
        super::intersection::unwrap_periodic_points(&mut points, surface.domains());
        let sampled = super::intersection::interpolating_nurbs_guide(&points, &parameters)?;
        super::intersection::smooth_interpolating_guide(&sampled)
    };
    // Reparameterize only the retained edge interval. Its untrimmed support
    // can extend past either shared corner and has different endpoints.
    Ok(SurfaceIntersectionCurve::new(intersection.first_surface().clone(), intersection.second_surface().clone(),
        guide(&exact[0], intersection.first_surface())?, guide(&exact[1], intersection.second_surface())?, range,
        tolerance, intersection.achieved_error())?.into())
}

fn reparameterize_join_intersection(
    intersection: super::SurfaceSurfaceCurve,
    domains: [[ParameterRange; 2]; 2],
    tolerance: super::ModelTolerance,
) -> KernelResult<super::SurfaceSurfaceCurve> {
    let Curve3::SurfaceIntersection(curve) = &intersection.curve else { return Ok(intersection); };
    let range = curve.range();
    let exact = curve.exact_pcurves();
    let start = exact[0].evaluate(range.start)?.point;
    let end = exact[0].evaluate(range.end)?.point;
    if (start.y - end.y).abs() < 1.0 - tolerance.parameter * 64.0 { return Ok(intersection); }
    // A contact rail is a second intersection branch at the end of a join.
    // Continue across the join angle, rather than allowing the guide's gauge
    // to flatten onto that rail near a consumed fillet.
    let mut points = [vec![start], vec![exact[1].evaluate(range.start)?.point]];
    for index in 1..64 {
        let fraction = index as f64 / 64.0;
        let v = start.y + (end.y - start.y) * fraction;
        let mut low = range.start;
        let mut high = range.end;
        for _ in 0..32 {
            let middle = (low + high) * 0.5;
            let value = curve.first_pcurve().evaluate(middle)?.point.y;
            if (value < v) == (start.y < end.y) { low = middle; } else { high = middle; }
        }
        let parameter = (low + high) * 0.5;
        let a = curve.first_pcurve().evaluate(parameter)?.point;
        let b = curve.second_pcurve().evaluate(parameter)?.point;
        let uv = super::intersection::correct_surface_pair_with_gauge(curve.first_surface(), curve.second_surface(),
                [a.x, v, b.x, b.y], [0.0, 1.0, 0.0, 0.0], domains[0], domains[1], tolerance)?
                .ok_or_else(|| invalid("join intersection could not continue across its contact angle"))?;
        points[0].push(Point2::new(uv[0], uv[1]));
        points[1].push(Point2::new(uv[2], uv[3]));
    }
    points[0].push(end);
    points[1].push(exact[1].evaluate(range.end)?.point);
    let rebuilt = SurfaceIntersectionCurve::new(curve.first_surface().clone(), curve.second_surface().clone(),
        super::intersection::smooth_nurbs_guide(&points[0], range)?.into(),
        super::intersection::smooth_nurbs_guide(&points[1], range)?.into(), range, tolerance, curve.achieved_error())?;
    let exact = rebuilt.exact_pcurves();
    Ok(super::SurfaceSurfaceCurve { curve: rebuilt.into(), first_pcurve: exact[0].clone(),
        second_pcurve: exact[1].clone(), achieved_error: intersection.achieved_error })
}

/// Arrange offset supports and optional rolling joins when their boundary graph
/// changes. Intersections between newly adjacent faces trim overlaps after a
/// smaller blend disappears or opposing offset caps cross.
fn offset_regions_with_joins(
    source: &Compound,
    offset: Option<&Compound>,
    joins: &[EdgeId],
    openings: &BTreeSet<FaceId>,
    radius: f64,
    sharp: bool,
) -> KernelResult<Compound> {
    use super::trim::{FaceCurve, align_pcurve_to_face_chart, face_boundary_curves, intersection_chart_copies,
        face_parameter_bounds as trim_bounds, point_in_trim, trim_to_faces};
    use super::brep_classification::{BrepClassifier, PointClassification};
    let tolerance = source.tolerance();
    let topology = source.topology();
    let skin = offset.unwrap_or(source);
    let skin_topology = skin.topology();
    let mut skin_faces = Vec::new();
    let mut supports = Vec::new();
    let mut orientations = Vec::new();
    let mut ancestry = Vec::new();
    let mut boundaries = Vec::new();
    for face in skin.faces() {
        let value = skin_topology.face(face.topology_id())?;
        let surface = if offset.is_some() || openings.contains(&face.topology_id()) {
            face.surface().clone()
        } else {
            let sign = value.orientation().sign();
            let bounds = face_parameter_bounds(topology, face.topology_id(), tolerance)?;
            let base = extended_support(face.surface(), bounds, -radius, tolerance)?;
            let distance = -radius * sign;
            if collapsed_support(&base, distance, tolerance)?.is_some() {
                continue;
            }
            base.offset(distance, tolerance)?
        };
        let boundary = face_boundary_curves(skin_topology, face.topology_id(), tolerance)?.into_iter().map(|mut curve| {
            let mut provenance = SegmentProvenance::from_coedge(skin_topology, curve.provenance)?;
            if let Some(offset) = offset {
                provenance.through(offset.topology_history());
            } else {
                provenance = provenance.modified();
                curve.curve = surface.boundary_curve(curve.pcurve.clone(), curve.range, tolerance)?;
            }
            Ok(curve.with_provenance(provenance))
        }).collect::<KernelResult<Vec<_>>>()?;
        ancestry.push(vec![offset.map_or(face.persistent_id(), |offset| {
            offset.topology_history().primary_source(face.persistent_id())
        })]);
        orientations.push(value.orientation());
        supports.push(surface);
        boundaries.push(boundary);
        skin_faces.push(face.clone());
    }
    let by_source = ancestry.iter().enumerate().map(|(index, names)| (names[0], index)).collect::<BTreeMap<_, _>>();
    let mut curves = boundaries.clone();
    let mut parents = Vec::new();
    let candidate = |curve: Curve3, pcurve: Curve2, range: ParameterRange, seam: bool| FaceCurve {
        provenance: SegmentProvenance::default(), curve, pcurve, range,
        loop_end: false, loop_kind: super::LoopKind::Outer, parameter_seam: seam,
    };
    let mut clipping_boundaries = boundaries.clone();
    if offset.is_none() {
        let mut extended_faces = BTreeSet::new();
        let surviving = skin_faces.iter().map(FaceHandle::topology_id).collect::<BTreeSet<_>>();
        let mut neighbors = BTreeMap::new();
        for face in &skin_faces {
            let mut pending = vec![face.topology_id()];
            let mut visited = BTreeSet::new();
            let mut adjacent = BTreeSet::new();
            while let Some(id) = pending.pop() {
                if !visited.insert(id) { continue; }
                if id != face.topology_id() { extended_faces.insert(face.topology_id()); }
                for edge in topology.face_edges(id)? {
                    for neighbor in topology.edge_faces(edge)? {
                        if neighbor == face.topology_id() { continue; }
                        if surviving.contains(&neighbor) { adjacent.insert(neighbor); }
                        else { pending.push(neighbor); }
                    }
                }
            }
            neighbors.insert(face.topology_id(), adjacent);
        }
        for (index, face) in skin_faces.iter().enumerate() {
            if !sharp && !extended_faces.contains(&face.topology_id()) { continue; }
            let original = trim_bounds(&boundaries[index], tolerance)?;
            let mut bounds = expanded_offset_bounds(&supports[index], original, -radius, tolerance)?;
            // Surviving tangent neighbors retain their isoparametric contact
            // rails. Expand across consumed blends, while keeping these chart
            // limits so an intersection stops at the shared corner.
            for edge_id in topology.face_edges(face.topology_id())? {
                let adjacent = topology.edge_faces(edge_id)?;
                let Some(other) = adjacent.into_iter().find(|id| *id != face.topology_id() && surviving.contains(id)) else { continue; };
                if tangent_contact(topology, edge_id, face.topology_id(), other, tolerance)?.is_empty() { continue; }
                let edge = topology.edge(edge_id)?;
                let coedge = edge.coedges().iter().map(|id| topology.coedge(*id)).collect::<KernelResult<Vec<_>>>()?
                    .into_iter().find(|coedge| topology.loop_(coedge.loop_id()).unwrap().face() == face.topology_id()).unwrap();
                let range = coedge.range();
                let points = [range.start, range.start + range.span() * 0.5, range.end].map(|t| coedge.pcurve().evaluate(t).map(|v|v.point));
                let points = points.into_iter().collect::<KernelResult<Vec<_>>>()?;
                for axis in 0..2 {
                    if points.iter().all(|point| (point[axis] - original[axis].start).abs() <= tolerance.parameter * 64.0) { bounds[axis].start = original[axis].start; }
                    if points.iter().all(|point| (point[axis] - original[axis].end).abs() <= tolerance.parameter * 64.0) { bounds[axis].end = original[axis].end; }
                }
            }
            let points = [Point2::new(bounds[0].start, bounds[1].start),
                Point2::new(bounds[0].end, bounds[1].start), Point2::new(bounds[0].end, bounds[1].end),
                Point2::new(bounds[0].start, bounds[1].end)];
            clipping_boundaries[index] = (0..4).map(|side| {
                let pcurve: Curve2 = super::Line2::new(points[side], points[(side + 1) % 4] - points[side])?.into();
                let range = ParameterRange::new(0.0, 1.0)?;
                Ok(candidate(supports[index].boundary_curve(pcurve.clone(), range, tolerance)?, pcurve, range, false))
            }).collect::<KernelResult<Vec<_>>>()?;
            clipping_boundaries[index].last_mut().unwrap().loop_end = true;
        }
        for first in 0..supports.len() {
            for second in 0..first {
                let first_id = skin_faces[first].topology_id();
                let second_id = skin_faces[second].topology_id();
                let shared = topology.face_edges(first_id)?.into_iter().find(|edge| {
                    topology.edge_faces(*edge).unwrap().contains(&second_id)
                });
                if !sharp && !neighbors[&first_id].contains(&second_id)
                    && !extended_faces.contains(&first_id) && !extended_faces.contains(&second_id) { continue; }
                if let Some(edge) = shared {
                    if !sharp && (joins.contains(&edge) || !tangent_contact(topology, edge, first_id, second_id, tolerance)?.is_empty()) { continue; }
                }
                let intersections = supports[first].intersect_surface_in_bounds(&supports[second],
                    trim_bounds(&clipping_boundaries[first], tolerance)?, trim_bounds(&clipping_boundaries[second], tolerance)?, tolerance)?;
                let IntersectionSet::Isolated(intersections) = intersections else { continue; };
                for intersection in intersections {
                    let first_pcurve = align_pcurve_to_face_chart(&intersection.first_pcurve, None,
                        &supports[first], &boundaries[first], tolerance)?;
                    let second_pcurve = align_pcurve_to_face_chart(&intersection.second_pcurve, None,
                        &supports[second], &boundaries[second], tolerance)?;
                    let first_copies = intersection_chart_copies(intersection.curve.domain().bounds, &first_pcurve,
                        &supports[first], trim_bounds(&boundaries[first], tolerance)?, tolerance)?;
                    let second_copies = intersection_chart_copies(intersection.curve.domain().bounds, &second_pcurve,
                        &supports[second], trim_bounds(&boundaries[second], tolerance)?, tolerance)?;
                    for first_pcurve in &first_copies {
                        for second_pcurve in &second_copies {
                            for range in trim_to_faces(&intersection.curve, first_pcurve, &clipping_boundaries[first],
                                second_pcurve, &clipping_boundaries[second], tolerance)? {
                                curves[first].push(candidate(intersection.curve.clone(), first_pcurve.clone(), range, false));
                                curves[second].push(candidate(intersection.curve.clone(), second_pcurve.clone(), range, false));
                            }
                        }
                    }
                }
            }
        }
    }
    for edge_id in joins.iter().filter(|_| !sharp) {
        let edge = topology.edge(*edge_id)?;
        let uses = edge.coedges().iter().map(|id| topology.coedge(*id)).collect::<KernelResult<Vec<_>>>()?;
        let faces = topology.edge_faces(*edge_id)?;
        let parent_names = faces.iter().map(|id| topology.face(*id).map(|face| face.name())).collect::<KernelResult<Vec<_>>>()?;
        // Contact guides must follow the coedge order used by the center curve.
        let parent_indices = uses.iter().map(|coedge| {
            Ok(by_source.get(&topology.face(topology.loop_(coedge.loop_id())?.face())?.name()).copied())
        }).collect::<KernelResult<Vec<_>>>()?;
        let contact_surfaces = uses.iter().zip(&parent_indices).map(|(coedge, index)| {
            if let Some(index) = index { return Ok(supports[*index].clone()); }
            let face = topology.face(topology.loop_(coedge.loop_id())?.face())?;
            let sign = face.orientation().sign();
            // A consumed face still defines the contact direction of its
            // adjacent concave-edge envelope. Keep that continuation solely
            // as a rail support; it cannot contribute an offset face.
            super::OffsetSurface3::new(face.surface().clone(), -radius * sign, tolerance).map(Surface3::Offset)
        }).collect::<KernelResult<Vec<_>>>()?;
        let range = edge.range();
        let closed = edge.vertices()[0] == edge.vertices()[1];
        let center = if offset.is_none() {
            rounded_join_center(topology, *edge_id, tolerance)?
        } else { edge.curve().clone() };
        let contact_guides = uses.iter().map(|coedge| {
            if offset.is_some() { return Ok(coedge.pcurve().clone()); }
            let face = topology.face(topology.loop_(coedge.loop_id())?.face())?;
            Ok(super::SurfaceProjectionPcurve2::new(center.clone(), face.surface().clone(),
                coedge.pcurve().clone(), range, tolerance)?.into())
        }).collect::<KernelResult<Vec<Curve2>>>()?;
        let blend: Surface3 = super::RollingBallSurface3::new(
            center.clone(), contact_surfaces[0].clone(), contact_surfaces[1].clone(),
            contact_guides[0].clone(), contact_guides[1].clone(), range, radius, tolerance,
        )?.with_center_period(closed.then_some(range.span().abs())).into();
        let mut boundary = Vec::new();
        for (start, end, trim, seam) in [
            (Point2::new(0.0, 0.0), Point2::new(1.0, 0.0), range, false),
            (Point2::new(range.end, 0.0), Point2::new(range.end, 1.0), ParameterRange::new(0.0, 1.0)?, closed),
            (Point2::new(0.0, 1.0), Point2::new(1.0, 1.0), range.reversed(), false),
            (Point2::new(range.start, 0.0), Point2::new(range.start, 1.0), ParameterRange::new(1.0, 0.0)?, closed),
        ] {
            let pcurve: Curve2 = super::Line2::new(start, end - start)?.into();
            let curve = blend.boundary_curve(pcurve.clone(), trim, tolerance)?;
            boundary.push(candidate(curve, pcurve, trim, seam));
        }
        boundary.last_mut().unwrap().loop_end = true;
        if offset.is_none() {
            for (side, coedge) in uses.iter().enumerate() {
                if parent_indices[side].is_some() { continue; }
                let face_id = topology.loop_(coedge.loop_id())?.face();
                let face = topology.face(face_id)?;
                let sign = face.orientation().sign();
                if !matches!(collapsed_support(face.surface(), -radius * sign, tolerance)?, Some((_, false))) { continue; }
                // At exact collapse the join's constructive contact rail lies
                // on the surviving tangent support. Share that rail directly;
                // projecting a different curve onto the singular join corner
                // gives an unstable p-curve even when the points agree.
                for contact in tangent_neighbors(topology, face_id, tolerance, |_| true)? {
                    let Some(&parent) = by_source.get(&topology.face(contact)?.name()) else { continue; };
                    let rail = &boundary[if side == 0 { 0 } else { 2 }];
                    let points = (0..=16).map(|index| {
                        let point = rail.curve.evaluate(range.start + range.span() * index as f64 / 16.0)?.point;
                        let uv = supports[parent].project(point, tolerance)?.parameters;
                        Ok(Point2::new(uv.0, uv.1))
                    }).collect::<KernelResult<Vec<_>>>()?;
                    let guide = super::intersection::smooth_nurbs_guide(&points, range)?.into();
                    let pcurve = SurfaceProjectionPcurve2::new(rail.curve.clone(), supports[parent].clone(), guide, range, tolerance)?.into();
                    let pcurve = align_pcurve_to_face_chart(&pcurve, Some(range), &supports[parent], &boundaries[parent], tolerance)?;
                    curves[parent].insert(0, candidate(rail.curve.clone(), pcurve, range, false));
                }
            }
        }
        if offset.is_some() {
            for side in 0..2 {
                let parent = parent_indices[side].unwrap();
                let rail = &boundary[if side == 0 { 0 } else { 2 }];
                let pcurve = align_pcurve_to_face_chart(&contact_guides[side], Some(range),
                    &supports[parent], &boundaries[parent], tolerance)?;
                curves[parent].push(candidate(rail.curve.clone(), pcurve, range, false));
            }
        }
        parents.push(parent_indices.into_iter().flatten().collect::<Vec<_>>());
        supports.push(blend);
        ancestry.push(parent_names);
        curves.push(boundary.clone());
        boundaries.push(boundary);
    }
    let base_count = skin_faces.len();
    for first in base_count..supports.len() {
        for second in 0..first {
            if parents[first - base_count].contains(&second) { continue; }
            let intersections = supports[first].intersect_surface_in_bounds(&supports[second],
                trim_bounds(&boundaries[first], tolerance)?, trim_bounds(&boundaries[second], tolerance)?, tolerance)?;
            let IntersectionSet::Isolated(intersections) = intersections else { continue; };
            for intersection in intersections {
                let intersection = reparameterize_join_intersection(intersection,
                    [trim_bounds(&boundaries[first], tolerance)?, trim_bounds(&boundaries[second], tolerance)?], tolerance)?;
                let first_pcurve = align_pcurve_to_face_chart(&intersection.first_pcurve, None,
                    &supports[first], &boundaries[first], tolerance)?;
                let second_pcurve = align_pcurve_to_face_chart(&intersection.second_pcurve, None,
                    &supports[second], &boundaries[second], tolerance)?;
                // The join can extend beyond the old offset trim. Clip to its
                // own chart here; the source-distance classifier replaces the
                // competing support's old boundary during arrangement.
                for range in trim_to_faces(&intersection.curve, &first_pcurve, &boundaries[first],
                    &first_pcurve, &boundaries[first], tolerance)? {
                    curves[first].push(candidate(intersection.curve.clone(), first_pcurve.clone(), range, false));
                    curves[second].push(candidate(intersection.curve.clone(), second_pcurve.clone(), range, false));
                }
            }
        }
    }
    let original_boundaries = source.faces().map(|face| {
        let boundary = face_boundary_curves(topology, face.topology_id(), tolerance)?;
        let bounds = trim_bounds(&boundary, tolerance)?;
        Ok((face.persistent_id(), (face.surface(), boundary, bounds)))
    }).collect::<KernelResult<BTreeMap<_, _>>>()?;
    let opening_names = openings.iter().map(|id| topology.face(*id).map(|face| face.name()))
        .collect::<KernelResult<BTreeSet<_>>>()?;
    let in_source_chart = |uv: Point2, surface: &Surface3, bounds: [ParameterRange; 2]| {
        nearest_uv(surface.domains(), uv, box_center(bounds))
    };
    let classifier = BrepClassifier::for_compound(source)?;
    let contact_cache = std::cell::RefCell::new(BTreeMap::new());
    // Keep the radius-distance envelope of the trimmed source boundary.
    // Extended supports alone include pieces hidden behind another source face.
    let distance_to_source = |point: Point3| -> KernelResult<f64> {
        let mut distance = f64::INFINITY;
        for (name, (surface, boundary, bounds)) in &original_boundaries {
            if opening_names.contains(name) { continue; }
            let projection = surface.project(point, tolerance)?;
            let uv = in_source_chart(Point2::new(projection.parameters.0, projection.parameters.1), surface, *bounds);
            if point_in_trim(uv, boundary)? {
                distance = distance.min(point.distance(surface.evaluate(uv.x, uv.y)?.point));
            }
        }
        for (edge_id, edge) in topology.edges() {
            let projection = edge.curve().project_on_trim(point, edge.range(), tolerance)?;
            let edge_distance = if sharp && joins.contains(&edge_id) {
                // A sharp concave crease uses the larger signed depth of its
                // contact supports inside their common wedge. Include the
                // corner contacts when the nearest point reaches an end.
                let mut faces = topology.edge_faces(edge_id)?.into_iter().collect::<BTreeSet<_>>();
                if edge.vertices()[0] != edge.vertices()[1] {
                    let closest = edge.curve().evaluate(projection.parameter)?.point;
                    for vertex in edge.vertices() {
                        if closest.distance(topology.vertex(vertex)?.point()) <= tolerance.linear_at_scale(closest.to_vec().magnitude()) * 16.0 {
                            faces.extend(topology.vertex_faces(vertex)?);
                        }
                    }
                }
                // The same incident-face set recurs at many probes. Resolve
                // its surviving tangent contacts only on the first visit.
                let mut cache = contact_cache.borrow_mut();
                if !cache.contains_key(&faces) {
                    let mut pending = faces.iter().copied().collect::<Vec<_>>();
                    let mut visited = BTreeSet::new();
                    let mut contacts = BTreeSet::new();
                    while let Some(id) = pending.pop() {
                        if !visited.insert(id) { continue; }
                        let face = topology.face(id)?;
                        let sign = face.orientation().sign();
                        if collapsed_support(face.surface(), -radius * sign, tolerance)?.is_none() {
                            contacts.insert(id);
                            continue;
                        }
                        pending.extend(tangent_neighbors(topology, id, tolerance, |_| true)?);
                    }
                    cache.insert(faces.clone(), contacts);
                }
                let depths = cache[&faces].iter().map(|id| {
                    let face = topology.face(*id)?;
                    let face_projection = face.surface().project(point, tolerance)?;
                    let bounds = original_boundaries[&face.name()].2;
                    let mut uv = in_source_chart(Point2::new(face_projection.parameters.0, face_projection.parameters.1), face.surface(), bounds);
                    // An ended crease cannot constrain a distant angular
                    // sector of a curved support. Beyond that sector use the
                    // corner's contact normal, rather than another periodic
                    // branch of the untrimmed surface.
                    if (0..2).any(|axis| face.surface().domains()[axis].period.is_some()
                        && !bounds[axis].contains(uv[axis], tolerance)) {
                        let closest = edge.curve().evaluate(projection.parameter)?.point;
                        let contact = face.surface().project(closest, tolerance)?.parameters;
                        uv = Point2::new(contact.0, contact.1);
                    }
                    let value = face.surface().evaluate(uv.x, uv.y)?;
                    let sign = face.orientation().sign();
                    Ok((value.point - point).dot(value.normal()? * sign))
                }).collect::<KernelResult<Vec<_>>>()?;
                if depths.iter().any(|depth| *depth < -tolerance.linear) {
                    projection.distance
                } else { depths.into_iter().fold(f64::NEG_INFINITY, f64::max) }
            } else {
                projection.distance
            };
            distance = distance.min(edge_distance);
        }
        Ok(distance)
    };
    let mut patches = Vec::new();
    for index in 0..supports.len() {
        let surface = &supports[index];
        let bounds = trim_bounds(&boundaries[index], tolerance)?;
        let uv_probe_limit = bounds[0].span().abs().min(bounds[1].span().abs()) * 0.05;
        let mut regions = super::boolean::arrange_surface_regions(surface, &curves[index],
            radius * 1.0e-3, uv_probe_limit, tolerance, |uv| {
                if index >= base_count && !point_in_trim(uv, &boundaries[index])? { return Ok(None); }
                if index < base_count && !sharp {
                    let (_, original, bounds) = &original_boundaries[&ancestry[index][0]];
                    let original_uv = in_source_chart(uv, surface, *bounds);
                    // On tangent supports a distance probe alone cannot
                    // distinguish the two sides of the contact rail.
                    if !point_in_trim(original_uv, original)? { return Ok(None); }
                }
                let point = surface.evaluate(uv.x, uv.y)?.point;
                if classifier.classify(point)? == PointClassification::Outside {
                    return Ok(None);
                }
                let distance = distance_to_source(point)?;
                let is_opening = index < base_count && opening_names.contains(&ancestry[index][0]);
                let retained = if is_opening {
                    point_in_trim(uv, &boundaries[index])? && distance >= radius - tolerance.linear * 8.0
                } else {
                    // These are points on exact offset supports. A model-sized
                    // band admits both sides of a narrow overlap; allow only
                    // evaluation roundoff on the side hidden by another face.
                    distance >= radius - f64::EPSILON * point.to_vec().magnitude().max(radius) * 2048.0
                        && distance <= radius + tolerance.linear * 8.0
                };
                if !retained { return Ok(None); }
                let orientation = if index >= base_count {
                    let Surface3::RollingBall(blend) = surface else { unreachable!() };
                    let center = blend.center_curve().evaluate(uv.x)?.point;
                    let normal = surface.normal(uv.x, uv.y)?;
                    // Continuing across consumed blends can reverse the join's
                    // chart normal along the source edge. Orient each retained
                    // region toward its center, rather than using one sample
                    // for the entire support.
                    if normal.dot(point - center) < 0.0 { FaceOrientation::Forward }
                    else { FaceOrientation::Reversed }
                } else { orientations[index] };
                Ok(Some(orientation))
            })?;
        for region in &mut regions { region.sources = ancestry[index].clone(); }
        patches.extend(regions);
    }
    if sharp { regularize_coincident_offset_faces(&mut patches, -radius, tolerance)?; }
    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    transaction.brep_shell(&patches)?;
    let committed = transaction.finish()?;
    Compound::from_operation(kernel, committed, [])
}

pub(crate) fn shell(
    source: &Compound,
    openings: &[FaceHandle],
    distance: f64,
    join_type: &str,
) -> KernelResult<Compound> {
    if !distance.is_finite() || distance.abs() <= source.tolerance().linear {
        return Err(invalid(
            "shell thickness must be finite and exceed model tolerance",
        ));
    }
    let opening_ids = openings
        .iter()
        .map(FaceHandle::topology_id)
        .collect::<BTreeSet<_>>();

    // A rolling ball of radius |distance| between the diverging offset
    // supports has its center on their original edge. Only convex edges of
    // the offset skin need these joins; converging offsets remain intersected.
    // Exclude the unchanged source skin and the opening rims.
    let opening_names = openings
        .iter()
        .map(FaceHandle::persistent_id)
        .collect::<BTreeSet<_>>();
    // Classify the original edge, not an offset intersection: numerical
    // support reconstruction must not turn a tangent seam into a new join.
    let topology = source.topology();
    let mut divergent = BTreeSet::new();
    let mut divergent_edges = Vec::new();
    for (edge_id, edge) in topology.edges() {
        if edge.coedges().len() != 2 {
            continue;
        }
        let uses = edge
            .coedges()
            .iter()
            .map(|id| topology.coedge(*id))
            .collect::<KernelResult<Vec<_>>>()?;
        let faces = uses
            .iter()
            .map(|coedge| topology.face(topology.loop_(coedge.loop_id())?.face()))
            .collect::<KernelResult<Vec<_>>>()?;
        if faces
            .iter()
            .any(|face| opening_names.contains(&face.name()))
        {
            continue;
        }
        let parameter = (edge.range().start + edge.range().end) * 0.5;
        let sample = edge.curve().evaluate(parameter)?;
        let tangent = sample.first_derivative.normalize() * uses[0].range().span().signum();
        let normals = faces
            .iter()
            .map(|face| face.oriented_normal_near(sample.point, source.tolerance()))
            .collect::<KernelResult<Vec<_>>>()?;
        if tangent.dot(normals[0].cross(normals[1])) * distance.signum()
            > source.tolerance().angular
        {
            let mut pair = [faces[0].name(), faces[1].name()];
            pair.sort();
            divergent.insert(pair);
            divergent_edges.push(edge_id);
        }
    }
    let closed_joins = divergent_edges.iter().try_fold(true, |closed, id| {
        let edge = topology.edge(*id)?;
        Ok::<_, KernelError>(closed && edge.vertices()[0] == edge.vertices()[1])
    })?;
    let curved_open_join = divergent_edges.iter().any(|id| {
        let edge = topology.edge(*id).unwrap();
        edge.vertices()[0] != edge.vertices()[1] && matches!(edge.curve(), Curve3::SurfaceIntersection(_))
    });
    if join_type == "sharp" {
        let mut disjoint_contacts = false;
        let mut crossed_caps = false;
        if distance < 0.0 && curved_open_join {
            let caps = source.faces().filter(|face| !opening_ids.contains(&face.topology_id()))
                .filter_map(|face| match face.surface() {
                    Surface3::Plane(plane) => Some((face.topology_id(), plane)),
                    _ => None,
                }).collect::<Vec<_>>();
            for first in 0..caps.len() {
                let sign = topology.face(caps[first].0)?.orientation().sign();
                let normal = caps[first].1.normal() * sign;
                for second in 0..first {
                    let other_sign = topology.face(caps[second].0)?.orientation().sign();
                    if normal.dot(caps[second].1.normal() * other_sign) > -1.0 + source.tolerance().angular { continue; }
                    let gap = (caps[second].1.origin() - caps[first].1.origin()).dot(normal);
                    if gap < -source.tolerance().linear && gap - 2.0 * distance > source.tolerance().linear {
                        crossed_caps = true;
                    }
                }
            }
            for face in source.faces() {
                let id = face.topology_id();
                let value = topology.face(id)?;
                let sign = value.orientation().sign();
                if collapsed_support(face.surface(), distance * sign, source.tolerance())?.is_none() { continue; }
                let mut contacts = Vec::new();
                for neighbor in tangent_neighbors(topology, id, source.tolerance(), |_| true)? {
                    let face = topology.face(neighbor)?;
                    if !matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_)) { continue; }
                    let sign = face.orientation().sign();
                    if collapsed_support(face.surface(), distance * sign, source.tolerance())?.is_some() { continue; }
                    contacts.push(face.surface().offset(distance * sign, source.tolerance())?);
                }
                for first in 0..contacts.len() {
                    for second in 0..first {
                        if matches!(contacts[first].intersect_surface(&contacts[second], source.tolerance())?, IntersectionSet::Empty) {
                            disjoint_contacts = true;
                        }
                    }
                }
            }
        }
        // Reusing the original edge graph cannot represent a wall that has
        // disappeared, or caps that have passed each other. Arrange the sharp
        // support envelope directly at those topology transitions.
        let raw_offset = if disjoint_contacts || crossed_caps {
            offset_regions_with_joins(source, None, &divergent_edges, &opening_ids, distance.abs(), true)?
        } else { offset_body(source, &opening_ids, distance, false)?.0 };
        return assemble_shell_boundary(source, &raw_offset, &opening_ids, distance)?
            .with_feature_parent(source, super::DerivationRole::Shell);
    }
    if distance < 0.0 && curved_open_join {
        let rounded = offset_regions_with_joins(source, None, &divergent_edges, &opening_ids, distance.abs(), false)?;
        return assemble_shell_boundary(source, &rounded, &opening_ids, distance)?
            .with_feature_parent(source, super::DerivationRole::Shell);
    }
    let (raw_offset, has_vanished_blends) = offset_body(source, &opening_ids, distance, false)?;
    if distance < 0.0 && has_vanished_blends && closed_joins
        && !divergent_edges.is_empty()
    {
        let rounded = offset_regions_with_joins(source, Some(&raw_offset), &divergent_edges, &opening_ids, distance.abs(), false)?;
        return assemble_shell_boundary(source, &rounded, &opening_ids, distance)?
            .with_feature_parent(source, super::DerivationRole::Shell);
    }
    let shell = assemble_shell_boundary(source, &raw_offset, &opening_ids, distance)?
        .with_feature_parent(source, super::DerivationRole::Shell)?;
    let offset_surfaces = raw_offset
        .faces()
        .filter(|face| {
            !opening_names.contains(
                &raw_offset.topology_history().primary_source(face.persistent_id()),
            )
        })
        .map(|face| face.surface().clone())
        .collect::<Vec<_>>();
    let topology = shell.topology();
    let mut joins = Vec::new();
    for handle in shell.edges() {
        let Some(super::EntityId::Edge(edge_id)) = topology.entity(handle.persistent_id()) else {
            return Err(invalid("shell join edge is missing from its topology"));
        };
        let faces = topology.edge_faces(edge_id)?;
        if faces.len() != 2 {
            continue;
        }
        let faces = [topology.face(faces[0])?, topology.face(faces[1])?];
        if faces
            .iter()
            .any(|face| !offset_surfaces.contains(face.surface()))
        {
            continue;
        }
        let mut pair = [
            shell.topology_history().primary_source(faces[0].name()),
            shell.topology_history().primary_source(faces[1].name()),
        ];
        pair.sort();
        if divergent.contains(&pair) {
            joins.push(handle.clone());
        }
    }
    if joins.is_empty() {
        return Ok(shell);
    }
    shell.fillet_with_options(
        &joins,
        super::FilletOptions::new(distance.abs()).with_tangent_propagation(false),
    ).map(|result| result.with_construction_roles_from(&[&shell], &[]))
}

pub(crate) fn split(source: &Compound, plane: Plane3, keep: SplitKeep) -> KernelResult<Compound> {
    if source.solids().len() == 0 {
        return source.unchanged();
    }
    // The cutter occupies the half-space along the plane normal.
    let cutter = half_space_cutter(source, plane)?;
    let cut = |kind| {
        super::boolean::boolean_brep_with(
            source,
            &cutter,
            kind,
            super::boolean::CoincidentFaces::PreserveFirst,
        )
    };
    match keep {
        SplitKeep::Left => cut(super::BooleanKind::Difference),
        SplitKeep::Right => cut(super::BooleanKind::Intersection),
        SplitKeep::Both => {
            let left = cut(super::BooleanKind::Difference)?;
            let right = cut(super::BooleanKind::Intersection)?;
            left.combined(&[right])
        }
    }
}

pub(crate) fn dissolve_faces(source: &Compound, faces: &[FaceHandle]) -> KernelResult<Compound> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let removed = faces.iter().map(FaceHandle::topology_id).collect::<BTreeSet<_>>();
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    for face in source.faces().filter(|face| !removed.contains(&face.topology_id())) {
        supports.insert(face.topology_id(), face.surface().clone());
        bounds.insert(face.topology_id(), face_parameter_bounds(topology, face.topology_id(), tolerance)?);
    }
    if supports.is_empty() {
        return Err(invalid("dissolve must leave faces that can enclose a solid"));
    }

    // A pocket, boss or bore bounded only by complete inner loops heals by
    // removing those loops. Other features collapse onto intersections of the
    // remaining supports, using the same reconstruction as vanished blends.
    let mut collapsed = BTreeMap::new();
    let mut visited = BTreeSet::<FaceId>::new();
    for id in &removed {
        if visited.contains(id) { continue; }
        let mut component = BTreeSet::new();
        let mut pending = vec![*id];
        while let Some(face) = pending.pop() {
            if !component.insert(face) { continue; }
            for edge in topology.face_edges(face)? {
                pending.extend(topology.edge_faces(edge)?.into_iter().filter(|id| removed.contains(id)));
            }
        }
        visited.extend(component.iter().copied());
        let mut inner_only = true;
        let mut has_boundary = false;
        for face in supports.keys() {
            for loop_id in topology.face(*face)?.loops() {
                let edges = topology.loop_coedges(*loop_id)?.into_iter().map(|id| {
                    topology.edge_faces(topology.coedge(id)?.edge())
                }).collect::<KernelResult<Vec<_>>>()?;
                let touches = edges.iter().map(|faces| faces.iter().any(|id| component.contains(id))).collect::<Vec<_>>();
                if touches.iter().any(|touches| *touches) {
                    has_boundary = true;
                    inner_only &= topology.loop_(*loop_id)?.kind() == super::LoopKind::Inner
                        && touches.iter().all(|touches| *touches);
                }
            }
        }
        if inner_only && has_boundary {
            // No retained boundary vertex lies on these consumed inner loops.
            // Point collapse suppresses their rails during reconstruction.
            for face in component {
                let edge = topology.edge(topology.face_edges(face)?[0])?;
                collapsed.insert(face, CollapsedSupport::Point(topology.vertex(edge.vertices()[0])?.point()));
            }
        }
    }

    let mut corners = Vec::new();
    for id in removed.iter().filter(|id| !collapsed.contains_key(id)).copied().collect::<Vec<_>>() {
        let edges = topology.face_edges(id)?;
        let mut neighbors = BTreeMap::<FaceId, BTreeSet<VertexId>>::new();
        for edge in &edges {
            let edge_value = topology.edge(*edge)?;
            for face in topology.edge_faces(*edge)?.into_iter().filter(|face| supports.contains_key(face)) {
                neighbors.entry(face).or_default().extend(edge_value.vertices());
            }
        }
        let face_bounds = face_parameter_bounds(topology, id, tolerance)?;
        let seed = topology.face(id)?.surface().evaluate(
            (face_bounds[0].start + face_bounds[0].end) * 0.5,
            (face_bounds[1].start + face_bounds[1].end) * 0.5,
        )?.point;
        if neighbors.len() < 2 || (neighbors.len() == 3 && edges.len() == 3) {
            corners.push((id, seed, neighbors.into_keys().collect::<Vec<_>>()));
            continue;
        }
        // Opposite contact boundaries define the rails. End caps meet the
        // rails at vertices and only trim their intersection. Choose the local
        // intersection if both pairs have nonparallel supports.
        let mut rails = neighbors.into_iter().collect::<Vec<_>>();
        let tangent = tangent_neighbors(topology, id, tolerance, |face| supports.contains_key(&face))?;
        if tangent.len() == 2 { rails.retain(|(face, _)| tangent.contains(face)); }
        let mut candidates = Vec::new();
        for first in 0..rails.len() {
            for second in first + 1..rails.len() {
                if !rails[first].1.is_disjoint(&rails[second].1) { continue; }
                let guide_edge = edges.iter().find(|edge| topology.edge_faces(**edge)
                    .is_ok_and(|faces| faces.contains(&rails[first].0))).unwrap();
                let guide = topology.edge(*guide_edge)?;
                if let Ok(curve) = intersect_collapsed_blend(&supports[&rails[first].0], &supports[&rails[second].0],
                    guide.curve(), guide.range(), guide.curve().domain().period, tolerance) {
                    candidates.push((curve.project(seed, tolerance)?.distance, curve));
                }
            }
        }
        let curve = candidates.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, curve)| curve)
            .ok_or_else(|| invalid("selected faces have no intersecting surviving support surfaces"))?;
        collapsed.insert(id, CollapsedSupport::Curve(curve));
    }
    for (id, seed, neighbors) in corners {
        let mut constraints = neighbors.iter().map(|face| CornerConstraint::Surface(&supports[face], None)).collect::<Vec<_>>();
        for edge in topology.face_edges(id)? {
            for neighbor in topology.edge_faces(edge)? {
                if let Some(CollapsedSupport::Curve(curve)) = collapsed.get(&neighbor) {
                    constraints.push(CornerConstraint::Curve(curve));
                }
            }
        }
        if constraints.len() < 2 {
            return Err(invalid("selected faces cannot be healed from the remaining support surfaces"));
        }
        let point = refine_support_corner(seed, &constraints, tolerance)?;
        collapsed.insert(id, CollapsedSupport::Point(point));
    }
    let mut changed = removed;
    for id in collapsed.keys() {
        for edge in topology.face_edges(*id)? { changed.extend(topology.edge_faces(edge)?); }
    }
    let mut patches = support_patches(
        topology,
        tolerance,
        SupportEdit {
            supports,
            bounds,
            changed: changed.clone(),
            preserve_orientation: true,
            collapsed,
            ..Default::default()
        },
    )?;
    for patch in patches.values_mut().flatten() {
        patch.loops.retain(|loop_| !loop_.segments.is_empty());
    }
    commit_support_patches(source, patches, changed)?
        .with_feature_parent(source, super::DerivationRole::DissolveFaces)
}

pub(crate) fn move_faces(
    source: &Compound,
    faces: &[FaceHandle],
    transform: RigidTransform3,
) -> KernelResult<Compound> {
    if transform == RigidTransform3::identity() {
        return source.unchanged();
    }
    if faces.len() == source.faces().count() {
        return source.transformed(transform);
    }
    let topology = source.topology();
    let tolerance = source.tolerance();
    let selected = faces
        .iter()
        .map(FaceHandle::persistent_id)
        .collect::<BTreeSet<_>>();
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    let mut displacement = transform.translation().magnitude();
    for face in faces {
        for edge in topology.face_edges(face.topology_id())? {
            for vertex in topology.edge(edge)?.vertices() {
                let point = topology.vertex(vertex)?.point();
                displacement = displacement.max(point.distance(transform.transform_point(point)));
            }
        }
    }
    for face in source.faces() {
        let id = face.topology_id();
        let original_bounds = face_parameter_bounds(topology, id, tolerance)?;
        let mut surface = extended_support(face.surface(), original_bounds, displacement, tolerance)?;
        if selected.contains(&face.persistent_id()) {
            surface = super::transform::transform_surface(surface, transform, tolerance)?;
        }
        bounds.insert(
            id,
            expanded_offset_bounds(&surface, original_bounds, displacement, tolerance)?,
        );
        supports.insert(id, surface);
    }
    let contacts = move_blend_contacts(source)?;
    let mut changed_supports = faces
        .iter()
        .map(FaceHandle::topology_id)
        .collect::<BTreeSet<_>>();
    changed_supports.extend(adjust_move_blends(
        source,
        &selected,
        transform,
        &contacts,
        &mut supports,
    )?);
    let result = rebuild_supports(
        source,
        SupportEdit {
            supports,
            bounds,
            changed: changed_supports,
            preserve_orientation: true,
            ..Default::default()
        },
    )?;
    // A valid closed shell alone does not certify that a former fillet is still
    // tangent. Check the reconstructed contact rails before publishing it.
    let mut tangent_pairs = BTreeSet::new();
    for (id, rails) in &contacts {
        let name = topology.face(*id)?.name();
        for (other, _, _) in rails {
            tangent_pairs.insert((name, topology.face(*other)?.name()));
        }
    }
    let result_topology = result.topology();
    for (edge_id, _) in result_topology.edges() {
        let adjacent = result_topology.edge_faces(edge_id)?;
        let [first, second] = adjacent.as_slice() else {
            continue;
        };
        let pair = (
            result
                .topology_history()
                .primary_source(result_topology.face(*first)?.name()),
            result
                .topology_history()
                .primary_source(result_topology.face(*second)?.name()),
        );
        if !tangent_pairs.contains(&pair) {
            continue;
        }
        if tangent_contact(
            result_topology,
            edge_id,
            *first,
            *second,
            tolerance,
        )?
        .is_empty()
        {
            return Err(not_implemented(
                "face movement that cannot preserve blend tangency",
            ));
        }
    }
    result.with_feature_parent(source, super::DerivationRole::MoveFaces)
}

type BlendContacts = BTreeMap<FaceId, Vec<(FaceId, Point3, crate::Vec3)>>;

fn tangent_neighbors(
    topology: &Topology, face: FaceId, tolerance: super::ModelTolerance,
    include: impl Fn(FaceId) -> bool,
) -> KernelResult<BTreeSet<FaceId>> {
    let mut neighbors = BTreeSet::new();
    for edge in topology.face_edges(face)? {
        for other in topology.edge_faces(edge)? {
            if other != face && include(other) && !neighbors.contains(&other)
                && !tangent_contact(topology, edge, face, other, tolerance)?.is_empty() {
                neighbors.insert(other);
            }
        }
    }
    Ok(neighbors)
}

fn tangent_contact(
    topology: &Topology,
    edge: EdgeId,
    first: FaceId,
    second: FaceId,
    tolerance: super::ModelTolerance,
) -> KernelResult<Vec<(FaceId, Point3, crate::Vec3)>> {
    let edge = topology.edge(edge)?;
    let first = topology.face(first)?;
    let other = topology.face(second)?;
    let mut contacts = Vec::new();
    // Sample the contact curve, not just a vertex at which two sharp faces
    // could happen to share a normal. Offset and Move use the same criterion.
    for fraction in [0.05, 0.2, 0.35, 0.5, 0.65, 0.8, 0.95] {
        let point = edge
            .curve()
            .evaluate(edge.range().start + edge.range().span() * fraction)?
            .point;
        let n = first.oriented_normal_near(point, tolerance)?;
        if (n - other.oriented_normal_near(point, tolerance)?).magnitude() > tolerance.angular * 8.0 {
            return Ok(Vec::new());
        }
        contacts.push((second, point, n));
    }
    Ok(contacts)
}

fn move_blend_contacts(source: &Compound) -> KernelResult<BlendContacts> {
    let topology = source.topology();
    let mut contacts = BlendContacts::new();
    for face in source.faces() {
        let id = face.topology_id();
        for edge in topology.face_edges(id)? {
            for other in topology
                .edge_faces(edge)?
                .into_iter()
                .filter(|other| *other != id)
            {
                contacts.entry(id).or_default().extend(tangent_contact(
                    topology,
                    edge,
                    id,
                    other,
                    source.tolerance(),
                )?);
            }
        }
    }
    Ok(contacts)
}

fn adjust_move_blends(
    source: &Compound,
    selected: &BTreeSet<PersistentId>,
    transform: RigidTransform3,
    contacts: &BlendContacts,
    supports: &mut BTreeMap<FaceId, Surface3>,
) -> KernelResult<BTreeSet<FaceId>> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let selected_faces = source
        .faces()
        .filter(|face| selected.contains(&face.persistent_id()))
        .map(FaceHandle::topology_id)
        .collect::<BTreeSet<_>>();
    let mut changed = BTreeSet::new();
    let mut blends = BTreeSet::new();
    for face in source.faces() {
        let id = face.topology_id();
        if matches!(face.surface(), Surface3::Plane(_)) {
            continue;
        }
        // A full cylindrical wall between rounded rims is a support, not a
        // blend. Propagating through it would move the opposite end of a part.
        if matches!(face.surface(), Surface3::Cylinder(_))
            && face_parameter_bounds(topology, id, tolerance)?[0]
                .span()
                .abs()
                >= std::f64::consts::TAU - tolerance.parameter * 8.0
        {
            continue;
        }
        blends.insert(id);
    }
    let mut pending = BTreeSet::new();
    loop {
        let additions = contacts
            .iter()
            .filter(|(id, rails)| {
                !selected_faces.contains(*id)
                    && !pending.contains(*id)
                    && blends.contains(*id)
                    && rails
                        .iter()
                        .map(|(other, _, _)| other)
                        .collect::<BTreeSet<_>>()
                        .len()
                        >= 2
                    && rails
                        .iter()
                        .any(|(other, _, _)| selected_faces.contains(other) || pending.contains(other))
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        if additions.is_empty() {
            break;
        }
        pending.extend(additions);
    }
    while !pending.is_empty() {
        let mut resolved = None;
        for id in &pending {
            let rails = contacts[id]
                .iter()
                .filter(|(other, _, _)| !pending.contains(other))
                .copied()
                .collect::<Vec<_>>();
            if rails
                .iter()
                .map(|(other, _, _)| other)
                .collect::<BTreeSet<_>>()
                .len()
                < 2
            {
                continue;
            }
            let original = topology.face(*id)?.surface();
            let fits = |motion: RigidTransform3| -> KernelResult<bool> {
                for (other, point, normal) in &rails {
                    let point = motion.transform_point(*point);
                    let projection = supports[other].project(point, tolerance)?;
                    let uv = projection.parameters;
                    let sign = topology.face(*other)?.orientation().sign();
                    if projection.distance
                        > tolerance.linear_at_scale(point.to_vec().magnitude()) * 8.0
                        || (supports[other].normal(uv.0, uv.1)? * sign
                            - motion.transform_vector(*normal))
                        .magnitude()
                            > tolerance.angular * 8.0
                    {
                        return Ok(false);
                    }
                }
                Ok(true)
            };
            // Moving an entire blend along an invariant neighboring support is
            // exact (for example, raising a rounded cylindrical cap).
            let surface = if fits(transform)? {
                super::transform::transform_surface(original.clone(), transform, tolerance)?
            } else if let Surface3::Cylinder(cylinder) = original {
                let planes = rails
                    .iter()
                    .map(|(other, _, _)| *other)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .filter(|other| matches!(supports[other], Surface3::Plane(_)))
                    .collect::<Vec<_>>();
                if planes.len() != 2 {
                    continue;
                }
                let mut normals = Vec::new();
                let mut equations = Vec::new();
                for other in planes {
                    let Surface3::Plane(old) = topology.face(other)?.surface() else {
                        continue;
                    };
                    let Surface3::Plane(new) = supports[&other] else {
                        unreachable!()
                    };
                    let normal = new.normal();
                    let side = (cylinder.origin() - old.origin())
                        .dot(old.normal())
                        .signum();
                    equations.push((
                        [normal.x, normal.y, normal.z, -side],
                        (new.origin() - cylinder.origin()).dot(normal) + side * cylinder.radius(),
                    ));
                    normals.push(normal);
                }
                if normals.len() != 2 {
                    continue;
                }
                let axis = normals[0].cross(normals[1]);
                let parallel = axis.magnitude() <= tolerance.angular;
                // A semicircular slot end joins parallel walls. Moving one
                // wall changes both its center and radius by half the width
                // change; a fixed-radius fillet cannot satisfy both contacts.
                let solution = solve_blend_contacts(&equations, false, tolerance).or_else(|| {
                    if parallel {
                        solve_blend_contacts(&equations, true, tolerance)
                    } else {
                        None
                    }
                });
                let Some(solution) = solution else {
                    continue;
                };
                let radius = cylinder.radius() + solution[3];
                if radius <= tolerance.linear {
                    return Err(invalid("moving these faces collapses the rounded end"));
                }
                let axis = if parallel {
                    cylinder.axis().normalize()
                } else {
                    axis.normalize() * axis.dot(cylinder.axis()).signum()
                };
                let rotation = crate::Quaternion::from_arc(cylinder.axis().normalize(), axis, None);
                let scale = radius / cylinder.radius();
                super::Cylinder3::new(
                    cylinder.origin() + crate::Vec3::new(solution[0], solution[1], solution[2]),
                    axis * cylinder.axis().magnitude(),
                    rotation.rotate_vector(cylinder.x_radius()) * scale,
                    rotation.rotate_vector(cylinder.y_radius()) * scale,
                    tolerance,
                )?
                .into()
            } else if let Surface3::Sphere(sphere) = original {
                // Equal-radius cylindrical edge blends meet a spherical corner
                // with its center on all their axes, including after rotation.
                let mut equations = Vec::new();
                for other in rails
                    .iter()
                    .map(|(other, _, _)| *other)
                    .collect::<BTreeSet<_>>()
                {
                    if let Surface3::Cylinder(cylinder) = &supports[&other] {
                        if (cylinder.radius() - sphere.radius()).abs() > tolerance.linear {
                            continue;
                        }
                        for normal in [
                            cylinder.x_radius().normalize(),
                            cylinder.y_radius().normalize(),
                        ] {
                            equations.push((
                                [normal.x, normal.y, normal.z, 0.0],
                                (cylinder.origin() - sphere.center()).dot(normal),
                            ));
                        }
                    }
                }
                if equations.len() < 4 {
                    continue;
                }
                let Some(solution) = solve_blend_contacts(&equations, false, tolerance) else {
                    continue;
                };
                let motion = RigidTransform3::new(
                    crate::Matrix3::identity(),
                    crate::Vec3::new(solution[0], solution[1], solution[2]),
                    tolerance,
                )?;
                super::transform::transform_surface(original.clone(), motion, tolerance)?
            } else {
                let equations = rails
                    .iter()
                    .map(|(other, point, normal)| {
                        let projection = supports[other].project(*point, tolerance)?;
                        Ok((
                            [normal.x, normal.y, normal.z, 0.0],
                            (projection.point - point).dot(*normal),
                        ))
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                let Some(solution) = solve_blend_contacts(&equations, false, tolerance) else {
                    continue;
                };
                let motion = RigidTransform3::new(
                    crate::Matrix3::identity(),
                    crate::Vec3::new(solution[0], solution[1], solution[2]),
                    tolerance,
                )?;
                if !fits(motion)? {
                    continue;
                }
                super::transform::transform_surface(original.clone(), motion, tolerance)?
            };
            resolved = Some((*id, surface));
            break;
        }
        let Some((id, surface)) = resolved else {
            return Err(not_implemented("face movement of this connected blend network"));
        };
        pending.remove(&id);
        supports.insert(id, surface);
        changed.insert(id);
    }
    Ok(changed)
}

pub(crate) fn replace_face(
    source: &Compound,
    target: &FaceHandle,
    replacement: &FaceHandle,
) -> KernelResult<Compound> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    if target == replacement {
        return source.unchanged();
    }
    let id = target.topology_id();
    let mut displacement = 0.0_f64;
    for edge in target.boundary_edges()? {
        for point in edge.endpoints()? {
            displacement = displacement.max(replacement.surface().project(point, tolerance)?.distance);
        }
    }
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    for face in source.faces() {
        let face_id = face.topology_id();
        let (surface, face_bounds) = if face_id == id {
            // The replacement supplies its support, not its trim or orientation.
            // Its UV chart is independent of the face being replaced.
            (replacement.surface(), replacement.parameter_bounds()?)
        } else {
            (face.surface(), face_parameter_bounds(topology, face_id, tolerance)?)
        };
        let surface = extended_support(surface, face_bounds, displacement, tolerance)?;
        bounds.insert(face_id, expanded_offset_bounds(
            &surface, face_bounds, displacement, tolerance,
        )?);
        supports.insert(face_id, surface);
    }
    let changed = BTreeSet::from([id]);
    let patches = support_patches(
        topology,
        tolerance,
        SupportEdit {
            supports,
            bounds,
            changed: changed.clone(),
            reparameterized: changed.clone(),
            ..Default::default()
        },
    )?;
    commit_support_patches(source, patches, changed)?
        .with_feature_parent(source, super::DerivationRole::ReplaceFace)
}

impl Profile {
    pub fn revolve(&self, plane: Plane3, axis: Axis3, angle: f64) -> KernelResult<Compound> {
        if !angle.is_finite() || angle.abs() <= self.tolerance().parameter {
            return Err(invalid("revolution angle must be finite and nonzero"));
        }
        if angle.abs() > std::f64::consts::TAU + self.tolerance().parameter {
            return Err(invalid("revolution angle may not exceed one full turn"));
        }
        let full = (angle.abs() - std::f64::consts::TAU).abs() <= self.tolerance().parameter;
        if full {
            if let Some(exact) = exact_full_revolution(self, plane, axis)? {
                return Ok(exact);
            }
        }
        super::sweeps::revolve(self, plane, axis, angle)
    }

    pub fn loft<'a>(
        &'a self,
        plane: Plane3,
        sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
    ) -> KernelResult<Compound> {
        self.loft_with_alignment(plane, sections, true)
    }

    pub fn loft_with_alignment<'a>(
        &'a self,
        plane: Plane3,
        sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
        align_sections: bool,
    ) -> KernelResult<Compound> {
        let sections = std::iter::once((plane, self))
            .chain(sections)
            .collect::<Vec<_>>();
        super::sweeps::loft(&sections, align_sections)
    }
}

/// A finite half-space proxy enclosing the operand, with margin beyond its
/// conservative geometric enclosure. Only the plane itself can intersect the body.
fn half_space_cutter(source: &Compound, plane: Plane3) -> KernelResult<Compound> {
    let tolerance = source.tolerance();
    let bounds = super::bounds::compound(source)?;
    let mut minimum = crate::Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut maximum = -minimum;
    let plane = Plane3::from_origin_normal(plane.origin(), plane.normal())?;
    for point in bounds.corners() {
        let delta = point - plane.origin();
        let local = crate::Vec3::new(
            delta.dot(plane.u()),
            delta.dot(plane.v()),
            delta.dot(plane.normal()),
        );
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(local[axis]);
            maximum[axis] = maximum[axis].max(local[axis]);
        }
    }
    let margin = (maximum - minimum).magnitude().max(1.0) + tolerance.linear * 64.0;
    let wire = Wire2::polygon(
        [
            Point2::new(minimum.x - margin, minimum.y - margin),
            Point2::new(maximum.x + margin, minimum.y - margin),
            Point2::new(maximum.x + margin, maximum.y + margin),
            Point2::new(minimum.x - margin, maximum.y + margin),
        ],
        tolerance,
    )?;
    Profile::new(wire, [], tolerance)?
        .extrude(plane, plane.normal() * (maximum.z.max(0.0) + margin))
}

pub(crate) fn draft(
    source: &Compound,
    faces: &[FaceHandle],
    neutral: Plane3,
    angle: f64,
) -> KernelResult<Compound> {
    if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 {
        return Err(invalid(
            "draft angle must be finite and smaller than 90 degrees",
        ));
    }
    if angle.abs() <= source.tolerance().angular {
        return source.unchanged();
    }
    let tolerance = source.tolerance();
    let selected = faces
        .iter()
        .map(FaceHandle::persistent_id)
        .collect::<BTreeSet<_>>();
    let direction = neutral.normal();
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    for handle in source.faces() {
        let face = source.topology().face(handle.topology_id())?;
        let mut surface = face.surface().clone();
        let orientation = face.orientation().sign();
        if selected.contains(&face.name()) {
            surface = match surface {
                Surface3::Plane(plane) => {
                    let normal = plane.normal() * orientation;
                    let horizontal = normal - direction * normal.dot(direction);
                    if horizontal.magnitude() <= tolerance.angular {
                        return Err(invalid("draft face must intersect the neutral plane"));
                    }
                    let horizontal = horizontal.normalize();
                    let point = neutral.origin()
                        + horizontal * (plane.origin() - neutral.origin()).dot(normal)
                            / normal.dot(horizontal);
                    let normal = horizontal * angle.cos() + direction * angle.sin();
                    Plane3::from_origin_normal(point, normal * orientation)?.into()
                }
                Surface3::Cylinder(cylinder) => {
                    if cylinder.axis().normalize().dot(direction).abs() < 1.0 - tolerance.angular {
                        return Err(not_implemented(
                            "draft of a cylinder oblique to the pull direction",
                        ));
                    }
                    // Retain the cylindrical UV frame so its seam and cap edges
                    // can be rebuilt by the same support-intersection machinery.
                    let height = (cylinder.origin() - neutral.origin()).dot(direction);
                    let radial_sign = Surface3::Cylinder(cylinder)
                        .normal(0.0, 0.0)?
                        .dot(cylinder.x_radius())
                        .signum()
                        * orientation;
                    let slope = -angle.tan() * radial_sign;
                    let origin_radius = cylinder.radius() + slope * height;
                    let axial_slope = slope * cylinder.axis().dot(direction);
                    let apex_parameter = -origin_radius / axial_slope;
                    super::Cone3::new(
                        cylinder.origin() + cylinder.axis() * apex_parameter,
                        -cylinder.axis() * apex_parameter,
                        cylinder.x_radius() * (origin_radius / cylinder.radius()),
                        cylinder.y_radius() * (origin_radius / cylinder.radius()),
                        tolerance,
                    )?
                    .into()
                }
                Surface3::Cone(cone) => {
                    if cone.axis().normalize().dot(direction).abs() < 1.0 - tolerance.angular {
                        return Err(not_implemented("draft of a cone oblique to the pull direction"));
                    }
                    let neutral_parameter = (neutral.origin() - cone.apex()).dot(direction)
                        / cone.axis().dot(direction);
                    let neutral_radius = cone.reference_radius() * neutral_parameter;
                    let reference_center = cone.apex() + cone.axis();
                    let radial_sign = Surface3::Cone(cone)
                        .normal(0.0, 1.0)?
                        .dot(cone.x_radius())
                        .signum()
                        * orientation;
                    let slope = -angle.tan() * radial_sign;
                    let reference_radius = neutral_radius
                        + slope * (reference_center - neutral.origin()).dot(direction);
                    if reference_radius <= tolerance.linear {
                        return Err(invalid("draft collapses the conical reference section"));
                    }
                    let axis = direction * (reference_radius / slope);
                    super::Cone3::new(
                        reference_center - axis,
                        axis,
                        cone.x_radius() * (reference_radius / cone.reference_radius()),
                        cone.y_radius() * (reference_radius / cone.reference_radius()),
                        tolerance,
                    )?
                    .into()
                }
                Surface3::Nurbs(spline) if spline.degrees().1 == 1
                    && spline.control_points().len() == 2
                    && spline.weights()[0] == spline.weights()[1] => {
                    let controls = spline.control_points();
                    let heights = [0, 1].map(|row| (controls[row][0] - neutral.origin()).dot(direction));
                    if (heights[1] - heights[0]).abs() <= tolerance.linear
                        || controls.iter().enumerate().any(|(row, points)| points.iter().any(|p|
                            ((p - neutral.origin()).dot(direction) - heights[row]).abs() > tolerance.linear)) {
                        return Err(not_implemented("draft requires sections parallel to the neutral plane"));
                    }
                    let fraction = -heights[0] / (heights[1] - heights[0]);
                    let section = controls[0].iter().zip(&controls[1])
                        .map(|(a, b)| *a + (*b - *a) * fraction).collect::<Vec<_>>();
                    let base = super::NurbsSurface3::new(spline.degrees(),
                        heights.iter().map(|h| section.iter().map(|p| *p + direction * *h).collect()).collect(),
                        spline.weights().to_vec(), (spline.knots().0.to_vec(), spline.knots().1.to_vec()))?;
                    let super::DomainBounds::Bounded(v) = base.domains()[1].bounds else { unreachable!() };
                    let slope = -orientation * angle.tan() * (heights[1] - heights[0]) / v.span();
                    let distance = -orientation * angle.tan() * heights[0] - slope * v.start;
                    let bounds = face_parameter_bounds(source.topology(), handle.topology_id(), tolerance)?;
                    let padding = offset_parameter_padding(&base.clone().into(), bounds,
                        heights[0].abs().max(heights[1].abs()) * angle.tan(), tolerance)?;
                    super::OffsetSurface3::with_linear_distance(base.extended_bezier(padding)?.into(), distance, slope, tolerance)?.into()
                }
                _ => return Err(not_implemented("draft of this face support")),
            };
        }
        bounds.insert(
            handle.topology_id(),
            face_parameter_bounds(source.topology(), handle.topology_id(), tolerance)?,
        );
        supports.insert(handle.topology_id(), surface);
    }
    let changed_supports = faces.iter().map(FaceHandle::topology_id).collect();
    let result = rebuild_supports(
        source,
        SupportEdit {
            supports,
            bounds,
            changed: changed_supports,
            ..Default::default()
        },
    )?;
    result.with_feature_parent(source, super::DerivationRole::Draft)
}
