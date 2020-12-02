use std::ptr;
use std::rc::{Rc, Weak};

// use uuid::Uuid;

use crate::base::*;
use crate::curve::*;
use crate::surface::*;
use crate::mesh::*;
use crate::geom2d;


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
  pub outer_ring: Ref<Ring>,
  pub rings: Vec<Ref<Ring>>,
  pub surface: Box<dyn Surface>,
}


#[derive(Debug)]
pub struct Ring {
  pub half_edge: Ref<HalfEdge>,
  pub face: WeakRef<Face>,
}


#[derive(Debug)]
pub struct Edge {
  pub left_half: Ref<HalfEdge>,
  pub right_half: Ref<HalfEdge>,
  pub curve: SketchElement,
  pub curve_direction: bool, // true means forward according to left_half
}


#[derive(Debug, Clone)]
pub struct HalfEdge {
  pub next: WeakRef<Self>,
  pub previous: WeakRef<Self>,
  pub origin: Ref<Vertex>,
  pub edge: WeakRef<Edge>,
  pub ring: WeakRef<Ring>,
}


#[derive(Debug)]
pub struct Vertex {
  // pub id: Uuid,
  pub point: Point3,
  pub half_edge: WeakRef<HalfEdge>, // half_edge emanating from this vertex
}


impl Solid {
  pub fn new(p: Point3, surface: Box<dyn Surface>) -> Self {
    let mut this = Self { shells: vec![] };
    this.mvfs(p, surface);
    this
  }

  pub fn new_lamina(region: Vec<TrimmedSketchElement>, top_plane: Plane) -> Self {
    let mut bottom = top_plane.clone();
    bottom.flip();
    // Create shell from bottom face with empty ring
    let first_elem = region.first().unwrap().clone();
    let mut this = Self::new(first_elem.bounds.0, Box::new(bottom));
    let shell = &mut this.shells[0];
    let mut he = shell.vertices.last().unwrap().borrow().half_edge.upgrade().unwrap();
    // Complete ring of bottom face
    let n = region.len() - 1;
    // for elem in region.into_iter().take(n) {
    for elem in WireIterator::new(&region).take(n) {
      let points = elem.bounds;
      let (new_edge, _) = shell.lmev(&he, &he, elem.base.clone(), points.1);
      he = new_edge.borrow().left_half.clone();
    }
    // Create top face
    let he1 = shell.edges[0].borrow().right_half.clone();
    let he2 = shell.edges.last().unwrap().borrow().left_half.clone();
    shell.lmef(&he1, &he2, first_elem.base, Box::new(top_plane));
    this
  }

  // https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/model/euler.html
  pub fn euler_characteristics(&self) -> i32 {
    let genus = self.shells.first().unwrap().genus(); //XXX should we fold over all shells?
    self.shells.iter().fold(0, |acc, shell| acc + shell.euler_characteristics() ) - 2 * (self.shells.len() as i32 - genus)
  }

  pub fn mvfs(&mut self, p: Point3, surface: Box<dyn Surface>) -> (Ref<Vertex>, Ref<Face>, &mut Shell) {
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
      ring: Weak::new(),
      edge: Weak::new(), // Left empty
    });
    vertex.borrow_mut().half_edge = Rc::downgrade(&he);
    let ring = rc(Ring {
      half_edge: he.clone(),
      face: Weak::new(),
    });
    let face = rc(Face {
      outer_ring: ring.clone(),
      rings: vec![ring.clone()],
      surface,
    });
    ring.borrow_mut().face = Rc::downgrade(&face);
    {
      let mut heb = he.borrow_mut();
      heb.previous = Rc::downgrade(&he);
      heb.next = Rc::downgrade(&he);
      heb.ring = Rc::downgrade(&ring);
    }
    shell.vertices.push(vertex.clone());
    shell.faces.push(face.clone());
    self.shells.push(shell);
    (vertex, face, self.shells.last_mut().unwrap())
  }

  pub fn boolean(&mut self, _tool: Self, _op: BooleanType) -> Vec<Solid> {
    vec![Self { shells: vec![] }]
  }

  pub fn boolean_all(tool: Self, others: &mut Vec<Solid>, _op: BooleanType) {
    others.push(tool);
  }

  pub fn tesselate(&self) -> Mesh {
    let mut mesh = Mesh::default();
    for face in &self.shells[0].faces {
      mesh.append(face.borrow().tesselate());
    }
    mesh
  }
}


impl Shell {
  pub fn euler_characteristics(&self) -> i32 {
    let num_faces = self.faces.len();
    let num_loops = self.faces.iter().fold(0, |acc, face| acc + face.borrow().rings.len());
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
    if !self.closed { panic!("Open Shell") } //XXX should return error
    let h = self.connectivity();
    if h >= 3 {
      (h - 1) / 2
    } else {
      0
    }
  }

  pub fn lmev(&mut self, he1: &Ref<HalfEdge>, he2: &Ref<HalfEdge>, curve: SketchElement, p: Point3) -> (Ref<Edge>, Ref<Vertex>) {
    let vertex = rc(Vertex {
      point: p,
      half_edge: Weak::new(),
    });
    let mut he = he1.clone();
    while !Rc::ptr_eq(&he, he2) {
      he = {
        let mut heb = he.borrow_mut();
        heb.origin = vertex.clone();
        let mate = heb.mate();
        let next = &mate.borrow().next;
        next.upgrade().unwrap().clone()
      }
    }
    let he2b = he2.borrow_mut();
    let edge = rc(Edge {
      left_half: HalfEdge::new_at(&vertex, he2),
      right_half: HalfEdge::new_at(&he2b.origin, he1),
      curve_direction: curve.as_curve().endpoints().0.almost(p),
      curve,
    });
    {
      let e = edge.borrow();
      e.left_half.borrow_mut().edge = Rc::downgrade(&edge);
      e.right_half.borrow_mut().edge = Rc::downgrade(&edge);
    }
    vertex.borrow_mut().half_edge = he2b.previous.clone();
    he2b.origin.borrow_mut().half_edge = Rc::downgrade(he2);
    self.vertices.push(vertex.clone());
    self.edges.push(edge.clone());
    (edge, vertex)
  }

  pub fn lmef(&mut self, he1: &Ref<HalfEdge>, he2: &Ref<HalfEdge>, curve: SketchElement, surface: Box<dyn Surface>) -> (Ref<Edge>, Ref<Face>) {
    let he1b = he1.borrow_mut();
    let he2b = he2.borrow_mut();
    let nhe1 = HalfEdge::new_at(&he2b.origin, he1);
    let nhe2 = HalfEdge::new_at(&he1b.origin, he2);
    let edge = rc(Edge {
      left_half: nhe2.clone(),
      right_half: nhe1.clone(),
      curve,
      curve_direction: true, //XXX
    });
    let ring = rc(Ring {
      half_edge: nhe1.clone(),
      face: Weak::new(),
    });
    let face = rc(Face {
      outer_ring: ring.clone(),
      rings: vec![ring.clone()],
      surface,
    });
    ring.borrow_mut().face = Rc::downgrade(&face);
    let mut he = he1.clone();
    while !Rc::ptr_eq(&he, he2) {
      he = {
        let mut heb = he.borrow_mut();
        heb.ring = Rc::downgrade(&ring);
        heb.next.upgrade().unwrap().clone()
      }
    }
    {
      let mut nhe1b = nhe1.borrow_mut();
      let mut nhe2b = nhe2.borrow_mut();
      nhe1b.ring = Rc::downgrade(&ring);
      nhe1b.previous.upgrade().unwrap().borrow_mut().next = Rc::downgrade(&nhe2);
      nhe2b.previous.upgrade().unwrap().borrow_mut().next = Rc::downgrade(&nhe1);
      let temp = nhe1b.previous.clone();
      nhe1b.previous = nhe2b.previous.clone();
      nhe2b.previous = temp;
    }
    he2b.ring.upgrade().unwrap().borrow_mut().half_edge = nhe2;
    self.edges.push(edge.clone());
    self.faces.push(face.clone());
    (edge, face)
  }

  pub fn sweep(&mut self, face: &Ref<Face>, vec: Vec3) {
    for ring in &face.borrow().rings {
      let first = &ring.borrow().half_edge;
      let mut scan = first.borrow().next.upgrade().unwrap().clone();
      self.sweep_mev(&scan, vec);
      while !Rc::ptr_eq(&scan, &first) {
        scan = {
          let scanb = scan.borrow();
          self.sweep_mev(&scanb.next.upgrade().unwrap(), vec);
          self.sweep_mef(&scan, vec);
          scanb.next.upgrade().unwrap().borrow().mate().borrow().next.upgrade().unwrap().clone()
        }
      }
      self.sweep_mef(&scan, vec);
    }
    //XXX move top surface by vec
  }

  fn sweep_mev(&mut self, scan: &Ref<HalfEdge>, vec: Vec3) {
    let vertex = &scan.borrow().origin;
    let point = vertex.borrow().point;
    let new_point = point + vec;
    let line = Line::new(new_point, point).into_enum();
    self.lmev(scan, scan, line, new_point);
  }

  fn sweep_mef(&mut self, scan: &Ref<HalfEdge>, vec: Vec3) {
    let scanb = scan.borrow();
    let mut curve = scanb.edge.upgrade().unwrap().borrow().curve.clone();
    curve.as_curve_mut().transform(vec);
    let next = scanb.next.upgrade().unwrap(); // Neccessary as next has changed because of lmev
    self.lmef(
      // New edge is oriented from..
      &scanb.previous.upgrade().unwrap(), // ..this half edge's vertex..
      &next.borrow().next.upgrade().unwrap(), // ..to this half edge's vertex
      curve,
      Box::new(Plane::default()), //XXX
    );
  }
}


impl Face {
  pub fn tesselate(&self) -> Mesh {
    let wire = self.outer_ring.borrow().get_wire();
    let polyline = geom2d::poly_from_wire(&wire);
    geom2d::tesselate_polygon(polyline)
  }
}


impl Ring {
  pub fn get_wire(&self) -> Vec<SketchElement> {
    self.half_edge.borrow().ring_iter().map(|he|
      he.borrow().edge.upgrade().unwrap().borrow().curve.clone()
    ).collect()
  }

  pub fn iter(&self) -> RingIterator  {
    RingIterator::new(self.half_edge.clone())
  }
}


impl HalfEdge {
  pub fn new_at(vertex: &Ref<Vertex>, at: &Ref<Self>) -> Ref<Self> {
    let mut atb = at.borrow_mut();
    if let Some(_) = atb.edge.upgrade() {
      let he = rc(Self {
        next: Rc::downgrade(at),
        previous: atb.previous.clone(),
        origin: vertex.clone(),
        ring: atb.ring.clone(),
        edge: Weak::new(),
      });
      atb.previous.upgrade().unwrap().borrow_mut().next = Rc::downgrade(&he);
      atb.previous = Rc::downgrade(&he);
      he
    } else {
      atb.origin = vertex.clone();
      at.clone()
    }
  }

  pub fn remove(&mut self) -> WeakRef<Self> {
    if !self.edge.upgrade().is_some() {
      Weak::new()
    } else if ptr::eq(self, &*self.next.upgrade().unwrap().borrow()) {
      let this = Rc::downgrade(&self.mate().borrow().mate());
      self.edge = Weak::new();
      this
    } else {
      self.previous.upgrade().unwrap().borrow_mut().next = self.next.clone();
      self.next.upgrade().unwrap().borrow_mut().previous = self.previous.clone();
      self.previous.clone()
    }
  }

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
    if Rc::ptr_eq(&self.current_edge, &self.start_edge) {
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
    Self {
      start_edge: (*he).clone(),
      current_edge: (*he).clone(),
    }
  }
}

impl Iterator for VertexEdgesIterator {
  type Item = Ref<HalfEdge>;

  //XXX last element is never returned
  fn next(&mut self) -> Option<Self::Item> {
    let current_edge = self.current_edge.clone();
    self.current_edge = current_edge.borrow().mate().borrow().next.upgrade().unwrap().clone();
    if Rc::ptr_eq(&self.current_edge, &self.start_edge) {
      None
    } else {
      Some(current_edge)
    }
  }
}
