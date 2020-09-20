use crate::base::*;
use uuid::Uuid;
use std::convert::TryInto;


pub type PolyLine = Vec<Point3>;


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

pub fn cross_2d(vec1: Vec3, vec2: Vec3) -> f64 {
  vec1.x * vec2.y - vec1.y * vec2.x
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
