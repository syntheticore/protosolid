use crate::base::*;
pub use cgmath::prelude::Transform as OtherTransform;


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
}


pub trait Transformable {
  fn transform(&mut self, transform: &Transform);

  fn translate(&mut self, vec: Vec3) {
    self.transform(&Transform::from_matrix(Matrix4::from_translation(vec)))
  }
}


// #[derive(Debug)]
// pub struct Transform {
//   matrix: Matrix4,
// }

// impl Transform {
//   pub fn mirror(&mut self, axis: &Axis) {
//      self.matrix = Matrix4::from_nonuniform_scale(axis.direction.x, axis.direction.y, axis.direction.z);
//   }
// }


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
