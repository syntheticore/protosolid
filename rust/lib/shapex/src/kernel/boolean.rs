use std::collections::BTreeSet;

use cgmath::prelude::*;

use crate::{Point3, Vec3};

use super::polyhedron::{PolyhedralShellOrientation, PolyhedralTriangle, PolyhedralVertex};
use super::{
    BodyId, DiagnosticKind, Kernel, KernelError, KernelResult, ModelTolerance, PersistentId,
    PolyhedralMesh, Surface3, TessellationOptions, Topology,
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

type CoplanarGroup = (CsgPlane, Vec<(PersistentId, Vec<CsgVertex>)>);

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

pub(crate) fn boolean_polyhedron(
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
    let mut first_polygons = body_polygons(topology, tolerance, first)?;
    let mut second_polygons = body_polygons(topology, tolerance, second)?;
    let linear = body_scale(&first_polygons, &second_polygons)
        .map(|scale| tolerance.linear_at_scale(scale))
        .unwrap_or(tolerance.linear);
    let polygons = match kind {
        BooleanKind::Union => bsp_union(&mut first_polygons, &mut second_polygons, linear)?,
        BooleanKind::Intersection => {
            bsp_intersection(&mut first_polygons, &mut second_polygons, linear)?
        }
        BooleanKind::Difference => {
            bsp_difference(&mut first_polygons, &mut second_polygons, linear)?
        }
    };
    if polygons.is_empty() {
        Ok(Vec::new())
    } else {
        group_material_bodies(polygons_to_mesh(polygons, tolerance)?, tolerance)
    }
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
    let kernel = Kernel {
        tolerance,
        topology: topology.clone(),
    };
    let options = TessellationOptions::new(tolerance.linear, 0.25, 8)?;
    let mut polygons = Vec::new();
    for shell_id in body.shells() {
        for face_id in topology.shell(*shell_id)?.faces() {
            let face = topology.face(*face_id)?;
            if !matches!(face.surface(), Surface3::Plane(_)) {
                return Err(KernelError::new(
                    DiagnosticKind::UnsupportedGeometry,
                    "certified Boolean operations currently require planar input faces",
                ));
            }
            let mesh = kernel.tessellate_face(*face_id, options)?;
            let topology_vertices = topology.face_vertices(*face_id)?;
            for triangle in mesh.triangles {
                let points = [
                    mesh.positions[triangle[0]],
                    mesh.positions[triangle[1]],
                    mesh.positions[triangle[2]],
                ];
                let mut values = Vec::with_capacity(3);
                for point in points {
                    let source =
                        nearest_vertex_name(topology, &topology_vertices, point, tolerance)?;
                    values.push(CsgVertex {
                        point,
                        sources: BTreeSet::from([source]),
                    });
                }
                polygons.push(CsgPolygon::new(values, face.name(), tolerance)?);
            }
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

fn nearest_vertex_name(
    topology: &Topology,
    candidates: &[super::VertexId],
    point: Point3,
    tolerance: ModelTolerance,
) -> KernelResult<PersistentId> {
    let mut nearest = None;
    for vertex_id in candidates {
        let vertex = topology.vertex(*vertex_id)?;
        let distance = vertex.point().distance(point);
        if nearest
            .map(|(_, current)| distance < current)
            .unwrap_or(true)
        {
            nearest = Some((vertex.name(), distance));
        }
    }
    let (name, distance) = nearest.ok_or_else(|| {
        KernelError::new(
            DiagnosticKind::TopologyInvariant,
            "Boolean face has no topological vertices",
        )
    })?;
    if distance > tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0)) {
        return Err(KernelError::new(
            DiagnosticKind::GeometryInvariant,
            "Boolean triangulation introduced a vertex without topological provenance",
        ));
    }
    Ok(name)
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
                    front.push(CsgPolygon::new(
                        front_vertices,
                        polygon.source,
                        ModelTolerance::new(tolerance, f64::EPSILON, 1.0e-10, 1.0e-12),
                    )?);
                }
                if back_vertices.len() >= 3 {
                    back.push(CsgPolygon::new(
                        back_vertices,
                        polygon.source,
                        ModelTolerance::new(tolerance, f64::EPSILON, 1.0e-10, 1.0e-12),
                    )?);
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

fn polygons_to_mesh(
    polygons: Vec<CsgPolygon>,
    tolerance: ModelTolerance,
) -> KernelResult<Vec<PolyhedralMesh>> {
    let mut cleaned_polygons = Vec::new();
    let mut global_vertices = Vec::<CsgVertex>::new();
    for polygon in polygons {
        let cleaned = clean_polygon(polygon.vertices, tolerance.linear);
        if cleaned.len() < 3 {
            continue;
        }
        for vertex in &cleaned {
            if let Some(existing) = global_vertices
                .iter_mut()
                .find(|candidate| candidate.point.distance(vertex.point) <= tolerance.linear)
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
                split_polygon_edges(polygon, &global_vertices, tolerance.linear),
            )
        })
        .collect();
    let cleaned_polygons = merge_coplanar_polygons(
        split_polygons,
        &global_vertices,
        tolerance.linear,
        tolerance.angular,
    )?;

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
                .position(|candidate| candidate.point.distance(vertex.point) <= tolerance.linear);
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
        let center_point = Point3::from_vec(
            indices
                .iter()
                .map(|index| vertices[*index].point.to_vec())
                .fold(Vec3::new(0.0, 0.0, 0.0), |sum, point| sum + point)
                / indices.len() as f64,
        );
        let mut center_sources = BTreeSet::from([source]);
        for index in &indices {
            center_sources.extend(vertex_sources[*index].iter().copied());
        }
        let center = vertices.len();
        vertex_sources.push(center_sources);
        vertices.push(PolyhedralVertex {
            key: String::new(),
            point: center_point,
            sources: Vec::new(),
        });
        for edge in 0..indices.len() {
            triangles.push(PolyhedralTriangle {
                key: format!(
                    "fragment.{}.{:016x}.{polygon_index}.{edge}",
                    source.namespace.as_u128(),
                    source.local,
                ),
                vertices: [indices[edge], indices[(edge + 1) % indices.len()], center],
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
    let mut groups: Vec<CoplanarGroup> = Vec::new();
    for (source, plane, vertices) in polygons {
        if let Some((_, members)) = groups.iter_mut().find(|(candidate, _)| {
            candidate.normal.dot(plane.normal) >= 1.0 - angular
                && (candidate.offset - plane.offset).abs() <= linear
        }) {
            members.push((source, vertices));
        } else {
            groups.push((plane, vec![(source, vertices)]));
        }
    }

    let mut merged = Vec::new();
    for (_, members) in groups {
        let mut unique_polygons = BTreeSet::new();
        let mut edges = BTreeSet::<(usize, usize)>::new();
        let mut sources = BTreeSet::new();
        for (source, polygon) in members {
            let indices = polygon
                .iter()
                .map(|vertex| global_vertex_index(global, vertex.point, linear))
                .collect::<KernelResult<Vec<_>>>()?;
            let mut signature = indices.clone();
            signature.sort_unstable();
            if !unique_polygons.insert(signature) {
                continue;
            }
            sources.insert(source);
            for index in 0..indices.len() {
                let edge = (indices[index], indices[(index + 1) % indices.len()]);
                if !edges.remove(&(edge.1, edge.0)) {
                    edges.insert(edge);
                }
            }
        }
        while let Some(start_edge) = edges.iter().next().copied() {
            edges.remove(&start_edge);
            let mut loop_indices = vec![start_edge.0, start_edge.1];
            let mut current = start_edge.1;
            while current != start_edge.0 {
                let candidates: Vec<_> = edges
                    .iter()
                    .copied()
                    .filter(|edge| edge.0 == current)
                    .collect();
                if candidates.len() != 1 {
                    return Err(KernelError::new(
                        DiagnosticKind::UnsupportedGeometry,
                        "coplanar Boolean fragments do not form unambiguous boundary loops",
                    ));
                }
                let next = candidates[0];
                edges.remove(&next);
                current = next.1;
                if current != start_edge.0 {
                    loop_indices.push(current);
                }
                if loop_indices.len() > global.len() + 1 {
                    return Err(KernelError::new(
                        DiagnosticKind::NumericalFailure,
                        "coplanar Boolean boundary loop does not close",
                    ));
                }
            }
            if loop_indices.len() >= 3 {
                if sources.is_empty() {
                    return Err(KernelError::new(
                        DiagnosticKind::TopologyInvariant,
                        "merged Boolean face has no source provenance",
                    ));
                }
                merged.push((
                    sources.clone(),
                    loop_indices
                        .into_iter()
                        .map(|index| global[index].clone())
                        .collect(),
                ));
            }
        }
    }
    Ok(merged)
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
