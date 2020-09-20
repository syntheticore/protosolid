use crate::base::*;
use crate::curve::*;
use crate::surface::*;


#[derive(Debug)]
pub struct Solid {
  shells: Vec<Shell>, //XXX Should outer shell be handled separately?
}


#[derive(Debug)]
pub struct Shell {
  closed: bool,
  faces: Vec<Face>,
  edges: Vec<Edge>,
  vertices: Vec<Vertex>,
}

impl Shell {
  pub fn euler_characteristics(&self) -> usize {
    let num_faces = self.faces.len();
    let num_loops = self.faces.iter().fold(0, |acc, face| acc + 1 + face.inner_loops.len());
    num_faces - self.edges.len() + self.vertices.len() + (num_faces - num_loops)
  }

  pub fn connectivity(&self) -> usize {
    // Closed shells have odd connectivity
    if self.closed {
      3 - self.euler_characteristics()
    } else {
      panic!("How to calculate connectivity of open shells?")
    }
  }

  // Topological type (Number of handles on a sphere)
  pub fn genus(&self) -> usize {
    if !self.closed { panic!("Open Shell"); }
    let h = self.connectivity();
    if h >= 3 {
      (h - 1) / 2
    } else {
      0
    }
  }
}


#[derive(Debug)]
pub struct Face {
  outer_loop: Vec<OrientedEdge>,
  inner_loops: Vec<Vec<OrientedEdge>>,
  surface: Box<dyn Surface>,
}


#[derive(Debug)]
pub struct Edge {
  direction: bool,
  faces: (*mut Face, *mut Face),
  vertices: (*mut Vertex, *mut Vertex),
  curve: Box<dyn Curve>,
}


#[derive(Debug)]
pub struct OrientedEdge {
  edge: *mut Edge,
  orientation: bool,
}


#[derive(Debug)]
pub struct Vertex {
  point: Point3,
}


pub fn make_solid() -> Solid {
  let plane = Plane::new();
  let mut face1 = Face {
    outer_loop: vec![],
    inner_loops: vec![],
    surface: Box::new(plane.clone()),
  };
  let mut face2 = Face {
    outer_loop: vec![],
    inner_loops: vec![],
    surface: Box::new(plane),
  };
  let mut vertices = vec![
    Vertex { point: Point3::new(0.0, 0.0, 0.0) },
    Vertex { point: Point3::new(1.0, 0.0, 0.0) },
  ];
  let mut edges = vec![
    Edge {
      direction: true,
      curve: Box::new(Line::new((vertices[0].point, vertices[1].point))),
      vertices: (&mut vertices[0], &mut vertices[1]),
      faces: (&mut face1, &mut face2),
    }
  ];
  face1.outer_loop.push(OrientedEdge {
    edge: &mut edges[0],
    orientation: true,
  });
  let shell = Shell {
    closed: true,
    faces: vec![face1, face2],
    edges: edges,
    vertices: vertices,
  };
  Solid {
    shells: vec![shell],
  }
}
