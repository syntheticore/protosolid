use std::ptr;
use std::rc::{Rc, Weak};
use std::collections::HashSet;

use uuid::Uuid;
use serde::{Serialize, Deserialize};

use crate::internal::*;
use crate::curve::*;
use crate::surface::*;
use crate::wire::*;

mod volume;
mod boolean;
mod tesselation;
mod serialize;
mod repair;

/// High level modeling operations.

pub mod features;

pub use boolean::Boolean;
pub use boolean::BooleanType;
pub use volume::Volume;
pub use repair::Repairable;
pub use serialize::DeepClone;


/// Collection of solids.
///
/// An abstraction over the number of solids returned by operations
/// that might split or merge bodies depending on geometric conditions.

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Compound {
  pub solids: Vec<Solid>,
}


/// Solid body with a finite, non-zero volume.
///
/// May contain inner cavities, represented by additional [Shell]s.

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Solid {
  pub id: Uuid,
  pub shells: Vec<Shell>, // Shell 0 is outer shell
}


/// Topological entity.
/// A closed volume in space, defining the interior or exterior boundary of a [Solid].

#[derive(Debug, Clone)]
pub struct Shell {
  pub faces: Vec<Ref<Face>>,
  pub edges: Vec<Ref<Edge>>,
  pub vertices: Vec<Ref<Vertex>>,
}


/// Topological entity. Part of a [Shell]. Connected to other faces at its [Edge]s.

#[derive(Debug, Clone)]
pub struct Face {
  pub id: Uuid,
  pub outer_ring: Ref<Ring>,
  pub rings: Vec<Ref<Ring>>,
  pub surface: SurfaceType,
  pub flip_normal: bool, // If cross product of U and V derivatives points into the body
}


/// Closed loop of [half edges](HalfEdge), forming the inner or outer boundary of a [Face].

#[derive(Debug, Clone)]
pub struct Ring {
  pub half_edge: Ref<HalfEdge>,
  pub face: WeakRef<Face>,
}


/// Topological entity.
/// Consists of two [half edges](HalfEdge).
/// Bounded by [Vertices](Vertex). Joins exactly two [Face]s.

#[derive(Debug, Clone)]
pub struct Edge {
  pub id: Uuid,
  pub left_half: Ref<HalfEdge>,
  pub right_half: Ref<HalfEdge>,
  pub curve: CurveType,
  // pub curve_direction: bool, // true means forward according to left_half
}


/// Topological entity. Half edges form [Ring]s arounds the [Face]s they belong to.

#[derive(Debug, Clone)]
pub struct HalfEdge {
  pub id: Uuid, //TEMP
  next: WeakRef<Self>,
  previous: WeakRef<Self>,
  pub origin: Ref<Vertex>,
  edge: WeakRef<Edge>,
  pub ring: WeakRef<Ring>,
}


/// Topological entity. Joins [Edge]s at a point.

#[derive(Debug, Clone)]
pub struct Vertex {
  // pub id: Uuid,
  pub point: Point3,
  half_edge: WeakRef<HalfEdge>, // half_edge emanating from this vertex
}


impl Compound {
  pub fn find_face(&self, id: Uuid) -> Option<&Ref<Face>> {
    for solid in &self.solids {
      let face = solid.find_face(id);
      if face.is_some() { return face }
    }
    None
  }

  pub fn find_face_from_bounds(&self, ids: &HashSet<Uuid>) -> Option<&Ref<Face>> {
    self.faces_iter().find(|face| {
      let hashset = face.borrow().edge_ids();
      hashset.intersection(&ids).count() >= 2
    })
  }

  pub fn faces_iter(&self) -> impl Iterator<Item = &Ref<Face>> {
    self.solids.iter().flat_map(|solid| solid.faces_iter() )
  }
}


impl Solid {
  pub fn new() -> Self {
    Self {
      id: Uuid::new_v4(),
      shells: vec![],
    }
  }

  pub fn lamina(wire: Wire, top_surface: SurfaceType) -> Self {
    println!("Creating Lamina:");
    let mut bottom = top_surface.clone();
    bottom.as_surface_mut().flip();
    let mut this = Self::new();
    // Create shell from bottom face with empty ring
    this.mvfs(wire[0].bounds.0, bottom);
    let shell = &mut this.shells[0];
    // Complete ring of bottom face
    let mut he = shell.vertices.last().unwrap().borrow().half_edge();
    for elem in wire.iter().take(wire.len() - 1) {
      let points = elem.bounds;
      println!("\n-> lmev from {:?} to {:?}", points.1, he.borrow().origin.borrow().point);
      let (new_edge, _) = shell.lmev(&he, &he, elem.base.clone(), points.1);
      he = new_edge.borrow().left_half.clone();
    }
    // Create top face
    // let he1 = shell.edges[0].borrow().right_half.clone();
    // let he2 = shell.edges.last().unwrap().borrow().left_half.clone();
    let he1 = shell.vertices[0].borrow().half_edge();
    let he2 = shell.vertices.last().unwrap().borrow().half_edge();
    shell.lmef(&he1, &he2, wire.last().unwrap().base.clone(), top_surface);
    this
  }

  // https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/model/euler.html
  pub fn euler_characteristics(&self) -> i32 {
    let genus = self.shells.first().unwrap().genus(); //XXX should we fold over all shells?
    self.shells.iter().fold(0, |acc, shell| acc + shell.euler_characteristics() ) - 2 * (self.shells.len() as i32 - genus)
  }

  pub fn validate(&self) -> Result<(), String> {
    for shell in &self.shells { shell.validate()? }
    Ok(())
  }

  pub fn find_face(&self, id: Uuid) -> Option<&Ref<Face>> {
    for shell in &self.shells {
      let face = shell.find_face(id);
      if face.is_some() { return face }
    }
    None
  }

  pub fn faces_iter(&self) -> impl Iterator<Item = &Ref<Face>> {
    self.shells.iter().flat_map(|shell| shell.faces.iter() )
  }

  pub fn mvfs(&mut self, p: Point3, surface: SurfaceType) -> (Ref<Vertex>, Ref<Face>, &mut Shell) {
    let mut shell = Shell {
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
      flip_normal: false,
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

  pub fn into_compound(self) -> Compound {
    Compound { solids: vec![self] }
  }
}


impl Shell {
  pub fn euler_characteristics(&self) -> i32 {
    let num_faces = self.faces.len() as i32;
    let num_loops = self.faces.iter().fold(0, |acc, face| acc + face.borrow().rings.len()) as i32;
    num_faces - self.edges.len() as i32 + self.vertices.len() as i32 + (num_faces - num_loops)
  }

  pub fn connectivity(&self) -> i32 {
    3 - self.euler_characteristics()
  }

  // Topological type (Number of handles on a sphere)
  pub fn genus(&self) -> i32 {
    let h = self.connectivity();
    if h >= 3 {
      (h - 1) / 2
    } else {
      0
    }
  }

  pub fn validate(&self) -> Result<(), String> {
    // Closed shells have odd connectivity
    if self.connectivity() % 2 == 0 {
      return Err("Open shell".into())
    }
    Ok(())
  }

  pub fn find_face(&self, id: Uuid) -> Option<&Ref<Face>> {
    for face in &self.faces {
      if face.borrow().id == id { return Some(face) }
    }
    None
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
        heb.next()
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
      flip_normal: false,
    });
    println!("  Made face {:?}", face.borrow().id);
    ring.borrow_mut().face = Rc::downgrade(&face);
    {
      let previous = nhe1.borrow().previous();
      previous.borrow_mut().next = Rc::downgrade(&nhe2);
      let previous = nhe2.borrow().previous();
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

  pub fn sweep<C,S>(&mut self, face: &Ref<Face>, transform: &Matrix4, make_curve: C, make_surface: S)
  where
    C: Fn(Point3) -> CurveType,
    S: Fn(&TrimmedCurve) -> SurfaceType,
  {
    for ring in &face.borrow().rings {
      let first = ring.borrow().half_edge.clone();
      let mut scan = first.borrow().next();
      self.sweep_mev(&scan, transform, &make_curve);
      while !Rc::ptr_eq(&scan, &first) {
        scan = {
          let scan_next = scan.borrow().next();
          self.sweep_mev(&scan_next, transform, &make_curve);
          self.sweep_mef(&scan, transform, &make_surface);
          let scanb = scan.borrow();
          scanb.next().borrow().mate().borrow().next()
        }
      }
      self.sweep_mef(&scan, transform, &make_surface);
    }
    face.borrow_mut().surface.as_surface_mut().transform(transform);
  }

  fn sweep_mev<C: Fn(Point3) -> CurveType>(&mut self, scan: &Ref<HalfEdge>, transform: &Matrix4, make_curve: C) {
    let point = scan.borrow().origin.borrow().point;
    let curve = make_curve(point);
    self.lmev(scan, scan, curve, transform.transform_point(point));
  }

  fn sweep_mef<S: Fn(&TrimmedCurve) -> SurfaceType>(&mut self, scan: &Ref<HalfEdge>, transform: &Matrix4, make_surface: S) {
    let scan_previous = scan.borrow().previous();
    let next = scan.borrow().next();
    let next_next = next.borrow().next();
    let mut curve = scan.borrow().edge().borrow().curve.clone();
    curve.as_curve_mut().transform(transform);
    // Create new stable id for cloned curve
    let curve_id = curve.id();
    let fields = curve_id.as_fields();
    curve.set_id(Uuid::from_fields(fields.0, fields.1 + 1, fields.2, fields.3));
    // Sweep actual surface
    let surface = make_surface(&scan.borrow().make_curve());
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
  pub fn make_surface(&self) -> TrimmedSurface {
    let wire = self.outer_ring.borrow().make_wire();
    TrimmedSurface::new(self.surface.clone(), wire)
  }

  pub fn edge_ids(&self) -> HashSet<Uuid> {
    self.outer_ring.borrow().iter().map(|he|
      he.borrow().edge().borrow().curve.id()
    ).collect()
  }

  pub fn print(&self) {
    println!("\n  Face {:?}:", self.id);
    for he in self.outer_ring.borrow().half_edge.borrow().ring_iter() {
      he.borrow().print();
    }
  }
}


impl Ring {
  pub fn make_wire(&self) -> Wire {
    Wire::new(self.iter().map(|he|
      he.borrow().make_curve()
    ).collect())
  }

  pub fn iter(&self) -> RingIterator  {
    RingIterator::new(self.half_edge.clone())
  }

  pub fn vertex_iter(&self) -> impl Iterator<Item = Ref<Vertex>>  {
    self.iter().map(|he| he.borrow().origin.clone() )
  }
}


impl Edge {
  pub fn left_face(&self) -> Ref<Face> {
    self.left_half.borrow().face()
  }

  pub fn right_face(&self) -> Ref<Face> {
    self.right_half.borrow().face()
  }

  pub fn top_face(&self) -> Ref<Face> {
    self.left_half.borrow().next().borrow().mate().borrow().face()
  }

  pub fn bottom_face(&self) -> Ref<Face> {
    self.left_half.borrow().previous().borrow().mate().borrow().face()
  }

  pub fn is_inner(&self) -> bool {
    Rc::ptr_eq(&self.left_face(), &self.right_face()) && false
  }

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
    let previous = at.borrow().previous();
    previous.borrow_mut().next = Rc::downgrade(&he);
    at.borrow_mut().previous = Rc::downgrade(&he);
    println!("  Made half edge");
    he
  }

  pub fn remove(&mut self) -> WeakRef<Self> {
    if !self.edge.upgrade().is_some() {
      Weak::new()
    // } else if ptr::eq(self, &*self.next.upgrade().unwrap().borrow()) {
    //   let this = Rc::downgrade(&self.mate().borrow().mate());
    //   // self.edge = Weak::new();
    //   this
    } else {
      self.previous().borrow_mut().next = self.next.clone();
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
      self.origin.borrow().half_edge()
    }
  }

  pub fn end_vertex(&self) -> Ref<Vertex> {
    self.mate().borrow().origin.clone()
  }

  pub fn edge(&self) -> Ref<Edge> {
    self.edge.upgrade().unwrap()
  }

  pub fn face(&self) -> Ref<Face> {
    self.ring.upgrade().unwrap().borrow().face.upgrade().unwrap()
  }

  pub fn next(&self) -> Ref<Self> {
    self.next.upgrade().unwrap()
  }

  pub fn previous(&self) -> Ref<Self> {
    self.previous.upgrade().unwrap()
  }

  pub fn make_curve(&self) -> TrimmedCurve {
    let edge = self.edge();
    let curve = &edge.borrow().curve;
    let bounds = (self.origin.borrow().point, self.end_vertex().borrow().point);
    TrimmedCurve::from_bounds(curve.clone(), bounds, curve.clone())
  }

  pub fn ring_iter(&self) -> RingIterator  {
    RingIterator::new(self.as_rc())
  }

  fn as_rc(&self) -> Ref<Self> {
    self.mate().borrow().mate()
  }

  pub fn print(&self) {
    println!("\n    Half Edge {:?}:", self.id);
    println!("      origin   {:?}", self.origin.borrow().point);
    println!("      face     {:?}", self.face().borrow().id);
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
  pub fn half_edge(&self) -> Ref<HalfEdge> {
    self.half_edge.upgrade().unwrap()
  }

  pub fn edges_iter(&self) -> VertexEdgesIterator  {
    VertexEdgesIterator::new(self)
  }
}


/// Iterator that follows [half edges](HalfEdge) until the start element is encountered again.

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
    self.current_edge = current_edge.borrow().next();
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


/// Iterator that returns all [half edges](HalfEdge) emanating from its start vertex.

pub struct VertexEdgesIterator {
  start_edge: Option<Ref<HalfEdge>>,
  current_edge: Ref<HalfEdge>,
}

impl VertexEdgesIterator {
  fn new(start_vertex: &Vertex) -> Self {
    let he = &start_vertex.half_edge();
    Self {
      start_edge: Some((*he).clone()),
      current_edge: (*he).clone(),
    }
  }
}

impl Iterator for VertexEdgesIterator {
  type Item = Ref<HalfEdge>;

  fn next(&mut self) -> Option<Self::Item> {
    if let Some(start_edge) = &self.start_edge {
      let current_edge = self.current_edge.clone();
      self.current_edge = current_edge.borrow().mate().borrow().next();
      if Rc::ptr_eq(&self.current_edge, start_edge) {
        self.start_edge = None;
      }
      Some(current_edge)
    } else {
      None
    }
  }
}


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
