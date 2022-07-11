use std::ptr;
use std::rc::{Rc, Weak};
use std::collections::HashSet;
use std::iter;

use uuid::Uuid;

use crate::base::*;
use crate::curve::*;
use crate::surface::*;

mod volume;
mod boolean;
mod tesselation;
mod serde;

pub mod features;
pub use boolean::Boolean;
pub use boolean::BooleanType;
pub use volume::Volume;

// use crate::log;


#[derive(Debug, Default, Clone)]
pub struct Compound {
  pub solids: Vec<Solid>,
}


#[derive(Debug, Default, Clone)]
pub struct Solid {
  pub id: Uuid,
  pub shells: Vec<Shell>, // Shell 0 is outer shell
}


#[derive(Debug, Clone)]
pub struct Shell {
  // id: Uuid,
  pub faces: Vec<Ref<Face>>,
  pub edges: Vec<Ref<Edge>>,
  pub vertices: Vec<Ref<Vertex>>,
}


#[derive(Debug, Clone)]
pub struct Face {
  pub id: Uuid,
  pub outer_ring: Ref<Ring>,
  pub rings: Vec<Ref<Ring>>,
  pub surface: SurfaceType,
  pub flip_normal: bool, // If cross product of U and V derivatives points into the body
}


#[derive(Debug, Clone)]
pub struct Ring { //XXX Eliminate
  pub half_edge: Ref<HalfEdge>,
  pub face: WeakRef<Face>,
}


#[derive(Debug, Clone)]
pub struct Edge {
  pub id: Uuid,
  pub left_half: Ref<HalfEdge>,
  pub right_half: Ref<HalfEdge>,
  pub curve: CurveType,
  // pub curve_direction: bool, // true means forward according to left_half
}


#[derive(Debug, Clone)]
pub struct HalfEdge {
  pub id: Uuid, //TEMP
  pub next: WeakRef<Self>,
  pub previous: WeakRef<Self>,
  pub origin: Ref<Vertex>,
  pub edge: WeakRef<Edge>,
  pub ring: WeakRef<Ring>,
}


#[derive(Debug, Clone)]
pub struct Vertex {
  // pub id: Uuid,
  pub point: Point3,
  pub half_edge: WeakRef<HalfEdge>, // half_edge emanating from this vertex
}


impl Compound {
  pub fn get_face(&self, id: Uuid) -> Option<&Ref<Face>> {
    for solid in &self.solids {
      let face = solid.get_face(id);
      if face.is_some() { return face }
    }
    None
  }

  pub fn find_face_from_bounds(&self, ids: &HashSet<Uuid>) -> Option<&Ref<Face>> {
    self.faces_iter().find(|face| {
      let hashset = face.borrow().get_edge_set();
      hashset.intersection(&ids).count() > 1
    })
  }

  pub fn faces_iter(&self) -> impl Iterator<Item = &Ref<Face>> {
    self.solids.iter().flat_map(|solid|
      solid.shells.iter().flat_map(|shell|
        shell.faces.iter()
      )
    )
  }
}


impl Solid {
  pub fn new() -> Self {
    Self {
      id: Uuid::new_v4(),
      shells: vec![],
    }
  }

  pub fn new_lamina(wire: Wire, top_surface: SurfaceType) -> Self {
    println!("Creating Lamina:");
    let mut bottom = top_surface.clone();
    bottom.as_surface_mut().flip();
    let mut this = Self::new();
    // Create shell from bottom face with empty ring
    let first_elem = wire[0].clone();
    this.mvfs(first_elem.bounds.0, bottom);
    let shell = &mut this.shells[0];
    // Complete ring of bottom face
    let mut he = shell.vertices.last().unwrap().borrow().half_edge.upgrade().unwrap();
    for elem in wire.iter().take(wire.len() - 1) {
      let points = elem.bounds;
      println!("\n-> lmev from {:?} to {:?}", points.1, he.borrow().origin.borrow().point);
      let (new_edge, _) = shell.lmev(&he, &he, elem.base.clone(), points.1);
      he = new_edge.borrow().left_half.clone();
    }
    // Create top face
    // let he1 = shell.edges[0].borrow().right_half.clone();
    // let he2 = shell.edges.last().unwrap().borrow().left_half.clone();
    let he1 = shell.vertices[0].borrow().half_edge.upgrade().unwrap().clone();
    let he2 = shell.vertices.last().unwrap().borrow().half_edge.upgrade().unwrap().clone();
    shell.lmef(&he1, &he2, wire.last().unwrap().base.clone(), top_surface);
    this
  }

  // https://pages.mtu.edu/~shene/COURSES/cs3621/NOTES/model/euler.html
  pub fn euler_characteristics(&self) -> i32 {
    let genus = self.shells.first().unwrap().genus(); //XXX should we fold over all shells?
    self.shells.iter().fold(0, |acc, shell| acc + shell.euler_characteristics() ) - 2 * (self.shells.len() as i32 - genus)
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

  pub fn validate(&self) -> Result<(), String> {
    for shell in &self.shells { shell.validate()? }
    Ok(())
  }

  pub fn get_face(&self, id: Uuid) -> Option<&Ref<Face>> {
    for shell in &self.shells {
      let face = shell.get_face(id);
      if face.is_some() { return face }
    }
    None
  }

  pub fn into_compound(self) -> Compound {
    Compound { solids: vec![self] }
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

  //XXX
  pub fn validate(&self) -> Result<(), String> { Ok(()) }

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
      flip_normal: false,
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
    // Create new stable id for cloned curve
    let curve_id = curve.get_id();
    let fields = curve_id.as_fields();
    curve.set_id(Uuid::from_fields(fields.0, fields.1 + 1, fields.2, fields.3).unwrap());
    // Sweep actual surface
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
          origin: circle.plane.origin,
          radius: circle.radius,
          direction: vec,
          bounds: (0.0, 1.0),
        }.into_enum()
      },
      CurveType::Arc(arc) => {
        CylindricalSurface {
          origin: arc.plane.origin,
          radius: arc.radius,
          direction: vec,
          bounds: arc.bounds,
        }.into_enum()
      },
      _ => todo!()
    }
  }

  pub fn get_face(&self, id: Uuid) -> Option<&Ref<Face>> {
    for face in &self.faces {
      if face.borrow().id == id { return Some(face) }
    }
    None
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
  pub fn get_surface(&self) -> TrimmedSurface {
    let wire = self.outer_ring.borrow().get_wire();
    TrimmedSurface::new(self.surface.clone(), wire)
  }

  pub fn get_edge_set(&self) -> HashSet<Uuid> {
    self.outer_ring.borrow().iter().map(|he|
      he.borrow().edge.upgrade().unwrap().borrow().curve.get_id()
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
  pub fn get_wire(&self) -> Wire {
    self.iter().map(|he|
      he.borrow().get_curve()
    ).collect()
  }

  pub fn iter(&self) -> RingIterator  {
    RingIterator::new(self.half_edge.clone())
  }
}


impl Edge {
  pub fn is_inner(&self) -> bool {
    let left_face = self.left_half.borrow().ring.upgrade().unwrap().borrow().face.upgrade().unwrap();
    let right_face = self.right_half.borrow().ring.upgrade().unwrap().borrow().face.upgrade().unwrap();
    Rc::ptr_eq(&left_face, &right_face) && false
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
    let previous = at.borrow().previous.upgrade().unwrap();
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
    let curve = &edge.borrow().curve;
    let bounds = (self.origin.borrow().point, self.end_vertex().borrow().point);
    TrimmedCurve::from_bounds(curve.clone(), bounds, curve.clone())
  }

  pub fn end_vertex(&self) -> Ref<Vertex> {
    self.mate().borrow().origin.clone()
  }

  pub fn get_face(&self) -> Ref<Face> {
    self.ring.upgrade().unwrap().borrow().face.upgrade().unwrap()
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
  start_edge: Option<Ref<HalfEdge>>,
  current_edge: Ref<HalfEdge>,
}

impl VertexEdgesIterator {
  fn new(start_vertex: &Vertex) -> Self {
    let he = &start_vertex.half_edge.upgrade().unwrap();
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
      self.current_edge = current_edge.borrow().mate().borrow().next.upgrade().unwrap().clone();
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
