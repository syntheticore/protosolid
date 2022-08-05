use crate::internal::*;
use crate::transform::*;


/// All types that can be tessellated, generating a polygonal [Mesh].

pub trait Meshable {
  fn tesselate(&self) -> Mesh;
}


/// Simple polygonal mesh.
///
/// Meshes are stored as a list of vertices, as well as a flat list of indices pointing into the former, representing its faces.
/// Faces are strictly triangular. Normals are stored per vertex.

#[derive(Debug, Default)]
pub struct Mesh {
  pub vertices: Vec<Point3>,
  pub faces: Vec<usize>,
  pub normals: Vec<Vec3>,
}

impl Mesh {
  pub fn to_buffer_geometry(&self) -> (Vec<f64>, Vec<f64>) {
    let positions = self.faces.iter()
    .map(|&face| &self.vertices[face] )
    .flat_map(|vertex| vec![vertex.x, vertex.y, vertex.z] )
    .collect();
    let normals = self.normals.iter()
    .flat_map(|normal| vec![normal.x, normal.y, normal.z] )
    .collect();
    (positions, normals)
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
    self.faces.append(&mut other_faces.iter().map(|&f| (f + offset) as usize ).collect());
    self.normals.append(&mut other.normals);
  }

  pub fn invert_normals(&mut self) { todo!() }

  pub fn heal(&mut self) { todo!() }
}

impl Transformable for Mesh {
  fn transform(&mut self, transform: &Matrix4) {
    for vertex in &mut self.vertices {
      *vertex = transform.transform_point(*vertex);
    }
    for normal in &mut self.normals {
      *normal = transform.transform_vector(*normal);
    }
  }
}
