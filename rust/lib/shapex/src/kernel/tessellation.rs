use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec3};

use super::{
    BodyId, Diagnostic, DiagnosticKind, Edge, EdgeId, FaceId, FaceOrientation, Kernel, KernelError,
    KernelResult, LoopId, ModelTolerance, ShellId,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TessellationOptions {
    pub chord_tolerance: f64,
    pub angular_tolerance: f64,
    pub maximum_depth: u8,
}

impl TessellationOptions {
    pub fn new(
        chord_tolerance: f64,
        angular_tolerance: f64,
        maximum_depth: u8,
    ) -> KernelResult<Self> {
        if !chord_tolerance.is_finite()
            || chord_tolerance <= 0.0
            || !angular_tolerance.is_finite()
            || angular_tolerance <= 0.0
            || angular_tolerance >= std::f64::consts::PI
            || maximum_depth == 0
        {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "tessellation tolerances must be finite and positive, with nonzero depth",
            ));
        }
        Ok(Self {
            chord_tolerance,
            angular_tolerance,
            maximum_depth,
        })
    }
}

impl Default for TessellationOptions {
    fn default() -> Self {
        Self {
            chord_tolerance: 1.0e-3,
            angular_tolerance: 0.1,
            maximum_depth: 24,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EdgePolyline {
    parameters: Vec<f64>,
    points: Vec<Point3>,
}

/// A geometry value that can produce a display approximation under an explicit
/// error policy. Faces, edges, solids, and compounds all implement this trait.
pub trait Tessellate {
    type Output;

    fn tessellate(&self, options: TessellationOptions) -> KernelResult<Self::Output>;
}

/// Ordered samples of an exact curve segment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Polyline {
    pub parameters: Vec<f64>,
    pub points: Vec<Point3>,
}

#[derive(Debug, Clone, Default)]
pub struct TriangleMesh {
    pub positions: Vec<Point3>,
    pub normals: Vec<Vec3>,
    pub triangles: Vec<[usize; 3]>,
    pub triangle_faces: Vec<FaceId>,
}

impl TriangleMesh {
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    pub fn append(&mut self, mut other: TriangleMesh) {
        let offset = self.positions.len();
        self.positions.append(&mut other.positions);
        self.normals.append(&mut other.normals);
        self.triangles
            .extend(other.triangles.into_iter().map(|triangle| {
                [
                    triangle[0] + offset,
                    triangle[1] + offset,
                    triangle[2] + offset,
                ]
            }));
        self.triangle_faces.append(&mut other.triangle_faces);
    }
}

#[derive(Debug, Clone)]
struct BoundaryVertex {
    uv: Point2,
    point: Point3,
}

#[derive(Debug, Clone, Copy)]
struct CurveSample {
    point: Point3,
    first_derivative: Vec3,
}

fn curve_sample(curve: &super::Curve3, parameter: f64) -> KernelResult<CurveSample> {
    let (point, first_derivative) = curve.first_order(parameter)?;
    Ok(CurveSample {
        point,
        first_derivative,
    })
}

/// Choose a regular local parameter chart for a trimmed spherical face.
///
/// A sphere's stored longitude/latitude frame is intrinsic geometry metadata,
/// not a mandatory rendering chart. A small exact face may contain one of
/// that frame's poles, where longitude is undefined and planar trim
/// triangulation produces a spurious wraparound. Reorient an equivalent exact
/// sphere for this tessellation only, placing the boundary's mean direction on
/// the chart equator and choosing the pole farthest from all boundary samples.
fn sphere_tessellation_chart(
    surface: &super::Surface3,
    rings: &[Vec<BoundaryVertex>],
    tolerance: ModelTolerance,
) -> KernelResult<Option<super::Surface3>> {
    let super::Surface3::Sphere(sphere) = surface else {
        return Ok(None);
    };
    let directions = rings
        .iter()
        .flatten()
        .map(|vertex| vertex.point - sphere.center())
        .filter(|direction| direction.magnitude2() > 0.0)
        .map(|direction| direction.normalize())
        .collect::<Vec<_>>();
    let Some(first) = directions.first().copied() else {
        return Ok(None);
    };
    let mean = directions
        .iter()
        .copied()
        .fold(Vec3::new(0.0, 0.0, 0.0), |sum, direction| sum + direction);
    let x = if mean.magnitude2() > f64::EPSILON {
        mean.normalize()
    } else {
        first
    };
    let z = [Vec3::unit_x(), Vec3::unit_y(), Vec3::unit_z()]
        .into_iter()
        .map(|axis| x.cross(axis))
        .filter(|candidate| candidate.magnitude2() > f64::EPSILON)
        .map(|candidate| candidate.normalize())
        .min_by(|first, second| {
            let first_proximity = directions
                .iter()
                .map(|direction| direction.dot(*first).abs())
                .fold(0.0_f64, f64::max);
            let second_proximity = directions
                .iter()
                .map(|direction| direction.dot(*second).abs())
                .fold(0.0_f64, f64::max);
            first_proximity.total_cmp(&second_proximity)
        })
        .ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "spherical face has no regular local tessellation chart",
            )
        })?;
    let y = z.cross(x).normalize();
    let radius = sphere.radius();
    Ok(Some(
        super::Sphere3::new(
            sphere.center(),
            x * radius,
            y * radius,
            z * radius,
            tolerance,
        )?
        .into(),
    ))
}

impl Kernel {
    pub fn tessellate_edge(
        &self,
        edge: EdgeId,
        options: TessellationOptions,
    ) -> KernelResult<Polyline> {
        options.validate_against(self.tolerance())?;
        let sampled = sample_edge(self.topology().edge(edge)?, options)?;
        Ok(Polyline {
            parameters: sampled.parameters,
            points: sampled.points,
        })
    }

    pub fn tessellate_body(
        &self,
        body: BodyId,
        options: TessellationOptions,
    ) -> KernelResult<TriangleMesh> {
        options.validate_against(self.tolerance())?;
        let mut cache = BTreeMap::new();
        let mut mesh = TriangleMesh::default();
        for shell in self.topology().body(body)?.shells() {
            mesh.append(self.tessellate_shell_cached(*shell, options, &mut cache)?);
        }
        Ok(mesh)
    }

    pub fn tessellate_shell(
        &self,
        shell: ShellId,
        options: TessellationOptions,
    ) -> KernelResult<TriangleMesh> {
        options.validate_against(self.tolerance())?;
        self.tessellate_shell_cached(shell, options, &mut BTreeMap::new())
    }

    pub fn tessellate_face(
        &self,
        face: FaceId,
        options: TessellationOptions,
    ) -> KernelResult<TriangleMesh> {
        options.validate_against(self.tolerance())?;
        self.tessellate_face_cached(face, options, &mut BTreeMap::new())
    }

    fn tessellate_shell_cached(
        &self,
        shell: ShellId,
        options: TessellationOptions,
        cache: &mut BTreeMap<EdgeId, EdgePolyline>,
    ) -> KernelResult<TriangleMesh> {
        let mut mesh = TriangleMesh::default();
        for face in self.topology().shell(shell)?.faces() {
            mesh.append(self.tessellate_face_cached(*face, options, cache)?);
        }
        Ok(mesh)
    }

    fn tessellate_face_cached(
        &self,
        face_id: FaceId,
        options: TessellationOptions,
        cache: &mut BTreeMap<EdgeId, EdgePolyline>,
    ) -> KernelResult<TriangleMesh> {
        let face = self.topology().face(face_id)?;
        let (mut rings, parameters_are_continuous) =
            if let Some(ring) = self.sample_full_periodic_cylinder_band(face_id, options)? {
                (vec![ring], true)
            } else {
                let mut loop_ids = face.loops().to_vec();
                loop_ids.sort_by_key(|loop_id| {
                    self.topology()
                        .loop_(*loop_id)
                        .map(|loop_| match loop_.kind() {
                            super::LoopKind::Outer => 0,
                            super::LoopKind::Inner => 1,
                        })
                        .unwrap_or(2)
                });
                let mut rings = Vec::new();
                for loop_id in loop_ids {
                    rings.push(self.sample_loop(loop_id, options, cache)?);
                }
                (rings, false)
            };
        let chart = sphere_tessellation_chart(face.surface(), &rings, self.tolerance())?;
        let tessellation_surface = chart.as_ref().unwrap_or_else(|| face.surface());
        if !parameters_are_continuous {
            for ring in &mut rings {
                unwrap_periodic_ring(tessellation_surface, self.tolerance(), ring)?;
            }
        }
        if rings.is_empty() || rings[0].len() < 3 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "a tessellated face needs an outer loop with at least three boundary samples",
            ));
        }

        let mut flat_uv = Vec::new();
        let mut uvs = Vec::new();
        let mut positions = Vec::new();
        let mut holes = Vec::new();
        let mut boundary_edges = BTreeSet::new();
        let mut ring_offset = 0usize;
        for ring in &rings {
            for (index, vertex) in ring.iter().enumerate() {
                flat_uv.extend([vertex.uv.x, vertex.uv.y]);
                uvs.push(vertex.uv);
                positions.push(vertex.point);
                boundary_edges.insert(ordered_edge(
                    ring_offset + index,
                    ring_offset + (index + 1) % ring.len(),
                ));
            }
            ring_offset += ring.len();
        }
        // Hole indices are starts, not ends.
        let mut running = rings[0].len();
        for ring in rings.iter().skip(1) {
            holes.push(running);
            running += ring.len();
        }
        let indices = earcutr::earcut(&flat_uv, &holes, 2);
        if indices.len() < 3 || indices.len() % 3 != 0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "trimmed face triangulation failed",
            ));
        }

        let mut normals = Vec::with_capacity(positions.len());
        for ring in &rings {
            for vertex in ring {
                let (lifted, mut normal) =
                    surface_point_normal(tessellation_surface, vertex.uv.x, vertex.uv.y)?;
                let allowed = self
                    .tolerance()
                    .linear_at_scale(vertex.point.to_vec().magnitude());
                if lifted.distance(vertex.point) > allowed {
                    return Err(KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "tessellation chart does not lift its exact BREP boundary",
                    ));
                }
                if face.orientation() == FaceOrientation::Reversed {
                    normal = -normal;
                }
                normals.push(normal);
            }
        }
        let mut triangles = Vec::with_capacity(indices.len() / 3);
        for values in indices.chunks_exact(3) {
            let mut triangle = [values[0], values[1], values[2]];
            if uv_triangle_is_degenerate(triangle, &uvs) {
                continue;
            }
            let geometric_normal = (positions[triangle[1]] - positions[triangle[0]])
                .cross(positions[triangle[2]] - positions[triangle[0]]);
            if geometric_normal.magnitude()
                <= self
                    .tolerance()
                    .linear_at_scale(positions[triangle[0]].to_vec().magnitude())
                    .powi(2)
            {
                // Earcut may emit zero-area ears at exact collinear BREP
                // vertices. They cover no surface and must not enter either
                // display meshes or physical certification.
                continue;
            }
            let desired_normal = normals[triangle[0]] + normals[triangle[1]] + normals[triangle[2]];
            if geometric_normal.dot(desired_normal) < 0.0 {
                triangle.swap(1, 2);
            }
            triangles.push(triangle);
        }
        refine_surface_triangles(
            tessellation_surface,
            face.orientation(),
            options,
            &boundary_edges,
            &mut uvs,
            &mut positions,
            &mut normals,
            &mut triangles,
        )?;
        Ok(TriangleMesh {
            positions,
            normals,
            triangle_faces: vec![face_id; triangles.len()],
            triangles,
        })
    }

    fn sample_loop(
        &self,
        loop_id: LoopId,
        options: TessellationOptions,
        cache: &mut BTreeMap<EdgeId, EdgePolyline>,
    ) -> KernelResult<Vec<BoundaryVertex>> {
        let loop_value = self.topology().loop_(loop_id)?;
        let first = loop_value.first().ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "cannot tessellate an empty loop",
            )
        })?;
        let face = self.topology().face(loop_value.face())?;
        let mut output = Vec::new();
        let mut current = first;
        loop {
            let coedge = self.topology().coedge(current)?;
            if let std::collections::btree_map::Entry::Vacant(entry) = cache.entry(coedge.edge()) {
                let polyline = self.sample_edge_for_faces(coedge.edge(), options)?;
                entry.insert(polyline);
            }
            let polyline = cache.get(&coedge.edge()).ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::MissingEntity,
                    "edge tessellation cache lost a sampled edge",
                )
            })?;
            let mut samples: Vec<(f64, Point3)> = polyline
                .parameters
                .iter()
                .copied()
                .zip(polyline.points.iter().copied())
                .collect();
            if !coedge.is_forward() {
                samples.reverse();
            }
            let edge = self.topology().edge(coedge.edge())?;
            for (sample_index, (parameter, edge_point)) in samples.iter().enumerate() {
                if sample_index + 1 == samples.len() {
                    continue;
                }
                let alpha = if coedge.is_forward() {
                    (*parameter - edge.range().start) / edge.range().span()
                } else {
                    (edge.range().end - *parameter) / edge.range().span()
                };
                let pcurve_parameter = coedge.range().start + alpha * coedge.range().span();
                let uv = coedge.pcurve().evaluate(pcurve_parameter)?.point;
                let surface_point = face.surface().first_order(uv.x, uv.y)?.0;
                let allowed = self
                    .tolerance()
                    .linear_at_scale(edge_point.to_vec().magnitude())
                    .max(edge.tolerance())
                    .max(face.tolerance());
                if surface_point.distance(*edge_point) > allowed {
                    return Err(KernelError::from_diagnostic(
                        Diagnostic::new(
                            DiagnosticKind::GeometryInvariant,
                            "edge and p-curve disagree between vertices",
                        )
                        .with_entity(coedge.name()),
                    ));
                }
                output.push(BoundaryVertex {
                    uv,
                    point: *edge_point,
                });
            }
            current = coedge.next();
            if current == first {
                break;
            }
            if output.len() > 10_000_000 {
                return Err(KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "face boundary exceeded tessellation safety limit",
                ));
            }
        }
        Ok(output)
    }

    fn sample_edge_for_faces(
        &self,
        edge_id: EdgeId,
        options: TessellationOptions,
    ) -> KernelResult<EdgePolyline> {
        let edge = self.topology().edge(edge_id)?;
        sample_edge_with_normal_constraint(edge, options, &|first, last| {
            let mut largest = 0.0_f64;
            for coedge_id in edge.coedges() {
                let coedge = self.topology().coedge(*coedge_id)?;
                let loop_value = self.topology().loop_(coedge.loop_id())?;
                let face = self.topology().face(loop_value.face())?;
                let normal_at = |parameter: f64| -> KernelResult<Vec3> {
                    let alpha = if coedge.is_forward() {
                        (parameter - edge.range().start) / edge.range().span()
                    } else {
                        (edge.range().end - parameter) / edge.range().span()
                    };
                    let pcurve_parameter = coedge.range().start + alpha * coedge.range().span();
                    let uv = coedge.pcurve().evaluate(pcurve_parameter)?.point;
                    surface_point_normal(face.surface(), uv.x, uv.y).map(|value| value.1)
                };
                largest = largest.max(normal_at(first)?.angle(normal_at(last)?).0);
            }
            Ok(largest)
        })
    }

    /// Sample the canonical trim of a complete periodic cylinder band.
    ///
    /// An exact circle is partitioned into arcs while Euler construction is in
    /// progress. After adjacent bands are sewn, the remaining same-face seam
    /// edges are topological parameter seams rather than trim boundaries. A
    /// planar triangulator cannot consume that seam-spiked loop directly, so
    /// derive the equivalent rectangle in the cylinder's exact `(u, v)`
    /// domain. Partial cylinders and cylinders with non-circular trims fall
    /// through to ordinary p-curve tessellation.
    fn sample_full_periodic_cylinder_band(
        &self,
        face_id: FaceId,
        options: TessellationOptions,
    ) -> KernelResult<Option<Vec<BoundaryVertex>>> {
        let face = self.topology().face(face_id)?;
        let super::Surface3::Cylinder(cylinder) = face.surface() else {
            return Ok(None);
        };
        let mut levels = Vec::new();
        let mut has_parameter_seam = false;
        for edge_id in self.topology().face_edges(face_id)? {
            let edge = self.topology().edge(edge_id)?;
            let adjacent = self.topology().edge_faces(edge_id)?;
            if adjacent.len() == 1 {
                // The intrinsic cylinder seam is longitudinal. Sewing two
                // complete bands can additionally leave a circular same-face
                // seam at the former interface; neither trims the face.
                has_parameter_seam = true;
                continue;
            }
            if !matches!(edge.curve(), super::Curve3::Circle(_)) {
                return Ok(None);
            }
            let point = edge.curve().evaluate(edge.range().start)?.point;
            let projection = face.surface().project(point, self.tolerance())?;
            if !levels.iter().any(|level: &f64| {
                (*level - projection.parameters.1).abs() <= self.tolerance().parameter * 8.0
            }) {
                levels.push(projection.parameters.1);
            }
        }
        if !has_parameter_seam || levels.len() != 2 {
            return Ok(None);
        }
        levels.sort_by(f64::total_cmp);

        let radius = cylinder.radius();
        let chord_angle = if options.chord_tolerance >= radius * 2.0 {
            std::f64::consts::PI
        } else {
            2.0 * (1.0 - options.chord_tolerance / radius)
                .clamp(-1.0, 1.0)
                .acos()
        };
        let maximum_angle = options.angular_tolerance.min(chord_angle);
        let segments = (std::f64::consts::TAU / maximum_angle).ceil().max(8.0) as usize;
        let mut ring = Vec::with_capacity((segments + 1) * 2);
        for index in 0..=segments {
            let u = std::f64::consts::TAU * index as f64 / segments as f64;
            let point = face.surface().first_order(u, levels[0])?.0;
            ring.push(BoundaryVertex {
                uv: Point2::new(u, levels[0]),
                point,
            });
        }
        for index in (0..=segments).rev() {
            let u = std::f64::consts::TAU * index as f64 / segments as f64;
            let point = face.surface().first_order(u, levels[1])?.0;
            ring.push(BoundaryVertex {
                uv: Point2::new(u, levels[1]),
                point,
            });
        }
        Ok(Some(ring))
    }
}

/// Lift periodic surface parameters onto one continuous sheet before planar
/// triangulation.  P-curves are allowed to use different representatives of
/// the same periodic coordinate (for example `0` and `2π` on a cylinder),
/// especially after exact Euler sewing.  Earcut needs a continuous planar
/// polygon, so choose the equivalent representative nearest the preceding
/// boundary sample.  This changes only the rendering parameter lift; every
/// lifted value evaluates to the same exact surface point.
fn unwrap_periodic_ring(
    surface: &super::Surface3,
    tolerance: ModelTolerance,
    ring: &mut [BoundaryVertex],
) -> KernelResult<()> {
    let domains = surface.domains();
    if !domains.iter().any(|domain| domain.period.is_some()) {
        return Ok(());
    }
    // Re-project the exact 3D boundary point before choosing a lift. Sewn
    // p-curves can legitimately inherit different angular frames from their
    // source faces; projection supplies one canonical representative on the
    // surviving surface and the loop order supplies the winding.
    for vertex in ring.iter_mut() {
        let original_uv = vertex.uv;
        let (_, du, dv) = surface.first_order(original_uv.x, original_uv.y)?;
        let singular_periodic_coordinate =
            domains.iter().enumerate().any(|(coordinate, domain)| {
                domain.period.is_some()
                    && if coordinate == 0 {
                        du.magnitude() <= tolerance.linear
                    } else {
                        dv.magnitude() <= tolerance.linear
                    }
            });
        // Projection cannot choose a unique periodic parameter at a collapsed
        // pole. Preserve the exact p-curve representative there; replacing it
        // with an arbitrary canonical angle can make a small face appear to
        // wind almost a full turn in its tessellation chart.
        if singular_periodic_coordinate {
            let lifted = surface.evaluate(original_uv.x, original_uv.y)?.point;
            if lifted.distance(vertex.point)
                > tolerance.linear_at_scale(vertex.point.to_vec().magnitude().max(1.0))
            {
                return Err(KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "periodic face boundary does not lie on its retained surface",
                ));
            }
            continue;
        }
        let projection = surface.project(vertex.point, tolerance)?;
        if projection.distance
            > tolerance.linear_at_scale(vertex.point.to_vec().magnitude().max(1.0))
        {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "periodic face boundary does not lie on its retained surface",
            ));
        }
        vertex.uv = Point2::new(projection.parameters.0, projection.parameters.1);
    }
    for (coordinate, period) in domains
        .iter()
        .enumerate()
        .filter_map(|(coordinate, domain)| domain.period.map(|period| (coordinate, period)))
    {
        let Some(first) = ring.first() else {
            continue;
        };
        let mut previous = if coordinate == 0 {
            first.uv.x
        } else {
            first.uv.y
        };
        for vertex in ring.iter_mut().skip(1) {
            let value = if coordinate == 0 {
                &mut vertex.uv.x
            } else {
                &mut vertex.uv.y
            };
            *value += ((previous - *value) / period).round() * period;
            previous = *value;
        }
    }
    Ok(())
}

fn ordered_edge(first: usize, second: usize) -> (usize, usize) {
    if first < second {
        (first, second)
    } else {
        (second, first)
    }
}

#[allow(clippy::too_many_arguments)]
fn refine_surface_triangles(
    surface: &super::Surface3,
    orientation: FaceOrientation,
    options: TessellationOptions,
    boundary_edges: &BTreeSet<(usize, usize)>,
    uvs: &mut Vec<Point2>,
    positions: &mut Vec<Point3>,
    normals: &mut Vec<Vec3>,
    triangles: &mut Vec<[usize; 3]>,
) -> KernelResult<()> {
    for depth in 0..=options.maximum_depth {
        let mut marked_edges = BTreeSet::new();
        let mut edge_scores = BTreeMap::new();
        let mut centroid_splits = BTreeSet::new();
        let mut unique_edges = BTreeSet::new();
        for triangle in triangles.iter() {
            for edge in triangle_edges(*triangle) {
                unique_edges.insert(edge);
            }
        }
        for edge in unique_edges {
            if !boundary_edges.contains(&edge) {
                let (chord_error, mut angular_error) =
                    surface_edge_error(surface, orientation, edge, uvs, positions)?;
                if let Some(resolved_error) = collapsed_periodic_angular_error(
                    surface,
                    &[uvs[edge.0], uvs[edge.1]],
                    options.chord_tolerance,
                )? {
                    angular_error = resolved_error;
                }
                edge_scores.insert(
                    edge,
                    (chord_error / options.chord_tolerance)
                        .max(angular_error / options.angular_tolerance),
                );
                if chord_error <= options.chord_tolerance
                    && angular_error <= options.angular_tolerance
                {
                    continue;
                }
                marked_edges.insert(edge);
            }
        }
        for (index, triangle) in triangles.iter().copied().enumerate() {
            let (chord_error, mut angular_error) =
                surface_triangle_error(surface, orientation, triangle, uvs, positions)?;
            let [a, b, c] = triangle;
            if let Some(resolved_error) = collapsed_periodic_angular_error(
                surface,
                &[uvs[a], uvs[b], uvs[c]],
                options.chord_tolerance,
            )? {
                angular_error = resolved_error;
            }
            if chord_error > options.chord_tolerance || angular_error > options.angular_tolerance {
                let interior: Vec<_> = triangle_edges(triangle)
                    .into_iter()
                    .filter(|edge| !boundary_edges.contains(edge))
                    .collect();
                if interior.is_empty() {
                    centroid_splits.insert(index);
                } else if let Some(edge) = interior.into_iter().max_by(|first, second| {
                    edge_scores
                        .get(first)
                        .copied()
                        .unwrap_or_default()
                        .total_cmp(&edge_scores.get(second).copied().unwrap_or_default())
                }) {
                    marked_edges.insert(edge);
                }
            }
        }
        if marked_edges.is_empty() && centroid_splits.is_empty() {
            return Ok(());
        }
        if depth == options.maximum_depth {
            let mut worst_chord = 0.0_f64;
            let mut worst_angle = 0.0_f64;
            for triangle in triangles.iter().copied() {
                let (chord, mut angle) =
                    surface_triangle_error(surface, orientation, triangle, uvs, positions)?;
                let [a, b, c] = triangle;
                if let Some(resolved_error) = collapsed_periodic_angular_error(
                    surface,
                    &[uvs[a], uvs[b], uvs[c]],
                    options.chord_tolerance,
                )? {
                    angle = resolved_error;
                }
                worst_chord = worst_chord.max(chord);
                worst_angle = worst_angle.max(angle);
            }
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                format!(
                    "adaptive {} surface tessellation could not satisfy requested tolerances at depth {} ({} triangles; chord error {}, requested {}; angular error {}, requested {})",
                    surface_kind(surface),
                    options.maximum_depth,
                    triangles.len(),
                    worst_chord,
                    options.chord_tolerance,
                    worst_angle,
                    options.angular_tolerance,
                ),
            ));
        }

        let mut midpoints = BTreeMap::new();
        for edge in &marked_edges {
            let uv = Point2::from_vec((uvs[edge.0].to_vec() + uvs[edge.1].to_vec()) * 0.5);
            let (point, mut normal) = surface_point_normal(surface, uv.x, uv.y)?;
            if orientation == FaceOrientation::Reversed {
                normal = -normal;
            }
            let index = positions.len();
            uvs.push(uv);
            positions.push(point);
            normals.push(normal);
            midpoints.insert(*edge, index);
        }

        let old_triangles = std::mem::take(triangles);
        for (index, triangle) in old_triangles.into_iter().enumerate() {
            let [a, b, c] = triangle;
            let ab = midpoints.get(&ordered_edge(a, b)).copied();
            let bc = midpoints.get(&ordered_edge(b, c)).copied();
            let ca = midpoints.get(&ordered_edge(c, a)).copied();
            match (ab, bc, ca) {
                (None, None, None) if centroid_splits.contains(&index) => {
                    let uv = Point2::from_vec(
                        (uvs[a].to_vec() + uvs[b].to_vec() + uvs[c].to_vec()) / 3.0,
                    );
                    let (point, mut normal) = surface_point_normal(surface, uv.x, uv.y)?;
                    if orientation == FaceOrientation::Reversed {
                        normal = -normal;
                    }
                    let center = positions.len();
                    uvs.push(uv);
                    positions.push(point);
                    normals.push(normal);
                    triangles.extend([[a, b, center], [b, c, center], [c, a, center]]);
                }
                (None, None, None) => triangles.push(triangle),
                (Some(ab), None, None) => triangles.extend([[a, ab, c], [ab, b, c]]),
                (None, Some(bc), None) => triangles.extend([[b, bc, a], [bc, c, a]]),
                (None, None, Some(ca)) => triangles.extend([[c, ca, b], [ca, a, b]]),
                (Some(ab), Some(bc), None) => {
                    triangles.extend([[b, bc, ab], [a, ab, c], [ab, bc, c]])
                }
                (None, Some(bc), Some(ca)) => {
                    triangles.extend([[c, ca, bc], [b, bc, a], [bc, ca, a]])
                }
                (Some(ab), None, Some(ca)) => {
                    triangles.extend([[a, ab, ca], [b, c, ab], [ab, c, ca]])
                }
                (Some(ab), Some(bc), Some(ca)) => {
                    triangles.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]])
                }
            }
        }
        triangles.retain(|triangle| !uv_triangle_is_degenerate(*triangle, uvs));
    }
    Ok(())
}

fn uv_triangle_is_degenerate(triangle: [usize; 3], uvs: &[Point2]) -> bool {
    let first = uvs[triangle[1]] - uvs[triangle[0]];
    let second = uvs[triangle[2]] - uvs[triangle[0]];
    let positive = first.x * second.y;
    let negative = first.y * second.x;
    let determinant = positive - negative;
    let rounding_bound = (positive.abs() + negative.abs()) * f64::EPSILON * 64.0;
    determinant.abs() <= rounding_bound
}

fn surface_kind(surface: &super::Surface3) -> &'static str {
    match surface {
        super::Surface3::Plane(_) => "plane",
        super::Surface3::Cylinder(_) => "cylinder",
        super::Surface3::Cone(_) => "cone",
        super::Surface3::Sphere(_) => "sphere",
        super::Surface3::Torus(_) => "torus",
        super::Surface3::Revolved(_) => "revolved",
        super::Surface3::Nurbs(_) => "NURBS",
        super::Surface3::Offset(_) => "offset",
        super::Surface3::RollingBall(_) => "rolling-ball",
    }
}

fn collapsed_periodic_angular_error(
    surface: &super::Surface3,
    uvs: &[Point2],
    chord_tolerance: f64,
) -> KernelResult<Option<f64>> {
    // These periodic analytical surfaces are regular everywhere, so avoid
    // extra first-order evaluations on their hot tessellation paths.
    if matches!(
        surface,
        super::Surface3::Cylinder(_) | super::Surface3::Torus(_)
    ) {
        return Ok(None);
    }

    let domains = surface.domains();
    let periodic_coordinates = domains
        .iter()
        .enumerate()
        .filter_map(|(coordinate, domain)| domain.period.map(|period| (coordinate, period)))
        .collect::<Vec<_>>();
    if periodic_coordinates.is_empty() {
        return Ok(None);
    }
    let samples = uvs
        .iter()
        .map(|uv| surface.first_order(uv.x, uv.y))
        .collect::<KernelResult<Vec<_>>>()?;
    for (coordinate, period) in periodic_coordinates {
        let tangent = |sample: &(Point3, Vec3, Vec3)| {
            if coordinate == 0 {
                sample.1
            } else {
                sample.2
            }
        };
        let has_pole = samples.iter().any(|sample| {
            let derivative_scale = sample.1.magnitude().max(sample.2.magnitude()).max(1.0);
            tangent(sample).magnitude() <= derivative_scale * f64::EPSILON.sqrt()
        });
        if !has_pole {
            continue;
        }
        let values = uvs
            .iter()
            .map(|uv| if coordinate == 0 { uv.x } else { uv.y });
        let (minimum, maximum) = values.fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
        );
        let parameter_span = (maximum - minimum).min(period);
        let maximum_tangent = samples
            .iter()
            .map(|sample| tangent(sample).magnitude())
            .fold(0.0_f64, f64::max);
        if maximum_tangent * parameter_span > chord_tolerance {
            continue;
        }

        // At a pole, changing the periodic parameter changes the limiting
        // normal but not the surface point. Once that collapsed-direction
        // width is below the chord tolerance, retain only the genuine normal
        // variation along the profile direction. Unlike setting the angular
        // error to zero, this still refines curved meridians normally.
        let reference = if coordinate == 0 { uvs[0].x } else { uvs[0].y };
        let normals = uvs
            .iter()
            .map(|uv| {
                let fixed = if coordinate == 0 {
                    Point2::new(reference, uv.y)
                } else {
                    Point2::new(uv.x, reference)
                };
                surface_point_normal(surface, fixed.x, fixed.y).map(|value| value.1)
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let mut angular_error = 0.0_f64;
        for first in 0..normals.len() {
            for second in first + 1..normals.len() {
                angular_error = angular_error.max(normals[first].angle(normals[second]).0);
            }
        }
        return Ok(Some(angular_error));
    }
    Ok(None)
}

fn surface_point_normal(surface: &super::Surface3, u: f64, v: f64) -> KernelResult<(Point3, Vec3)> {
    let (point, du, dv) = surface.first_order(u, v)?;
    let mut normal = match surface {
        // Longitude is singular at a sphere's poles even though the exact
        // surface normal remains well defined there.
        super::Surface3::Sphere(sphere) => point - sphere.center(),
        _ => du.cross(dv),
    };
    if normal.magnitude2() == 0.0 {
        if let super::Surface3::Revolved(revolved) = surface {
            let range = revolved.profile_range();
            let inset = range.span().abs().max(1.0) * f64::EPSILON.sqrt();
            let shifted = if (v - range.start).abs() <= (v - range.end).abs() {
                range.start + range.span().signum() * inset
            } else {
                range.end - range.span().signum() * inset
            };
            let (_, shifted_du, shifted_dv) = surface.first_order(u, shifted)?;
            normal = shifted_du.cross(shifted_dv);
        }
    }
    if normal.magnitude2() == 0.0
        || !normal.x.is_finite()
        || !normal.y.is_finite()
        || !normal.z.is_finite()
    {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "surface derivatives are singular at the requested parameter",
        ));
    }
    Ok((point, normal.normalize()))
}

fn triangle_edges(triangle: [usize; 3]) -> [(usize, usize); 3] {
    [
        ordered_edge(triangle[0], triangle[1]),
        ordered_edge(triangle[1], triangle[2]),
        ordered_edge(triangle[2], triangle[0]),
    ]
}

#[allow(clippy::too_many_arguments)]
fn surface_edge_error(
    surface: &super::Surface3,
    orientation: FaceOrientation,
    edge: (usize, usize),
    uvs: &[Point2],
    positions: &[Point3],
) -> KernelResult<(f64, f64)> {
    let evaluate = |fraction: f64| -> KernelResult<(Point3, Point3, Vec3)> {
        let uv = Point2::from_vec(
            uvs[edge.0].to_vec() * (1.0 - fraction) + uvs[edge.1].to_vec() * fraction,
        );
        let (point, mut normal) = surface_point_normal(surface, uv.x, uv.y)?;
        let chord = Point3::from_vec(
            positions[edge.0].to_vec() * (1.0 - fraction) + positions[edge.1].to_vec() * fraction,
        );
        if orientation == FaceOrientation::Reversed {
            normal = -normal;
        }
        Ok((point, chord, normal))
    };
    let first = evaluate(0.25)?;
    let middle = evaluate(0.5)?;
    let last = evaluate(0.75)?;
    let chord_error = first
        .0
        .distance(first.1)
        .max(middle.0.distance(middle.1))
        .max(last.0.distance(last.1));
    let angular_error = first.2.angle(last.2).0;
    Ok((chord_error, angular_error))
}

#[allow(clippy::too_many_arguments)]
fn surface_triangle_error(
    surface: &super::Surface3,
    orientation: FaceOrientation,
    triangle: [usize; 3],
    uvs: &[Point2],
    positions: &[Point3],
) -> KernelResult<(f64, f64)> {
    let [a, b, c] = triangle;
    let center_uv = Point2::from_vec((uvs[a].to_vec() + uvs[b].to_vec() + uvs[c].to_vec()) / 3.0);
    let center_chord = Point3::from_vec(
        (positions[a].to_vec() + positions[b].to_vec() + positions[c].to_vec()) / 3.0,
    );
    let evaluate = |vertex: usize| -> KernelResult<(Point3, Point3, Vec3)> {
        let uv = Point2::from_vec((uvs[vertex].to_vec() + center_uv.to_vec()) * 0.5);
        let (point, mut normal) = surface_point_normal(surface, uv.x, uv.y)?;
        let chord = Point3::from_vec((positions[vertex].to_vec() + center_chord.to_vec()) * 0.5);
        if orientation == FaceOrientation::Reversed {
            normal = -normal;
        }
        Ok((point, chord, normal))
    };
    let first = evaluate(a)?;
    let second = evaluate(b)?;
    let third = evaluate(c)?;
    let chord_error = first
        .0
        .distance(first.1)
        .max(second.0.distance(second.1))
        .max(third.0.distance(third.1));
    let angular_error = first
        .2
        .angle(second.2)
        .0
        .max(second.2.angle(third.2).0)
        .max(third.2.angle(first.2).0);
    Ok((chord_error, angular_error))
}

impl TessellationOptions {
    fn validate_against(self, model: ModelTolerance) -> KernelResult<()> {
        Self::new(
            self.chord_tolerance,
            self.angular_tolerance,
            self.maximum_depth,
        )?;
        if self.chord_tolerance < model.linear {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "tessellation chord tolerance may not be smaller than model tolerance",
            ));
        }
        Ok(())
    }
}

fn sample_edge(edge: &Edge, options: TessellationOptions) -> KernelResult<EdgePolyline> {
    sample_edge_with_normal_constraint(edge, options, &|_, _| Ok(0.0))
}

fn sample_edge_with_normal_constraint(
    edge: &Edge,
    options: TessellationOptions,
    normal_angle: &dyn Fn(f64, f64) -> KernelResult<f64>,
) -> KernelResult<EdgePolyline> {
    let start = curve_sample(edge.curve(), edge.range().start)?;
    let end = curve_sample(edge.curve(), edge.range().end)?;
    let mut samples = vec![(edge.range().start, start.point)];
    sample_interval(
        edge,
        edge.range().start,
        start,
        edge.range().end,
        end,
        0,
        options,
        normal_angle,
        &mut samples,
    )?;
    let (parameters, points): (Vec<_>, Vec<_>) = samples.into_iter().unzip();
    Ok(EdgePolyline { parameters, points })
}

#[allow(clippy::too_many_arguments)]
fn sample_interval(
    edge: &Edge,
    start_parameter: f64,
    start: CurveSample,
    end_parameter: f64,
    end: CurveSample,
    depth: u8,
    options: TessellationOptions,
    normal_angle: &dyn Fn(f64, f64) -> KernelResult<f64>,
    output: &mut Vec<(f64, Point3)>,
) -> KernelResult<()> {
    let middle_parameter = (start_parameter + end_parameter) * 0.5;
    let middle = curve_sample(edge.curve(), middle_parameter)?;
    let first_quarter_parameter = (start_parameter + middle_parameter) * 0.5;
    let third_quarter_parameter = (middle_parameter + end_parameter) * 0.5;
    let first_quarter = curve_sample(edge.curve(), first_quarter_parameter)?;
    let third_quarter = curve_sample(edge.curve(), third_quarter_parameter)?;
    let chord_middle = start.point + (end.point - start.point) * 0.5;
    let chord_first_quarter = start.point + (end.point - start.point) * 0.25;
    let chord_third_quarter = start.point + (end.point - start.point) * 0.75;
    let deviation = middle
        .point
        .distance(chord_middle)
        .max(first_quarter.point.distance(chord_first_quarter))
        .max(third_quarter.point.distance(chord_third_quarter));
    // Implicit support intersections can have no unique two-sided tangent at a
    // trim endpoint even though the retained one-sided edge is regular. Use
    // interior tangent samples for the angular error test; the three chord
    // samples still constrain the complete closed interval geometrically.
    let tangent_angle = if first_quarter.first_derivative.magnitude2() == 0.0
        || third_quarter.first_derivative.magnitude2() == 0.0
    {
        std::f64::consts::PI
    } else {
        first_quarter
            .first_derivative
            .angle(third_quarter.first_derivative)
            .0
    };
    let adjacent_normal_angle = normal_angle(first_quarter_parameter, third_quarter_parameter)?;
    // The interior samples span the middle half of the interval. Limiting
    // their turn to half the requested angle bounds the complete segment
    // without evaluating potentially ambiguous two-sided endpoint tangents.
    let interior_angular_tolerance = options.angular_tolerance * 0.5;
    if deviation <= options.chord_tolerance
        && tangent_angle <= interior_angular_tolerance
        && adjacent_normal_angle <= interior_angular_tolerance
    {
        output.push((end_parameter, end.point));
        return Ok(());
    }
    if depth >= options.maximum_depth {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "adaptive edge tessellation could not satisfy requested tolerances",
        ));
    }
    sample_interval(
        edge,
        start_parameter,
        start,
        middle_parameter,
        middle,
        depth + 1,
        options,
        normal_angle,
        output,
    )?;
    sample_interval(
        edge,
        middle_parameter,
        middle,
        end_parameter,
        end,
        depth + 1,
        options,
        normal_angle,
        output,
    )
}
