use crate::base::*;


#[derive(Debug, Clone)]
pub struct Axis {
  origin: Point3,
  direction: Vec3,
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
