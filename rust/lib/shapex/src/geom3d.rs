pub use cgmath::prelude::Transform as OtherTransform;

use crate::base::*;
use crate::Plane;
use crate::PolyLine;


#[derive(Debug, Clone)]
pub struct Axis {
  origin: Point3,
  direction: Vec3,
}


#[derive(Debug)]
pub struct Transform {
  matrix: Matrix4,
}

impl Transform {
  pub fn from_matrix(m: Matrix4) -> Self {
    Self {
      matrix: m,
    }
  }

  pub fn apply(&self, p: Point3) -> Point3 {
    self.matrix.transform_point(p)
  }

  pub fn apply_vec(&self, v: Vec3) -> Vec3 {
    self.matrix.transform_vector(v)
  }

  pub fn invert(&self) -> Self {
    Self::from_matrix(self.matrix.invert().unwrap())
  }
}


pub trait Transformable {
  fn transform(&mut self, transform: &Transform);

  fn translate(&mut self, vec: Vec3) {
    self.transform(&Transform::from_matrix(Matrix4::from_translation(vec)));
  }

  fn mirror(&mut self, axis: &Axis) {
    self.transform(&Transform::from_matrix(Matrix4::from_nonuniform_scale(
      axis.direction.x,
      axis.direction.y,
      axis.direction.z,
    )));
  }
}


// Find suitable points in wire to build a matching plane
pub fn detect_plane(poly: &PolyLine) -> Result<Plane, String> {
  //XXX use points with greatest distance as start points
  let v1 = (poly[2] - poly[1]).normalize(); // Skip duplicate first point
  if let Some(p3) = poly.iter().skip(3).min_by(|p1, p2| {
    let v2 = (*p1 - poly[1]).normalize();
    let v3 = (*p2 - poly[1]).normalize();
    v1.dot(v2).abs().partial_cmp(&v1.dot(v3).abs()).unwrap()
  }) {
    if v1.almost((p3 - poly[1]).normalize()) {
      Err(format!("P3 {:?} p2 {:?} p1 {:?}", p3, poly[2], poly[1]))
    } else {
      Ok(Plane::from_triangle(
        poly[1],
        poly[2],
        *p3,
      ))
    }
  } else { Err("Could not detect plane from wire".to_string()) }
}


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
