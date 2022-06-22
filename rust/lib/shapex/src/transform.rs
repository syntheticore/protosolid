use crate::base::*;
use crate::geom3d::Axis;


pub trait Transformable {
  fn transform(&mut self, transform: &Matrix4);

  fn translate(&mut self, vec: Vec3) {
    self.transform(&Matrix4::from_translation(vec));
  }

  fn scale(&mut self, scalar: f64) {
    self.transform(&Matrix4::from_scale(scalar));
  }

  fn mirror(&mut self, axis: &Axis) {
    self.transform(&Matrix4::from_nonuniform_scale(
      axis.direction.x,
      axis.direction.y,
      axis.direction.z,
    ));
  }
}


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
