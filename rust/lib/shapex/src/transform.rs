pub use cgmath::prelude::Transform as OtherTransform;

use crate::base::*;
use crate::geom3d::Axis;


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

  fn scale(&mut self, scalar: f64) {
    self.transform(&Transform::from_matrix(Matrix4::from_scale(scalar)));
  }

  fn mirror(&mut self, axis: &Axis) {
    self.transform(&Transform::from_matrix(Matrix4::from_nonuniform_scale(
      axis.direction.x,
      axis.direction.y,
      axis.direction.z,
    )));
  }
}


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
