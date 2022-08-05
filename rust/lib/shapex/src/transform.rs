use crate::internal::*;
use crate::geom3d;
use crate::geom3d::Axis;


/// All types that can have affine transformations applied in form of a [Matrix4].

pub trait Transformable {
  fn transform(&mut self, transform: &Matrix4);

  fn translate(&mut self, vec: Vec3) {
    self.transform(&Matrix4::from_translation(vec));
  }

  fn scale(&mut self, scalar: f64) {
    self.transform(&Matrix4::from_scale(scalar));
  }

  fn rotate_about_axis(&mut self, axis: &Axis, angle: Deg<f64>) {
    self.transform(&geom3d::rotation_about_axis(axis, angle));
  }
}


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
