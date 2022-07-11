use std::rc::{Rc, Weak};

use uuid::Uuid;
use serde::{Serialize, Serializer, Deserialize, Deserializer};

use crate::base::*;
use crate::curve;
use crate::surface;
use crate::solid;


impl Serialize for solid::Solid {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    dump_solid(self).serialize(serializer)
  }
}


impl<'de> Deserialize<'de> for solid::Solid {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    Ok(undump_solid(Solid::deserialize(deserializer)?))
  }
}


impl solid::Solid {
  pub fn deep_clone(&self) -> Self {
    undump_solid(dump_solid(self))
  }
}


fn undump_solid(solid: Solid) -> solid::Solid {
  solid::Solid {
    id: Uuid::new_v4(),

    // Shells
    shells: solid.shells.into_iter().map(|shell| {

      // Vertices
      let vertices: Vec<Ref<solid::Vertex>> = shell.vertices.into_iter().map(|vertex| rc(solid::Vertex {
        half_edge: Weak::new(),
        point: vertex.point,
      })).collect();

      // Faces
      let mut all_half_edges = vec![];
      let faces = shell.faces.iter().map(|face| {
        // Rings
        let rings: Vec<Ref<solid::Ring>> = face.rings.iter().map(|ring| {
          // Half Edges
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

          // Connect Half Edges in a loop
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

          // Connect Half Edges to Rings
          for he in half_edges {
            he.borrow_mut().ring = Rc::downgrade(&out_ring);
          }
          out_ring
        }).collect();

        let out_face = rc(solid::Face {
          id: face.id,
          outer_ring: rings[0].clone(),
          rings: rings.clone(),
          surface: face.surface.clone(),
          flip_normal: false,
        });

        // Connect rings to face
        for ring in rings {
          ring.borrow_mut().face = Rc::downgrade(&out_face);
        }
        out_face
      }).collect();

      // Create flat list of serialized HEs in same order as real ones
      let all_half_edge_dummies: Vec<&HalfEdge> = shell.faces.iter().flat_map(|face|
        face.rings.iter().flat_map(|ring| ring ).collect::<Vec<&HalfEdge>>()
      ).collect();

      // Edges
      let edges = shell.edges.into_iter().enumerate().map(|(i, edge)| {
        // Find matching serialized HEs and map them to real ones
        let half_edges: Vec<Ref<solid::HalfEdge>> = all_half_edge_dummies.iter().enumerate()
        .filter_map(|(j, he)| if he.edge == i {
          Some(all_half_edges[j].clone())
        } else {
          None
        }).collect();
        let out_edge = rc(solid::Edge {
          id: edge.id,
          left_half: half_edges[0].clone(),
          right_half: half_edges[1].clone(),
          curve: edge.curve,
        });
        // Connect Half Edges to Edge
        half_edges[0].borrow_mut().edge = Rc::downgrade(&out_edge);
        half_edges[1].borrow_mut().edge = Rc::downgrade(&out_edge);
        out_edge
      }).collect();

      solid::Shell {
        vertices,
        faces,
        edges,
      }
    }).collect(),
  }
}

fn dump_solid(solid: &solid::Solid) -> Solid {
  Solid {
    // Shells
    shells: solid.shells.iter().map(|shell| {

      // Edges
      let edges = shell.edges.iter().map(|edge| Edge {
        id: edge.borrow().id,
        curve: edge.borrow().curve.clone(),
      }).collect();

      // Vetices
      let vertices = shell.vertices.iter().map(|vertex| Vertex {
        point: vertex.borrow().point,
      }).collect();

      Shell {
        edges,
        vertices,
        // Faces
        faces: shell.faces.iter().map(|face| {
          let face = face.borrow();
          Face {
            id: face.id,
            rings: face.rings.iter().map(|ring|
              ring.borrow().iter().map(|he| {
                let he = he.borrow();
                HalfEdge {
                  origin: get_vertex_index(&he.origin, &shell.vertices),
                  edge: get_edge_index(&he.edge.upgrade().unwrap(), &shell.edges),
                }
              }).collect()
            ).collect(),
            surface: face.surface.clone(),
            flip_normal: face.flip_normal,
          }
        }).collect(),
      }
    }).collect(),
  }
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
  pub id: Uuid,
  pub rings: Vec<Vec<HalfEdge>>,
  pub surface: surface::SurfaceType,
  pub flip_normal: bool,
}


#[derive(Debug, Serialize, Deserialize)]
struct Edge {
  pub id: Uuid,
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
  fn serialize() {
    let cube = &features::make_cube(1.5, 1.5, 1.5).unwrap().solids[0];
    ron::to_string(cube).unwrap();
  }
}
