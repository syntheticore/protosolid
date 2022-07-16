use crate::base::*;
use crate::geom3d;


pub trait Transformable {
  fn transform(&mut self, transform: &Matrix4);

  fn translate(&mut self, vec: Vec3) {
    self.transform(&Matrix4::from_translation(vec));
  }

  fn scale(&mut self, scalar: f64) {
    self.transform(&Matrix4::from_scale(scalar));
  }

  fn rotate_about_axis(&mut self, axis: geom3d::Axis, angle: Deg<f64>) {
    self.transform(&geom3d::rotation_about_axis(axis, angle));
  }
}


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
