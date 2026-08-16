use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cgmath::prelude::*;

use crate::Point3;

use super::{DiagnosticKind, KernelError, KernelResult, ModelTolerance, PersistentId};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PolyhedralVertex {
    pub(crate) key: String,
    pub(crate) point: Point3,
    pub(crate) sources: Vec<PersistentId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PolyhedralTriangle {
    pub(crate) key: String,
    /// Counter-clockwise vertex indices as viewed from outside the solid.
    pub(crate) vertices: [usize; 3],
    pub(crate) sources: Vec<PersistentId>,
}

/// Which side of an oriented closed polyhedral shell contains material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PolyhedralShellOrientation {
    /// Counter-clockwise triangles as viewed from outside the material.
    Outer,
    /// Counter-clockwise triangles as viewed from inside a void.
    Cavity,
}

/// Validated, oriented triangular boundary of one connected polyhedron.
///
/// This is an exact construction input, not a display mesh. Every undirected edge
/// must occur twice with opposite directions and the signed volume must be positive.
#[derive(Debug, Clone, PartialEq)]
pub struct PolyhedralMesh {
    vertices: Vec<PolyhedralVertex>,
    triangles: Vec<PolyhedralTriangle>,
    edge_sources: BTreeMap<(usize, usize), Vec<PersistentId>>,
    orientation: PolyhedralShellOrientation,
}

impl PolyhedralMesh {
    pub(crate) fn from_triangle_soup(
        triangles: impl IntoIterator<Item = [Point3; 3]>,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        Self::from_tagged_triangle_soup(
            triangles.into_iter().map(|triangle| (triangle, Vec::new())),
            tolerance,
        )
    }

    pub(crate) fn from_tagged_triangle_soup(
        triangles: impl IntoIterator<Item = ([Point3; 3], Vec<PersistentId>)>,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        // Welding must not depend on topology storage order. That order includes
        // persistent names and can legitimately differ between equivalent model
        // histories, while the geometric result must remain identical.
        let mut triangles = triangles.into_iter().collect::<Vec<_>>();
        triangles.sort_by(|(first, _), (second, _)| {
            let mut first = *first;
            let mut second = *second;
            first.sort_by(compare_points);
            second.sort_by(compare_points);
            first
                .iter()
                .zip(second.iter())
                .find_map(|(a, b)| {
                    let ordering = compare_points(a, b);
                    (ordering != std::cmp::Ordering::Equal).then_some(ordering)
                })
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut points = Vec::<Point3>::new();
        let mut vertex_sources = Vec::<BTreeSet<PersistentId>>::new();
        let mut indexed = Vec::<([usize; 3], Vec<PersistentId>)>::new();
        let mut triangle_signatures = BTreeSet::<[usize; 3]>::new();
        for (triangle, sources) in triangles {
            let mut welded = [0usize; 3];
            for (slot, point) in welded.iter_mut().zip(triangle) {
                *slot = points
                    .iter()
                    .position(|candidate| candidate.distance(point) <= tolerance.linear)
                    .unwrap_or_else(|| {
                        points.push(point);
                        vertex_sources.push(BTreeSet::new());
                        points.len() - 1
                    });
                vertex_sources[*slot].extend(sources.iter().copied());
            }
            if welded[0] == welded[1] || welded[1] == welded[2] || welded[2] == welded[0] {
                continue;
            }
            let mut signature = welded;
            signature.sort_unstable();
            if !triangle_signatures.insert(signature) {
                continue;
            }
            let normal = (points[welded[1]] - points[welded[0]])
                .cross(points[welded[2]] - points[welded[0]]);
            if normal.magnitude2() <= tolerance.linear.powi(4) {
                continue;
            }
            indexed.push((welded, sources));
        }
        let signed_volume = indexed.iter().fold(0.0, |volume, ([a, b, c], _)| {
            volume
                + points[*a]
                    .to_vec()
                    .dot(points[*b].to_vec().cross(points[*c].to_vec()))
                    / 6.0
        });
        if signed_volume < 0.0 {
            indexed
                .iter_mut()
                .for_each(|(triangle, _)| triangle.swap(1, 2));
        }
        Self::new_oriented(
            points
                .into_iter()
                .zip(vertex_sources)
                .enumerate()
                .map(|(index, (point, sources))| PolyhedralVertex {
                    key: format!("vertex{index}"),
                    point,
                    sources: sources.into_iter().collect(),
                }),
            indexed
                .into_iter()
                .enumerate()
                .map(|(index, (vertices, sources))| PolyhedralTriangle {
                    key: format!("triangle{index}"),
                    vertices,
                    sources,
                }),
            PolyhedralShellOrientation::Outer,
            tolerance,
        )
    }

    pub(crate) fn into_cavity(mut self) -> Self {
        for triangle in &mut self.triangles {
            triangle.vertices.swap(1, 2);
        }
        self.orientation = PolyhedralShellOrientation::Cavity;
        self
    }

    pub fn new(
        points: impl IntoIterator<Item = Point3>,
        triangles: impl IntoIterator<Item = [usize; 3]>,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        Self::new_oriented(
            points
                .into_iter()
                .enumerate()
                .map(|(index, point)| PolyhedralVertex {
                    key: format!("vertex{index}"),
                    point,
                    sources: Vec::new(),
                }),
            triangles
                .into_iter()
                .enumerate()
                .map(|(index, vertices)| PolyhedralTriangle {
                    key: format!("triangle{index}"),
                    vertices,
                    sources: Vec::new(),
                }),
            PolyhedralShellOrientation::Outer,
            tolerance,
        )
    }

    pub(crate) fn new_oriented(
        vertices: impl IntoIterator<Item = PolyhedralVertex>,
        triangles: impl IntoIterator<Item = PolyhedralTriangle>,
        orientation: PolyhedralShellOrientation,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        let vertices: Vec<_> = vertices.into_iter().collect();
        let triangles: Vec<_> = triangles.into_iter().collect();
        if vertices.len() < 4 || triangles.len() < 4 {
            return Err(invalid(
                "a closed polyhedron needs at least four vertices and faces",
            ));
        }
        let mut vertex_keys = BTreeSet::new();
        if vertices.iter().any(|vertex| {
            vertex.key.is_empty()
                || !vertex_keys.insert(vertex.key.clone())
                || !finite(vertex.point)
        }) {
            return Err(invalid(
                "polyhedron vertices need unique nonempty keys and finite points",
            ));
        }
        let mut triangle_keys = BTreeSet::new();
        let mut directed = BTreeMap::<(usize, usize), usize>::new();
        let mut undirected = BTreeMap::<(usize, usize), Vec<(usize, usize)>>::new();
        let scale = vertices.iter().fold(1.0_f64, |value, vertex| {
            value.max(vertex.point.to_vec().magnitude())
        });
        let area_tolerance = tolerance.linear_at_scale(scale).powi(2);
        for (triangle_index, triangle) in triangles.iter().enumerate() {
            let [a, b, c] = triangle.vertices;
            if triangle.key.is_empty()
                || !triangle_keys.insert(triangle.key.clone())
                || a >= vertices.len()
                || b >= vertices.len()
                || c >= vertices.len()
                || a == b
                || b == c
                || c == a
            {
                return Err(invalid(
                    "polyhedron triangles need unique keys and three valid distinct indices",
                ));
            }
            let normal = (vertices[b].point - vertices[a].point)
                .cross(vertices[c].point - vertices[a].point);
            if normal.magnitude2() <= area_tolerance * area_tolerance {
                return Err(invalid("polyhedron contains a degenerate triangle"));
            }
            for (start, end) in [(a, b), (b, c), (c, a)] {
                if let Some(previous) = directed.insert((start, end), triangle_index) {
                    let normal = |triangle: &PolyhedralTriangle| {
                        let [a, b, c] = triangle.vertices;
                        (vertices[b].point - vertices[a].point)
                            .cross(vertices[c].point - vertices[a].point)
                            .normalize()
                    };
                    return Err(invalid(format!(
                        "oriented polyhedron edge {start}->{end} occurs in triangles {previous} ({}, {:?}) and {triangle_index} ({}, {:?}) ({:?} to {:?})",
                        triangles[previous].key,
                        normal(&triangles[previous]),
                        triangles[triangle_index].key,
                        normal(&triangles[triangle_index]),
                        vertices[start].point,
                        vertices[end].point,
                    )));
                }
                let edge = ordered(start, end);
                undirected.entry(edge).or_default().push((start, end));
            }
        }
        if let Some(((start, end), uses)) = undirected
            .iter()
            .find(|(_, uses)| uses.len() != 2 || uses[0].0 != uses[1].1 || uses[0].1 != uses[1].0)
        {
            return Err(invalid(format!(
                "polyhedron edge {:?} to {:?} needs two opposite triangle uses, found {uses:?}",
                vertices[*start].point, vertices[*end].point,
            )));
        }
        let euler_characteristic =
            vertices.len() as isize - undirected.len() as isize + triangles.len() as isize;
        if euler_characteristic > 2 || (2 - euler_characteristic) % 2 != 0 {
            return Err(invalid(
                "polyhedron input is not an orientable closed manifold",
            ));
        }

        let mut adjacency = vec![Vec::new(); triangles.len()];
        for uses in undirected.values() {
            let first = directed[&uses[0]];
            let second = directed[&uses[1]];
            adjacency[first].push(second);
            adjacency[second].push(first);
        }
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::from([0usize]);
        while let Some(triangle) = queue.pop_front() {
            if visited.insert(triangle) {
                queue.extend(adjacency[triangle].iter().copied());
            }
        }
        if visited.len() != triangles.len() {
            return Err(invalid("polyhedron triangle adjacency is disconnected"));
        }

        let signed_volume = triangles.iter().fold(0.0, |volume, triangle| {
            let [a, b, c] = triangle.vertices;
            volume
                + vertices[a]
                    .point
                    .to_vec()
                    .dot(vertices[b].point.to_vec().cross(vertices[c].point.to_vec()))
                    / 6.0
        });
        let minimum_volume = tolerance.linear_at_scale(scale).powi(3);
        let orientation_is_valid = match orientation {
            PolyhedralShellOrientation::Outer => signed_volume > minimum_volume,
            PolyhedralShellOrientation::Cavity => signed_volume < -minimum_volume,
        };
        if !signed_volume.is_finite() || !orientation_is_valid {
            return Err(invalid(
                "polyhedron triangle orientation does not match its material side",
            ));
        }
        Ok(Self {
            vertices,
            triangles,
            edge_sources: BTreeMap::new(),
            orientation,
        })
    }

    pub(crate) fn vertices(&self) -> &[PolyhedralVertex] {
        &self.vertices
    }

    pub(crate) fn triangles(&self) -> &[PolyhedralTriangle] {
        &self.triangles
    }

    pub(crate) fn assign_edge_sources(
        &mut self,
        sources: impl Fn(Point3, Point3, &[PersistentId]) -> Vec<PersistentId>,
    ) {
        let mut edges = BTreeMap::<(usize, usize), BTreeSet<PersistentId>>::new();
        for triangle in &self.triangles {
            let [a, b, c] = triangle.vertices;
            for edge in [ordered(a, b), ordered(b, c), ordered(c, a)] {
                edges
                    .entry(edge)
                    .or_default()
                    .extend(triangle.sources.iter().copied());
            }
        }
        self.edge_sources = edges
            .into_iter()
            .filter_map(|(edge, faces)| {
                let faces = faces.into_iter().collect::<Vec<_>>();
                let values = sources(
                    self.vertices[edge.0].point,
                    self.vertices[edge.1].point,
                    &faces,
                );
                (!values.is_empty()).then_some((edge, values))
            })
            .collect();
    }

    pub(crate) fn edge_sources(&self, first: usize, second: usize) -> &[PersistentId] {
        self.edge_sources
            .get(&ordered(first, second))
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub(crate) fn orientation(&self) -> PolyhedralShellOrientation {
        self.orientation
    }

    pub(crate) fn genus(&self) -> usize {
        let mut edges = BTreeSet::new();
        for triangle in &self.triangles {
            let [a, b, c] = triangle.vertices;
            edges.extend([ordered(a, b), ordered(b, c), ordered(c, a)]);
        }
        let characteristic =
            self.vertices.len() as isize - edges.len() as isize + self.triangles.len() as isize;
        ((2 - characteristic) / 2).max(0) as usize
    }
}

fn compare_points(first: &Point3, second: &Point3) -> std::cmp::Ordering {
    first
        .x
        .total_cmp(&second.x)
        .then_with(|| first.y.total_cmp(&second.y))
        .then_with(|| first.z.total_cmp(&second.z))
}

fn finite(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

pub(crate) fn ordered(first: usize, second: usize) -> (usize, usize) {
    if first < second {
        (first, second)
    } else {
        (second, first)
    }
}

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::InvalidInput, message)
}
