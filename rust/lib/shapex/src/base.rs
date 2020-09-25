pub type Vec2 = cgmath::Vector2<f64>;
pub type Vec3 = cgmath::Vector3<f64>;
pub type Vec4 = cgmath::Vector4<f64>;
pub type Point2 = cgmath::Point2<f64>;
pub type Point3 = cgmath::Point3<f64>;
pub type Matrix4 = cgmath::Matrix4<f64>;


#[derive(Debug, PartialEq)]
pub enum Intersection {
  None,
  Touch(Point3), // Touching endpoints
  Hit(Vec<Point3>), // Actual intersections
  Extended(Vec<Point3>), // Intersections outside geometric bounds
  Contained, // Overlap, Infinite intersections
}


pub fn cross_2d(vec1: Vec3, vec2: Vec3) -> f64 {
  vec1.x * vec2.y - vec1.y * vec2.x
}
