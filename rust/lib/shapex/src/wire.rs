use std::ops::Deref;
use std::ops::DerefMut;

use crate::internal::*;
use crate::transform::*;
use crate::curve::*;
use crate::mesh::*;
use crate::geom2d;


pub type PolyLine = Vec<Point3>;


/// Elements in a region are sorted in a closed loop and connected by their endpoints
pub type Region = Vec<TrimmedCurve>;


/// Wires fulfill all properties of Regions, but their element's
/// bounds are ordered in the direction of the loop

#[derive(Debug, Clone)]
pub struct Wire(Vec<TrimmedCurve>);

impl Wire {
  pub fn new(mut region: Region) -> Self {
    if region.len() >= 2 {
      // Find starting point from element order
      let bounds = region[0].bounds;
      let next_bounds = region[1].bounds;
      let mut point = if bounds.0.almost(next_bounds.0) || bounds.0.almost(next_bounds.1) {
        bounds.1
      } else {
        bounds.0
      };
      // Flip curves to flow consistently along element order
      for tcurve in &mut region {
        if tcurve.bounds.1.almost(point) {
          point = tcurve.bounds.0;
          tcurve.flip();
        } else {
          point = tcurve.bounds.1;
        }
      }
    }
    Self(region)
  }

  pub fn is_clockwise(&self) -> bool {
    let poly = geom2d::poly_from_wirebounds(self);
    geom2d::is_clockwise(&poly)
  }

  pub fn reverse(&mut self) {
    self.0 = self.0.iter().rev().cloned().collect();
    for tcurve in self.iter_mut() {
      tcurve.flip();
    }
  }
}

impl<'a> IntoIterator for &'a Wire {
  type Item = &'a TrimmedCurve;
  type IntoIter = std::slice::Iter<'a, TrimmedCurve>;
  fn into_iter(self) -> Self::IntoIter { self.0.iter() }
}

// impl AsRef<[TrimmedCurve]> for Wire {
//   fn as_ref(&self) -> &[TrimmedCurve] { &self.0 }
// }

impl Deref for Wire {
  type Target = Vec<TrimmedCurve>;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for Wire {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

// impl Index<usize> for Wire {
//   type Output = TrimmedCurve;

//   fn index(&self, index: usize) -> &Self::Output {
//     &self.0[index]
//   }
// }


/// Profiles contain one or more wires, representing the outer and inner rings
/// The outer ring runs counter-clockwise and inner rings run clockwise

#[derive(Debug, Clone)]
pub struct Profile {
  pub plane: Plane,
  pub rings: Vec<Wire>,
}

impl Profile {
  pub fn new(plane: Plane, rings: Vec<Wire>) -> Self {
    Self {
      plane,
      rings,
    }
  }
}

impl Meshable for Profile {
  fn tesselate(&self) -> Mesh {
    let mut mesh = geom2d::tesselate_profile(&self.rings);
    mesh.transform(&self.plane.as_transform());
    mesh
  }
}

