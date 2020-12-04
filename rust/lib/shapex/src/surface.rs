use cgmath::prelude::*;

use crate::base::*;
use crate::curve::*;
use crate::curve::intersection::CurveIntersection;


pub trait Surface {
  fn sample(&self, u: f64, v: f64) -> Point3;
  fn normal_at(&self, u: f64, v: f64) -> Vec3;
}

impl core::fmt::Debug for dyn Surface {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Foo")
  }
}


type Wire = Vec<TrimmedSketchElement>;

#[derive(Debug)]
pub struct TrimmedSurface {
  base: Box<dyn Surface>,
  bounds: Vec<Wire>,
}


#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
  pub origin: Point3,
  pub u: Vec3,
  pub v: Vec3,
}

impl Plane {
  pub fn default() -> Self {
    Self {
      origin: Point3::new(0.0, 0.0, 0.0),
      u: Vec3::new(1.0, 0.0, 0.0),
      v: Vec3::new(0.0, 1.0, 0.0),
    }
  }

  pub fn normal(&self) -> Vec3 {
    self.u.cross(self.v)
  }

  pub fn flip(&mut self) {
    self.v = -self.v;
  }

  pub fn intersect_line(&self, line: (Point3, Point3)) -> CurveIntersection {
    let n = self.normal();
    let u = line.1 - line.0;
    let n_dot_u = n.dot(u);
    if n_dot_u <= EPSILON {
      // Line is parallel to this plane
      if self.contains_point(line.0) {
        // Line lies completely on this plane
        CurveIntersection::Contained
      } else {
        CurveIntersection::None
      }
    } else {
      let s = n.dot(self.origin - line.0) / n_dot_u;
      let p = line.0 + u * s;
      if s >= 0.0 && s <= 1.0 {
        // Line segment intersects this plane
        if s == 0.0 || s == 1.0 {
          CurveIntersection::Pierce(vec![p])
        } else {
          CurveIntersection::Cross(vec![p])
        }
      } else {
        // The ray along the given line intersects this plane
        CurveIntersection::Extended(vec![p])
      }
    }
  }

  pub fn contains_point(&self, p: Point3) -> bool {
    self.normal().dot(p - self.origin) <= EPSILON
  }
}

impl Surface for Plane {
  fn sample(&self, u: f64, v: f64) -> Point3 {
    self.origin + self.u * u + self.v * v
  }

  fn normal_at(&self, _u: f64, _v: f64) -> Vec3 {
    self.normal()
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn plane_normal() {
    let plane = Plane::default();
    assert_eq!(plane.normal(), Vec3::new(0.0, 0.0, 1.0));
  }
}
