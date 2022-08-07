use serde::{Serialize, Deserialize};

use std::ops::Deref;
use std::ops::DerefMut;

use crate::internal::*;
use crate::transform::*;
use crate::curve::*;
use crate::mesh::*;
use crate::geom2d;


pub type PolyLine = Vec<Point3>;


/// [Elements](TrimmedCurve) in a region are sorted in a closed loop and connected by their endpoints.
pub type Region = Vec<TrimmedCurve>;


/// Wires maintain consistent orientation across a series of curves, forming a closed loop.
///
/// Wires fulfill all properties of [Region]s, but their element's
/// bounds are ordered in the direction of the loop.

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    geom2d::is_clockwise(&self.cage())
  }

  pub fn reverse(&mut self) {
    self.0 = self.0.iter().rev().cloned().collect();
    for tcurve in self.iter_mut() {
      tcurve.flip();
    }
  }

  pub fn area(&self) -> f64 {
    geom2d::polygon_area(&self.cage())
  }

  pub fn cage(&self) -> PolyLine {
    let polyline = self.iter().map(|curve| curve.bounds.0 ).collect();
    // polyline.push(self[0].bounds.0);
    polyline
  }

  pub fn contains_point(&self, p: Point3) -> bool {
    let ray = TrimmedCurve::new(Line::new(p, p + Vec3::unit_x() * 9999999.0).into_enum());
    let num_hits: usize = parallel!(self.0).flat_map(|elem| {
      let intersections = ray.intersect(&elem);
      intersections.iter().map(|isect| match isect {
        CurveIntersectionType::Pierce(_)
        | CurveIntersectionType::Cross(_)
          => 1,
        _ => 0,
      }).collect::<Vec<usize>>()
    }).sum();
    num_hits % 2 != 0
  }

  pub fn encloses(&self, other: &Self) -> bool {
    parallel!(other).all(|elem| self.contains_point(elem.bounds.0) )
  }

  pub fn tesselate(&self) -> PolyLine {
    let polyline = parallel!(self)
    .flat_map(|curve| {
      let poly = curve.tesselate();
      let n = poly.len() - 1;
      poly.into_iter().take(n).collect::<PolyLine>()
    }).collect();
    polyline
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

// impl Index<usize> for Wire {
//   type Output = TrimmedCurve;

//   fn index(&self, index: usize) -> &Self::Output {
//     &self.0[index]
//   }
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


/// Profiles use coplanar [wires](Wire) to form enclosed regions.
///
/// Profiles must contain one or more wires, representing the outer and inner rings.
/// The outer ring runs counter-clockwise and inner rings run clockwise.

#[derive(Debug, Clone, Serialize, Deserialize)]
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

  pub fn area(&self) -> f64 {
    self.rings[0].area() - self.rings.iter().skip(1).fold(0.0, |acc, wire| acc + wire.area() )
  }

  pub fn contains_point(&self, p: Point3) -> bool {
    let p = Point3::from_vec(self.plane.unsample(p).to_vec().extend(0.0));
    self.rings[0].contains_point(p) && !self.rings.iter().skip(1).any(|wire| wire.contains_point(p) )
  }
}

impl Meshable for Profile {
  fn tesselate(&self) -> Mesh {
    let poly_rings: Vec<PolyLine> = parallel!(self.rings).map(|wire| {
      wire.tesselate()
    }).collect();
    let mut i = 0;
    let mut holes = Vec::with_capacity(poly_rings.len());
    for ring in &poly_rings {
      i += ring.len();
      holes.push(i);
    }
    holes.pop();
    let vertices: Vec<Point3> = poly_rings.into_iter().flatten().collect();
    let mut mesh = geom2d::tesselate_polygon(vertices, holes);
    mesh.transform(&self.plane.as_transform());
    mesh
  }
}
