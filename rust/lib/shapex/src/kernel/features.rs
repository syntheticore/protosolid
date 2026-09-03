use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3};

use super::euler::{BrepPatch, BrepPatchLoop, BrepPatchSegment};
use super::{
    Axis3, CoedgeId, Compound, Curve2, Curve3, DiagnosticKind, EdgeId, FaceHandle, FaceId,
    FaceOrientation, IntersectionSet, Kernel, KernelError, KernelResult, ParameterRange,
    PersistentId, Plane3, Profile, RigidTransform3, Surface3, SurfaceCurve3,
    SurfaceProjectionPcurve2, TessellationOptions, Topology, TrimmedCurve2, VertexId, Wire2,
};

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}

fn unsupported(operation: &str) -> KernelError {
    KernelError::new(
        DiagnosticKind::UnsupportedGeometry,
        format!(
            "exact analytical {operation} is not implemented; polyhedral BREP reconstruction has been removed"
        ),
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

fn incident_faces(topology: &Topology, edge: EdgeId) -> KernelResult<Vec<FaceId>> {
    let mut faces = BTreeSet::new();
    for coedge in topology.edge(edge)?.coedges() {
        faces.insert(topology.loop_(topology.coedge(*coedge)?.loop_id())?.face());
    }
    Ok(faces.into_iter().collect())
}

fn offset_edge(
    topology: &Topology,
    edge: EdgeId,
    supports: &BTreeMap<FaceId, Surface3>,
    bounds: &BTreeMap<FaceId, [ParameterRange; 2]>,
    tolerance: super::ModelTolerance,
) -> KernelResult<OffsetEdge> {
    let faces = incident_faces(topology, edge)?;
    if faces.len() == 1 {
        // A one-face edge is a periodic parameter seam. Its existing p-curve
        // remains the exact seam on the normally offset support.
        let face = faces[0];
        let coedge_id = topology.edge(edge)?.coedges()[0];
        let coedge = topology.coedge(coedge_id)?;
        let range = coedge.range();
        let pcurve = coedge.pcurve().clone();
        let curve: Curve3 =
            SurfaceCurve3::new(supports[&face].clone(), pcurve.clone(), range, tolerance)?.into();
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
    let intersections = supports[&first].intersect_surface_in_bounds(
        &supports[&second],
        bounds[&first],
        bounds[&second],
        tolerance,
    )?;
    let IntersectionSet::Isolated(intersections) = intersections else {
        return Err(KernelError::new(
            DiagnosticKind::UnsupportedGeometry,
            "offsets of adjacent shell faces do not have a unique regular intersection",
        ));
    };
    let source_edge = topology.edge(edge)?;
    let source_midpoint = source_edge
        .curve()
        .evaluate(source_edge.range().start + source_edge.range().span() * 0.5)?
        .point;
    let chosen = intersections
        .into_iter()
        .filter_map(|candidate| {
            candidate
                .curve
                .project(source_midpoint, tolerance)
                .ok()
                .map(|projection| (projection.distance, candidate))
        })
        .min_by(|first, second| first.0.total_cmp(&second.0))
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
    tolerance: super::ModelTolerance,
) -> KernelResult<Point3> {
    let source_point = topology.vertex(vertex)?.point();
    let incident = topology
        .vertex_edges(vertex)?
        .into_iter()
        .filter(|edge| edges.contains_key(edge))
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
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
            let allowed = tolerance.linear_at_scale(scale) * 128.0;
            if incident.iter().all(|edge| {
                edges[edge]
                    .curve
                    .project(hit.point, tolerance)
                    .is_ok_and(|projection| projection.distance <= allowed)
            }) {
                candidates.push(hit.point);
            }
        }
    }
    candidates
        .into_iter()
        .min_by(|first, second| {
            first
                .distance(source_point)
                .total_cmp(&second.distance(source_point))
        })
        .ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "offset shell faces do not meet at a common vertex",
            )
        })
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
        let source_parameter = source.curve().project(reference, tolerance)?.parameter;
        let source_tangent = source.curve().evaluate(source_parameter)?.first_derivative
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
    let first = edge.curve.project(vertices[&first_vertex], tolerance)?;
    let second = edge.curve.project(vertices[&second_vertex], tolerance)?;
    let mut end = second.parameter;
    if let Some(period) = edge.curve.domain().period {
        let source_midpoint = source
            .curve()
            .evaluate(source.range().start + source.range().span() * 0.5)?
            .point;
        end = (-1..=1)
            .map(|turn| second.parameter + period * turn as f64)
            .min_by(|first_end, second_end| {
                let first_distance = edge
                    .curve
                    .evaluate((first.parameter + *first_end) * 0.5)
                    .map(|value| value.point.distance(source_midpoint))
                    .unwrap_or(f64::INFINITY);
                let second_distance = edge
                    .curve
                    .evaluate((first.parameter + *second_end) * 0.5)
                    .map(|value| value.point.distance(source_midpoint))
                    .unwrap_or(f64::INFINITY);
                first_distance.total_cmp(&second_distance)
            })
            .unwrap_or(end);
    }
    ParameterRange::new(first.parameter, end).map_err(|_| {
        KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "shell thickness collapses an offset edge",
        )
    })
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
    let guide: Curve2 = super::intersection::smooth_nurbs_guide(&points, range)?.into();
    Ok(
        SurfaceProjectionPcurve2::new(curve.clone(), surface.clone(), guide, range, tolerance)?
            .into(),
    )
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
        supports.insert(*face_id, surface);
        bounds.insert(
            *face_id,
            face_parameter_bounds(topology, *face_id, tolerance)?,
        );
    }

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
    for vertex in required_vertices {
        vertices.insert(vertex, offset_vertex(topology, vertex, &edges, tolerance)?);
    }
    for (edge_id, edge) in &mut edges {
        edge.range = trim_offset_edge(topology, *edge_id, edge, &vertices, tolerance)?;
    }

    let mut patches_by_shell = BTreeMap::<super::ShellId, Vec<BrepPatch>>::new();
    let mut patch_sources = Vec::new();
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
                surface: surface.clone(),
                orientation: face.orientation(),
                loops,
            });
        patch_sources.push(face.name());
    }

    let mut kernel = Kernel::new(tolerance)?;
    let mut transaction = kernel.begin_operation();
    let mut bodies = Vec::new();
    for patches in patches_by_shell.values() {
        bodies.push(transaction.brep_shell(patches)?);
    }
    transaction.nest_brep_shells(&bodies, TessellationOptions::default())?;
    let committed = transaction.finish()?;
    Ok((
        Compound::from_operation(kernel, committed, [])?,
        patch_sources,
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
    // Periodic curved skins still need a canonical seam rewrite before their
    // exact mass properties can be certified. Keep them outside the published
    // set rather than returning a valid-looking body with a bad certificate.
    if source
        .faces()
        .any(|face| !matches!(face.surface(), Surface3::Plane(_)))
        || source
            .edges()
            .any(|edge| !matches!(edge.curve(), Curve3::Line(_)))
    {
        return Err(unsupported("shelling for curved faces"));
    }
    let opening_ids = openings
        .iter()
        .map(FaceHandle::topology_id)
        .collect::<BTreeSet<_>>();
    let (raw_offset, patch_sources) = offset_body(source, &opening_ids, distance)?;
    let mut origins = BTreeMap::new();
    for (face, source_name) in raw_offset.faces().zip(patch_sources) {
        origins.insert(face.persistent_id(), vec![source_name]);
    }
    let offset = raw_offset.with_feature_parent(source, "shell.offset", &origins)?;
    if distance < 0.0 {
        source.difference(&offset)
    } else {
        offset.difference(source)
    }
}

pub(crate) fn split(_source: &Compound, _plane: Plane3, keep: &str) -> KernelResult<Compound> {
    if !matches!(keep, "left" | "right" | "both") {
        return Err(invalid("split keep mode must be left, right, or both"));
    }
    Err(unsupported("splitting"))
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
        Err(unsupported("revolution"))
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
        _plane: Plane3,
        _sections: impl IntoIterator<Item = (Plane3, &'a Profile)>,
        _align_sections: bool,
    ) -> KernelResult<Compound> {
        Err(unsupported("lofting"))
    }
}
