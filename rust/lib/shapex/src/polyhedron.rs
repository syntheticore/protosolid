use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cgmath::prelude::*;

use crate::Point3;

use super::geometry::finite_point;
use super::polygon::ordered;
use super::{invalid, KernelResult, ModelTolerance};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PolyhedralVertex {
    pub(crate) key: String,
    pub(crate) point: Point3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PolyhedralTriangle {
    pub(crate) key: String,
    /// Counter-clockwise vertex indices as viewed from outside the solid.
    pub(crate) vertices: [usize; 3],
}

/// Validated, oriented triangular boundary of one connected polyhedron.
///
/// This is an exact construction input, not a display mesh. Every undirected edge
/// must occur twice with opposite directions and the signed volume must be positive.
#[derive(Debug, Clone, PartialEq)]
pub struct PolyhedralMesh {
    vertices: Vec<PolyhedralVertex>,
    triangles: Vec<PolyhedralTriangle>,
}

impl PolyhedralMesh {
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
                }),
            triangles
                .into_iter()
                .enumerate()
                .map(|(index, vertices)| PolyhedralTriangle {
                    key: format!("triangle{index}"),
                    vertices,
                }),
            tolerance,
        )
    }

    fn new_oriented(
        vertices: impl IntoIterator<Item = PolyhedralVertex>,
        triangles: impl IntoIterator<Item = PolyhedralTriangle>,
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
                || !finite_point(vertex.point)
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
        if !signed_volume.is_finite() || signed_volume <= minimum_volume {
            return Err(invalid(
                "polyhedron triangles must be oriented counter-clockwise from outside",
            ));
        }
        Ok(Self {
            vertices,
            triangles,
        })
    }

    pub(crate) fn vertices(&self) -> &[PolyhedralVertex] {
        &self.vertices
    }

    pub(crate) fn triangles(&self) -> &[PolyhedralTriangle] {
        &self.triangles
    }
}

