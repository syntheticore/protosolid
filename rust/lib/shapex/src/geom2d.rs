use crate::curve::PolyLine;
use crate::solid::Mesh;
use earcutr;


pub fn tesselate_polygon(vertices: PolyLine) -> Mesh {
  let flat_vertices: Vec<f64> = vertices.iter().flat_map(|vertex| vec![vertex.x, vertex.z] ).collect();
  let triangles: Vec<usize> = earcutr::earcut(&flat_vertices, &vec![], 2);
  Mesh {
    vertices: vertices,
    faces: triangles,
  }
}
