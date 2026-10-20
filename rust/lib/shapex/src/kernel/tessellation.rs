use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec3};

use super::polygon::ordered;
use super::{
    nearest_uv, BodyId, Diagnostic, DiagnosticKind, Edge, EdgeId, FaceId, FaceOrientation, Kernel, KernelError,
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

/// Retain boundary samples across face calls on the same BREP state. Only
/// the most recent error policy is retained, bounding storage by its edges.
#[derive(Debug, Default)]
pub(super) struct TessellationCache {
    options: Option<TessellationOptions>,
    edges: BTreeMap<EdgeId, EdgePolyline>,
}

impl TessellationCache {
    fn edges(&mut self, options: TessellationOptions) -> &mut BTreeMap<EdgeId, EdgePolyline> {
        if self.options != Some(options) {
            self.edges.clear();
            self.options = Some(options);
        }
        &mut self.edges
    }
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

struct SurfaceTriangulationSeed {
    uvs: Vec<Point2>,
    positions: Vec<Point3>,
    boundary_edges: BTreeSet<(usize, usize)>,
    triangles: Vec<[usize; 3]>,
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
    // A retained pole on the boundary already has explicit meridian charts.
    // Reorienting such a cell can put a new pole inside it; keep its native
    // chart and the collapsed boundary bridge instead.
    if rings.iter().flatten().any(|vertex| {
        (vertex.point - sphere.center()).dot(sphere.z_radius()).abs()
            >= sphere.radius().powi(2) - tolerance.linear * sphere.radius()
    }) {
        return Ok(None);
    }
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
        let mut cache = self.tessellation_cache.lock().unwrap();
        let edges = cache.edges(options);
        let mut mesh = TriangleMesh::default();
        for shell in self.topology().body(body)?.shells() {
            mesh.append(self.tessellate_shell_cached(*shell, options, edges)?);
        }
        Ok(mesh)
    }

    pub fn tessellate_shell(
        &self,
        shell: ShellId,
        options: TessellationOptions,
    ) -> KernelResult<TriangleMesh> {
        options.validate_against(self.tolerance())?;
        let mut cache = self.tessellation_cache.lock().unwrap();
        self.tessellate_shell_cached(shell, options, cache.edges(options))
    }

    pub fn tessellate_face(
        &self,
        face: FaceId,
        options: TessellationOptions,
    ) -> KernelResult<TriangleMesh> {
        options.validate_against(self.tolerance())?;
        let mut cache = self.tessellation_cache.lock().unwrap();
        self.tessellate_face_cached(face, options, cache.edges(options))
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
        if let Some(seed) = self.sample_rectangular_chart(face_id, options)?
            .or(self.sample_full_periodic_band(face_id, options)?) {
            return finish_surface_triangulation(
                face.surface(),
                face.orientation(),
                face_id,
                self.tolerance(),
                options,
                seed,
            );
        }

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
        let chart = sphere_tessellation_chart(face.surface(), &rings, self.tolerance())?;
        let tessellation_surface = chart.as_ref().unwrap_or_else(|| face.surface());
        for ring in &mut rings {
            if chart.is_some() {
                // The retained UVs belong to the BREP frame. Rebind them before
                // unwrapping so a former pole is not mistaken for a pole of
                // the independently oriented rendering chart.
                for vertex in ring.iter_mut() {
                    let uv = tessellation_surface.project(vertex.point, self.tolerance())?.parameters;
                    vertex.uv = Point2::new(uv.0, uv.1);
                }
            }
            unwrap_periodic_ring(tessellation_surface, self.tolerance(), ring)?;
        }
        if rings.is_empty() || rings[0].len() < 3 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "a tessellated face needs an outer loop with at least three boundary samples",
            ));
        }

        // Unwrapping makes each loop continuous, but its first coedge can
        // still select a different periodic copy from the outer boundary.
        // Place holes on the outer loop's sheet before planar triangulation.
        let bounds_center = |ring: &[BoundaryVertex]| {
            let mut min = Point2::new(f64::INFINITY, f64::INFINITY);
            let mut max = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
            for vertex in ring {
                min.x = min.x.min(vertex.uv.x);
                min.y = min.y.min(vertex.uv.y);
                max.x = max.x.max(vertex.uv.x);
                max.y = max.y.max(vertex.uv.y);
            }
            Point2::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5)
        };
        let outer_center = bounds_center(&rings[0]);
        let domains = tessellation_surface.domains();
        for ring in rings.iter_mut().skip(1) {
            let center = bounds_center(ring);
            let offset = crate::Vec2::new(
                domains[0]
                    .period
                    .map_or(0.0, |p| ((outer_center.x - center.x) / p).round() * p),
                domains[1]
                    .period
                    .map_or(0.0, |p| ((outer_center.y - center.y) / p).round() * p),
            );
            for vertex in ring {
                vertex.uv += offset;
            }
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
                boundary_edges.insert(ordered(
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
        // Short strip diagonals avoid excessive refinement of triangle fans
        // on curved faces. This depends on the trim's shape, not its support
        // type; cylinders and tori benefit just like swept and blended faces.
        if holes.is_empty() && !matches!(tessellation_surface, super::Surface3::Plane(_)) {
            if let Some(indices) = triangulate_boundary_strip(&uvs) {
                let seed = SurfaceTriangulationSeed {
                    uvs,
                    positions,
                    boundary_edges,
                    triangles: indices
                        .chunks_exact(3)
                        .map(|v| [v[0], v[1], v[2]])
                        .collect(),
                };
                return finish_surface_triangulation(
                    tessellation_surface,
                    face.orientation(),
                    face_id,
                    self.tolerance(),
                    options,
                    seed,
                );
            }
        }
        let indices = earcutr::earcut(&flat_uv, &holes, 2);
        if indices.len() < 3 || indices.len() % 3 != 0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "trimmed face triangulation failed",
            ));
        }

        let mut triangles = Vec::with_capacity(indices.len() / 3);
        for values in indices.chunks_exact(3) {
            let triangle = [values[0], values[1], values[2]];
            if uv_triangle_is_degenerate(triangle, &uvs) {
                continue;
            }
            triangles.push(triangle);
        }
        finish_surface_triangulation(
            tessellation_surface,
            face.orientation(),
            face_id,
            self.tolerance(),
            options,
            SurfaceTriangulationSeed {
                uvs,
                positions,
                boundary_edges,
                triangles,
            },
        )
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
                    let end = coedge.pcurve().evaluate(coedge.range().end)?.point;
                    let next = self.topology().coedge(coedge.next())?;
                    let start = next.pcurve().evaluate(next.range().start)?.point;
                    let (_, du, dv) = face.surface().first_order(end.x, end.y)?;
                    let collapsed =
                        face.surface()
                            .domains()
                            .iter()
                            .enumerate()
                            .any(|(axis, domain)| {
                                domain.period.is_some()
                                    && if axis == 0 {
                                        du.magnitude() <= self.tolerance().linear
                                    } else {
                                        dv.magnitude() <= self.tolerance().linear
                                    }
                            });
                    if !collapsed || end.distance(start) <= self.tolerance().parameter {
                        continue;
                    }
                    // A pole joins two distinct UV endpoints at one 3D point.
                    // Keep the collapsed chart edge for planar triangulation.
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
        sample_edge_with_surface_constraint(edge, options, &|first, last| {
            let mut chord_error = 0.0_f64;
            let mut angular_error = 0.0_f64;
            for coedge_id in edge.coedges() {
                let coedge = self.topology().coedge(*coedge_id)?;
                let loop_value = self.topology().loop_(coedge.loop_id())?;
                let face = self.topology().face(loop_value.face())?;
                let uv_at = |parameter: f64| -> KernelResult<Point2> {
                    let alpha = if coedge.is_forward() {
                        (parameter - edge.range().start) / edge.range().span()
                    } else {
                        (edge.range().end - parameter) / edge.range().span()
                    };
                    let pcurve_parameter = coedge.range().start + alpha * coedge.range().span();
                    Ok(coedge.pcurve().evaluate(pcurve_parameter)?.point)
                };
                let normal_at = |parameter| -> KernelResult<Vec3> {
                    let uv = uv_at(parameter)?;
                    surface_point_normal(face.surface(), uv.x, uv.y).map(|value| value.1)
                };
                angular_error = angular_error.max(
                    normal_at(first + (last - first) * 0.25)?
                        .angle(normal_at(first + (last - first) * 0.75)?)
                        .0,
                );
                // Interior refinement keeps shared boundary segments fixed.
                // Their straight UV chords must therefore satisfy the same
                // surface error policy as interior edges. A 3D edge can be
                // almost straight while its support's parameterization is not.
                let uvs = [uv_at(first)?, uv_at(last)?];
                let positions = [
                    curve_sample(edge.curve(), first)?.point,
                    curve_sample(edge.curve(), last)?.point,
                ];
                let (chord, angle) = surface_edge_error(
                    face.surface(), face.orientation(), (0, 1), &uvs, &positions,
                )?;
                chord_error = chord_error.max(chord);
                angular_error = angular_error.max(angle);
            }
            Ok((chord_error, angular_error))
        })
    }

    /// Sweeps and edge blends can retain rectangular charts even when sewing
    /// reparameterizes their shared edges. Sample these as a grid instead of
    /// refining skinny triangle fans across long, narrow curved faces.
    fn sample_rectangular_chart(
        &self,
        face_id: FaceId,
        options: TessellationOptions,
    ) -> KernelResult<Option<SurfaceTriangulationSeed>> {
        let face = self.topology().face(face_id)?;
        if !matches!(
            face.surface(),
            super::Surface3::Swept(_) | super::Surface3::RollingBall(_) | super::Surface3::Ruled(_)
        ) || face.loops().len() != 1
        {
            return Ok(None);
        }
        let mut vertices = Vec::new();
        let mut edge_samples = Vec::new();
        let mut previous: Option<Point2> = None;
        let domains = face.surface().domains();
        for id in self.topology().loop_coedges(face.loops()[0])? {
            let edge = self.topology().coedge(id)?;
            let mut samples = (0..=8)
                .map(|i| {
                    edge.pcurve()
                        .evaluate(edge.range().start + edge.range().span() * i as f64 / 8.0)
                        .map(|value| value.point)
                })
                .collect::<KernelResult<Vec<_>>>()?;
            for sample in &mut samples {
                if let Some(prior) = previous {
                    *sample = nearest_uv(domains, *sample, prior);
                }
                previous = Some(*sample);
            }
            vertices.extend([samples[0], samples[8]]);
            edge_samples.push(samples);
        }
        let bounds = [0, 1].map(|axis| {
            let low = vertices
                .iter()
                .map(|p| p[axis])
                .fold(f64::INFINITY, f64::min);
            let high = vertices
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max);
            [low, high]
        });
        let epsilon = self.tolerance().parameter * 64.0;
        let mut area = 0.0;
        for samples in &edge_samples {
            let on_side = (0..2).any(|axis| {
                bounds[axis]
                    .iter()
                    .any(|level| samples.iter().all(|p| (p[axis] - level).abs() <= epsilon))
            });
            if !on_side {
                return Ok(None);
            }
            let a = samples[0];
            let b = samples[8];
            area += a.x * b.y - b.x * a.y;
        }
        let expected = (bounds[0][1] - bounds[0][0]) * (bounds[1][1] - bounds[1][0]);
        if expected <= epsilon || (area.abs() * 0.5 - expected).abs() > epsilon * expected.max(1.0)
        {
            return Ok(None);
        }
        let mut axes = [Vec::new(), Vec::new()];
        for axis in 0..2 {
            let other = bounds[1 - axis];
            axes[axis] = adaptive_isoparametric_samples(
                face.surface(),
                axis,
                bounds[axis][0],
                bounds[axis][1],
                &[other[0], (other[0] + other[1]) * 0.5, other[1]],
                4,
                options,
            )?;
        }
        Ok(Some(rectangular_surface_seed(face.surface(), axes)?))
    }

    /// Sample the canonical trim of a complete band on a periodic surface.
    ///
    /// A face spanning one full surface period retains a same-face edge so its
    /// Euler loop has a finite chart. That edge is a parameter seam, not a
    /// material trim. Feeding the seam-spiked loop to a planar triangulator
    /// creates triangles across the wrapped chart. Recognize the topological
    /// pattern and express its exact trim as a rectangle on one continuous
    /// parameter sheet. Shapes that are not constant-width periodic bands fall
    /// through to ordinary p-curve tessellation.
    fn sample_full_periodic_band(
        &self,
        face_id: FaceId,
        options: TessellationOptions,
    ) -> KernelResult<Option<SurfaceTriangulationSeed>> {
        let face = self.topology().face(face_id)?;
        if face.loops().len() != 1 {
            return Ok(None);
        }

        let domains = face.surface().domains();
        if !domains.iter().any(|domain| domain.period.is_some()) {
            return Ok(None);
        }

        let loop_id = face.loops()[0];
        let coedges = self.topology().loop_coedges(loop_id)?;
        let parameter_tolerance = self.tolerance().parameter * 64.0;
        let coordinates = |point: Point2| [point.x, point.y];
        let aligned = |value: f64, reference: f64, axis: usize| domains[axis].nearest(value, reference);

        // A genuine chart seam has two uses in this face. Its periodic
        // coordinate is constant while the transverse coordinate changes.
        let mut periodic_axis = None;
        let mut seam_parameter = 0.0;
        let mut transverse_center = 0.0;
        for coedge_id in &coedges {
            let coedge = self.topology().coedge(*coedge_id)?;
            if self.topology().edge_faces(coedge.edge())?.len() != 1 {
                continue;
            }
            let range = coedge.range();
            let samples = [
                coordinates(coedge.pcurve().evaluate(range.start)?.point),
                coordinates(
                    coedge
                        .pcurve()
                        .evaluate((range.start + range.end) * 0.5)?
                        .point,
                ),
                coordinates(coedge.pcurve().evaluate(range.end)?.point),
            ];
            for axis in 0..2 {
                let Some(period) = domains[axis].period else {
                    continue;
                };
                let transverse = 1 - axis;
                let constant = samples.iter().all(|sample| {
                    (aligned(sample[axis], samples[0][axis], axis) - samples[0][axis]).abs()
                        <= parameter_tolerance
                });
                let transverse_span = samples
                    .iter()
                    .map(|sample| {
                        (aligned(sample[transverse], samples[0][transverse], transverse)
                            - samples[0][transverse])
                            .abs()
                    })
                    .fold(0.0_f64, f64::max);
                if constant && transverse_span > parameter_tolerance {
                    periodic_axis = Some(axis);
                    seam_parameter = domains[axis].normalize(samples[0][axis]);
                    transverse_center = samples[1][transverse];
                    debug_assert!(period.is_finite());
                    break;
                }
            }
            if periodic_axis.is_some() {
                break;
            }
        }
        let Some(periodic_axis) = periodic_axis else {
            return Ok(None);
        };
        let transverse_axis = 1 - periodic_axis;
        let period = domains[periodic_axis]
            .period
            .expect("the selected surface coordinate is periodic");

        // Every material boundary of a simple periodic band is an isoparametric
        // curve at one of two transverse levels. Work from its retained
        // p-curve instead of projecting back to an ambiguous periodic chart.
        let mut levels = Vec::new();
        for coedge_id in &coedges {
            let coedge = self.topology().coedge(*coedge_id)?;
            if self.topology().edge_faces(coedge.edge())?.len() == 1 {
                continue;
            }
            let range = coedge.range();
            let samples = [
                coordinates(coedge.pcurve().evaluate(range.start)?.point),
                coordinates(
                    coedge
                        .pcurve()
                        .evaluate((range.start + range.end) * 0.5)?
                        .point,
                ),
                coordinates(coedge.pcurve().evaluate(range.end)?.point),
            ];
            // The seam runs through the interior of the intended band. Its
            // midpoint selects the sheet even when the two boundary levels
            // are half a period apart, or bound a band wider than half a turn.
            let level = aligned(samples[1][transverse_axis], transverse_center, transverse_axis);
            if samples.iter().any(|sample| {
                (aligned(sample[transverse_axis], level, transverse_axis) - level).abs()
                    > parameter_tolerance
            }) {
                return Ok(None);
            }
            if !levels
                .iter()
                .any(|existing: &f64| (*existing - level).abs() <= parameter_tolerance)
            {
                levels.push(level);
            }
        }
        if levels.len() != 2 {
            return Ok(None);
        }
        levels.sort_by(f64::total_cmp);

        // Use a common adaptive subdivision for both boundary curves. Besides
        // satisfying the requested display error, sharing the periodic samples
        // prevents a diagonal mismatch between the two sides of the band.
        let periodic_parameters = adaptive_isoparametric_samples(
            face.surface(),
            periodic_axis,
            seam_parameter,
            seam_parameter + period,
            &levels,
            8,
            options,
        )?;
        // The two chart sides are boundary edges too. Sample them in the
        // transverse direction; otherwise the adaptive triangle refiner can
        // never reduce the normal change on that protected boundary edge.
        let transverse_parameters = adaptive_isoparametric_samples(
            face.surface(),
            transverse_axis,
            levels[0],
            levels[1],
            &[seam_parameter, seam_parameter + period],
            1,
            options,
        )?;
        let axes = if periodic_axis == 0 {
            [periodic_parameters, transverse_parameters]
        } else { [transverse_parameters, periodic_parameters] };
        Ok(Some(rectangular_surface_seed(face.surface(), axes)?))
    }
}

/// Share structured chart triangulation between periodic bands and swept faces.
fn rectangular_surface_seed(
    surface: &super::Surface3,
    axes: [Vec<f64>; 2],
) -> KernelResult<SurfaceTriangulationSeed> {
    let columns = axes[0].len();
    let rows = axes[1].len();
    let mut uvs = Vec::with_capacity(columns * rows);
    let mut positions = Vec::with_capacity(columns * rows);
    for v in &axes[1] {
        for u in &axes[0] {
            uvs.push(Point2::new(*u, *v));
            positions.push(surface.first_order(*u, *v)?.0);
        }
    }
    let mut boundary_edges = BTreeSet::new();
    for column in 0..columns - 1 {
        boundary_edges.insert(ordered(column, column + 1));
        let upper = (rows - 1) * columns + column;
        boundary_edges.insert(ordered(upper, upper + 1));
    }
    for row in 0..rows - 1 {
        let lower = row * columns;
        let upper = (row + 1) * columns;
        boundary_edges.insert(ordered(lower, upper));
        boundary_edges.insert(ordered(lower + columns - 1, upper + columns - 1));
    }
    let mut triangles = Vec::with_capacity((columns - 1) * (rows - 1) * 2);
    for row in 0..rows - 1 {
        for column in 0..columns - 1 {
            let a = row * columns + column;
            let b = a + 1;
            let c = a + columns;
            let d = c + 1;
            triangles.extend([[a, b, d], [a, d, c]]);
        }
    }
    Ok(SurfaceTriangulationSeed {
        uvs,
        positions,
        boundary_edges,
        triangles,
    })
}

/// Zip two boundary chains along the chart's u direction.
/// Accept only a complete triangulation with consistent winding. Keep every BREP
/// boundary vertex, including projected seams, instead of creating a fan of
/// long diagonals. Unsupported trims fall back to the general triangulator.
fn triangulate_boundary_strip(points: &[Point2]) -> Option<Vec<usize>> {
    if points.len() < 4 {
        return None;
    }
    let origin = points[0];
    let area = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| {
            let a = *a - origin;
            let b = *b - origin;
            a.x * b.y - a.y * b.x
        })
        .sum::<f64>();
    let orientation = area.signum();
    let compare = |a: &usize, b: &usize| {
        points[*a]
            .x
            .total_cmp(&points[*b].x)
            .then(points[*a].y.total_cmp(&points[*b].y))
    };
    let start = (0..points.len()).min_by(compare)?;
    let end = (0..points.len()).max_by(compare)?;
    if start == end {
        return None;
    }
    let chain = |step: usize| {
        let mut path = vec![start];
        let mut index = start;
        while index != end {
            index = (index + step) % points.len();
            path.push(index);
        }
        path
    };
    let a = chain(1);
    let b = chain(points.len() - 1);
    let mut i = 1;
    let mut j = 0;
    let mut triangles = Vec::with_capacity((points.len() - 2) * 3);
    while triangles.len() < (points.len() - 2) * 3 {
        let candidates = [
            (i + 1 < a.len()).then(|| [a[i], a[i + 1], b[j]]),
            (j + 1 < b.len()).then(|| [a[i], b[j + 1], b[j]]),
        ];
        let valid = |triangle: Option<[usize; 3]>| {
            triangle.is_some_and(|[a, b, c]| {
                let u = points[b] - points[a];
                let v = points[c] - points[a];
                (u.x * v.y - u.y * v.x) * orientation > 0.0
            })
        };
        let advance_a = match (valid(candidates[0]), valid(candidates[1])) {
            (true, true) => {
                // Keep the shared endpoint until all intermediate seam
                // vertices are consumed, including collinear samples.
                if i + 2 == a.len() {
                    false
                } else if j + 2 == b.len() {
                    true
                } else {
                    points[a[i + 1]].x <= points[b[j + 1]].x
                }
            }
            (true, false) => true,
            (false, true) => false,
            (false, false) => break,
        };
        triangles.extend(candidates[usize::from(!advance_a)]?);
        if advance_a {
            i += 1;
        } else {
            j += 1;
        }
    }
    if triangles.len() == (points.len() - 2) * 3 {
        return Some(triangles);
    }
    None
}

fn finish_surface_triangulation(
    surface: &super::Surface3,
    orientation: FaceOrientation,
    face_id: FaceId,
    tolerance: ModelTolerance,
    options: TessellationOptions,
    seed: SurfaceTriangulationSeed,
) -> KernelResult<TriangleMesh> {
    let SurfaceTriangulationSeed {
        mut uvs,
        mut positions,
        boundary_edges,
        mut triangles,
    } = seed;
    let mut normals = Vec::with_capacity(positions.len());
    for (uv, point) in uvs.iter().zip(&positions) {
        let (lifted, mut normal) = surface_point_normal(surface, uv.x, uv.y)?;
        let allowed = tolerance.linear_at_scale(point.to_vec().magnitude());
        if lifted.distance(*point) > allowed {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "tessellation chart does not lift its exact BREP boundary",
            ));
        }
        if orientation == FaceOrientation::Reversed {
            normal = -normal;
        }
        normals.push(normal);
    }
    // Coarse triangles can span more than half a curved support. Their flat
    // normals do not determine the winding of the refined surface patches.
    // Keep the chart winding during subdivision and orient the final mesh.
    triangles.retain(|triangle| !uv_triangle_is_degenerate(*triangle, &uvs));
    refine_surface_triangles(
        surface,
        orientation,
        options,
        &boundary_edges,
        &mut uvs,
        &mut positions,
        &mut normals,
        &mut triangles,
    )?;
    triangles.retain_mut(|triangle| {
        let geometric_normal = (positions[triangle[1]] - positions[triangle[0]])
            .cross(positions[triangle[2]] - positions[triangle[0]]);
        if geometric_normal.magnitude()
            <= tolerance
                .linear_at_scale(positions[triangle[0]].to_vec().magnitude())
                .powi(2)
        {
            return false;
        }
        let desired_normal = normals[triangle[0]] + normals[triangle[1]] + normals[triangle[2]];
        if geometric_normal.dot(desired_normal) < 0.0 {
            triangle.swap(1, 2);
        }
        true
    });
    Ok(TriangleMesh {
        positions,
        normals,
        triangle_faces: vec![face_id; triangles.len()],
        triangles,
    })
}

/// Subdivide a surface isoparametric interval until both exact boundary
/// curves satisfy the tessellation error policy. `fixed_parameters` lets both
/// sides of a band share an identical subdivision.
fn adaptive_isoparametric_samples(
    surface: &super::Surface3,
    varying_axis: usize,
    start: f64,
    end: f64,
    fixed_parameters: &[f64],
    initial_segments: usize,
    options: TessellationOptions,
) -> KernelResult<Vec<f64>> {
    let parameters = |varying: f64, fixed: f64| {
        if varying_axis == 0 {
            (varying, fixed)
        } else {
            (fixed, varying)
        }
    };
    let mut samples = (0..=initial_segments)
        .map(|index| start + (end - start) * index as f64 / initial_segments as f64)
        .collect::<Vec<_>>();
    for depth in 0..options.maximum_depth {
        let mut refined = Vec::with_capacity(samples.len() * 2);
        refined.push(samples[0]);
        let mut split_any = false;
        for interval in samples.windows(2) {
            let first = interval[0];
            let last = interval[1];
            let middle = (first + last) * 0.5;
            let mut split = false;
            for fixed in fixed_parameters {
                let (first_u, first_v) = parameters(first, *fixed);
                let (middle_u, middle_v) = parameters(middle, *fixed);
                let (last_u, last_v) = parameters(last, *fixed);
                let (first_point, first_normal) = surface_point_normal(surface, first_u, first_v)?;
                let (middle_point, _) = surface_point_normal(surface, middle_u, middle_v)?;
                let (last_point, last_normal) = surface_point_normal(surface, last_u, last_v)?;
                let chord_middle =
                    Point3::from_vec((first_point.to_vec() + last_point.to_vec()) * 0.5);
                if middle_point.distance(chord_middle) > options.chord_tolerance
                    || first_normal.angle(last_normal).0 > options.angular_tolerance
                {
                    split = true;
                    break;
                }
            }
            if split {
                refined.push(middle);
                split_any = true;
            }
            refined.push(last);
        }
        samples = refined;
        if !split_any {
            return Ok(samples);
        }
        if depth + 1 == options.maximum_depth {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "periodic band boundary did not satisfy tessellation tolerance",
            ));
        }
    }
    Ok(samples)
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
    // A pole can join distant angular representatives at the same 3D point.
    // Cut the traversal there, so unwrapping follows the regular boundary all
    // the way around instead of introducing a full-turn jump on its closing
    // edge. The collapsed edge itself does not constrain the angular span.
    for index in 1..ring.len() {
        if ring[index - 1].point.distance(ring[index].point) > tolerance.linear
            || ring[index - 1].uv.distance(ring[index].uv) <= tolerance.parameter
        {
            continue;
        }
        let uv = ring[index].uv;
        let (_, du, dv) = surface.first_order(uv.x, uv.y)?;
        if (domains[0].period.is_some() && du.magnitude() <= tolerance.linear)
            || (domains[1].period.is_some() && dv.magnitude() <= tolerance.linear)
        {
            ring.rotate_left(index);
            break;
        }
    }
    // Retained p-curves already select a surface chart. Only project a
    // boundary point when that chart does not reproduce its exact 3D position;
    // a global inverse search is especially expensive on swept/blend supports.
    for vertex in ring.iter_mut() {
        let original_uv = vertex.uv;
        let (lifted, du, dv) = surface.first_order(original_uv.x, original_uv.y)?;
        if lifted.distance(vertex.point)
            <= tolerance.linear_at_scale(vertex.point.to_vec().magnitude().max(1.0))
        {
            continue;
        }
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
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "periodic face boundary does not lie on its retained surface",
            ));
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
    for (coordinate, domain) in domains.iter().enumerate() {
        let Some(first) = ring.first().filter(|_| domain.period.is_some()) else {
            continue;
        };
        let mut previous = first.uv[coordinate];
        let mut previous_point = first.point;
        for vertex in ring.iter_mut().skip(1) {
            if previous_point.distance(vertex.point) > tolerance.linear {
                vertex.uv[coordinate] = domain.nearest(vertex.uv[coordinate], previous);
            }
            previous = vertex.uv[coordinate];
            previous_point = vertex.point;
        }
    }
    Ok(())
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
    // Refinement only appends vertices. Unchanged edges and triangles retain
    // identical UVs, so their exact error estimates can be reused across rounds.
    let mut edge_errors = BTreeMap::new();
    let mut triangle_errors = BTreeMap::new();
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
                let (chord_error, mut angular_error) = if let Some(error) = edge_errors.get(&edge) {
                    *error
                } else {
                    let error = surface_edge_error(surface, orientation, edge, uvs, positions)?;
                    edge_errors.insert(edge, error);
                    error
                };
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
            let (chord_error, mut angular_error) = if let Some(error) =
                triangle_errors.get(&triangle)
            {
                *error
            } else {
                let error = surface_triangle_error(surface, orientation, triangle, uvs, positions)?;
                triangle_errors.insert(triangle, error);
                error
            };
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
                    surface.kind_name(),
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
            let ab = midpoints.get(&ordered(a, b)).copied();
            let bc = midpoints.get(&ordered(b, c)).copied();
            let ca = midpoints.get(&ordered(c, a)).copied();
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
    let normal = match surface {
        // Longitude is singular at a sphere's poles even though the exact
        // surface normal remains well defined there.
        super::Surface3::Sphere(sphere) => point - sphere.center(),
        super::Surface3::Cone(_) if v == 0.0 => {
            // A cone tip has one limiting normal per meridian. Retain that
            // meridian's normal at the shared apex instead of normalizing zero.
            let (_, tangent, generator) = surface.first_order(u, 1.0)?;
            tangent.cross(generator)
        }
        super::Surface3::Revolved(revolved) => {
            // The angular derivative carries the profile radius. At an axis
            // pole that radius vanishes, and roundoff can even make it slightly
            // negative. Remove the radius factor to retain the one-sided normal
            // of the nonnegative-radius profile, including tangent poles.
            let frame = revolved.frame();
            let (sin, cos) = u.sin_cos();
            let tangent = -frame.x() * sin + frame.y() * cos;
            tangent.cross(dv)
        }
        _ => du.cross(dv),
    };
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
        ordered(triangle[0], triangle[1]),
        ordered(triangle[1], triangle[2]),
        ordered(triangle[2], triangle[0]),
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
    sample_edge_with_surface_constraint(edge, options, &|_, _| Ok((0.0, 0.0)))
}

fn sample_edge_with_surface_constraint(
    edge: &Edge,
    options: TessellationOptions,
    surface_error: &dyn Fn(f64, f64) -> KernelResult<(f64, f64)>,
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
        surface_error,
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
    surface_error: &dyn Fn(f64, f64) -> KernelResult<(f64, f64)>,
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
    let (surface_chord_error, adjacent_normal_angle) = surface_error(start_parameter, end_parameter)?;
    // The interior samples span the middle half of the interval. Limiting
    // their turn to half the requested angle bounds the complete segment
    // without evaluating potentially ambiguous two-sided endpoint tangents.
    let interior_angular_tolerance = options.angular_tolerance * 0.5;
    if deviation <= options.chord_tolerance
        && surface_chord_error <= options.chord_tolerance
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
        surface_error,
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
        surface_error,
        output,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_sampling_cache_tracks_options_and_published_geometry() {
        let tolerance = ModelTolerance::default();
        let mut kernel = Kernel::new(tolerance).unwrap();
        let mut transaction = kernel.begin_operation();
        let body = transaction.cylinder(
            Point3::origin(), Vec3::unit_x(), Vec3::unit_y(), Vec3::unit_z() * 2.0,
        ).unwrap();
        transaction.commit().unwrap();
        let face = kernel.topology.faces().find(|(_, face)| {
            matches!(face.surface(), super::super::Surface3::Plane(_))
        }).unwrap().0;
        let coarse = TessellationOptions::new(0.1, 0.2, 24).unwrap();
        let fine = TessellationOptions::new(0.01, 0.05, 24).unwrap();
        let first = kernel.tessellate_face(face, coarse).unwrap();
        let sampled_edges = kernel.tessellation_cache.lock().unwrap().edges.len();
        assert!(sampled_edges > 0);
        let repeated = kernel.tessellate_face(face, coarse).unwrap();
        assert_eq!(first.positions, repeated.positions);
        assert_eq!(first.triangles, repeated.triangles);
        assert_eq!(kernel.tessellation_cache.lock().unwrap().edges.len(), sampled_edges);

        let refined = kernel.tessellate_face(face, fine).unwrap();
        let fresh = Kernel {
            tolerance, topology: kernel.topology.clone(), tessellation_cache: Default::default(),
        }.tessellate_face(face, fine).unwrap();
        assert!(refined.triangles.len() > first.triangles.len());
        assert_eq!(refined.positions, fresh.positions);
        assert_eq!(refined.triangles, fresh.triangles);
        assert_eq!(kernel.tessellation_cache.lock().unwrap().options, Some(fine));

        let translation = Vec3::new(7.0, -3.0, 2.0);
        kernel.transform_body(body, super::super::RigidTransform3::new(
            crate::Matrix3::identity(), translation, tolerance,
        ).unwrap()).unwrap();
        assert!(kernel.tessellation_cache.lock().unwrap().edges.is_empty());
        let moved = kernel.tessellate_face(face, fine).unwrap();
        assert_eq!(moved.triangles, refined.triangles);
        for (before, after) in refined.positions.iter().zip(&moved.positions) {
            assert!((*after - *before - translation).magnitude() < 1e-10);
        }

        let mut transaction = kernel.begin_operation();
        transaction.cylinder(
            Point3::new(20.0, 0.0, 0.0), Vec3::unit_x(), Vec3::unit_y(), Vec3::unit_z(),
        ).unwrap();
        transaction.commit().unwrap();
        assert!(kernel.tessellation_cache.lock().unwrap().edges.is_empty());
        let published = kernel.tessellate_face(face, fine).unwrap();
        assert_eq!(published.positions, moved.positions);
    }

    #[test]
    fn periodic_hole_mesh_is_independent_of_its_parameter_sheet() {
        use super::super::{Plane3, Profile, TrimmedCurve2, Wire2};
        let tolerance = ModelTolerance::default();
        let circle = |radius| {
            Profile::new(
                Wire2::new(
                    [TrimmedCurve2::circle(Point2::new(0.0, 0.0), radius).unwrap()],
                    tolerance,
                ).unwrap(),
                [],
                tolerance,
            ).unwrap()
        };
        let body = circle(10.0)
            .extrude(Plane3::xy(), Vec3::unit_z() * 20.0)
            .unwrap();
        let cutter = circle(2.0)
            .extrude(
                Plane3::new(
                    Point3::new(-15.0, 0.0, 10.0),
                    Vec3::unit_y(),
                    Vec3::unit_z(),
                    tolerance,
                ).unwrap(),
                Vec3::unit_x() * 30.0,
            ).unwrap();
        let cut = body.difference(&cutter).unwrap();
        let kernel = Kernel { tolerance, topology: cut.topology().clone(), tessellation_cache: Default::default() };
        let (face_id, face) = kernel.topology.faces().find(|(_, face)| {
            face.surface().domains()[0].period.is_some()
                && face.loops().iter().any(|id| kernel.topology.loop_(*id).unwrap().kind() == super::super::LoopKind::Inner)
        }).expect("cylindrical face with a hole");
        let options = TessellationOptions::new(0.1, 0.1, 24).unwrap();
        let area = |mesh: &TriangleMesh| mesh.triangles.iter().map(|triangle| {
            let [a, b, c] = triangle.map(|index| mesh.positions[index]);
            (b - a).cross(c - a).magnitude() * 0.5
        }).sum::<f64>();
        let expected = area(&kernel.tessellate_face(face_id, options).unwrap());
        let period = face.surface().domains()[0].period.unwrap();
        for turns in [-2.0, -1.0, 1.0, 2.0] {
            let mut shifted = Kernel { tolerance, topology: kernel.topology.clone(), tessellation_cache: Default::default() };
            for loop_id in face.loops() {
                if kernel.topology.loop_(*loop_id).unwrap().kind() != super::super::LoopKind::Inner {
                    continue;
                }
                for id in kernel.topology.loop_coedges(*loop_id).unwrap() {
                    let coedge = shifted.topology.coedges.get_mut(id).unwrap();
                    coedge.pcurve = super::super::trim::translate_pcurve(
                        &coedge.pcurve,
                        crate::Vec2::new(turns * period, 0.0),
                        tolerance,
                    ).unwrap();
                }
            }
            let actual = area(&shifted.tessellate_face(face_id, options).unwrap());
            assert!(
                (actual - expected).abs() < 1e-6,
                "hole must remain open after a {turns}-period chart shift: {actual} vs {expected}"
            );
        }
    }

    #[test]
    fn blend_strip_preserves_straight_and_curved_boundary_samples() {
        for bow in [0.0, 0.015] {
            let ring = vec![
                Point2::new(0.0, 0.0),
                Point2::new(0.3, 0.0),
                Point2::new(0.7, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(1.0 - bow, 0.25),
                Point2::new(1.0 - bow, 0.75),
                Point2::new(1.0, 1.0),
                Point2::new(0.7, 1.0),
                Point2::new(0.3, 1.0),
                Point2::new(0.0, 1.0),
                Point2::new(bow, 0.75),
                Point2::new(bow, 0.25),
            ];
            for reversed in [false, true] {
                let mut points = ring.clone();
                if reversed {
                    points.reverse();
                }
                for _ in 0..points.len() {
                    points.rotate_left(1);
                    let indices = triangulate_boundary_strip(&points).expect("trimmed band");
                    assert_eq!(indices.len(), (points.len() - 2) * 3);
                    let polygon_area = points
                        .iter()
                        .zip(points.iter().cycle().skip(1))
                        .take(points.len())
                        .map(|(a, b)| a.x * b.y - a.y * b.x)
                        .sum::<f64>();
                    let mut area = 0.0;
                    let mut edges = BTreeMap::new();
                    for triangle in indices.chunks_exact(3) {
                        let u = points[triangle[1]] - points[triangle[0]];
                        let v = points[triangle[2]] - points[triangle[0]];
                        let signed = u.x * v.y - u.y * v.x;
                        assert!(
                            signed * polygon_area > 0.0,
                            "no inverted or collapsed triangles"
                        );
                        area += signed;
                        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                            *edges
                                .entry(ordered(triangle[a], triangle[b]))
                                .or_insert(0) += 1;
                        }
                    }
                    assert!((area - polygon_area).abs() < 1.0e-12);
                    let boundary = (0..points.len())
                        .map(|i| ordered(i, (i + 1) % points.len()))
                        .collect::<BTreeSet<_>>();
                    assert_eq!(
                        edges
                            .iter()
                            .filter(|(_, count)| **count == 1)
                            .map(|(edge, _)| *edge)
                            .collect::<BTreeSet<_>>(),
                        boundary
                    );
                    assert!(edges.values().all(|count| *count == 1 || *count == 2));
                }
            }
        }
    }
}
