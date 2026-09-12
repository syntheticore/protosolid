use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3};

use super::euler::{BrepPatch, BrepPatchLoop, BrepPatchSegment};
use super::{
    Axis3, CoedgeId, Compound, Curve2, Curve3, DiagnosticKind, EdgeId, FaceHandle, FaceId,
    FaceOrientation, IntersectionSet, Kernel, KernelError, KernelResult, Line3, ParameterRange,
    PersistentId, Plane3, Profile, RigidTransform3, Surface3, SurfaceCurve3,
    SurfaceIntersectionCurve, SurfaceProjectionPcurve2, TessellationOptions, Topology,
    TrimmedCurve2, VertexId, Wire2,
};

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}

fn unsupported(operation: &str) -> KernelError {
    KernelError::new(
        DiagnosticKind::UnsupportedGeometry,
        format!("exact analytical {operation} is not implemented for the supplied geometry"),
    )
}

fn on_plane(plane: Plane3, point: Point2) -> Point3 {
    plane.origin() + plane.u() * point.x + plane.v() * point.y
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
            let section_center = on_plane(plane, circle.center());
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
        .map(|point| on_plane(plane, point))
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
    annulus
        .extrude(
            section_plane,
            axis_direction * (axial_values[1] - axial_values[0]),
        )
        .map(Some)
}

#[derive(Debug, Clone)]
struct OffsetEdge {
    curve: Curve3,
    pcurves: BTreeMap<CoedgeId, Curve2>,
    range: ParameterRange,
}

fn face_parameter_bounds(
    topology: &Topology,
    face: FaceId,
    tolerance: super::ModelTolerance,
) -> KernelResult<[ParameterRange; 2]> {
    let mut minimum = Point2::new(f64::INFINITY, f64::INFINITY);
    let mut maximum = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for loop_id in topology.face(face)?.loops() {
        for coedge_id in topology.loop_coedges(*loop_id)? {
            let coedge = topology.coedge(coedge_id)?;
            let range = coedge.range();
            for index in 0..=16 {
                let parameter = range.start + range.span() * index as f64 / 16.0;
                let point = coedge.pcurve().evaluate(parameter)?.point;
                minimum.x = minimum.x.min(point.x);
                minimum.y = minimum.y.min(point.y);
                maximum.x = maximum.x.max(point.x);
                maximum.y = maximum.y.max(point.y);
            }
        }
    }
    if !minimum.x.is_finite()
        || !minimum.y.is_finite()
        || !maximum.x.is_finite()
        || !maximum.y.is_finite()
    {
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "a shell face has no finite parameter-space boundary",
        ));
    }
    let padded = |minimum: f64, maximum: f64| {
        let padding = tolerance.parameter * 64.0;
        if maximum - minimum <= padding {
            (minimum - padding, maximum + padding)
        } else {
            (minimum, maximum)
        }
    };
    let u = padded(minimum.x, maximum.x);
    let v = padded(minimum.y, maximum.y);
    Ok([
        ParameterRange::new(u.0, u.1)?,
        ParameterRange::new(v.0, v.1)?,
    ])
}

fn exact_pcurve(
    curve: &Curve3,
    surface: &Surface3,
    fallback: &Curve2,
    tolerance: super::ModelTolerance,
) -> KernelResult<Curve2> {
    if let (Curve3::Circle(circle), Surface3::Plane(plane)) = (curve, surface) {
        let center = plane.project(circle.center())?.parameters;
        let x = plane
            .project(circle.center() + circle.x_radius())?
            .parameters;
        let y = plane
            .project(circle.center() + circle.y_radius())?
            .parameters;
        let x = Point2::new(x.0 - center.0, x.1 - center.1);
        let y = Point2::new(y.0 - center.0, y.1 - center.1);
        let scale = x.to_vec().magnitude().max(y.to_vec().magnitude()).max(1.0);
        let allowed = tolerance.parameter * scale * 64.0;
        if x.x > 0.0
            && y.y > 0.0
            && x.y.abs() <= allowed
            && y.x.abs() <= allowed
            && (x.x - y.y).abs() <= allowed
        {
            return super::Circle2::new(Point2::new(center.0, center.1), (x.x + y.y) * 0.5)
                .map(Into::into);
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

fn exact_surface_curve(
    surface: &Surface3,
    pcurve: &Curve2,
    range: ParameterRange,
    tolerance: super::ModelTolerance,
) -> KernelResult<Curve3> {
    // A meridian seam on a cylinder is spatially a line even though it is
    // represented as a one-face surface curve in topology. Preserve that
    // simplest exact form so the seam can extend to newly offset end caps;
    // retaining the old bounded SurfaceCurve range would artificially pin the
    // seam to the source face's axial extent.
    if let Curve2::Line(line) = pcurve {
        let direction = line.direction();
        if matches!(surface, Surface3::Plane(_))
            || (matches!(surface, Surface3::Cylinder(_) | Surface3::Cone(_))
                && direction.x.abs() <= tolerance.parameter * direction.y.abs().max(1.0))
        {
            let parameter = if range.contains(0.0, tolerance) {
                0.0
            } else {
                range.start
            };
            let uv = pcurve.evaluate(parameter)?.point;
            let (point, du, dv) = surface.first_order(uv.x, uv.y)?;
            return Line3::new(
                point - (du * direction.x + dv * direction.y) * parameter,
                du * direction.x + dv * direction.y,
            )
            .map(Into::into);
        }
    }
    SurfaceCurve3::new(surface.clone(), pcurve.clone(), range, tolerance).map(Into::into)
}

fn offset_edge(
    topology: &Topology,
    edge: EdgeId,
    supports: &BTreeMap<FaceId, Surface3>,
    bounds: &BTreeMap<FaceId, [ParameterRange; 2]>,
    tolerance: super::ModelTolerance,
) -> KernelResult<OffsetEdge> {
    let faces = topology.edge_faces(edge)?;
    if faces.len() == 1 {
        // A one-face edge is a periodic parameter seam. Its existing p-curve
        // remains the exact seam on the normally offset support.
        let face = faces[0];
        let coedge_id = topology.edge(edge)?.coedges()[0];
        let coedge = topology.coedge(coedge_id)?;
        let range = coedge.range();
        let pcurve = coedge.pcurve().clone();
        let curve = exact_surface_curve(&supports[&face], &pcurve, range, tolerance)?;
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
    let carried_curve = |face: FaceId, pcurve: &Curve2| -> KernelResult<Curve3> {
        let mut range = source_range;
        if let Curve2::Line(line) = pcurve {
            let origin = line.origin();
            let direction = line.direction();
            let domains = supports[&face].domains();
            // A shared cross-section may need to extend past its old end trim
            // to meet an opening. Keep periodic contact/seam ranges unchanged.
            if !domains.iter().enumerate().any(|(axis, domain)| {
                direction[axis].abs() > tolerance.parameter
                    && matches!(domain.bounds, super::DomainBounds::Unbounded)
            }) {
                return exact_surface_curve(&supports[&face], pcurve, range, tolerance);
            }
            let mut lower = f64::NEG_INFINITY;
            let mut upper = f64::INFINITY;
            for axis in 0..2 {
                if direction[axis].abs() <= tolerance.parameter {
                    continue;
                }
                let limits = bounds[&face][axis];
                let a = (limits.start - origin[axis]) / direction[axis];
                let b = (limits.end - origin[axis]) / direction[axis];
                lower = lower.max(a.min(b));
                upper = upper.min(a.max(b));
            }
            if lower.is_finite() && upper.is_finite() && lower < upper {
                range = ParameterRange::new(lower, upper)?;
            }
        }
        exact_surface_curve(&supports[&face], pcurve, range, tolerance)
    };
    let mut shared_pcurves = BTreeMap::new();
    let mut shared_curve = None;
    let mut shared_points = Vec::<Vec<Point3>>::new();
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
    let intersections = supports[&first]
        .intersect_surface(&supports[&second], tolerance)
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
                let first_uv = topology
                    .coedge(uses[0])?
                    .pcurve()
                    .evaluate(parameter)?
                    .point;
                let second_uv = topology
                    .coedge(uses[1])?
                    .pcurve()
                    .evaluate(parameter)?
                    .point;
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
    let incident = topology
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
    if incident.iter().any(|edge| {
        matches!(
            edges[edge].curve,
            Curve3::SurfaceCurve(_) | Curve3::SurfaceIntersection(_)
        )
    }) {
        if let Ok(point) =
            refine_offset_corner(source_point, &faces, &incident, edges, supports, tolerance)
        {
            return Ok(point);
        }
    }
    let mut candidates = Vec::new();
    let complexity = |curve: &Curve3| match curve {
        Curve3::Line(_) => 0_u8,
        Curve3::Circle(_) => 1,
        Curve3::Nurbs(_) => 2,
        Curve3::SurfaceCurve(_) => 3,
        Curve3::SurfaceIntersection(_) => 4,
    };
    let distance_to_support = |curve: &Curve3, point: Point3| -> KernelResult<f64> {
        if let Curve3::SurfaceIntersection(intersection) = curve {
            let first = intersection
                .first_surface()
                .project(point, tolerance)?
                .distance;
            let second = intersection
                .second_surface()
                .project(point, tolerance)?
                .distance;
            Ok(first.max(second))
        } else {
            curve
                .project(point, tolerance)
                .map(|projection| projection.distance)
        }
    };
    for first in 0..incident.len() {
        for second in first + 1..incident.len() {
            let first_curve = &edges[&incident[first]].curve;
            let second_curve = &edges[&incident[second]].curve;
            let Ok(first_guess) = first_curve.project(source_point, tolerance) else {
                continue;
            };
            let Ok(second_guess) = second_curve.project(source_point, tolerance) else {
                continue;
            };
            let Ok(hit) = first_curve.intersect_curve_near(
                second_curve,
                first_guess.parameter,
                second_guess.parameter,
                tolerance,
            ) else {
                continue;
            };
            let scale = hit.point.to_vec().magnitude().max(1.0);
            let allowed = tolerance.linear_at_scale(scale);
            let distances = incident
                .iter()
                .map(|edge| distance_to_support(&edges[edge].curve, hit.point))
                .collect::<KernelResult<Vec<_>>>()?;
            if distances.iter().all(|distance| *distance <= allowed) {
                candidates.push((
                    complexity(first_curve) + complexity(second_curve),
                    distances.into_iter().fold(0.0_f64, f64::max),
                    hit.point,
                ));
            }
        }
    }
    candidates
        .into_iter()
        .min_by(|first, second| {
            first
                .0
                .cmp(&second.0)
                .then_with(|| first.1.total_cmp(&second.1))
                .then_with(|| {
                    first
                        .2
                        .distance(source_point)
                        .total_cmp(&second.2.distance(source_point))
                })
        })
        .map(|(_, _, point)| point)
        .ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::GeometryInvariant,
                format!(
                    "offset shell faces do not meet at a common vertex ({vertex:?}, {} incident edges)",
                    incident.len()
                ),
            )
        })
}

// Solve the supporting surfaces together. Tangent contact curves add the
// constraints that coincident surface normals cannot supply on their own.
fn refine_offset_corner(
    mut point: Point3,
    faces: &BTreeSet<FaceId>,
    incident: &[EdgeId],
    edges: &BTreeMap<EdgeId, OffsetEdge>,
    supports: &BTreeMap<FaceId, Surface3>,
    tolerance: super::ModelTolerance,
) -> KernelResult<Point3> {
    for _ in 0..96 {
        let mut matrix = [[0.0; 3]; 3];
        let mut rhs = [0.0; 3];
        let mut worst = 0.0_f64;
        let mut cost = 0.0;
        for face in faces {
            let surface = &supports[face];
            let projection = surface.project(point, tolerance)?;
            let normal = surface.normal(projection.parameters.0, projection.parameters.1)?;
            let residual = (point - projection.point).dot(normal);
            worst = worst.max(projection.distance);
            cost += projection.distance.powi(2);
            for row in 0..3 {
                rhs[row] -= normal[row] * residual;
                for col in 0..3 {
                    matrix[row][col] += normal[row] * normal[col];
                }
            }
        }
        for edge in incident {
            let curve = &edges[edge].curve;
            if matches!(curve, Curve3::SurfaceIntersection(_)) {
                continue;
            }
            let projection = curve.project(point, tolerance)?;

            let tangent = curve
                .evaluate(projection.parameter)?
                .first_derivative
                .normalize();
            let residual = point - projection.point;
            worst = worst.max(projection.distance);
            cost += projection.distance.powi(2);
            for row in 0..3 {
                rhs[row] -= residual[row];
                for col in 0..3 {
                    matrix[row][col] +=
                        (if row == col { 1.0 } else { 0.0 }) - tangent[row] * tangent[col];
                }
            }
        }
        for axis in 0..3 {
            matrix[axis][axis] += 1e-12;
        }
        let Some(step) = super::intersection::solve_linear_3(matrix, rhs) else {
            break;
        };
        let step = crate::Vec3::new(step[0], step[1], step[2]);
        // Near tangency a small normal residual can still hide a large movement
        // along the surfaces. Require positional convergence as well.
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
            let candidate_cost = (|| -> KernelResult<f64> {
                let mut cost = 0.0;
                for face in faces {
                    cost += supports[face]
                        .project(candidate, tolerance)?
                        .distance
                        .powi(2);
                }
                for edge in incident {
                    if !matches!(edges[edge].curve, Curve3::SurfaceIntersection(_)) {
                        cost += edges[edge]
                            .curve
                            .project(candidate, tolerance)?
                            .distance
                            .powi(2);
                    }
                }
                Ok(cost)
            })();
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
    let Curve3::SurfaceIntersection(intersection) = &edge.curve else {
        return Ok(());
    };
    let source = topology.edge(edge_id)?;
    if source.vertices()[0] == source.vertices()[1] {
        return Ok(());
    }
    let curve_range = intersection.range();
    let exact = intersection.exact_pcurves();
    let mut first_points = Vec::new();
    let mut second_points = Vec::new();
    for index in 0..=32 {
        let parameter = curve_range.start + curve_range.span() * index as f64 / 32.0;
        first_points.push(exact[0].evaluate(parameter)?.point);
        second_points.push(exact[1].evaluate(parameter)?.point);
    }
    let endpoint_window = curve_range.span().abs() * 0.05 + tolerance.parameter * 64.0;
    for vertex in source.vertices() {
        let point = vertices[&vertex];
        let projection = edge.curve.project(point, tolerance)?;
        let support_distance = intersection
            .first_surface()
            .project(point, tolerance)?
            .distance
            .max(
                intersection
                    .second_surface()
                    .project(point, tolerance)?
                    .distance,
            );
        let endpoint = if projection.distance > tolerance.linear
            && support_distance <= tolerance.linear * 8.0
        {
            if (projection.parameter - curve_range.start).abs()
                <= (projection.parameter - curve_range.end).abs()
            {
                Some(0)
            } else {
                Some(32)
            }
        } else if (projection.parameter - curve_range.start).abs() <= endpoint_window {
            Some(0)
        } else if (projection.parameter - curve_range.end).abs() <= endpoint_window {
            Some(32)
        } else {
            None
        };
        let Some(endpoint) = endpoint else {
            continue;
        };
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
            let domains = surface.domains();
            if let Some(period) = domains[0].period {
                points[endpoint].x +=
                    ((points[neighbor].x - points[endpoint].x) / period).round() * period;
            }
            if let Some(period) = domains[1].period {
                points[endpoint].y +=
                    ((points[neighbor].y - points[endpoint].y) / period).round() * period;
            }
        }
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
        return if let Some(period) = domain.period {
            ParameterRange::new(
                projection.parameter,
                projection.parameter + direction * period,
            )
        } else if direction > 0.0 {
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
    let first = project_endpoint(vertices[&first_vertex])?;
    let second = project_endpoint(vertices[&second_vertex])?;
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
                    let source_coedge = topology.coedge(*coedge_id)?;
                    let source_range = source_coedge.range();
                    let candidate_range = if source_coedge.is_forward() {
                        candidate
                    } else {
                        candidate.reversed()
                    };
                    let source_start = source_coedge.pcurve().evaluate(source_range.start)?.point;
                    let source_end = source_coedge.pcurve().evaluate(source_range.end)?.point;
                    let candidate_pcurve = &edge.pcurves[coedge_id];
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
    if guide.domain().contains_range(range, tolerance.parameter) {
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

fn expanded_offset_bounds(
    surface: &Surface3,
    bounds: [ParameterRange; 2],
    distance: f64,
    tolerance: super::ModelTolerance,
) -> KernelResult<[ParameterRange; 2]> {
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
    let domains = surface.domains();
    let mut expanded = bounds;
    for axis in 0..2 {
        let rate = minimum_rates[axis];
        if !rate.is_finite() {
            continue;
        }
        let padding = distance.abs() * 2.0 / rate + tolerance.parameter * 64.0;
        let minimum = bounds[axis].start.min(bounds[axis].end) - padding;
        let maximum = bounds[axis].start.max(bounds[axis].end) + padding;
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
) -> KernelResult<(Compound, Vec<PersistentId>)> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let source_faces = source
        .faces()
        .map(|face| face.topology_id())
        .collect::<BTreeSet<_>>();
    let mut supports = BTreeMap::new();
    let mut bounds = BTreeMap::new();
    for face_id in &source_faces {
        let face = topology.face(*face_id)?;
        let surface = if openings.contains(face_id) {
            face.surface().clone()
        } else {
            let orientation = match face.orientation() {
                FaceOrientation::Forward => 1.0,
                FaceOrientation::Reversed => -1.0,
            };
            face.surface().offset(distance * orientation, tolerance)?
        };
        let face_bounds = face_parameter_bounds(topology, *face_id, tolerance)?;
        let expanded = expanded_offset_bounds(&surface, face_bounds, distance, tolerance)?;
        supports.insert(*face_id, surface);
        bounds.insert(*face_id, expanded);
    }

    rebuild_supports(source, supports, bounds)
}

fn rebuild_supports(
    source: &Compound,
    supports: BTreeMap<FaceId, Surface3>,
    bounds: BTreeMap<FaceId, [ParameterRange; 2]>,
) -> KernelResult<(Compound, Vec<PersistentId>)> {
    let topology = source.topology();
    let tolerance = source.tolerance();
    let source_faces = source
        .faces()
        .map(|face| face.topology_id())
        .collect::<BTreeSet<_>>();
    let mut source_edges = BTreeSet::new();
    for face_id in &source_faces {
        source_edges.extend(topology.face_edges(*face_id)?);
    }
    let mut edges = BTreeMap::new();
    for edge_id in source_edges {
        edges.insert(
            edge_id,
            offset_edge(topology, edge_id, &supports, &bounds, tolerance)?,
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
                            let face = topology.face(excluded)?;
                            let sign = if face.orientation() == FaceOrientation::Forward {
                                1.0
                            } else {
                                -1.0
                            };
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
    for (edge_id, edge) in &mut edges {
        let endpoints = topology
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
        retarget_offset_intersection_endpoints(
            topology, *edge_id, edge, &endpoints, &supports, tolerance,
        )?;
        edge.range = trim_offset_edge(topology, *edge_id, edge, &endpoints, tolerance)?;
    }

    let mut patches_by_shell = BTreeMap::<super::ShellId, Vec<(BrepPatch, PersistentId)>>::new();
    for face_id in source_faces {
        let face = topology.face(face_id)?;
        let surface = &supports[&face_id];
        let mut loops = Vec::new();
        for loop_id in face.loops() {
            let loop_value = topology.loop_(*loop_id)?;
            let mut segments = Vec::new();
            for coedge_id in topology.loop_coedges(*loop_id)? {
                let coedge = topology.coedge(coedge_id)?;
                let edge = &edges[&coedge.edge()];
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
                segments.push(BrepPatchSegment {
                    curve: edge.curve.clone(),
                    pcurve,
                    range,
                });
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
        patches_by_shell.entry(face.shell()).or_default().push((
            BrepPatch {
                surface: surface.clone(),
                orientation: {
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
                    } else if face.orientation() == FaceOrientation::Forward {
                        FaceOrientation::Reversed
                    } else {
                        FaceOrientation::Forward
                    }
                },
                loops,
            },
            face.name(),
        ));
    }

    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    let mut bodies = Vec::new();
    let mut patch_sources = Vec::new();
    for patches_and_sources in patches_by_shell.into_values() {
        let (patches, sources): (Vec<_>, Vec<_>) = patches_and_sources.into_iter().unzip();
        bodies.push(transaction.brep_shell(&patches)?);
        patch_sources.extend(sources);
    }
    transaction.nest_brep_shells(&bodies, TessellationOptions::default())?;
    let committed = transaction.finish()?;
    Ok((
        Compound::from_operation(kernel, committed, [])?,
        patch_sources,
    ))
}

pub(crate) fn face_patch(topology: &Topology, face_id: FaceId, reverse: bool) -> KernelResult<BrepPatch> {
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
            }
        }
        loops.push(BrepPatchLoop {
            kind: loop_value.kind(),
            segments,
        });
    }
    Ok(BrepPatch {
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
                let mut parameter = projection.parameter;
                if let Some(period) = segment.curve.domain().period {
                    let center = (segment.range.start + segment.range.end) * 0.5;
                    parameter += ((center - parameter) / period).round() * period;
                }
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
    patch_sources: &[PersistentId],
    openings: &BTreeSet<FaceId>,
    distance: f64,
) -> KernelResult<(Compound, Vec<PersistentId>)> {
    let source_topology = source.topology();
    let offset_topology = raw_offset.topology();
    let source_by_name = source
        .faces()
        .map(|face| (face.persistent_id(), face.topology_id()))
        .collect::<BTreeMap<_, _>>();
    if raw_offset.faces().count() != patch_sources.len() {
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "offset face count does not match its source-face correspondence",
        ));
    }

    let mut offset_by_source = BTreeMap::new();
    for (offset_face, source_name) in raw_offset.faces().zip(patch_sources.iter().copied()) {
        let source_face = source_by_name.get(&source_name).copied().ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "an offset face lost its source-face correspondence",
            )
        })?;
        if offset_by_source
            .insert(source_face, offset_face.topology_id())
            .is_some()
        {
            return Err(KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "multiple offset faces claim the same source face",
            ));
        }
    }

    let source_shells = source
        .faces()
        .map(|face| {
            source_topology
                .face(face.topology_id())
                .map(|value| value.shell())
        })
        .collect::<KernelResult<BTreeSet<_>>>()?;
    let direct_closed_pair = source_shells.len() == 1 && openings.is_empty();
    let mut groups = Vec::<Vec<(BrepPatch, PersistentId)>>::new();
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
                Ok((
                    face_patch(source_topology, face, distance > 0.0)?,
                    source_topology.face(face)?.name(),
                ))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let offset_skin = shell_faces
            .iter()
            .copied()
            .filter(|face| !openings.contains(face))
            .map(|face| {
                Ok((
                    face_patch(offset_topology, offset_by_source[&face], distance < 0.0)?,
                    source_topology.face(face)?.name(),
                ))
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
                if let Some(rim) = rim_face_patch(
                    source_face.surface(),
                    source_face.orientation(),
                    outer_loop,
                    inner_loop,
                    source.tolerance(),
                )? {
                    connected.push((rim, source_face.name()));
                }
            }
        }
        groups.push(connected);
    }

    let mut kernel = Kernel::new(source.tolerance())?;
    let mut transaction = kernel.begin_operation();
    let mut bodies = Vec::new();
    let mut output_sources = Vec::new();
    if direct_closed_pair {
        let mut groups = groups.into_iter();
        let (source_patches, source_names): (Vec<_>, Vec<_>) = groups
            .next()
            .expect("a closed source shell has a source skin")
            .into_iter()
            .unzip();
        let (offset_patches, offset_names): (Vec<_>, Vec<_>) = groups
            .next()
            .expect("a closed source shell has an offset skin")
            .into_iter()
            .unzip();
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
        output_sources.extend(source_names);
        output_sources.extend(offset_names);
    } else {
        for group in groups {
            let (patches, sources): (Vec<_>, Vec<_>) = group.into_iter().unzip();
            bodies.push(transaction.brep_shell(&patches)?);
            output_sources.extend(sources);
        }
        transaction.nest_brep_shells(&bodies, TessellationOptions::default())?;
    }
    let committed = transaction.finish()?;
    Ok((
        Compound::from_operation(kernel, committed, [])?,
        output_sources,
    ))
}

pub(crate) fn shell(
    source: &Compound,
    openings: &[FaceHandle],
    distance: f64,
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
    let (raw_offset, patch_sources) = offset_body(source, &opening_ids, distance)?;
    let (raw_shell, shell_sources) =
        assemble_shell_boundary(source, &raw_offset, &patch_sources, &opening_ids, distance)?;
    if raw_shell.faces().count() != shell_sources.len() {
        return Err(KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "shell face count does not match its source-face correspondence",
        ));
    }
    let mut origins = BTreeMap::new();
    for (face, source_name) in raw_shell.faces().zip(shell_sources) {
        origins.insert(face.persistent_id(), vec![source_name]);
    }
    raw_shell.with_feature_parent(source, "shell", &origins)
}

pub(crate) fn split(source: &Compound, plane: Plane3, keep: &str) -> KernelResult<Compound> {
    if !matches!(keep, "left" | "right" | "both") {
        return Err(invalid("split keep mode must be left, right, or both"));
    }
    if source.solids().len() == 0 {
        return Ok(source.clone());
    }
    let cutter = half_space_cutter(source, plane)?;
    match keep {
        "left" => source.difference(&cutter),
        "right" => source.intersection(&cutter),
        _ => {
            let left = source.difference(&cutter)?;
            let right = source.intersection(&cutter)?;
            left.combined(&[right])
        }
    }
}

pub(crate) fn move_faces(
    _source: &Compound,
    faces: &[FaceHandle],
    _transform: RigidTransform3,
) -> KernelResult<Compound> {
    if faces.is_empty() {
        return Err(invalid("moving faces needs at least one face"));
    }
    Err(unsupported("face movement"))
}

pub(crate) fn replace_face(
    _source: &Compound,
    _target: &FaceHandle,
    _replacement: &FaceHandle,
) -> KernelResult<Compound> {
    Err(unsupported("face replacement"))
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
/// certified display envelope. Only the plane itself can intersect the body.
fn half_space_cutter(source: &Compound, plane: Plane3) -> KernelResult<Compound> {
    let tolerance = source.tolerance();
    let options = TessellationOptions::default();
    let mesh = source.tessellate(options)?;
    let mut minimum = crate::Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut maximum = -minimum;
    let plane = Plane3::from_origin_normal(plane.origin(), plane.normal())?;
    for point in mesh.positions {
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
    let margin = (maximum - minimum).magnitude().max(1.0) + options.chord_tolerance * 4.0;
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
        return Ok(source.clone());
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
        let orientation = if face.orientation() == FaceOrientation::Forward {
            1.0
        } else {
            -1.0
        };
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
                        return Err(unsupported(
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
                        return Err(unsupported("draft of a cone oblique to the pull direction"));
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
                _ => return Err(unsupported("draft of this face support")),
            };
        }
        bounds.insert(
            handle.topology_id(),
            face_parameter_bounds(source.topology(), handle.topology_id(), tolerance)?,
        );
        supports.insert(handle.topology_id(), surface);
    }
    let (result, origins) = rebuild_supports(source, supports, bounds)?;
    let origins = result
        .faces()
        .zip(origins)
        .map(|(face, origin)| (face.persistent_id(), vec![origin]))
        .collect();
    result.with_feature_parent(source, "draft", &origins)
}
