pub type Vec2 = cgmath::Vector2<f64>;
pub type Vec3 = cgmath::Vector3<f64>;
pub type Vec4 = cgmath::Vector4<f64>;
pub type Point2 = cgmath::Point2<f64>;
pub type Point3 = cgmath::Point3<f64>;
pub type Matrix4 = cgmath::Matrix4<f64>;


pub trait Almost {
  const EPS: f64 = core::f64::EPSILON * 100000.0;
  fn almost(&self, other: Self) -> bool;
}


impl Almost for Point3 {
  fn almost(&self, other: Self) -> bool {
    (self.x - other.x).abs() < Self::EPS &&
    (self.y - other.y).abs() < Self::EPS &&
    (self.z - other.z).abs() < Self::EPS
  }
}


#[derive(Debug, PartialEq)]
pub enum Intersection {
  None,
  Touch(Point3), // Touching endpoints
  Hit(Vec<Point3>), // Actual intersections
  Extended(Vec<Point3>), // Intersections outside geometric bounds
  Contained, // Overlap, Infinite intersections
}


pub fn cross_2d(vec1: Vec3, vec2: Vec3) -> f64 {
  vec1.x * vec2.z - vec1.z * vec2.x
}
