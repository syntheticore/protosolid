use std::rc::{Rc, Weak};

use uuid::Uuid;
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
  undump_solid(solid)
}


fn undump_solid(solid: Solid) -> solid::Solid {
  solid::Solid {
    id: Uuid::new_v4(),
    shells: solid.shells.into_iter().map(|shell| {
      let vertices: Vec<Ref<solid::Vertex>> = shell.vertices.into_iter().map(|vertex| rc(solid::Vertex {
        half_edge: Weak::new(),
        point: vertex.point,
      })).collect();
      let mut all_half_edges = vec![];
      let faces = shell.faces.iter().map(|face| {

        let rings: Vec<Ref<solid::Ring>> = face.rings.iter().map(|ring| {
          let half_edges: Vec<Ref<solid::HalfEdge>> = ring.iter().map(|he| {
            let vertex = &vertices[he.origin];
            let half_edge = rc(solid::HalfEdge {
              id: Uuid::new_v4(),
              next: Weak::new(),
              previous: Weak::new(),
              origin: vertex.clone(),
              edge: Weak::new(),
              ring: Weak::new(),
            });
            vertex.borrow_mut().half_edge = Rc::downgrade(&half_edge);
            all_half_edges.push(half_edge.clone());
            half_edge
          }).collect();
          let len = half_edges.len();
          for i in 0..len {
            let he = &half_edges[i];
            let next_i = (i + 1) % len;
            let previous_i = (len + i - 1) % len;
            let mut he = he.borrow_mut();
            he.next = Rc::downgrade(&half_edges[next_i]);
            he.previous = Rc::downgrade(&half_edges[previous_i]);
          }
          let out_ring = rc(solid::Ring {
            half_edge: half_edges[0].clone(),
            face: Weak::new(),
          });
          for he in half_edges {
            he.borrow_mut().ring = Rc::downgrade(&out_ring);
          }
          out_ring
        }).collect();

        let out_face = rc(solid::Face {
          id: Uuid::new_v4(),
          outer_ring: rings[0].clone(),
          rings: rings.clone(),
          surface: face.surface.clone(),
        });

        for ring in rings {
          ring.borrow_mut().face = Rc::downgrade(&out_face);
        }

        out_face
      }).collect();

      let all_half_edge_dummies: Vec<&HalfEdge> = shell.faces.iter().flat_map(|face| face.rings.iter().flat_map(|ring| ring ).collect::<Vec<&HalfEdge>>() ).collect();
      let edges = shell.edges.into_iter().enumerate().map(|(i, edge)| {
        let half_edges: Vec<Ref<solid::HalfEdge>> = all_half_edge_dummies.iter().enumerate()
        .filter_map(|(j, he)| if he.edge == i {
          Some(all_half_edges[j].clone())
        } else {
          None
        }).collect();
        let out_edge = rc(solid::Edge {
          id: Uuid::new_v4(),
          left_half: half_edges[0].clone(),
          right_half: half_edges[1].clone(),
          curve: edge.curve,
        });
        half_edges[0].borrow_mut().edge = Rc::downgrade(&out_edge);
        half_edges[1].borrow_mut().edge = Rc::downgrade(&out_edge);
        out_edge
      }).collect();

      solid::Shell {
        closed: true,
        vertices,
        faces,
        edges,
      }
    }).collect(),
  }
}

fn dump_solid(solid: &solid::Solid) -> Solid {
  Solid {
    shells: solid.shells.iter().map(|shell| {
      let edges = shell.edges.iter().map(|edge| Edge {
        curve: edge.borrow().curve.clone(),
      }).collect();
      let vertices = shell.vertices.iter().map(|vertex| Vertex {
        point: vertex.borrow().point,
      }).collect();
      Shell {
        edges,
        vertices,
        faces: shell.faces.iter().map(|face| Face {
          rings: face.borrow().rings.iter().map(|ring|
            convert_ring(&ring.borrow(), &shell.edges, &shell.vertices)
          ).collect(),
          surface: face.borrow().surface.clone(),
        }).collect(),
      }
    }).collect(),
  }
}

fn convert_ring(
  ring: &solid::Ring,
  edges: &Vec<Ref<solid::Edge>>,
  vertices: &Vec<Ref<solid::Vertex>>,
) -> Vec<HalfEdge> {
  ring.iter().map(|he| {
    let he = he.borrow();
    HalfEdge {
      origin: get_vertex_index(&he.origin, vertices),
      edge: get_edge_index(&he.edge.upgrade().unwrap(), edges),
    }
  }).collect()
}

fn get_edge_index(edge: &Ref<solid::Edge>, edges: &Vec<Ref<solid::Edge>>) -> usize {
  edges.iter().position(|e| Rc::ptr_eq(e, edge) ).unwrap()
}

fn get_vertex_index(vertex: &Ref<solid::Vertex>, vertices: &Vec<Ref<solid::Vertex>>) -> usize {
  vertices.iter().position(|e| Rc::ptr_eq(e, vertex) ).unwrap()
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
