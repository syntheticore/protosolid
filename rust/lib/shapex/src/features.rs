// use std::rc::{Rc, Weak};

use crate::base::*;
use crate::curve::*;
use crate::surface::*;
use crate::solid::*;


pub trait Feature {}


pub fn extrude(region: Vec<TrimmedSketchElement>, distance: f64) -> Solid {
  let mut solid = Solid::new_lamina(region, Plane::default());
  let shell = &mut solid.shells[0];
  shell.sweep(&shell.faces.last().unwrap().clone(), Vec3::new(0.0, 0.0, distance));
  solid
}

pub fn fillet_edges(_solid: &mut Solid, _edges: Vec<&Edge>) {

}

pub fn make_cube(dx: f64, dy: f64, dz: f64) -> Solid {
  let mut top = Box::new(Plane::default());
  let mut bottom = top.clone();
  bottom.flip();
  top.origin.z = dz;
  let points = [
    Point3::new(0.0, 0.0, 0.0),
    Point3::new(dx, 0.0, 0.0),
    Point3::new(dx, dy, 0.0),
    Point3::new(0.0, dy, 0.0),
  ];
  // Create solid from bottom face with empty loop
  let mut solid = Solid::new(points[0], bottom);
  let shell = &mut solid.shells[0];
  let he = shell.vertices.last().unwrap().borrow().half_edge.upgrade().unwrap();
  // Front edge
  let (front_edge, _) = shell.lmev(&he, &he, make_line(points[1], points[0]), points[1]);
  let he = &front_edge.borrow().left_half;
  // Right edge
  let (right_edge, _) = shell.lmev(he, he, make_line(points[2], points[1]), points[2]);
  let he = &right_edge.borrow().left_half;
  // Back edge
  let (back_edge, _) = shell.lmev(he, he, make_line(points[3], points[2]), points[3]);
  // Close left edge to create top face
  let (_, _top_face) = shell.lmef(&front_edge.borrow().right_half, &back_edge.borrow().left_half, make_line(points[0], points[3]), top);
  //4x shell.mev()
  //4x shell.mef() side faces
  solid
}

pub fn make_cube2(dx: f64, dy: f64, dz: f64) -> Solid {
  let points = [
    Point3::new(0.0, 0.0, 0.0),
    Point3::new(dx, 0.0, 0.0),
    Point3::new(dx, dy, 0.0),
    Point3::new(0.0, dy, 0.0),
  ];
  let mut region = vec![];
  let mut iter = points.iter().peekable();
  while let Some(&p) = iter.next() {
    let next = if let Some(&next) = iter.peek() {
      next
    } else {
      &points[0]
    };
    region.push(TrimmedSketchElement::new(make_line(p, *next)));
  }
  let mut solid = Solid::new_lamina(region, Plane::default());
  let shell = &mut solid.shells[0];
  shell.sweep(&shell.faces.last().unwrap().clone(), Vec3::new(0.0, 0.0, dz));
  solid
}

fn make_line(p1: Point3, p2: Point3) -> SketchElement {
  Line::new(p1, p2).into_enum()
}

// pub fn extrude_region(region: Vec<TrimmedSketchElement>, _distance: f64) -> Solid {
//   let shell = Shell {
//     closed: true,
//     faces: vec![],
//     edges: vec![],
//     vertices: vec![],
//   };

//   for elem in region {
//     let vertex = rc(Vertex {
//       point: elem.bounds.0,
//       half_edge: Weak::new(),
//     });
//     let left_he = rc(HalfEdge {
//       next: Weak::new(), //XXX
//       previous: Weak::new(), //XXX
//       origin: vertex.clone(),
//       ring: Weak::new(), //XXX
//       edge: Weak::new(),
//     });
//     vertex.borrow_mut().half_edge = Rc::downgrade(&left_he);
//     let edge = rc(Edge {
//       left_half: left_he.clone(),
//       right_half: left_he.clone(),
//       curve: elem.clone(),
//       curve_direction: true, //XXX
//     });
//     left_he.borrow_mut().edge = Rc::downgrade(&edge);
//     let right_he = left_he.borrow().clone();
//     // right_he.next =
//     let _right_he = rc(right_he);
//   }

//   // let bottom = Face {
//   //   outer_ring: region_edges[0].clone(),
//   //   inner_rings: vec![],
//   //   surface: TrimmedSurface {
//   //     base: Box::new(Plane::default()),
//   //     bounds: vec![region],
//   //   },
//   // };
//   // shell.faces.push(bottom);

//   Solid {
//     shells: vec![shell],
//   }
// }
