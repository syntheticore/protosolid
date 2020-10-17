use crate::base::*;
use crate::curve::*;
use crate::surface::*;


/// Boundary representation of a solid body
/// # Examples
///
/// ```
/// let x = 5;
/// ```
#[derive(Debug)]
pub struct Solid {
  shells: Vec<Shell>, //XXX Should outer shell be handled separately?
}


/// A collection of faces forming a closed volume
/// # Examples
///
/// ```
/// let x = 5;
/// ```
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


/// A portion of an actual surface, bounded by edge loops
/// # Examples
///
/// ```
/// let x = 5;
/// ```
#[derive(Debug)]
pub struct Face {
  outer_loop: Loop,
  inner_loops: Vec<Loop>,
  surface: Box<dyn Surface>,
  normal_direction: bool,
}


// pub type Loop = Vec<OrientedEdge>;


/// A portion of an actual curve, bounded by vertices, separating exactly two faces
/// # Examples
///
/// ```
/// let x = 5;
/// ```
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


#[derive(Debug, Copy, Clone)]
pub struct Vertex {
  point: Point3,
}


#[derive(Debug)]
pub struct Loop {
  edges: Vec<OrientedEdge>
}

impl Loop {
  pub fn new() -> Self {
    Self {
      edges: vec![],
    }
  }

  pub fn iter(&self) -> impl Iterator<Item = *mut Vertex> + '_  {
    unsafe {
      self.edges.iter().map(|oedge| {
        let vertices = (*oedge.edge).vertices;
        if oedge.orientation {
          vertices.0
        } else {
          vertices.1
        }
      })
    }
  }
}


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


// impl<'a> Iterator for Loop {
//   type Item = &'a Vertex;

//   fn next(&mut self) -> Option<&'a Vertex> {
//     if let Some(ref mut vertex_iter) = self.vertex_iter {
//       let vertex = vertex_iter.next();
//       if vertex.is_some() {
//         vertex
//       } else {
//         self.vertex_iter = None;
//         self.next()
//       }
//     } else {
//       if let Some(line) = self.elem_iter.next() {
//         self.vertex_iter = Some(line.vertices.iter());
//         self.next()
//       } else {
//         None
//       }
//     }
//   }
// }


pub fn make_solid() -> Solid {
  let plane = Plane::default();
  let mut vertices = vec![
    Vertex { point: Point3::new(0.0, 0.0, 0.0) },
    Vertex { point: Point3::new(1.0, 0.0, 0.0) },
  ];
  let mut face1 = Face {
    outer_loop: Loop::new(),
    inner_loops: vec![],
    surface: Box::new(plane.clone()),
    normal_direction: true,
  };
  let mut face2 = Face {
    outer_loop: Loop::new(),
    inner_loops: vec![],
    surface: Box::new(plane),
    normal_direction: true,
  };
  let mut edges = vec![
    Edge {
      direction: true,
      curve: Box::new(Line::new(vertices[0].point, vertices[1].point)),
      vertices: (&mut vertices[0], &mut vertices[1]),
      faces: (&mut face1, &mut face2),
    }
  ];
  face1.outer_loop.edges.push(OrientedEdge {
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
