use serde::{Serialize, Deserialize};

use std::ops::Deref;
use std::ops::DerefMut;

use crate::internal::*;
use crate::transform::*;
use crate::curve::*;
use crate::mesh::*;
use crate::geom2d;
use crate::SurfaceArea;


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
    #[cfg(debug_assertions)]
    {
      if region.len() == 0 { panic!("Wires may not be empty") }
      let first_point = region.first().unwrap().endpoints().0;
      let last_point = region.last().unwrap().endpoints().1;
      if !first_point.almost(last_point) { panic!("Wires must be closed") }
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

  pub fn cage(&self) -> PolyLine {
    let polyline = self.iter().map(|curve| curve.bounds.0 ).collect();
    // polyline.push(self[0].bounds.0);
    polyline
  }

  pub fn contains_point(&self, p: Point3) -> bool {
    use CurveIntersectionType::*;
    let ray = TrimmedCurve::new(Line::new(p, p + Vec3::unit_x() * 999999.0).into_enum());
    let per_elem: Vec<Vec<CurveIntersectionType>> = self.0.iter().map(|elem| ray.intersect(&elem) ).collect();
    let num_hits: usize = per_elem.iter().enumerate().flat_map(|(i, intersections)| {
      intersections.iter().map(|isect| match isect {
        Cross(_) => 1,
        Pierce(_) => {
          let j = (i + 1) % self.0.len();
          let next_intersections = &per_elem[j];
          if i != j && next_intersections.iter().any(|next_isect| matches!(next_isect, Pierce(_)) ) {
            0
          } else {
            1
          }
        },
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

impl SurfaceArea for Wire {
  fn area(&self) -> f64 {
    geom2d::polygon_area(&self.cage())
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


/// Profiles use coplanar [wires](Wire) to form closed regions.
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

  pub fn contains_point(&self, p: Point3) -> bool {
    let p = Point3::from_vec(self.plane.unsample(p).to_vec().extend(0.0));
    self.rings[0].contains_point(p) && !self.rings.iter().skip(1).any(|wire| wire.contains_point(p) )
  }
}

impl SurfaceArea for Profile {
  fn area(&self) -> f64 {
    self.rings[0].area() - self.rings.iter().skip(1).fold(0.0, |acc, wire| acc + wire.area() )
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


#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_data;
  use crate::test_data::make_generic;
  use crate::test_data::make_wire;

  #[test]
  fn point_in_rect() {
    let rect = make_wire(make_generic(test_data::rectangle()));
    assert!(rect.contains_point(Point3::origin()));
    assert!(!rect.contains_point(Point3::new(10.0, 0.0, 0.0)));
  }

  #[test]
  fn point_in_triangle() {
    let tri = make_wire(make_generic(test_data::triangle()));
    assert!(tri.contains_point(Point3::new(-7.0, 60.0, 0.0)));
  }

  #[test]
  fn point_in_circle() {
    let circle = make_wire(make_generic(vec![Circle::new(Point3::origin(), 20.0)]));
    assert!(circle.contains_point(Point3::origin()));
  }

  #[test]
  fn point_in_diamond() {
    let diamond = test_data::diamond();
    let wire = make_wire(make_generic(diamond));
    assert!(wire.contains_point(Point3::origin()));
  }

  #[test]
  fn circle_in_rect() {
    let circle = make_wire(make_generic(vec![Circle::new(Point3::origin(), 0.5)]));
    let rect = make_wire(make_generic(test_data::rectangle()));
    assert!(rect.encloses(&circle));
    assert!(!circle.encloses(&rect));
  }

  #[test]
  fn circle_in_circle() {
    let circle = make_wire(make_generic(vec![
      Circle::new(Point3::new(-27.0, 3.0, 0.0), 68.97340462273907)
    ]));
    let inner_circle = Circle::new(Point3::new(-1.0, 27.654544570311774, 0.0), 15.53598031475424);
    let inner_circle = make_wire(make_generic(vec![inner_circle]));
    assert!(circle.encloses(&inner_circle));
    assert!(!inner_circle.encloses(&circle));
  }

  #[test]
  fn rect_in_rect() {
    let rect = make_wire(make_generic(test_data::rectangle()));
    let mut inner_rect = test_data::rectangle();
    for line in &mut inner_rect {
      line.scale(0.5);
    }
    let inner_rect = make_wire(make_generic(inner_rect));
    assert!(rect.encloses(&inner_rect));
    assert!(!inner_rect.encloses(&rect));
  }
}
