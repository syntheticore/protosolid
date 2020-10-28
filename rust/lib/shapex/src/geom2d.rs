use crate::base::*;
use crate::curve::PolyLine;
use crate::solid::Mesh;
use earcutr;


pub fn cross_2d(vec1: Vec3, vec2: Vec3) -> f64 {
  vec1.x * vec2.z - vec1.z * vec2.x
}

pub fn tesselate_polygon(vertices: PolyLine) -> Mesh {
  let flat_vertices: Vec<f64> = vertices.iter().flat_map(|vertex| vec![vertex.x, vertex.z] ).collect();
  let triangles: Vec<usize> = earcutr::earcut(&flat_vertices, &vec![], 2);
  Mesh {
    vertices: vertices,
    faces: triangles,
  }
}

// Check if two line segments turn clockwise
// Returns values > 0 when clockwise, < 0 when anti-clockwise and 0 when segments are colinear
pub fn clockwise(p1: Point3, p2: Point3, p3: Point3) -> f64 {
  let v1 = p2 - p1;
  let v2 = p3 - p1;
  cross_2d(v1, v2)
}
