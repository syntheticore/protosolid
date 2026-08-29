use std::collections::BTreeSet;

use cgmath::prelude::*;

use crate::{Point3, Vec3};

use super::polyhedron::{PolyhedralShellOrientation, PolyhedralTriangle, PolyhedralVertex};
use super::{
    BodyId, Curve3, DiagnosticKind, FaceOrientation, KernelError, KernelResult, LoopKind,
    ModelTolerance, PersistentId, PolyhedralMesh, Surface3, Topology,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanKind {
    Union,
    Intersection,
    Difference,
}

#[derive(Debug, Clone)]
struct CsgVertex {
    point: Point3,
    sources: BTreeSet<PersistentId>,
}

#[derive(Debug, Clone)]
struct CsgPlane {
    normal: Vec3,
    offset: f64,
    source: PersistentId,
}

#[derive(Debug, Clone)]
struct CsgPolygon {
    vertices: Vec<CsgVertex>,
    plane: CsgPlane,
    source: PersistentId,
}

type CoplanarGroup = (CsgPlane, Vec<(PersistentId, CsgPlane, Vec<CsgVertex>)>);

#[derive(Debug, Clone, Default)]
struct BspNode {
    plane: Option<CsgPlane>,
    polygons: Vec<CsgPolygon>,
    front: Option<Box<BspNode>>,
    back: Option<Box<BspNode>>,
}

pub(crate) struct BooleanBodyMesh {
    pub outer: PolyhedralMesh,
    pub cavities: Vec<PolyhedralMesh>,
}

/// Boolean two straight-edged planar BREPs directly from their analytical face
/// loops. The returned polyhedral carrier is an exact topological subdivision
/// of those planes and lines; no tessellation or geometric perturbation occurs.
pub(crate) fn boolean_planar_brep(
    topology: &Topology,
    tolerance: ModelTolerance,
    first: BodyId,
    second: BodyId,
    kind: BooleanKind,
) -> KernelResult<Vec<BooleanBodyMesh>> {
    if first == second {
        return Err(KernelError::new(
            DiagnosticKind::InvalidInput,
            "a Boolean operation requires two distinct input bodies",
        ));
    }
    let first_polygons = body_polygons(topology, tolerance, first)?;
    let second_polygons = body_polygons(topology, tolerance, second)?;
    let linear = body_scale(&first_polygons, &second_polygons)
        .map(|scale| tolerance.linear_at_scale(scale))
        .unwrap_or(tolerance.linear);
    boolean_polygon_set(first_polygons, second_polygons, kind, linear)
        .and_then(|polygons| publish_boolean_polygons(polygons, tolerance))
}

fn boolean_polygon_set(
    mut first: Vec<CsgPolygon>,
    mut second: Vec<CsgPolygon>,
    kind: BooleanKind,
    tolerance: f64,
) -> KernelResult<Vec<CsgPolygon>> {
    let first_boundary = first.clone();
    let second_boundary = second.clone();
    let polygons = match kind {
        BooleanKind::Union => bsp_union(&mut first, &mut second, tolerance)?,
        BooleanKind::Intersection => bsp_intersection(&mut first, &mut second, tolerance)?,
        BooleanKind::Difference => bsp_difference(&mut first, &mut second, tolerance)?,
    };
    Ok(classify_material_boundary(
        polygons,
        &first_boundary,
        &second_boundary,
        kind,
        tolerance,
    ))
}

fn publish_boolean_polygons(
    polygons: Vec<CsgPolygon>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<BooleanBodyMesh>> {
    if polygons.is_empty() {
        Ok(Vec::new())
    } else {
        group_material_bodies(
            polygons_to_shelling_carriers(polygons, tolerance)?,
            tolerance,
        )
    }
}

fn classify_material_boundary(
    polygons: Vec<CsgPolygon>,
    first: &[CsgPolygon],
    second: &[CsgPolygon],
    kind: BooleanKind,
    tolerance: f64,
) -> Vec<CsgPolygon> {
    let occupied = |point| {
        let in_first = point_in_csg_boundary(point, first);
        let in_second = point_in_csg_boundary(point, second);
        match kind {
            BooleanKind::Union => in_first || in_second,
            BooleanKind::Intersection => in_first && in_second,
            BooleanKind::Difference => in_first && !in_second,
        }
    };
    polygons
        .into_iter()
        .filter_map(|polygon| {
            let centroid = polygon
                .vertices
                .iter()
                .fold(Vec3::new(0.0, 0.0, 0.0), |sum, vertex| {
                    sum + vertex.point.to_vec()
                })
                / polygon.vertices.len() as f64;
            let scale = polygon
                .vertices
                .iter()
                .map(|vertex| (vertex.point.to_vec() - centroid).magnitude())
                .fold(0.0_f64, f64::max);
            let offset = (tolerance * 16.0).max(scale * 1.0e-6);
            let outside = Point3::from_vec(centroid + polygon.plane.normal * offset);
            let inside = Point3::from_vec(centroid - polygon.plane.normal * offset);
            match (occupied(inside), occupied(outside)) {
                (true, false) => Some(polygon),
                (false, true) => Some(polygon.flipped()),
                _ => None,
            }
        })
        .collect()
}

fn point_in_csg_boundary(point: Point3, polygons: &[CsgPolygon]) -> bool {
    let mut solid_angle = 0.0;
    for polygon in polygons {
        if polygon.vertices.len() < 3 {
            continue;
        }
        let origin = polygon.vertices[0].point - point;
        for index in 1..polygon.vertices.len() - 1 {
            let first = polygon.vertices[index].point - point;
            let second = polygon.vertices[index + 1].point - point;
            let lengths = [origin.magnitude(), first.magnitude(), second.magnitude()];
            let numerator = origin.dot(first.cross(second));
            let denominator = lengths[0] * lengths[1] * lengths[2]
                + origin.dot(first) * lengths[2]
                + first.dot(second) * lengths[0]
                + second.dot(origin) * lengths[1];
            solid_angle += 2.0 * numerator.atan2(denominator);
        }
    }
    solid_angle.abs() > std::f64::consts::TAU
}

fn group_material_bodies(
    meshes: Vec<PolyhedralMesh>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<BooleanBodyMesh>> {
    let mut bodies = Vec::new();
    let mut cavities = Vec::new();
    for mesh in meshes {
        match mesh.orientation() {
            PolyhedralShellOrientation::Outer => bodies.push(BooleanBodyMesh {
                outer: mesh,
                cavities: Vec::new(),
            }),
            PolyhedralShellOrientation::Cavity => cavities.push(mesh),
        }
    }
    for cavity in cavities {
        let sample = cavity.vertices()[0].point;
        let owner = bodies
            .iter()
            .enumerate()
            .filter(|(_, body)| point_in_polyhedron(sample, &body.outer, tolerance))
            .min_by(|(_, first), (_, second)| {
                mesh_volume(&first.outer).total_cmp(&mesh_volume(&second.outer))
            })
            .map(|(index, _)| index)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "a Boolean cavity shell is not contained by an outer shell",
                )
            })?;
        bodies[owner].cavities.push(cavity);
    }
    Ok(bodies)
}

fn mesh_volume(mesh: &PolyhedralMesh) -> f64 {
    mesh.triangles()
        .iter()
        .map(|triangle| {
            let [a, b, c] = triangle.vertices;
            mesh.vertices()[a].point.to_vec().dot(
                mesh.vertices()[b]
                    .point
                    .to_vec()
                    .cross(mesh.vertices()[c].point.to_vec()),
            ) / 6.0
        })
        .sum::<f64>()
        .abs()
}

fn point_in_polyhedron(point: Point3, mesh: &PolyhedralMesh, tolerance: ModelTolerance) -> bool {
    let mut solid_angle = 0.0;
    for triangle in mesh.triangles() {
        let [a, b, c] = triangle.vertices;
        let a = mesh.vertices()[a].point - point;
        let b = mesh.vertices()[b].point - point;
        let c = mesh.vertices()[c].point - point;
        let lengths = [a.magnitude(), b.magnitude(), c.magnitude()];
        if lengths.iter().any(|length| *length <= tolerance.linear) {
            return true;
        }
        let numerator = a.dot(b.cross(c));
        let denominator = lengths[0] * lengths[1] * lengths[2]
            + a.dot(b) * lengths[2]
            + b.dot(c) * lengths[0]
            + c.dot(a) * lengths[1];
        solid_angle += 2.0 * numerator.atan2(denominator);
    }
    solid_angle.abs() > std::f64::consts::TAU
}

fn body_polygons(
    topology: &Topology,
    tolerance: ModelTolerance,
    body_id: BodyId,
) -> KernelResult<Vec<CsgPolygon>> {
    let body = topology.body(body_id)?;
    let mut polygons = Vec::new();
    for shell_id in body.shells() {
        for face_id in topology.shell(*shell_id)?.faces() {
            let face = topology.face(*face_id)?;
            let Surface3::Plane(surface) = face.surface() else {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "certified Boolean operations currently require planar input faces",
                ));
            };
            if face.loops().len() != 1 || topology.loop_(face.loops()[0])?.kind() != LoopKind::Outer
            {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "exact planar BSP currently requires one outer loop per input face",
                ));
            }
            let mut vertices = Vec::new();
            for coedge in topology.loop_coedges(face.loops()[0])? {
                let edge = topology.edge(topology.coedge(coedge)?.edge())?;
                if !matches!(edge.curve(), Curve3::Line(_)) {
                    return Err(KernelError::new(
                        DiagnosticKind::UnsupportedGeometry,
                        "exact planar BSP requires analytical line boundaries",
                    ));
                }
                let vertex = topology.coedge_vertices(coedge)?[0];
                let vertex = topology.vertex(vertex)?;
                vertices.push(CsgVertex {
                    point: vertex.point(),
                    sources: BTreeSet::from([vertex.name()]),
                });
            }
            let vertices = clean_polygon(vertices, tolerance.linear);
            let mut polygon = CsgPolygon::new(vertices, face.name(), tolerance)?;
            let outward = match face.orientation() {
                FaceOrientation::Forward => surface.normal(),
                FaceOrientation::Reversed => -surface.normal(),
            };
            if polygon.plane.normal.dot(outward) < 0.0 {
                polygon = polygon.flipped();
            }
            if !(0..polygon.vertices.len()).all(|index| {
                let current = polygon.vertices[index].point;
                let next = polygon.vertices[(index + 1) % polygon.vertices.len()].point;
                let following = polygon.vertices[(index + 2) % polygon.vertices.len()].point;
                (next - current).cross(following - next).dot(outward) >= -tolerance.linear.powi(2)
            }) {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "exact planar BSP currently requires convex input face loops",
                ));
            }
            polygons.push(polygon);
        }
    }
    if polygons.is_empty() {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "Boolean input body has no boundary polygons",
        ));
    }
    Ok(polygons)
}

impl CsgPolygon {
    fn new(
        vertices: Vec<CsgVertex>,
        source: PersistentId,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if vertices.len() < 3 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "a CSG polygon needs at least three vertices",
            ));
        }
        let normal =
            (vertices[1].point - vertices[0].point).cross(vertices[2].point - vertices[0].point);
        if normal.magnitude() <= tolerance.linear.powi(2) {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "a CSG polygon may not be degenerate",
            ));
        }
        let normal = normal.normalize();
        Ok(Self {
            plane: CsgPlane {
                normal,
                offset: normal.dot(vertices[0].point.to_vec()),
                source,
            },
            vertices,
            source,
        })
    }

    fn flipped(mut self) -> Self {
        self.vertices.reverse();
        self.plane.normal = -self.plane.normal;
        self.plane.offset = -self.plane.offset;
        self
    }
}

impl BspNode {
    fn from_polygons(polygons: Vec<CsgPolygon>, tolerance: f64) -> KernelResult<Self> {
        let mut node = Self::default();
        node.build(polygons, tolerance)?;
        Ok(node)
    }

    fn build(&mut self, polygons: Vec<CsgPolygon>, tolerance: f64) -> KernelResult<()> {
        if polygons.is_empty() {
            return Ok(());
        }
        if self.plane.is_none() {
            self.plane = polygons.first().map(|polygon| polygon.plane.clone());
        }
        let plane = self.plane.clone().ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::NumericalFailure,
                "BSP node lost its splitting plane",
            )
        })?;
        let mut front = Vec::new();
        let mut back = Vec::new();
        let mut coplanar_front = Vec::new();
        let mut coplanar_back = Vec::new();
        for polygon in polygons {
            plane.split_polygon(
                polygon,
                &mut coplanar_front,
                &mut coplanar_back,
                &mut front,
                &mut back,
                tolerance,
            )?;
        }
        self.polygons.extend(coplanar_front);
        self.polygons.extend(coplanar_back);
        if !front.is_empty() {
            self.front
                .get_or_insert_with(|| Box::new(Self::default()))
                .build(front, tolerance)?;
        }
        if !back.is_empty() {
            self.back
                .get_or_insert_with(|| Box::new(Self::default()))
                .build(back, tolerance)?;
        }
        Ok(())
    }

    fn all_polygons(&self) -> Vec<CsgPolygon> {
        let mut polygons = self.polygons.clone();
        if let Some(front) = &self.front {
            polygons.extend(front.all_polygons());
        }
        if let Some(back) = &self.back {
            polygons.extend(back.all_polygons());
        }
        polygons
    }

    fn invert(&mut self) {
        self.polygons = std::mem::take(&mut self.polygons)
            .into_iter()
            .map(CsgPolygon::flipped)
            .collect();
        if let Some(plane) = &mut self.plane {
            plane.normal = -plane.normal;
            plane.offset = -plane.offset;
        }
        if let Some(front) = &mut self.front {
            front.invert();
        }
        if let Some(back) = &mut self.back {
            back.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    fn clip_polygons(
        &self,
        polygons: Vec<CsgPolygon>,
        tolerance: f64,
    ) -> KernelResult<Vec<CsgPolygon>> {
        let Some(plane) = &self.plane else {
            return Ok(polygons);
        };
        let mut front = Vec::new();
        let mut back = Vec::new();
        let mut coplanar_front = Vec::new();
        let mut coplanar_back = Vec::new();
        for polygon in polygons {
            plane.split_polygon(
                polygon,
                &mut coplanar_front,
                &mut coplanar_back,
                &mut front,
                &mut back,
                tolerance,
            )?;
        }
        front.extend(coplanar_front);
        back.extend(coplanar_back);
        if let Some(front_node) = &self.front {
            front = front_node.clip_polygons(front, tolerance)?;
        }
        if let Some(back_node) = &self.back {
            back = back_node.clip_polygons(back, tolerance)?;
        } else {
            back.clear();
        }
        front.extend(back);
        Ok(front)
    }

    fn clip_to(&mut self, other: &Self, tolerance: f64) -> KernelResult<()> {
        self.polygons = other.clip_polygons(std::mem::take(&mut self.polygons), tolerance)?;
        if let Some(front) = &mut self.front {
            front.clip_to(other, tolerance)?;
        }
        if let Some(back) = &mut self.back {
            back.clip_to(other, tolerance)?;
        }
        Ok(())
    }
}

impl CsgPlane {
    #[allow(clippy::too_many_arguments)]
    fn split_polygon(
        &self,
        polygon: CsgPolygon,
        coplanar_front: &mut Vec<CsgPolygon>,
        coplanar_back: &mut Vec<CsgPolygon>,
        front: &mut Vec<CsgPolygon>,
        back: &mut Vec<CsgPolygon>,
        tolerance: f64,
    ) -> KernelResult<()> {
        const COPLANAR: u8 = 0;
        const FRONT: u8 = 1;
        const BACK: u8 = 2;
        let mut polygon_type = COPLANAR;
        let mut types = Vec::with_capacity(polygon.vertices.len());
        for vertex in &polygon.vertices {
            let distance = self.normal.dot(vertex.point.to_vec()) - self.offset;
            let kind = if distance < -tolerance {
                BACK
            } else if distance > tolerance {
                FRONT
            } else {
                COPLANAR
            };
            polygon_type |= kind;
            types.push(kind);
        }
        match polygon_type {
            COPLANAR => {
                if self.normal.dot(polygon.plane.normal) >= 0.0 {
                    coplanar_front.push(polygon);
                } else {
                    coplanar_back.push(polygon);
                }
            }
            FRONT => front.push(polygon),
            BACK => back.push(polygon),
            _ => {
                let mut front_vertices = Vec::new();
                let mut back_vertices = Vec::new();
                for index in 0..polygon.vertices.len() {
                    let next = (index + 1) % polygon.vertices.len();
                    let current_type = types[index];
                    let next_type = types[next];
                    let current = &polygon.vertices[index];
                    let following = &polygon.vertices[next];
                    if current_type != BACK {
                        front_vertices.push(current.clone());
                    }
                    if current_type != FRONT {
                        back_vertices.push(current.clone());
                    }
                    if (current_type | next_type) == (FRONT | BACK) {
                        let direction = following.point - current.point;
                        let denominator = self.normal.dot(direction);
                        if denominator.abs() <= f64::EPSILON {
                            return Err(KernelError::new(
                                DiagnosticKind::NumericalFailure,
                                "BSP spanning edge is parallel to its splitting plane",
                            ));
                        }
                        let parameter = ((self.offset - self.normal.dot(current.point.to_vec()))
                            / denominator)
                            .clamp(0.0, 1.0);
                        let mut sources = current.sources.clone();
                        sources.extend(following.sources.iter().copied());
                        sources.insert(self.source);
                        let split = CsgVertex {
                            point: current.point + direction * parameter,
                            sources,
                        };
                        front_vertices.push(split.clone());
                        back_vertices.push(split);
                    }
                }
                if front_vertices.len() >= 3 {
                    front.push(CsgPolygon {
                        vertices: front_vertices,
                        plane: polygon.plane.clone(),
                        source: polygon.source,
                    });
                }
                if back_vertices.len() >= 3 {
                    back.push(CsgPolygon {
                        vertices: back_vertices,
                        plane: polygon.plane,
                        source: polygon.source,
                    });
                }
            }
        }
        Ok(())
    }
}

fn bsp_union(
    first: &mut Vec<CsgPolygon>,
    second: &mut Vec<CsgPolygon>,
    tolerance: f64,
) -> KernelResult<Vec<CsgPolygon>> {
    let mut a = BspNode::from_polygons(std::mem::take(first), tolerance)?;
    let mut b = BspNode::from_polygons(std::mem::take(second), tolerance)?;
    a.clip_to(&b, tolerance)?;
    b.clip_to(&a, tolerance)?;
    b.invert();
    b.clip_to(&a, tolerance)?;
    b.invert();
    a.build(b.all_polygons(), tolerance)?;
    Ok(a.all_polygons())
}

fn bsp_difference(
    first: &mut Vec<CsgPolygon>,
    second: &mut Vec<CsgPolygon>,
    tolerance: f64,
) -> KernelResult<Vec<CsgPolygon>> {
    let mut a = BspNode::from_polygons(std::mem::take(first), tolerance)?;
    let mut b = BspNode::from_polygons(std::mem::take(second), tolerance)?;
    a.invert();
    a.clip_to(&b, tolerance)?;
    b.clip_to(&a, tolerance)?;
    b.invert();
    b.clip_to(&a, tolerance)?;
    b.invert();
    a.build(b.all_polygons(), tolerance)?;
    a.invert();
    Ok(a.all_polygons())
}

fn bsp_intersection(
    first: &mut Vec<CsgPolygon>,
    second: &mut Vec<CsgPolygon>,
    tolerance: f64,
) -> KernelResult<Vec<CsgPolygon>> {
    let mut a = BspNode::from_polygons(std::mem::take(first), tolerance)?;
    let mut b = BspNode::from_polygons(std::mem::take(second), tolerance)?;
    a.invert();
    b.clip_to(&a, tolerance)?;
    b.invert();
    a.clip_to(&b, tolerance)?;
    b.clip_to(&a, tolerance)?;
    a.build(b.all_polygons(), tolerance)?;
    a.invert();
    Ok(a.all_polygons())
}

fn polygons_to_shelling_carriers(
    polygons: Vec<CsgPolygon>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<PolyhedralMesh>> {
    let weld = tolerance.linear * 32.0;
    let mut cleaned_polygons = Vec::new();
    let mut global_vertices = Vec::<CsgVertex>::new();
    for polygon in polygons {
        let cleaned = clean_polygon(polygon.vertices, weld);
        if cleaned.len() < 3 {
            continue;
        }
        for vertex in &cleaned {
            if let Some(existing) = global_vertices
                .iter_mut()
                .find(|candidate| candidate.point.distance(vertex.point) <= weld)
            {
                existing.sources.extend(vertex.sources.iter().copied());
            } else {
                global_vertices.push(vertex.clone());
            }
        }
        cleaned_polygons.push((polygon.source, polygon.plane, cleaned));
    }

    let split_polygons = cleaned_polygons
        .into_iter()
        .map(|(source, plane, polygon)| {
            (
                source,
                plane,
                split_polygon_edges(polygon, &global_vertices, weld),
            )
        })
        .collect::<Vec<_>>();
    let cleaned_polygons =
        merge_coplanar_polygons(split_polygons, &global_vertices, weld, tolerance.angular)?;

    let mut vertices = Vec::<PolyhedralVertex>::new();
    let mut vertex_sources = Vec::<BTreeSet<PersistentId>>::new();
    let mut triangles = Vec::new();
    for (polygon_index, (face_sources, polygon)) in cleaned_polygons.into_iter().enumerate() {
        let source = face_sources.iter().next().copied().ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "Boolean output face has no provenance",
            )
        })?;
        let mut indices = Vec::with_capacity(polygon.len());
        for vertex in polygon {
            let existing = vertices
                .iter()
                .position(|candidate| candidate.point.distance(vertex.point) <= weld);
            let index = if let Some(index) = existing {
                vertex_sources[index].extend(vertex.sources);
                index
            } else {
                let index = vertices.len();
                vertex_sources.push(vertex.sources);
                vertices.push(PolyhedralVertex {
                    key: String::new(),
                    point: vertex.point,
                    sources: Vec::new(),
                });
                index
            };
            indices.push(index);
        }
        indices.dedup();
        if indices.len() > 1 && indices.first() == indices.last() {
            indices.pop();
        }
        loop {
            let removable = (0..indices.len()).find(|index| {
                let previous =
                    vertices[indices[(*index + indices.len() - 1) % indices.len()]].point;
                let current = vertices[indices[*index]].point;
                let next = vertices[indices[(*index + 1) % indices.len()]].point;
                (current - previous).cross(next - current).magnitude2() <= weld.powi(4)
            });
            if indices.len() <= 3 || removable.is_none() {
                break;
            }
            indices.remove(removable.expect("checked removable vertex"));
        }
        if indices.len() < 3 {
            continue;
        }
        let origin = vertices[indices[0]].point;
        let Some(polygon_normal) = (1..indices.len() - 1).find_map(|index| {
            let value = (vertices[indices[index]].point - origin)
                .cross(vertices[indices[index + 1]].point - origin);
            (value.magnitude2() > weld.powi(4)).then_some(value.normalize())
        }) else {
            continue;
        };
        let u = (vertices[indices[1]].point - origin).normalize();
        let v = polygon_normal.cross(u);
        let mut flat = Vec::with_capacity(indices.len() * 2);
        for index in &indices {
            let relative = vertices[*index].point - origin;
            flat.extend([relative.dot(u), relative.dot(v)]);
        }
        let triangulation = earcutr::earcut(&flat, &Vec::new(), 2);
        if triangulation.len() < 3 {
            return Err(KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "a merged Boolean polygon could not be triangulated",
            ));
        }
        for (triangle_index, triangle) in triangulation.chunks_exact(3).enumerate() {
            let mut triangle_vertices = [
                indices[triangle[0]],
                indices[triangle[1]],
                indices[triangle[2]],
            ];
            let normal = (vertices[triangle_vertices[1]].point
                - vertices[triangle_vertices[0]].point)
                .cross(vertices[triangle_vertices[2]].point - vertices[triangle_vertices[0]].point);
            let scale = triangle_vertices
                .iter()
                .map(|index| vertices[*index].point.to_vec().magnitude())
                .fold(1.0_f64, f64::max);
            let area_tolerance = tolerance.linear_at_scale(scale).powi(2);
            if normal.magnitude2() <= area_tolerance.powi(2) {
                continue;
            }
            if normal.dot(polygon_normal) < 0.0 {
                triangle_vertices.swap(1, 2);
            }
            triangles.push(PolyhedralTriangle {
                key: format!(
                    "fragment.{}.{:016x}.{polygon_index}.{triangle_index}",
                    source.namespace.as_u128(),
                    source.local,
                ),
                vertices: triangle_vertices,
                sources: face_sources.iter().copied().collect(),
            });
        }
    }
    for (index, sources) in vertex_sources.into_iter().enumerate() {
        vertices[index].key = provenance_key("vertex", &sources, index);
        vertices[index].sources = sources.into_iter().collect();
    }
    if triangles.is_empty() {
        return Ok(Vec::new());
    }
    split_polyhedral_components(vertices, triangles, tolerance)
}

fn split_polyhedral_components(
    vertices: Vec<PolyhedralVertex>,
    triangles: Vec<PolyhedralTriangle>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<PolyhedralMesh>> {
    use std::collections::{BTreeMap, VecDeque};

    let mut edge_triangles = BTreeMap::<(usize, usize), Vec<usize>>::new();
    for (triangle_index, triangle) in triangles.iter().enumerate() {
        let [a, b, c] = triangle.vertices;
        for edge in [(a, b), (b, c), (c, a)] {
            let edge = if edge.0 < edge.1 {
                edge
            } else {
                (edge.1, edge.0)
            };
            edge_triangles.entry(edge).or_default().push(triangle_index);
        }
    }
    let mut adjacency = vec![Vec::new(); triangles.len()];
    for incident in edge_triangles.values() {
        for first in incident {
            for second in incident {
                if first != second {
                    adjacency[*first].push(*second);
                }
            }
        }
    }
    let mut remaining: BTreeSet<_> = (0..triangles.len()).collect();
    let mut output = Vec::new();
    while let Some(start) = remaining.iter().next().copied() {
        let mut component = BTreeSet::new();
        let mut queue = VecDeque::from([start]);
        while let Some(triangle) = queue.pop_front() {
            if component.insert(triangle) {
                remaining.remove(&triangle);
                queue.extend(adjacency[triangle].iter().copied());
            }
        }
        let used_vertices: BTreeSet<_> = component
            .iter()
            .flat_map(|triangle| triangles[*triangle].vertices)
            .collect();
        let remap: BTreeMap<_, _> = used_vertices
            .iter()
            .copied()
            .enumerate()
            .map(|(new, old)| (old, new))
            .collect();
        let component_vertices = used_vertices
            .iter()
            .map(|index| vertices[*index].clone())
            .collect::<Vec<_>>();
        let component_triangles = component
            .iter()
            .map(|index| {
                let mut triangle = triangles[*index].clone();
                triangle.vertices = [
                    remap[&triangle.vertices[0]],
                    remap[&triangle.vertices[1]],
                    remap[&triangle.vertices[2]],
                ];
                triangle
            })
            .collect::<Vec<_>>();
        let signed_volume = component_triangles.iter().fold(0.0, |volume, triangle| {
            let [a, b, c] = triangle.vertices;
            volume
                + component_vertices[a].point.to_vec().dot(
                    component_vertices[b]
                        .point
                        .to_vec()
                        .cross(component_vertices[c].point.to_vec()),
                ) / 6.0
        });
        let orientation = if signed_volume >= 0.0 {
            PolyhedralShellOrientation::Outer
        } else {
            PolyhedralShellOrientation::Cavity
        };
        let mesh = PolyhedralMesh::new_oriented(
            component_vertices,
            component_triangles,
            orientation,
            tolerance,
        )
        .map_err(|error| {
            let mut wrapped = KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "a Boolean component did not form a certifiable genus-zero polyhedron",
            );
            for diagnostic in error.diagnostics() {
                wrapped.push(diagnostic.clone());
            }
            wrapped
        })?;
        output.push(mesh);
    }
    Ok(output)
}

fn merge_coplanar_polygons(
    polygons: Vec<(PersistentId, CsgPlane, Vec<CsgVertex>)>,
    global: &[CsgVertex],
    linear: f64,
    angular: f64,
) -> KernelResult<Vec<(BTreeSet<PersistentId>, Vec<CsgVertex>)>> {
    let all_polygons = polygons.clone();
    let mut groups: Vec<CoplanarGroup> = Vec::new();
    for (source, plane, vertices) in polygons {
        if let Some((_, members)) = groups.iter_mut().find(|(candidate, _)| {
            let alignment = candidate.normal.dot(plane.normal);
            alignment.abs() >= 1.0 - angular
                && if alignment >= 0.0 {
                    (candidate.offset - plane.offset).abs() <= linear
                } else {
                    (candidate.offset + plane.offset).abs() <= linear
                }
        }) {
            members.push((source, plane, vertices));
        } else {
            groups.push((plane.clone(), vec![(source, plane, vertices)]));
        }
    }

    let mut output = Vec::new();
    for (plane, members) in groups {
        let mut indexed = Vec::<(i32, Vec<usize>)>::new();
        let mut segments = BTreeSet::<(usize, usize)>::new();
        let mut forced_edges = BTreeSet::<(usize, usize)>::new();
        let mut sources = BTreeSet::new();
        let mut unique = BTreeSet::new();
        for (source, member_plane, polygon) in members {
            let indices = polygon
                .iter()
                .map(|vertex| global_vertex_index(global, vertex.point, linear))
                .collect::<KernelResult<Vec<_>>>()?;
            let sign = if plane.normal.dot(member_plane.normal) >= 0.0 {
                1
            } else {
                -1
            };
            let mut signature = indices.clone();
            signature.sort_unstable();
            if !unique.insert((sign, signature)) {
                continue;
            }
            sources.insert(source);
            for index in 0..indices.len() {
                for pair in split_coplanar_edge(
                    global,
                    indices[index],
                    indices[(index + 1) % indices.len()],
                    linear,
                )
                .windows(2)
                {
                    segments.insert((pair[0].min(pair[1]), pair[0].max(pair[1])));
                }
            }
            indexed.push((sign, indices));
        }
        for (_, support_plane, polygon) in &all_polygons {
            if plane.normal.dot(support_plane.normal).abs() >= 1.0 - angular {
                continue;
            }
            for index in 0..polygon.len() {
                let first = polygon[index].point;
                let second = polygon[(index + 1) % polygon.len()].point;
                if (plane.normal.dot(first.to_vec()) - plane.offset).abs() > linear * 16.0
                    || (plane.normal.dot(second.to_vec()) - plane.offset).abs() > linear * 16.0
                {
                    continue;
                }
                let first = global_vertex_index(global, first, linear * 16.0)?;
                let second = global_vertex_index(global, second, linear * 16.0)?;
                for pair in split_coplanar_edge(global, first, second, linear * 16.0).windows(2) {
                    segments.insert((pair[0].min(pair[1]), pair[0].max(pair[1])));
                    forced_edges.insert((pair[1], pair[0]));
                }
            }
        }

        let occupied = |point| {
            indexed
                .iter()
                .filter(|(_, polygon)| {
                    point_in_coplanar_polygon(point, polygon, global, plane.normal)
                })
                .map(|(sign, _)| *sign)
                .sum::<i32>()
                != 0
        };
        let mut edges = BTreeSet::<(usize, usize)>::new();
        for (first, second) in segments {
            let start = global[first].point;
            let end = global[second].point;
            let direction = end - start;
            if direction.magnitude2() <= linear.powi(2) {
                continue;
            }
            let midpoint = start + direction * 0.5;
            let left = plane.normal.cross(direction).normalize();
            let probe = linear.max(direction.magnitude() * 1.0e-7) * 4.0;
            match (
                occupied(midpoint + left * probe),
                occupied(midpoint - left * probe),
            ) {
                (true, false) => {
                    edges.insert((first, second));
                }
                (false, true) => {
                    edges.insert((second, first));
                }
                _ => {
                    let forward = forced_edges.contains(&(first, second));
                    let reverse = forced_edges.contains(&(second, first));
                    if forward != reverse {
                        edges.insert(if forward {
                            (first, second)
                        } else {
                            (second, first)
                        });
                    }
                }
            }
        }

        let mut loops = Vec::<(f64, Vec<usize>)>::new();
        while let Some(start_edge) = edges.iter().next().copied() {
            edges.remove(&start_edge);
            let mut ring = vec![start_edge.0, start_edge.1];
            let mut current = start_edge.1;
            while current != start_edge.0 {
                let previous = ring[ring.len() - 2];
                let incoming = global[current].point - global[previous].point;
                let next = edges
                    .iter()
                    .copied()
                    .filter(|edge| {
                        edge.0 == current
                            || global[edge.0].point.distance(global[current].point) <= linear * 16.0
                    })
                    .min_by(|first, second| {
                        let turn = |edge: &(usize, usize)| {
                            let outgoing = global[edge.1].point - global[current].point;
                            plane.normal.dot(incoming.cross(outgoing)).atan2(incoming.dot(outgoing))
                        };
                        turn(first).total_cmp(&turn(second))
                    })
                    .ok_or_else(|| KernelError::new(
                        DiagnosticKind::UnsupportedGeometry,
                        format!(
                            "coplanar Boolean fragments do not form a closed boundary loop at {:?} on plane {:?}",
                            global[current].point,
                            plane.normal,
                        ),
                    ))?;
                edges.remove(&next);
                current = next.1;
                if global[current].point.distance(global[start_edge.0].point) <= linear * 16.0 {
                    current = start_edge.0;
                }
                if current != start_edge.0 {
                    ring.push(current);
                }
                if ring.len() > global.len() + 1 {
                    return Err(KernelError::new(
                        DiagnosticKind::NumericalFailure,
                        "coplanar Boolean boundary loop exceeded its vertex limit",
                    ));
                }
            }
            loop {
                let removable = (0..ring.len()).find(|index| {
                    let previous = global[ring[(*index + ring.len() - 1) % ring.len()]].point;
                    let current = global[ring[*index]].point;
                    let next = global[ring[(*index + 1) % ring.len()]].point;
                    let first = current - previous;
                    let second = next - current;
                    first.cross(second).magnitude()
                        <= linear * 16.0 * (first.magnitude() + second.magnitude()).max(1.0)
                });
                if ring.len() <= 3 || removable.is_none() {
                    break;
                }
                ring.remove(removable.expect("checked removable loop vertex"));
            }
            let area = (0..ring.len())
                .fold(Vec3::new(0.0, 0.0, 0.0), |sum, index| {
                    sum + global[ring[index]]
                        .point
                        .to_vec()
                        .cross(global[ring[(index + 1) % ring.len()]].point.to_vec())
                })
                .dot(plane.normal)
                * 0.5;
            if area.abs() > linear.powi(2) {
                loops.push((area, ring));
            }
        }

        if !loops.is_empty() && loops.iter().all(|(area, _)| *area < 0.0) {
            for (area, ring) in &mut loops {
                *area = -*area;
                ring.reverse();
            }
        }
        let outer_indices = loops
            .iter()
            .enumerate()
            .filter_map(|(index, (area, _))| (*area > 0.0).then_some(index))
            .collect::<Vec<_>>();
        let mut hole_owner = vec![None; loops.len()];
        for (hole_index, (area, hole)) in loops.iter().enumerate() {
            if *area >= 0.0 {
                continue;
            }
            hole_owner[hole_index] = outer_indices
                .iter()
                .copied()
                .filter(|outer| {
                    point_in_coplanar_polygon(
                        global[hole[0]].point,
                        &loops[*outer].1,
                        global,
                        plane.normal,
                    )
                })
                .min_by(|first, second| loops[*first].0.total_cmp(&loops[*second].0));
        }
        for outer_index in outer_indices {
            let rings = std::iter::once(&loops[outer_index].1)
                .chain(loops.iter().enumerate().filter_map(|(index, (_, ring))| {
                    (hole_owner[index] == Some(outer_index)).then_some(ring)
                }))
                .collect::<Vec<_>>();
            let origin = global[rings[0][0]].point;
            let u = (global[rings[0][1]].point - origin).normalize();
            let v = plane.normal.cross(u);
            let mut flat = Vec::new();
            let mut vertices = Vec::new();
            let mut holes = Vec::new();
            for (ring_index, ring) in rings.iter().enumerate() {
                if ring_index > 0 {
                    holes.push(vertices.len());
                }
                for index in *ring {
                    let relative = global[*index].point - origin;
                    flat.extend([relative.dot(u), relative.dot(v)]);
                    vertices.push(global[*index].clone());
                }
            }
            for triangle in earcutr::earcut(&flat, &holes, 2).chunks_exact(3) {
                output.push((
                    sources.clone(),
                    vec![
                        vertices[triangle[0]].clone(),
                        vertices[triangle[1]].clone(),
                        vertices[triangle[2]].clone(),
                    ],
                ));
            }
        }
    }
    Ok(output)
}

fn point_in_coplanar_polygon(
    point: Point3,
    polygon: &[usize],
    global: &[CsgVertex],
    normal: Vec3,
) -> bool {
    let drop_axis = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() {
        0
    } else if normal.y.abs() >= normal.z.abs() {
        1
    } else {
        2
    };
    let project = |value: Point3| match drop_axis {
        0 => (value.y, value.z),
        1 => (value.x, value.z),
        _ => (value.x, value.y),
    };
    let point = project(point);
    let mut inside = false;
    for index in 0..polygon.len() {
        let first = project(global[polygon[index]].point);
        let second = project(global[polygon[(index + 1) % polygon.len()]].point);
        if (first.1 > point.1) != (second.1 > point.1) {
            let crossing =
                first.0 + (point.1 - first.1) * (second.0 - first.0) / (second.1 - first.1);
            if crossing > point.0 {
                inside = !inside;
            }
        }
    }
    inside
}

fn split_coplanar_edge(
    global: &[CsgVertex],
    start: usize,
    end: usize,
    tolerance: f64,
) -> Vec<usize> {
    let first = global[start].point;
    let direction = global[end].point - first;
    let length_squared = direction.magnitude2();
    if length_squared == 0.0 {
        return vec![start, end];
    }
    let mut points = global
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let parameter = (candidate.point - first).dot(direction) / length_squared;
            let projected = first + direction * parameter;
            (parameter >= -tolerance
                && parameter <= 1.0 + tolerance
                && projected.distance(candidate.point) <= tolerance)
                .then_some((parameter.clamp(0.0, 1.0), index))
        })
        .collect::<Vec<_>>();
    points.sort_by(|first, second| first.0.total_cmp(&second.0));
    points.dedup_by(|first, second| first.1 == second.1);
    points.into_iter().map(|(_, index)| index).collect()
}

fn global_vertex_index(global: &[CsgVertex], point: Point3, tolerance: f64) -> KernelResult<usize> {
    global
        .iter()
        .position(|candidate| candidate.point.distance(point) <= tolerance)
        .ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::NumericalFailure,
                "Boolean polygon vertex is absent from its global weld set",
            )
        })
}

fn split_polygon_edges(
    vertices: Vec<CsgVertex>,
    global: &[CsgVertex],
    tolerance: f64,
) -> Vec<CsgVertex> {
    let mut output = Vec::new();
    for index in 0..vertices.len() {
        let start = &vertices[index];
        let end = &vertices[(index + 1) % vertices.len()];
        let direction = end.point - start.point;
        let length_squared = direction.magnitude2();
        let mut points = vec![(0.0_f64, start.clone())];
        if length_squared > tolerance * tolerance {
            for candidate in global {
                let parameter = (candidate.point - start.point).dot(direction) / length_squared;
                if parameter <= tolerance || parameter >= 1.0 - tolerance {
                    continue;
                }
                let projected = start.point + direction * parameter;
                if projected.distance(candidate.point) <= tolerance {
                    points.push((parameter, candidate.clone()));
                }
            }
        }
        points.sort_by(|first, second| first.0.total_cmp(&second.0));
        for (_, point) in points {
            if output
                .last()
                .map(|previous: &CsgVertex| previous.point.distance(point.point) > tolerance)
                .unwrap_or(true)
            {
                output.push(point);
            }
        }
    }
    output
}

fn clean_polygon(mut vertices: Vec<CsgVertex>, tolerance: f64) -> Vec<CsgVertex> {
    if vertices.len() > 1
        && vertices[0]
            .point
            .distance(vertices[vertices.len() - 1].point)
            <= tolerance
    {
        vertices.pop();
    }
    let mut changed = true;
    while changed && vertices.len() >= 3 {
        changed = false;
        for index in 0..vertices.len() {
            let previous = (index + vertices.len() - 1) % vertices.len();
            let next = (index + 1) % vertices.len();
            let first = vertices[index].point - vertices[previous].point;
            let second = vertices[next].point - vertices[index].point;
            if first.magnitude() <= tolerance
                || second.magnitude() <= tolerance
                || first.cross(second).magnitude()
                    <= tolerance * first.magnitude().max(second.magnitude())
            {
                vertices.remove(index);
                changed = true;
                break;
            }
        }
    }
    vertices
}

fn provenance_key(role: &str, sources: &BTreeSet<PersistentId>, fallback: usize) -> String {
    if sources.is_empty() {
        return format!("{role}.generated.{fallback}");
    }
    let encoded = sources
        .iter()
        .map(|source| format!("{}-{:016x}", source.namespace.as_u128(), source.local))
        .collect::<Vec<_>>()
        .join("+");
    format!("{role}.{encoded}.{fallback}")
}

fn body_scale(first: &[CsgPolygon], second: &[CsgPolygon]) -> Option<f64> {
    first
        .iter()
        .chain(second)
        .flat_map(|polygon| &polygon.vertices)
        .map(|vertex| vertex.point.to_vec().magnitude())
        .max_by(f64::total_cmp)
}
