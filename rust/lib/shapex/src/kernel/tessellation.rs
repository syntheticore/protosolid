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

impl Kernel {
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
        let mut rings: Vec<Vec<BoundaryVertex>> = Vec::new();
        for loop_id in loop_ids {
            rings.push(self.sample_loop(loop_id, options, cache)?);
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
                let mut normal = face.surface().normal(vertex.uv.x, vertex.uv.y)?;
                if face.orientation() == FaceOrientation::Reversed {
                    normal = -normal;
                }
                normals.push(normal);
            }
        }
        let mut triangles = Vec::with_capacity(indices.len() / 3);
        for values in indices.chunks_exact(3) {
            let mut triangle = [values[0], values[1], values[2]];
            let geometric_normal = (positions[triangle[1]] - positions[triangle[0]])
                .cross(positions[triangle[2]] - positions[triangle[0]]);
            let desired_normal = normals[triangle[0]] + normals[triangle[1]] + normals[triangle[2]];
            if geometric_normal.dot(desired_normal) < 0.0 {
                triangle.swap(1, 2);
            }
            triangles.push(triangle);
        }
        refine_surface_triangles(
            face.surface(),
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
                let polyline = sample_edge(self.topology().edge(coedge.edge())?, options)?;
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
                let surface_point = face.surface().evaluate(uv.x, uv.y)?.point;
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
        let mut centroid_splits = BTreeSet::new();
        let mut unique_edges = BTreeSet::new();
        for triangle in triangles.iter() {
            for edge in triangle_edges(*triangle) {
                unique_edges.insert(edge);
            }
        }
        for edge in unique_edges {
            if !boundary_edges.contains(&edge)
                && surface_edge_exceeds_tolerance(
                    surface,
                    orientation,
                    edge,
                    options,
                    uvs,
                    positions,
                    normals,
                )?
            {
                marked_edges.insert(edge);
            }
        }
        for (index, triangle) in triangles.iter().copied().enumerate() {
            if surface_triangle_exceeds_tolerance(
                surface,
                orientation,
                triangle,
                options,
                uvs,
                positions,
                normals,
            )? {
                let interior: Vec<_> = triangle_edges(triangle)
                    .into_iter()
                    .filter(|edge| !boundary_edges.contains(edge))
                    .collect();
                if interior.is_empty() {
                    centroid_splits.insert(index);
                } else if let Some(edge) = interior.into_iter().max_by(|first, second| {
                    positions[first.0]
                        .distance2(positions[first.1])
                        .total_cmp(&positions[second.0].distance2(positions[second.1]))
                }) {
                    marked_edges.insert(edge);
                }
            }
        }
        if marked_edges.is_empty() && centroid_splits.is_empty() {
            return Ok(());
        }
        if depth == options.maximum_depth {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "adaptive surface tessellation could not satisfy requested tolerances",
            ));
        }

        let mut midpoints = BTreeMap::new();
        for edge in &marked_edges {
            let uv = Point2::from_vec((uvs[edge.0].to_vec() + uvs[edge.1].to_vec()) * 0.5);
            let evaluation = surface.evaluate(uv.x, uv.y)?;
            let mut normal = surface.normal(uv.x, uv.y)?;
            if orientation == FaceOrientation::Reversed {
                normal = -normal;
            }
            let index = positions.len();
            uvs.push(uv);
            positions.push(evaluation.point);
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
                    let evaluation = surface.evaluate(uv.x, uv.y)?;
                    let mut normal = surface.normal(uv.x, uv.y)?;
                    if orientation == FaceOrientation::Reversed {
                        normal = -normal;
                    }
                    let center = positions.len();
                    uvs.push(uv);
                    positions.push(evaluation.point);
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
    }
    Ok(())
}

fn triangle_edges(triangle: [usize; 3]) -> [(usize, usize); 3] {
    [
        ordered_edge(triangle[0], triangle[1]),
        ordered_edge(triangle[1], triangle[2]),
        ordered_edge(triangle[2], triangle[0]),
    ]
}

#[allow(clippy::too_many_arguments)]
fn surface_edge_exceeds_tolerance(
    surface: &super::Surface3,
    orientation: FaceOrientation,
    edge: (usize, usize),
    options: TessellationOptions,
    uvs: &[Point2],
    positions: &[Point3],
    normals: &[Vec3],
) -> KernelResult<bool> {
    let uv = Point2::from_vec((uvs[edge.0].to_vec() + uvs[edge.1].to_vec()) * 0.5);
    let point = surface.evaluate(uv.x, uv.y)?.point;
    let chord = Point3::from_vec((positions[edge.0].to_vec() + positions[edge.1].to_vec()) * 0.5);
    let mut normal = surface.normal(uv.x, uv.y)?;
    if orientation == FaceOrientation::Reversed {
        normal = -normal;
    }
    Ok(point.distance(chord) > options.chord_tolerance
        || normal.angle(normals[edge.0]).0 > options.angular_tolerance
        || normal.angle(normals[edge.1]).0 > options.angular_tolerance)
}

#[allow(clippy::too_many_arguments)]
fn surface_triangle_exceeds_tolerance(
    surface: &super::Surface3,
    orientation: FaceOrientation,
    triangle: [usize; 3],
    options: TessellationOptions,
    uvs: &[Point2],
    positions: &[Point3],
    normals: &[Vec3],
) -> KernelResult<bool> {
    let [a, b, c] = triangle;
    let uv = Point2::from_vec((uvs[a].to_vec() + uvs[b].to_vec() + uvs[c].to_vec()) / 3.0);
    let point = surface.evaluate(uv.x, uv.y)?.point;
    let chord = Point3::from_vec(
        (positions[a].to_vec() + positions[b].to_vec() + positions[c].to_vec()) / 3.0,
    );
    let mut normal = surface.normal(uv.x, uv.y)?;
    if orientation == FaceOrientation::Reversed {
        normal = -normal;
    }
    Ok(point.distance(chord) > options.chord_tolerance
        || normal.angle(normals[a]).0 > options.angular_tolerance
        || normal.angle(normals[b]).0 > options.angular_tolerance
        || normal.angle(normals[c]).0 > options.angular_tolerance)
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
    let start = edge.curve().evaluate(edge.range().start)?;
    let end = edge.curve().evaluate(edge.range().end)?;
    let mut samples = vec![(edge.range().start, start.point)];
    sample_interval(
        edge,
        edge.range().start,
        start,
        edge.range().end,
        end,
        0,
        options,
        &mut samples,
    )?;
    let (parameters, points): (Vec<_>, Vec<_>) = samples.into_iter().unzip();
    Ok(EdgePolyline { parameters, points })
}

#[allow(clippy::too_many_arguments)]
fn sample_interval(
    edge: &Edge,
    start_parameter: f64,
    start: super::CurveEvaluation,
    end_parameter: f64,
    end: super::CurveEvaluation,
    depth: u8,
    options: TessellationOptions,
    output: &mut Vec<(f64, Point3)>,
) -> KernelResult<()> {
    let middle_parameter = (start_parameter + end_parameter) * 0.5;
    let middle = edge.curve().evaluate(middle_parameter)?;
    let chord_middle = start.point + (end.point - start.point) * 0.5;
    let deviation = middle.point.distance(chord_middle);
    let tangent_angle =
        if start.first_derivative.magnitude2() == 0.0 || end.first_derivative.magnitude2() == 0.0 {
            std::f64::consts::PI
        } else {
            start.first_derivative.angle(end.first_derivative).0
        };
    if deviation <= options.chord_tolerance && tangent_angle <= options.angular_tolerance {
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
        output,
    )
}
