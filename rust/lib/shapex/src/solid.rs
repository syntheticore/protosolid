use std::ptr;
use std::rc::{Rc, Weak};

use uuid::Uuid;

use crate::base::*;
use crate::curve::*;
use crate::surface::*;
use crate::mesh::*;


#[derive(Debug)]
pub enum BooleanType {
  Add,
  Subtract,
  Intersection,
  Difference,
}


#[derive(Debug, Default)]
pub struct Solid {
  pub id: Uuid,
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
  pub id: Uuid,
  pub outer_ring: Ref<Ring>,
  pub rings: Vec<Ref<Ring>>,
  pub surface: SurfaceType,
}


#[derive(Debug)]
pub struct Ring {
  pub half_edge: Ref<HalfEdge>,
  pub face: WeakRef<Face>,
}


#[derive(Debug)]
pub struct Edge {
  pub id: Uuid,
  pub left_half: Ref<HalfEdge>,
  pub right_half: Ref<HalfEdge>,
  pub curve: CurveType,
  // pub curve_direction: bool, // true means forward according to left_half
}


#[derive(Debug, Clone)]
pub struct HalfEdge {
  pub id: Uuid,
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
  pub fn new_lamina(region: Region, top_surface: SurfaceType) -> Self {
    println!("Creating Lamina:");
    let mut bottom = top_surface.clone();
    bottom.as_surface_mut().flip();
    let mut this = Self::default();
    // Create shell from bottom face with empty ring
    let first_elem = region[0].clone();
    this.mvfs(first_elem.bounds.0, bottom);
    let shell = &mut this.shells[0];
    // Complete ring of bottom face
    let mut he = shell.vertices.last().unwrap().borrow().half_edge.upgrade().unwrap();
    for elem in region.iter().take(region.len() - 1) {
      let points = elem.bounds;
      println!("\n-> lmev from {:?} to {:?}", points.1, he.borrow().origin.borrow().point);
      let (new_edge, _) = shell.lmev(&he, &he, elem.cache.clone(), points.1); //XXX cache -> base
      he = new_edge.borrow().left_half.clone();
    }
    // Create top face
    // let he1 = shell.edges[0].borrow().right_half.clone();
    // let he2 = shell.edges.last().unwrap().borrow().left_half.clone();
    let he1 = shell.vertices[0].borrow().half_edge.upgrade().unwrap().clone();
    let he2 = shell.vertices.last().unwrap().borrow().half_edge.upgrade().unwrap().clone();
    shell.lmef(&he1, &he2, region.last().unwrap().clone().cache, top_surface); //XXX cache -> base
    this
  }

  // https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/model/euler.html
  pub fn euler_characteristics(&self) -> i32 {
    let genus = self.shells.first().unwrap().genus(); //XXX should we fold over all shells?
    self.shells.iter().fold(0, |acc, shell| acc + shell.euler_characteristics() ) - 2 * (self.shells.len() as i32 - genus)
  }

  pub fn mvfs(&mut self, p: Point3, surface: SurfaceType) -> (Ref<Vertex>, Ref<Face>, &mut Shell) {
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
    println!("Made initial Half Edge");
    let he = rc(HalfEdge {
      id: Uuid::new_v4(),
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
      id: Uuid::new_v4(),
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
    println!("Made initial face {:?}", face.borrow().id);
    face.borrow().print();

    shell.vertices.push(vertex.clone());
    shell.faces.push(face.clone());
    self.shells.push(shell);
    (vertex, face, self.shells.last_mut().unwrap())
  }

  pub fn boolean(&mut self, _tool: Self, _op: BooleanType) -> Vec<Solid> {
    vec![Self::default()]
  }

  pub fn boolean_all(tool: Self, others: &mut Vec<Solid>, _op: BooleanType) {
    others.push(tool);
  }

  pub fn tesselate(&self) -> Mesh {
    let mut mesh = Mesh::default();
    for shell in &self.shells {
      let is_inner = !ptr::eq(shell, &self.shells[0]);
      for face in &shell.faces {
        let mut face_mesh = face.borrow().get_surface().tesselate();
        if is_inner { face_mesh.invert_normals() }
        mesh.append(face_mesh);
      }
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

  pub fn lmev(&mut self, he1: &Ref<HalfEdge>, he2: &Ref<HalfEdge>, curve: CurveType, p: Point3) -> (Ref<Edge>, Ref<Vertex>) {
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
    let origin = he2.borrow().origin.clone();
    let right_half = if he1.borrow().edge.upgrade().is_some() {
      HalfEdge::new_at(&origin, he1)
    } else {
      // Use empty loop half edge as right half
      // he1.borrow_mut().origin = origin.clone();
      he1.clone()
    };
    let left_half = HalfEdge::new_at(&vertex, he2);
    let edge = rc(Edge {
      id: Uuid::new_v4(),
      left_half: left_half,
      right_half: right_half,
      // curve_direction: curve.as_curve().endpoints().0.almost(p),
      curve,
    });
    {
      let e = edge.borrow();
      e.left_half.borrow_mut().edge = Rc::downgrade(&edge);
      e.right_half.borrow_mut().edge = Rc::downgrade(&edge);
    }
    {
      let he2b = he2.borrow_mut();
      vertex.borrow_mut().half_edge = he2b.previous.clone();
      he2b.origin.borrow_mut().half_edge = Rc::downgrade(he2);
    }
    self.vertices.push(vertex.clone());
    self.edges.push(edge.clone());
    println!("<- completed lmev");
    self.print();
    (edge, vertex)
  }

  pub fn lmef(&mut self, he1: &Ref<HalfEdge>, he2: &Ref<HalfEdge>, curve: CurveType, surface: SurfaceType) -> (Ref<Edge>, Ref<Face>) {
    let ring = rc(Ring {
      half_edge: he1.clone(), // using he1 as dummy, just to be able to create the ring...
      face: Weak::new(),
    });
    let mut he = he1.clone();
    while !Rc::ptr_eq(&he, he2) {
      he = {
        let mut heb = he.borrow_mut();
        heb.ring = Rc::downgrade(&ring);
        heb.next.upgrade().unwrap().clone()
      }
    }
    let he1_origin = he1.borrow().origin.clone();
    let he2_origin = he2.borrow().origin.clone();
    let nhe1 = HalfEdge::new_at(&he2_origin, he1);
    let nhe2 = if he1.borrow().edge.upgrade().is_some() {
      HalfEdge::new_at(&he1_origin, he2)
    } else {
      // Use empty loop half edge as right half
      he1.clone()
    };
    let edge = rc(Edge {
      id: Uuid::new_v4(),
      left_half: nhe2.clone(),
      right_half: nhe1.clone(),
      // curve_direction: curve.as_curve().endpoints().0.almost(he1_origin.borrow().point),
      curve,
    });
    {
      let e = edge.borrow();
      e.left_half.borrow_mut().edge = Rc::downgrade(&edge);
      e.right_half.borrow_mut().edge = Rc::downgrade(&edge);
    }
    ring.borrow_mut().half_edge = nhe1.clone(); // ... now assigning real value
    let face = rc(Face {
      id: Uuid::new_v4(),
      outer_ring: ring.clone(),
      rings: vec![ring.clone()],
      surface,
    });
    println!("  Made face {:?}", face.borrow().id);
    ring.borrow_mut().face = Rc::downgrade(&face);
    {
      let previous = nhe1.borrow().previous.upgrade().unwrap();
      previous.borrow_mut().next = Rc::downgrade(&nhe2);
      let previous = nhe2.borrow().previous.upgrade().unwrap();
      previous.borrow_mut().next = Rc::downgrade(&nhe1);
      let mut nhe1b = nhe1.borrow_mut();
      let mut nhe2b = nhe2.borrow_mut();
      let temp = nhe1b.previous.clone();
      nhe1b.previous = nhe2b.previous.clone();
      nhe2b.previous = temp;
      // nhe1b.ring = Rc::downgrade(&ring);
    }
    he2.borrow().ring.upgrade().unwrap().borrow_mut().half_edge = nhe2;
    self.edges.push(edge.clone());
    self.faces.push(face.clone());
    (edge, face)
  }

  pub fn sweep(&mut self, face: &Ref<Face>, vec: Vec3) {
    for ring in &face.borrow().rings {
      let first = ring.borrow().half_edge.clone();
      let mut scan = first.borrow().next.upgrade().unwrap().clone();
      self.sweep_mev(&scan, vec);
      while !Rc::ptr_eq(&scan, &first) {
        scan = {
          let scan_next = scan.borrow().next.upgrade().unwrap();
          self.sweep_mev(&scan_next, vec);
          self.sweep_mef(&scan, vec);
          let scanb = scan.borrow();
          scanb.next.upgrade().unwrap().borrow().mate().borrow().next.upgrade().unwrap().clone()
        }
      }
      self.sweep_mef(&scan, vec);
    }
    face.borrow_mut().surface.as_surface_mut().translate(vec);
  }

  fn sweep_mev(&mut self, scan: &Ref<HalfEdge>, vec: Vec3) {
    let point = scan.borrow().origin.borrow().point;
    let new_point = point + vec;
    let line = Line::new(new_point, point).into_enum();
    self.lmev(scan, scan, line, new_point);
  }

  fn sweep_mef(&mut self, scan: &Ref<HalfEdge>, vec: Vec3) {
    let scan_previous = scan.borrow().previous.upgrade().unwrap();
    let next = scan.borrow().next.upgrade().unwrap();
    let next_next = next.borrow().next.upgrade().unwrap();
    let mut curve = scan.borrow().edge.upgrade().unwrap().borrow().curve.clone();
    curve.as_curve_mut().translate(vec);
    let surface = Self::sweep_surface(&scan.borrow().get_curve(), vec);
    // let p1 = scan_previous.borrow().origin.borrow().point;
    // let p2 = next_next.borrow().origin.borrow().point;
    self.lmef(
      // New edge is oriented from..
      &scan_previous, // ..this half edge's vertex..
      &next_next, // ..to this half edge's vertex
      curve,
      surface,
    );
  }

  fn sweep_surface(curve: &TrimmedCurve, vec: Vec3) -> SurfaceType {
    match &curve.base {
      CurveType::Line(_) => {
        Plane::from_triangle(
          curve.bounds.0,
          curve.bounds.0 + vec,
          curve.bounds.1,
        ).into_enum()
      },
      CurveType::Circle(circle) => {
        CylindricalSurface {
          origin: circle.center,
          radius: circle.radius,
          direction: vec,
          bounds: (0.0, 1.0),
        }.into_enum()
      },
      _ => { Plane::new().into_enum() }
    }
  }

  pub fn print(&self) {
    println!("\n  Debug Info: Shell");
    println!("  -------------------");
    println!("  Faces {:?}, Edges {:?}, Vertices {:?}", self.faces.len(), self.edges.len(), self.vertices.len());
    for face in &self.faces {
      face.borrow().print();
    }
    for edge in &self.edges {
      edge.borrow().print();
    }
  }
}


impl Face {
  // pub fn tesselate(&self) -> Mesh {
  //   self.get_surface().tesselate()
  // }

  pub fn get_surface(&self) -> TrimmedSurface {
    let wire = self.outer_ring.borrow().get_wire();
    TrimmedSurface::new(self.surface.clone(), wire)
  }

  pub fn print(&self) {
    println!("\n  Face {:?}:", self.id);
    for he in self.outer_ring.borrow().half_edge.borrow().ring_iter() {
      he.borrow().print();
    }
  }
}


impl Ring {
  pub fn get_wire(&self) -> Wire {
    // self.iter().filter_map(|he| {
    //   let he = he.borrow();
    //   if let Some(edge) = he.edge.upgrade() {
    //     // let mut curve = TrimmedCurve::new(edge.borrow().curve.clone());
    //     // curve.bounds = (he.origin.borrow().point, he.mate().borrow().origin.borrow().point);
    //     let curve = edge.borrow().get_curve();
    //     Some(curve)
    //   } else { None }
    // }).collect()
    self.iter().map(|he|
      he.borrow().get_curve()
    ).collect()
  }

  pub fn iter(&self) -> RingIterator  {
    RingIterator::new(self.half_edge.clone())
  }
}


impl Edge {
  // pub fn get_curve(&self) -> TrimmedCurve {
  //   let mut curve = TrimmedCurve::new(self.curve.clone());
  //   let left_origin = self.left_half.borrow().origin.borrow().point;
  //   let right_origin = self.right_half.borrow().origin.borrow().point;
  //   if self.curve_direction {
  //     curve.bounds = (left_origin, right_origin);
  //   } else {
  //     curve.bounds = (right_origin, left_origin);
  //   }
  //   curve
  // }

  pub fn print(&self) {
    println!("\n  Edge {:?}", self.id);
    println!("    left_half {:?}", self.left_half.borrow().id);
    println!("    right_half {:?}", self.right_half.borrow().id);
  }
}


impl HalfEdge {
  pub fn new_at(vertex: &Ref<Vertex>, at: &Ref<Self>) -> Ref<Self> {
    let he = rc(Self {
      id: Uuid::new_v4(),
      next: Rc::downgrade(at),
      previous: at.borrow().previous.clone(),
      origin: vertex.clone(),
      ring: at.borrow().ring.clone(),
      edge: Weak::new(),
    });
    let previous = at.borrow().previous.upgrade().unwrap();
    previous.borrow_mut().next = Rc::downgrade(&he);
    at.borrow_mut().previous = Rc::downgrade(&he);
    println!("  Made half edge");
    he
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
    if let Some(edge) = self.edge.upgrade() {
      let edge = edge.borrow();
      if ptr::eq(self, &*edge.left_half.borrow()) {
        edge.right_half.clone()
      } else {
        edge.left_half.clone()
      }
    } else {
      self.origin.borrow().half_edge.upgrade().unwrap()
    }
  }

  pub fn get_curve(&self) -> TrimmedCurve {
    let edge = self.edge.upgrade().unwrap();
    let mut curve = TrimmedCurve::new(edge.borrow().curve.clone());
    curve.bounds = (self.origin.borrow().point, self.mate().borrow().origin.borrow().point);
    curve
  }

  pub fn end_vertex(&self) -> Ref<Vertex> {
    self.mate().borrow().origin.clone()
  }

  pub fn get_face(&self) -> Ref<Face> {
    self.ring.upgrade().unwrap().borrow()
    .face.upgrade().unwrap()
  }

  pub fn ring_iter(&self) -> RingIterator  {
    RingIterator::new(self.mate().borrow().mate())
  }

  pub fn print(&self) {
    println!("\n    Half Edge {:?}:", self.id);
    println!("      origin   {:?}", self.origin.borrow().point);
    println!("      face     {:?}", self.get_face().borrow().id);
    if let Some(edge) = self.edge.upgrade() {
      println!("      edge     {:?}", edge.borrow().id);
    } else {
      println!("      edge     none");
    }
    // println!("    previous {:?}", self.previous.upgrade().is_some());
    // println!("    next     {:?}", self.next.upgrade().is_some());
  }
}


impl Vertex {
  pub fn edges_iter(&self) -> VertexEdgesIterator  {
    VertexEdgesIterator::new(self)
  }
}


pub struct RingIterator {
  start_edge: Option<Ref<HalfEdge>>,
  current_edge: Ref<HalfEdge>,
}

impl RingIterator {
  fn new(start_edge: Ref<HalfEdge>) -> Self {
    Self {
      start_edge: None,
      current_edge: start_edge,
    }
  }
}

impl Iterator for RingIterator {
  type Item = Ref<HalfEdge>;

  fn next(&mut self) -> Option<Self::Item> {
    let current_edge = self.current_edge.clone();
    self.current_edge = current_edge.borrow().next.upgrade().unwrap().clone();
    if self.start_edge.is_some() && Rc::ptr_eq(&current_edge, self.start_edge.as_ref().unwrap()) {
      None
    } else {
      if !self.start_edge.is_some() {
        self.start_edge = Some(current_edge.clone());
      }
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


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
