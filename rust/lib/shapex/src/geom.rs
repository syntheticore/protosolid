// mod vector;

// pub use self::vector::Vec3;
use std::convert::TryInto;
use cgmath::prelude::*;

pub type Vec2 = cgmath::Vector2<f64>;
pub type Vec3 = cgmath::Vector3<f64>;
pub type Vec4 = cgmath::Vector4<f64>;
pub type Point2 = cgmath::Point2<f64>;
pub type Point3 = cgmath::Point3<f64>;
pub type Matrix4 = cgmath::Matrix4<f64>;


// pub trait Differentiable {
//   fn sample(&self, t: f64) -> Vec3;
//
//   fn tesselate(&self) -> Mesh {
//     let steps = 10;
//     Mesh {
//       vertices: (0..steps).map(|i| {
//         self.sample(1.0 / steps as f64 * i as f64)
//       }).collect()
//     }
//   }
// }

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


pub trait Differentiable {
  fn sample(&self, t: f64) -> Point3;
  fn default_tesselation(&self) -> Vec<Point3>;

  fn tesselate(&self, steps: i32) -> Vec<Point3> {
    (0..steps + 1).map(|i| {
      self.sample(1.0 / steps as f64 * i as f64)
    }).collect()
  }

  fn tesselate_lines(&self, steps: i32) -> Vec<Line> {
    let points = self.tesselate(steps);
    let mut lines = vec![];
    for i in 0..points.len() - 1 {
      lines.push(Line{ points: (points[i], points[i + 1])});
    }
    lines
  }
}


#[derive(Debug)]
pub struct Line {
  pub points: (Point3, Point3)
}

impl Line {
  pub fn midpoint(&self) -> Point3 {
    (self.points.0 + self.points.1.to_vec()) / 2.0
  }
}

impl Differentiable for Line {
  fn sample(&self, t: f64) -> Point3 {
    let vec = self.points.1 - self.points.0;
    self.points.0 + vec * t
  }

  fn default_tesselation(&self) -> Vec<Point3> {
    self.tesselate(1)
  }
}


const LUT_STEPS: i32 = 100;

#[derive(Debug, Default)]
pub struct BezierSpline {
  pub vertices: Vec<Point3>,
  pub lut: Vec<Point3>,
}

impl BezierSpline {
  pub fn new(vertices: Vec<Point3>) -> Self {
    let mut this = Self {
      vertices: vertices,
      lut: vec![],
    };
    this.update();
    this
  }

  pub fn update(&mut self) {
    self.lut = self.tesselate(LUT_STEPS);
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

impl Differentiable for BezierSpline {
  fn sample(&self, t: f64) -> Point3 {
    self.real_sample(t, &self.vertices)
  }

  fn default_tesselation(&self) -> Vec<Point3> {
    self.tesselate((self.vertices.len() * 10).try_into().unwrap())
  }
}


#[derive(Debug)]
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

  pub fn eval(&self, u: f64, v: f64) -> Point3 {
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
        Intersection::One(p)
      } else {
        // The ray along the given line intersects this plane
        Intersection::Extended(p)
      }
    }
  }

  pub fn contains_point(&self, p: Point3) -> bool {
    self.normal().dot(p - self.origin) <= core::f64::EPSILON
  }
}


pub enum Intersection {
  None,
  One(Point3),
  Extended(Point3),
  Contained,
}


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


#[derive(Clone, Debug)]
pub struct PolyLine {
  pub vertices: Vec<Point3>
}

impl PolyLine {
  pub fn new() -> Self {
    Self {
      vertices: vec![]
    }
  }

  pub fn add_vertex(&mut self, vertex: Point3) {
    self.vertices.push(vertex);
  }
}

// impl Differentiable for PolyLine {
//   fn sample(&self, t: f64) -> Vec3 {
//     let index = ((self.vertices.len() - 1) as f64 * t).round() as usize;
//     self.vertices[index]
//   }
// }


#[derive(Default, Debug)]
pub struct Mesh {
  pub vertices: Vec<Vec3>
}


#[derive(Debug)]
pub struct Solid {}
