pub type Vec2 = cgmath::Vector2<f64>;
pub type Vec3 = cgmath::Vector3<f64>;
pub type Vec4 = cgmath::Vector4<f64>;
pub type Point2 = cgmath::Point2<f64>;
pub type Point3 = cgmath::Point3<f64>;
pub type Matrix4 = cgmath::Matrix4<f64>;

pub const EPSILON: f64 = core::f64::EPSILON * 10.0;

pub trait Almost {
  // const EPS: f64 = core::f64::EPSILON * 100000.0;
  fn almost(&self, other: Self) -> bool;
}


impl Almost for Point3 {
  fn almost(&self, other: Self) -> bool {
    (self.x - other.x).abs() < EPSILON &&
    (self.y - other.y).abs() < EPSILON &&
    (self.z - other.z).abs() < EPSILON
  }
}

impl Almost for f64 {
  fn almost(&self, other: Self) -> bool {
    (self - other).abs() < EPSILON
  }
}

#[derive(Debug, PartialEq)]
pub enum Intersection {
  None,
  Touch(Point3), // Touching endpoints
  Pierce(Vec<Point3>), // Endpoint touching curve/surface
  Cross(Vec<Point3>), // Actual intersections
  Extended(Vec<Point3>), // Intersections outside geometric bounds
  Contained, // Overlap, Infinite intersections
}
