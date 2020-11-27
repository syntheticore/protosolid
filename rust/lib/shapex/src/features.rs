use std::rc::{Rc, Weak};

use crate::base::*;
use crate::curve::*;
use crate::surface::*;
use crate::solid::*;


pub trait Feature {}


pub fn extrude_region(region: Vec<TrimmedSketchElement>, _distance: f64) -> Solid {
  let shell = Shell {
    closed: true,
    faces: vec![],
    edges: vec![],
    vertices: vec![],
  };

  for elem in region {
    let vertex = rc(Vertex {
      point: elem.bounds.0,
      half_edge: Weak::new(),
    });
    let left_he = rc(HalfEdge {
      next: Weak::new(), //XXX
      previous: Weak::new(), //XXX
      origin: vertex.clone(),
      ring: Weak::new(), //XXX
      edge: Weak::new(),
    });
    vertex.borrow_mut().half_edge = Rc::downgrade(&left_he);
    let edge = rc(Edge {
      left_half: left_he.clone(),
      right_half: left_he.clone(),
      // curve: elem.clone(),
      // curve_direction: true, //XXX
    });
    left_he.borrow_mut().edge = Rc::downgrade(&edge);
    let right_he = left_he.borrow().clone();
    // right_he.next =
    let _right_he = rc(right_he);
  }

  // let bottom = Face {
  //   outer_ring: region_edges[0].clone(),
  //   inner_rings: vec![],
  //   surface: TrimmedSurface {
  //     base: Box::new(Plane::default()),
  //     bounds: vec![region],
  //   },
  // };
  // shell.faces.push(bottom);

  Solid {
    shells: vec![shell],
  }
}


pub fn fillet_edges(_solid: &mut Solid, _edges: Vec<&Edge>) {

}

pub fn make_cube() -> Solid {
  let mut solid = Solid::new();
  let surface = Box::new(Plane::default());
  let shell = solid.mvfs(Point3::new(0.0, 0.0, 0.0), surface);
  //3x shell.mev()
  //1x shell.mef() to make lamina
  //4x shell.mev()
  //4x shell.mef() side faces
  solid
}
