use crate::base::*;
use crate::geom3d::*;


#[derive(Debug, Default)]
pub struct Mesh {
  pub vertices: Vec<Point3>,
  pub faces: Vec<usize>,
  // pub normals: Vec<Vec3>,
}

impl Mesh {
  pub fn to_buffer_geometry(&self) -> Vec<f64> {
    self.faces.iter()
    .map(|&face| &self.vertices[face] )
    .flat_map(|vertex| vec![vertex.x, vertex.y, vertex.z] )
    .collect()
  }

  pub fn append(&mut self, mut other: Self) {
    let offset = self.vertices.len();
    self.vertices.append(&mut other.vertices);
    self.faces.append(&mut other.faces.iter().map(|i| i + offset ).collect())
  }

  pub fn invert_normals(&mut self) { todo!() }

  pub fn heal(&mut self) {
    for vertex in self.vertices.iter_mut().enumerate() {

    }
  }
}

impl Transformable for Mesh {
  fn transform(&mut self, transform: &Transform) {
    for vertex in &mut self.vertices {
      *vertex = transform.apply(*vertex);
    }
  }
}
