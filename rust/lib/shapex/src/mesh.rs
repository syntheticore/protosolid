use crate::base::*;
use crate::geom3d::*;


#[derive(Debug, Default)]
pub struct Mesh {
  pub vertices: Vec<Point3>,
  pub faces: Vec<usize>,
  pub normals: Vec<Vec3>,
}

impl Mesh {
  pub fn to_buffer_geometry(&self) -> Vec<f64> {
    self.faces.iter()
    .map(|&face| &self.vertices[face] )
    .flat_map(|vertex| vec![vertex.x, vertex.y, vertex.z] )
    .collect()
  }

  pub fn append(&mut self, mut other: Self) {
    let offset = self.vertices.len() as i32;
    let other_faces: Vec<i32> = other.faces.iter().map(|&f| f as i32 ).collect();
    // for (i, vertex) in self.vertices.iter().enumerate() {
    //   for (j, other_vertex) in other.vertices.iter().enumerate() {
    //     if vertex == other_vertex {
    //       // Duplicate found
    //       // -> Update new faces to point to existing vertex
    //       for face in &mut other_faces {
    //         if *face == j as i32 {
    //           *face = i as i32 - offset;
    //         }
    //       }
    //       break
    //     }
    //   }
    // }
    //XXX remove duplicate vertices from other
    self.vertices.append(&mut other.vertices);
    self.faces.append(&mut other_faces.iter().map(|&f| (f + offset) as usize ).collect())
  }

  pub fn invert_normals(&mut self) { todo!() }

  pub fn heal(&mut self) { todo!() }
}

impl Transformable for Mesh {
  fn transform(&mut self, transform: &Transform) {
    for vertex in &mut self.vertices {
      *vertex = transform.apply(*vertex);
    }
  }
}
