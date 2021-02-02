use serde::{Serialize, Deserialize};

use crate::base::*;
use crate::curve;
use crate::surface;
use crate::solid;


pub fn export(solid: &solid::Solid) -> String {
  ron::to_string(&dump_solid(solid)).unwrap()
}

pub fn import(dump: String) -> solid::Solid {
  let solid: Solid = ron::from_str(&dump).unwrap();
  undump_solid(&solid)
}


fn undump_solid(_solid: &Solid) -> solid::Solid {
  solid::Solid::new()
}

fn dump_solid(solid: &solid::Solid) -> Solid {
  Solid {
    shells: solid.shells.iter().map(|shell| Shell {
      faces: shell.faces.iter().map(|face| Face {
        rings: face.borrow().rings.iter().map(|ring|
          convert_ring(&ring.borrow())
        ).collect(),
        surface: face.borrow().surface.clone(),
      }).collect(),
      edges: shell.edges.iter().map(|edge| Edge {
        curve: edge.borrow().curve.clone(),
      }).collect(),
      vertices: shell.vertices.iter().map(|vertex| Vertex {
        point: vertex.borrow().point,
      }).collect(),
    }).collect(),
  }
}

fn convert_ring(ring: &solid::Ring) -> Vec<HalfEdge> {
  ring.iter().map(|_he| {
    // let he = he.borrow();
    HalfEdge {
      origin: 0,
      edge: 0,
    }
  }).collect()
}


#[derive(Debug, Serialize, Deserialize)]
struct Solid {
  pub shells: Vec<Shell>,
}


#[derive(Debug, Serialize, Deserialize)]
struct Shell {
  pub faces: Vec<Face>,
  pub edges: Vec<Edge>,
  pub vertices: Vec<Vertex>,
}


#[derive(Debug, Serialize, Deserialize)]
struct Face {
  pub rings: Vec<Vec<HalfEdge>>,
  pub surface: surface::SurfaceType,
}


#[derive(Debug, Serialize, Deserialize)]
struct Edge {
  pub curve: curve::CurveType,
}


#[derive(Debug, Serialize, Deserialize)]
struct HalfEdge {
  pub origin: usize,
  pub edge: usize,
}


#[derive(Debug, Serialize, Deserialize)]
struct Vertex {
  pub point: Point3,
}


#[cfg(test)]
mod tests {
  use crate::solid::features;

  #[test]
  fn stl() {
    let cube = features::make_cube(1.5, 1.5, 1.5);
    let ron = super::export(&cube);
    println!("{}", ron);
  }
}
