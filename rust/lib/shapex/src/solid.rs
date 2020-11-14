use std::ptr;

// use uuid::Uuid;

use crate::base::*;
use crate::curve::*;
use crate::surface::*;


// Half-Edge Data Structure

#[derive(Debug)]
pub struct Solid {
  shells: Vec<Shell>, // Shell 0 is outer shell
}

#[derive(Debug)]
pub struct Shell {
  // id: Uuid,
  closed: bool,
  faces: Vec<Face>,
  edges: Vec<Edge>,
  vertices: Vec<Vertex>,
}

#[derive(Debug)]
pub struct Face {
  // id: Uuid,
  outer_ring: *mut HalfEdge, // clockwise
  inner_rings: Vec<*mut HalfEdge>, // counter-clockwise
  surface: Box<dyn Surface>,
  // shell: *mut Shell,
}

// #[derive(Debug)]
// pub struct Ring {
//   half_edge: *mut HalfEdge,
//   face: *mut Face,
// }

#[derive(Debug)]
pub struct Edge {
  left_half: HalfEdge,
  right_half: HalfEdge,
  curve: TrimmedSketchElement,
  curve_direction: bool, // true means forward according to left_half
}

#[derive(Debug)]
pub struct HalfEdge {
  next: *mut Self,
  previous: *mut Self,
  // pair: *mut Self,
  // ring: *mut Ring,
  start_vertex: *mut Vertex,
  face: *mut Face,
  edge: *mut Edge,
}

#[derive(Debug)]
pub struct Vertex {
  // id: Uuid,
  point: Point3,
  half_edge: *mut HalfEdge, // half_edge emanating from this vertex
}


impl Shell {
  pub fn euler_characteristics(&self) -> i32 {
    let num_faces = self.faces.len();
    let num_loops = self.faces.iter().fold(0, |acc, face| acc + 1 + face.inner_rings.len());
    (num_faces - self.edges.len() + self.vertices.len() + (num_faces - num_loops)) as i32
  }

  pub fn connectivity(&self) -> i32 {
    // Closed shells have odd connectivity
    if self.closed {
      3 - self.euler_characteristics()
    } else {
      panic!("How to calculate connectivity of open shells?")
    }
  }

  // Topological type (Number of handles on a sphere)
  pub fn genus(&self) -> i32 {
    if !self.closed { panic!("Open Shell"); }
    let h = self.connectivity();
    if h >= 3 {
      (h - 1) / 2
    } else {
      0
    }
  }
}


pub struct RingIterator<'a> {
  start_edge: &'a HalfEdge,
  current_edge: Option<&'a HalfEdge>,
}

impl<'a> RingIterator<'a> {
  fn new(start_edge: &'a HalfEdge) -> Self {
    Self {
      start_edge,
      current_edge: Some(start_edge),
    }
  }
}

impl<'a> Iterator for RingIterator<'a> {
  type Item = &'a HalfEdge;

  fn next(&mut self) -> Option<Self::Item> {
    if let Some(current_edge) = self.current_edge {
      let next_edge = (*current_edge).next;
      self.current_edge = if ptr::eq(next_edge, self.start_edge) {
        None
      } else {
        Some(unsafe { &*next_edge })
      };
      Some(current_edge)
    } else { None }
  }
}


pub struct VertexEdgesIterator<'a> {
  start_edge: &'a HalfEdge,
  current_edge: Option<&'a HalfEdge>,
}

impl VertexEdgesIterator<'_> {
  fn new(start_vertex: &Vertex) -> Self {
    let he = start_vertex.half_edge;
    let he = unsafe { &*he };
    Self {
      start_edge: he,
      current_edge: Some(he),
    }
  }
}

impl<'a> Iterator for VertexEdgesIterator<'a> {
  type Item = &'a HalfEdge;

  fn next(&mut self) -> Option<Self::Item> {
    if let Some(current_edge) = self.current_edge {
      let next_edge = (*current_edge).mate().next;
      self.current_edge = if ptr::eq(next_edge, self.start_edge) {
        None
      } else {
        Some(unsafe { &*next_edge })
      };
      Some(current_edge)
    } else { None }
  }
}


// impl Ring {
//   pub fn half_edge_iter(&self) -> RingIterator  {
//     RingIterator::new(self.half_edge)
//   }
// }


// impl Edge {
//   unsafe fn faces(&self) -> (*mut Face, *mut Face) {
//     (self.left_half.face, self.right_half.face)
//   }
// }


impl HalfEdge {
  pub fn mate(&self) -> &Self {
    unsafe {
      if ptr::eq(self, &(*self.edge).left_half) {
        &(*self.edge).right_half
      } else {
        &(*self.edge).left_half
      }
    }
  }

  pub fn end_vertex(&self) -> &Vertex {
    unsafe { &*self.mate().start_vertex }
  }

  pub fn ring_iter(&self) -> RingIterator  {
    RingIterator::new(self)
  }
}


impl Vertex {
  pub fn edges_iter(&self) -> VertexEdgesIterator  {
    VertexEdgesIterator::new(self)
  }
}


// pub fn make_solid() -> Solid {
//   let plane = Plane::default();
//   let mut vertices = vec![
//     Vertex { point: Point3::new(0.0, 0.0, 0.0) },
//     Vertex { point: Point3::new(1.0, 0.0, 0.0) },
//   ];
//   let mut face1 = Face {
//     outer_loop: Loop::new(),
//     inner_loops: vec![],
//     surface: Box::new(plane.clone()),
//     normal_direction: true,
//   };
//   let mut face2 = Face {
//     outer_loop: Loop::new(),
//     inner_loops: vec![],
//     surface: Box::new(plane),
//     normal_direction: true,
//   };
//   let mut curve1 = TrimmedSketchElement  {
//     base: SketchElement::Line(Line::new(vertices[0].point, vertices[1].point)),
//     bounds: (0.0, 1.0),
//   };
//   let mut edges = vec![
//     Edge {
//       direction: true,
//       // curve: Box::new(Line::new(vertices[0].point, vertices[1].point)),
//       curve: &mut curve1,
//       vertices: (&mut vertices[0], &mut vertices[1]),
//       faces: (&mut face1, &mut face2),
//     }
//   ];
//   face1.outer_loop.edges.push(OrientedEdge {
//     edge: &mut edges[0],
//     orientation: true,
//   });
//   let shell = Shell {
//     closed: true,
//     faces: vec![face1, face2],
//     edges: edges,
//     vertices: vertices,
//   };
//   Solid {
//     shells: vec![shell],
//   }
// }



// Winged-Edge Data Structure

pub mod winged_edge {
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
    pub fn euler_characteristics(&self) -> i32 {
      let num_faces = self.faces.len();
      let num_loops = self.faces.iter().fold(0, |acc, face| acc + 1 + face.inner_loops.len());
      (num_faces - self.edges.len() + self.vertices.len() + (num_faces - num_loops)) as i32
    }

    pub fn connectivity(&self) -> i32 {
      // Closed shells have odd connectivity
      if self.closed {
        3 - self.euler_characteristics()
      } else {
        panic!("How to calculate connectivity of open shells?")
      }
    }

    // Topological type (Number of handles on a sphere)
    pub fn genus(&self) -> i32 {
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
    // curve: Box<dyn Curve>,
    curve: *mut TrimmedSketchElement,
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
    edges: Vec<OrientedEdge>,
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
    let mut curve1 = TrimmedSketchElement  {
      base: SketchElement::Line(Line::new(vertices[0].point, vertices[1].point)),
      bounds: (0.0, 1.0),
    };
    let mut edges = vec![
      Edge {
        direction: true,
        // curve: Box::new(Line::new(vertices[0].point, vertices[1].point)),
        curve: &mut curve1,
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
