use std::ptr;
use std::rc::{Rc, Weak};

// use uuid::Uuid;

use crate::base::*;
use crate::curve::*;
use crate::surface::*;


#[derive(Debug)]
pub enum BooleanType {
  Add,
  Subtract,
  Intersection,
  Difference,
}


#[derive(Debug)]
pub struct Solid {
  pub shells: Vec<Shell>, // Shell 0 is outer shell
}


#[derive(Debug)]
pub struct Shell {
  // id: Uuid,
  pub closed: bool,
  pub faces: Vec<Ref<Face>>,
  pub edges: Vec<Ref<Edge>>,
  pub vertices: Vec<Ref<Vertex>>,
}


#[derive(Debug)]
pub struct Face {
  // pub id: Uuid,
  pub outer_ring: Ref<HalfEdge>, // clockwise
  pub inner_rings: Vec<Ref<HalfEdge>>, // counter-clockwise
  pub surface: TrimmedSurface,
}


#[derive(Debug)]
pub struct Edge {
  pub left_half: Ref<HalfEdge>,
  pub right_half: Ref<HalfEdge>,
  pub curve: TrimmedSketchElement,
  pub curve_direction: bool, // true means forward according to left_half
}


#[derive(Debug, Clone)]
pub struct HalfEdge {
  pub next: WeakRef<Self>,
  pub previous: WeakRef<Self>,
  pub origin: Ref<Vertex>,
  pub face: WeakRef<Face>,
  pub edge: WeakRef<Edge>,
}


#[derive(Debug)]
pub struct Vertex {
  // pub id: Uuid,
  pub point: Point3,
  pub half_edge: WeakRef<HalfEdge>, // half_edge emanating from this vertex
}


impl Solid {
  pub fn new() -> Self {
    Solid {
      shells: vec![],
    }
  }

  pub fn mvfs(&mut self, p: Point3, surface: TrimmedSurface) -> &Shell {
    let mut shell = Shell {
      closed: true,
      faces: vec![],
      edges: vec![],
      vertices: vec![],
    };
    let vertex = rc(Vertex {
      point: p,
      half_edge: Weak::new(),
    });
    let he = rc(HalfEdge {
      previous: Weak::new(),
      next: Weak::new(),
      origin: vertex.clone(),
      face: Weak::new(),
      edge: Weak::new(),
    });
    vertex.borrow_mut().half_edge = Rc::downgrade(&he);
    let face = rc(Face {
      outer_ring: he.clone(),
      inner_rings: vec![],
      surface,
    });
    {
      let mut heb = he.borrow_mut();
      heb.previous = Rc::downgrade(&he);
      heb.next = Rc::downgrade(&he);
      heb.face = Rc::downgrade(&face);
    }
    shell.vertices.push(vertex);
    shell.faces.push(face);
    self.shells.push(shell);
    self.shells.last().unwrap()
  }

  pub fn boolean(&mut self, _tool: Self, _op: BooleanType) -> Vec<Solid> {
    vec![Self { shells: vec![] }]
  }

  pub fn boolean_all(_tool: Self, _others: &Vec<&mut Self>, _op: BooleanType) {

  }
}


impl Shell {
  pub fn euler_characteristics(&self) -> i32 {
    let num_faces = self.faces.len();
    let num_loops = self.faces.iter().fold(0, |acc, face| acc + 1 + face.borrow().inner_rings.len());
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

  pub fn lmev(&mut self) {

  }
}


impl HalfEdge {
  pub fn mate(&self) -> Ref<Self> {
    let edge = self.edge.upgrade().unwrap();
    let edge = edge.borrow();
    if ptr::eq(self, &*edge.left_half.borrow()) {
      edge.right_half.clone()
    } else {
      edge.left_half.clone()
    }
  }

  pub fn end_vertex(&self) -> Ref<Vertex> {
    self.mate().borrow().origin.clone()
  }

  pub fn ring_iter(&self) -> RingIterator  {
    RingIterator::new(self.mate().borrow().mate())
  }
}


impl Vertex {
  pub fn edges_iter(&self) -> VertexEdgesIterator  {
    VertexEdgesIterator::new(self)
  }
}


pub struct RingIterator {
  start_edge: Ref<HalfEdge>,
  current_edge: Ref<HalfEdge>,
}

impl RingIterator {
  fn new(start_edge: Ref<HalfEdge>) -> Self {
    Self {
      start_edge: start_edge.clone(),
      current_edge: start_edge,
    }
  }
}

impl Iterator for RingIterator {
  type Item = Ref<HalfEdge>;

  fn next(&mut self) -> Option<Self::Item> {
    let current_edge = self.current_edge.clone();
    self.current_edge = current_edge.borrow().next.upgrade().unwrap().clone();
    if Rc::ptr_eq(&current_edge, &self.start_edge) {
      None
    } else {
      Some(current_edge)
    }
  }
}


pub struct VertexEdgesIterator {
  start_edge: Ref<HalfEdge>,
  current_edge: Ref<HalfEdge>,
}

impl VertexEdgesIterator {
  fn new(start_vertex: &Vertex) -> Self {
    let he = &start_vertex.half_edge.upgrade().unwrap();
    // let he = &*he.borrow();
    Self {
      start_edge: (*he).clone(),
      current_edge: (*he).clone(),
    }
  }
}

impl Iterator for VertexEdgesIterator {
  type Item = Ref<HalfEdge>;

  fn next(&mut self) -> Option<Self::Item> {
    let current_edge = self.current_edge.clone();
    self.current_edge = current_edge.borrow().mate().borrow().next.upgrade().unwrap().clone();
    if Rc::ptr_eq(&current_edge, &self.start_edge) {
      None
    } else {
      Some(current_edge)
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
