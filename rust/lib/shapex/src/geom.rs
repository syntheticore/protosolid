// mod vector;

// pub use self::vector::Vec3;
pub use cgmath::prelude::*;

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

#[derive(Debug)]
pub struct Plane {
  pub origin: Point3,
  pub u: Vec3,
  pub v: Vec3
}

impl Plane {
  pub fn new() -> Self {
    Self {
      origin: Point3::new(0.0, 0.0, 0.0),
      u: Vec3::new(1.0, 0.0, 0.0),
      v: Vec3::new(0.0, 0.0, 1.0)
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


#[derive(Debug)]
pub struct TreeNode<T> {
  pub item: Option<T>,
  pub transform: Transform,
  pub children: Vec<TreeNode<T>>
}

impl<T> TreeNode<T> {
  pub fn new(item: Option<T>) -> Self {
    Self {
      item: item,
      transform: Transform::new(),
      children: Default::default()
    }
  }

  pub fn add_child(&mut self, child: T) {
    self.children.push(TreeNode::new(Some(child)));
  }
}


#[derive(Debug)]
pub struct Transform {
  pub translation: Vec3
}

impl Transform {
  pub fn new() -> Self {
    Self {
      translation: Vec3::new(0.0, 0.0, 0.0)
    }
  }

  pub fn to_mat4(&self) -> Matrix4 {
    Matrix4::from_translation(self.translation)
  }
}


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
