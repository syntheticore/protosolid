use crate::base::*;


#[derive(Debug, Default)]
pub struct Mesh {
  pub vertices: Vec<Point3>,
  pub faces: Vec<usize>,
}

impl Mesh {
  pub fn to_buffer_geometry(&self) -> Vec<f64> {
    self.faces.iter()
      .map(|&face| &self.vertices[face] )
      .flat_map(|vertex| vec![vertex.x, vertex.y, vertex.z] )
      .collect()
  }
}
