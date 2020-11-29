// mod vector;
// pub use self::vector::Vec3;

use std::convert::TryInto;
use uuid::Uuid;
use cgmath::prelude::*;

pub type Vec2 = cgmath::Vector2<f64>;
pub type Vec3 = cgmath::Vector3<f64>;
pub type Vec4 = cgmath::Vector4<f64>;
pub type Point2 = cgmath::Point2<f64>;
pub type Point3 = cgmath::Point3<f64>;
pub type Matrix4 = cgmath::Matrix4<f64>;


// lut = [      [1],           // n=0
//             [1,1],          // n=1
//            [1,2,1],         // n=2
//           [1,3,3,1],        // n=3
//          [1,4,6,4,1],       // n=4
//         [1,5,10,10,5,1],    // n=5
//        [1,6,15,20,15,6,1]]  // n=6

// function binomial(n,k):
//   while(n >= lut.length):
//     s = lut.length
//     nextRow = new array(size=s+1)
//     nextRow[0] = 1
//     for(i=1, prev=s-1; i<s; i++):
//       nextRow[i] = lut[prev][i-1] + lut[prev][i]
//     nextRow[s] = 1
//     lut.add(nextRow)
//   return lut[n][k]

// function Bezier(n,t):
//   sum = 0
//   for(k=0; k<=n; k++):
//     sum += binomial(n,k) * (1-t)^(n-k) * t^(k)
//   return sum


pub enum Intersection {
  None,
  Touch(Point3), // Touching endpoints
  Hit(Vec<Point3>), // Actual intersections
  Extended(Vec<Point3>), // Intersections outside geometric bounds
  Contained, // Overlap, Infinite intersections
}


#[derive(Debug, Clone)]
pub enum SketchElement {
  Line(Line),
  Circle(Circle),
  BezierSpline(BezierSpline),
}

impl SketchElement {
  pub fn as_curve(&self) -> &dyn Curve {
    match self {
      Self::Line(line) => line,
      Self::Circle(circle) => circle,
      Self::BezierSpline(spline) => spline,
    }
  }

  pub fn as_curve_mut(&mut self) -> &mut dyn Curve {
    match self {
      Self::Line(line) => line,
      Self::Circle(circle) => circle,
      Self::BezierSpline(spline) => spline,
    }
  }
}


pub trait Curve {
  fn sample(&self, t: f64) -> Point3;
  fn default_tesselation(&self) -> Vec<Point3>;
  fn length(&self) -> f64;
  fn endpoints(&self) -> (Point3, Point3);
  fn intersect_line(&self, line: &Line) -> Intersection;
  fn intersect_circle(&self, circle: &Circle) -> Intersection;
  fn intersect_spline(&self, spline: &BezierSpline) -> Intersection;
  fn split(&self, elem: &SketchElement) -> Option<Vec<SketchElement>>;

  fn intersect(&self, other: &SketchElement) -> Intersection {
    match other {
      SketchElement::Line(other) => self.intersect_line(other),
      SketchElement::Circle(other) => self.intersect_circle(other),
      SketchElement::BezierSpline(other) => self.intersect_spline(other),
    }
  }

  fn tesselate(&self, steps: i32) -> Vec<Point3> {
    (0..steps + 1).map(|i| {
      self.sample(1.0 / steps as f64 * i as f64)
    }).collect()
  }
}

impl core::fmt::Debug for dyn Curve {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Foo")
  }
}


pub trait Surface {
  // fn sample(&self, u: f64, v: f64) -> Point3;
}

impl core::fmt::Debug for dyn Surface {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Foo")
  }
}


// pub fn perp_product(vec1: Vec3, vec2: Vec3) -> f64 {
//   vec1.x * vec2.z - vec1.z * vec2.x
// }

pub fn cross_2d(vec1: Vec3, vec2: Vec3) -> f64 {
  vec1.x * vec2.z - vec1.z * vec2.x
}


#[derive(Debug, Clone)]
pub struct Line {
  pub id: Uuid,
  pub points: (Point3, Point3)
}

impl Line {
  pub fn new(points: (Point3, Point3)) -> Self {
    Self {
      id: Uuid::new_v4(),
      points: points,
    }
  }

  pub fn midpoint(&self) -> Point3 {
    (self.points.0 + self.points.1.to_vec()) / 2.0
  }

  pub fn tangent(&self) -> Vec3 {
    (self.points.1 - self.points.0).normalize()
  }

//   pub fn intersect_line(&self, other: &Self) -> Intersection {
//     let u = self.points.1 - self.points.0;
//     let v = other.points.1 - other.points.0;
//     let w = self.points.0 - other.points.0;
//     let D = perp_product(u,v);

//     // test if  they are parallel (includes either being a point)
//     if D.abs() < core::f64::EPSILON {           // self and other are parallel
//         if perp_product(u,w) != 0.0 || perp_product(v,w) != 0.0  {
//             return 0;                    // they are NOT collinear
//         }
//         // they are collinear or degenerate
//         // check if they are degenerate  points
//         let du = u.dot(u);
//         let dv = v.dot(v);
//         if du==0.0 && dv==0.0 {            // both segments are points
//             if (self.points.0 !=  other.points.0)         // they are distinct  points
//                  return 0;
//             *I0 = self.points.0;                 // they are the same point
//             return 1;
//         }
//         if (du==0) {                     // self is a single point
//             if  (inSegment(self.points.0, other) == 0)  // but is not in other
//                  return 0;
//             *I0 = self.points.0;
//             return 1;
//         }
//         if (dv==0) {                     // other a single point
//             if  (inSegment(other.points.0, self) == 0)  // but is not in self
//                  return 0;
//             *I0 = other.points.0;
//             return 1;
//         }
//         // they are collinear segments - get  overlap (or not)
//         float t0, t1;                    // endpoints of self in eqn for other
//         Vector w2 = self.P1 - other.points.0;
//         if (v.x != 0) {
//                  t0 = w.x / v.x;
//                  t1 = w2.x / v.x;
//         }
//         else {
//                  t0 = w.y / v.y;
//                  t1 = w2.y / v.y;
//         }
//         if (t0 > t1) {                   // must have t0 smaller than t1
//                  float t=t0; t0=t1; t1=t;    // swap if not
//         }
//         if (t0 > 1 || t1 < 0) {
//             return 0;      // NO overlap
//         }
//         t0 = t0<0? 0 : t0;               // clip to min 0
//         t1 = t1>1? 1 : t1;               // clip to max 1
//         if (t0 == t1) {                  // intersect is a point
//             *I0 = other.points.0 +  t0 * v;
//             return 1;
//         }

//         // they overlap in a valid subsegment
//         *I0 = other.points.0 + t0 * v;
//         *I1 = other.points.0 + t1 * v;
//         return 2;
//     }

//     // the segments are skew and may intersect in a point
//     // get the intersect parameter for self
//     float     sI = perp(v,w) / D;
//     if (sI < 0 || sI > 1)                // no intersect with self
//         return 0;

//     // get the intersect parameter for other
//     float     tI = perp(u,w) / D;
//     if (tI < 0 || tI > 1)                // no intersect with other
//         return 0;

//     *I0 = self.points.0 + sI * u;                // compute self intersect point
//     return 1;
//   }
}

impl Curve for Line {
  fn sample(&self, t: f64) -> Point3 {
    let vec = self.points.1 - self.points.0;
    self.points.0 + vec * t
  }

  fn default_tesselation(&self) -> Vec<Point3> {
    self.tesselate(1)
  }

  fn length(&self) -> f64 {
    self.points.0.distance(self.points.1)
  }

  fn endpoints(&self) -> (Point3, Point3) {
    self.points
  }

  // https://stackoverflow.com/questions/563198/how-do-you-detect-where-two-line-segments-intersect
  fn intersect_line(&self, other: &Line) -> Intersection {
    let r = self.points.1 - self.points.0;
    let s = other.points.1 - other.points.0;

    let u_numerator = cross_2d(other.points.0 - self.points.0, r);
    let denominator = cross_2d(r, s);

    // Lines are coLlinear
    if u_numerator == 0.0 && denominator == 0.0 {

      // Lines touch at endpoints
      if self.points.0 == other.points.0 || self.points.0 == other.points.1 {
        return Intersection::Touch(self.points.0)
      } else if self.points.1 == other.points.0 || self.points.1 == other.points.1 {
        return Intersection::Touch(self.points.1)
      }

      // Lines overlap (All point differences in either direction have same sign)
      let overlap = ![
        (other.points.0.x - self.points.0.x < 0.0),
        (other.points.0.x - self.points.1.x < 0.0),
        (other.points.1.x - self.points.0.x < 0.0),
        (other.points.1.x - self.points.1.x < 0.0),
      ].windows(2).all(|w| w[0] == w[1]) || ![
        (other.points.0.y - self.points.0.y < 0.0),
        (other.points.0.y - self.points.1.y < 0.0),
        (other.points.1.y - self.points.0.y < 0.0),
        (other.points.1.y - self.points.1.y < 0.0),
      ].windows(2).all(|w| w[0] == w[1]);

      return if overlap {
        Intersection::Contained
      } else {
        Intersection::None
      }
    }

    if denominator == 0.0 {
      // Lines are paralell
      return Intersection::None;
    }

    // Lines cross
    let t = cross_2d(other.points.0 - self.points.0, s) / denominator;
    let u = u_numerator / denominator;
    let do_cross = (t >= 0.0) && (t <= 1.0) && (u >= 0.0) && (u <= 1.0);
    let intersection_point = self.points.0 + r * t;

    if do_cross {
      Intersection::Hit(vec![intersection_point])
    } else {
      Intersection::Extended(vec![intersection_point])
    }
  }

  fn intersect_circle(&self, _circle: &Circle) -> Intersection {
    Intersection::None
  }

  fn intersect_spline(&self, _spline: &BezierSpline) -> Intersection {
    Intersection::None
  }

  fn split(&self, cutter: &SketchElement) -> Option<Vec<SketchElement>> {
    match self.intersect(cutter) {
      Intersection::None | Intersection::Contained | Intersection::Touch(_) | Intersection::Extended(_) => None,
      Intersection::Hit(mut points) => { //XXX points are not sorted along line
        points.push(self.points.1);
        let mut segments = vec![SketchElement::Line(Self::new((self.points.0, points[0])))];
        let mut iter = points.iter().peekable();
        loop {
          match (iter.next(), iter.peek()) {
            (Some(p), Some(next_p)) => segments.push(SketchElement::Line(Self::new((*p, **next_p)))),
            _ => break,
          }
        }
        Some(segments)
      },
    }
  }
}


#[derive(Debug, Clone)]
pub struct Circle {
  pub id: Uuid,
  pub center: Point3,
  pub radius: f64,
}

impl Circle {
  pub fn new(center: Point3, radius: f64) -> Self {
    Self {
      id: Uuid::new_v4(),
      center,
      radius,
    }
  }
}

impl Curve for Circle {
  fn sample(&self, t: f64) -> Point3 {
    let t = t * std::f64::consts::PI * 2.0;
    Point3::new(
      self.center.x + t.sin() * self.radius,
      self.center.y,
      self.center.z + t.cos() * self.radius,
    )
  }

  fn default_tesselation(&self) -> Vec<Point3> {
    self.tesselate(120)
  }

  fn length(&self) -> f64 {
    std::f64::consts::PI * 2.0 * self.radius
  }

  fn endpoints(&self) -> (Point3, Point3) {
    let zero = self.sample(0.0);
    (zero, zero)
  }

  fn intersect_line(&self, _line: &Line) -> Intersection {
    Intersection::None
  }

  fn intersect_circle(&self, _circle: &Circle) -> Intersection {
    Intersection::None
  }

  fn intersect_spline(&self, _spline: &BezierSpline) -> Intersection {
    Intersection::None
  }

  fn split(&self, _elem: &SketchElement) -> Option<Vec<SketchElement>> {
    None
  }
}


const LUT_STEPS: usize = 10;

#[derive(Debug, Clone, Default)]
pub struct BezierSpline {
  pub id: Uuid,
  pub vertices: Vec<Point3>,
  pub lut: Vec<Point3>,
}

impl BezierSpline {
  pub fn new(vertices: Vec<Point3>) -> Self {
    let mut this = Self {
      id: Uuid::new_v4(),
      vertices: vertices,
      lut: vec![],
    };
    this.update();
    this
  }

  pub fn update(&mut self) {
    self.lut = self.tesselate((self.vertices.len() * LUT_STEPS).try_into().unwrap())
  }

  // de Casteljau's algorithm
  fn real_sample(&self, t: f64, vertices: &[Point3]) -> Point3 {
    if vertices.len() == 1 {
      vertices[0]
    } else {
      let len = vertices.len() - 1;
      let mut new_vertices: Vec<Point3> = vec![];
      for i in 0..len {
        new_vertices.push(vertices[i] * (1.0 - t) + (vertices[i + 1] * t).to_vec());
      }
      self.real_sample(t, &new_vertices)
    }
  }

  pub fn split(&self, t: f64) -> (Self, Self) {
    let mut left: Vec<Point3> = vec![];
    let mut right: Vec<Point3> = vec![];
    self.real_split(t, &self.vertices, &mut left, &mut right);
    (Self::new(left), Self::new(right))
  }

  fn real_split(&self, t: f64, vertices: &[Point3], left: &mut Vec<Point3>, right: &mut Vec<Point3>) -> Point3 {
    if vertices.len() == 1 {
      let p = vertices[0];
      left.push(p);
      right.push(p);
      p
    } else {
      let len = vertices.len() - 1;
      let mut new_vertices: Vec<Point3> = vec![];
      for i in 0..len {
        if i == 0 { left.push(vertices[i]) }
        if i == len - 1 { right.push(vertices[i + 1]) }
        new_vertices.push(vertices[i] * (1.0 - t) + (vertices[i + 1] * t).to_vec());
      }
      self.real_split(t, &new_vertices, left, right)
    }
  }

  pub fn tangent(&self, t: f64) -> Vec3 {
    self.derive().sample(t).to_vec().normalize()
  }

  // https://stackoverflow.com/questions/25453159/getting-consistent-normals-from-a-3d-cubic-bezier-path
  pub fn normal(&self, t: f64) -> Vec3 {
    let derivative = self.derive();
    let tan = derivative.sample(t).to_vec().normalize();
    let tan2 = (tan + derivative.derive().sample(t).to_vec()).normalize();
    let c = tan2.cross(tan).normalize();
    c.cross(tan).normalize()
  }

  pub fn derive(&self) -> Self {
    let mut derivative: Self = Default::default();
    let len = self.vertices.len() - 1;
    for i in 0..len {
      derivative.vertices[i] = (self.vertices[i + 1] - self.vertices[i].to_vec()) * len as f64;
    }
    derivative
  }

  pub fn closest(&self, point: Point3) -> Point3 {
    let mut mdist = 2_i32.pow(63).into();
    let mut closest = point;
    for &p in &self.lut {
      let d = point.distance(p);
      if point.distance(p) < mdist {
        mdist = d;
        closest = p;
      }
    }
    closest
  }
}

impl Curve for BezierSpline {
  fn sample(&self, t: f64) -> Point3 {
    self.real_sample(t, &self.vertices)
  }

  fn default_tesselation(&self) -> Vec<Point3> {
    self.lut.clone()
  }

  fn length(&self) -> f64 {
    1.0
  }

  fn endpoints(&self) -> (Point3, Point3) {
    (self.vertices[0], *self.vertices.last().unwrap())
  }

  fn intersect_line(&self, _line: &Line) -> Intersection {
    Intersection::None
  }

  fn intersect_circle(&self, _circle: &Circle) -> Intersection {
    Intersection::None
  }

  fn intersect_spline(&self, _spline: &BezierSpline) -> Intersection {
    Intersection::None
  }

  fn split(&self, _elem: &SketchElement) -> Option<Vec<SketchElement>> {
    None
  }
}


#[derive(Debug, Clone)]
pub struct Plane {
  pub origin: Point3,
  pub u: Vec3,
  pub v: Vec3,
}

impl Plane {
  pub fn new() -> Self {
    Self {
      origin: Point3::new(0.0, 0.0, 0.0),
      u: Vec3::new(1.0, 0.0, 0.0),
      v: Vec3::new(0.0, 0.0, 1.0),
    }
  }

  pub fn sample(&self, u: f64, v: f64) -> Point3 {
    self.origin + self.u * u + self.v * v
  }

  pub fn normal(&self) -> Vec3 {
    self.u.cross(self.v)
  }

  pub fn intersect_line(&self, line: (Point3, Point3)) -> Intersection {
    let n = self.normal();
    let u = line.1 - line.0;
    let n_dot_u = n.dot(u);
    if n_dot_u <= core::f64::EPSILON {
      // Line is parallel to this plane
      if self.contains_point(line.0) {
        // Line lies completely on this plane
        Intersection::Contained
      } else {
        Intersection::None
      }
    } else {
      let s = n.dot(self.origin - line.0) / n_dot_u;
      let p = line.0 + u * s;
      if s >= 0.0 && s <= 1.0 {
        // Line segment intersects this plane
        Intersection::Hit(vec![p])
      } else {
        // The ray along the given line intersects this plane
        Intersection::Extended(vec![p])
      }
    }
  }

  pub fn contains_point(&self, p: Point3) -> bool {
    self.normal().dot(p - self.origin) <= core::f64::EPSILON
  }
}

impl Surface for Plane {}


// #[derive(Clone, Debug)]
// pub struct Transform {
//   pub translation: Vec3
// }

// impl Transform {
//   pub fn new() -> Self {
//     Self {
//       translation: Vec3::new(0.0, 0.0, 0.0)
//     }
//   }

//   pub fn to_mat4(&self) -> Matrix4 {
//     Matrix4::from_translation(self.translation)
//   }
// }


// #[derive(Clone, Debug)]
// pub struct PolyLine {
//   pub vertices: Vec<Point3>
// }

// impl PolyLine {
//   pub fn new() -> Self {
//     Self {
//       vertices: vec![]
//     }
//   }

//   pub fn add_vertex(&mut self, vertex: Point3) {
//     self.vertices.push(vertex);
//   }
// }


pub type PolyLine = Vec<Point3>;


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
