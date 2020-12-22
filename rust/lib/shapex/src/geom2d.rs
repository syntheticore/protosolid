use earcutr;

use crate::base::*;
use crate::curve::*;
use crate::mesh::Mesh;


pub fn cross_2d(vec1: Vec3, vec2: Vec3) -> f64 {
  vec1.x * vec2.y - vec1.y * vec2.x
}

pub fn tesselate_polygon(vertices: PolyLine, normal: Vec3) -> Mesh {
  let flat_vertices: Vec<f64> = vertices.iter().flat_map(|v| vec![v.x, v.y] ).collect();
  let faces: Vec<usize> = earcutr::earcut(&flat_vertices, &vec![], 2);
  let mut normals = Vec::with_capacity(vertices.len());
  for _ in &vertices {
    normals.push(normal);
  }
  Mesh {
    vertices,
    faces,
    normals,
  }
}

// Check if two line segments turn clockwise
// Returns values > 0 when clockwise, < 0 when anti-clockwise and 0 when segments are colinear
pub fn clockwise(p1: Point3, p2: Point3, p3: Point3) -> f64 {
  let v1 = p2 - p1;
  let v2 = p3 - p1;
  cross_2d(v1, v2)
}

pub fn is_clockwise(closed_loop: &PolyLine) -> bool {
  println!("signed_polygon_area {}", signed_polygon_area(&closed_loop) );
  signed_polygon_area(&closed_loop) > 0.0
}

pub fn polygon_area(closed_loop: &PolyLine) -> f64 {
  signed_polygon_area(&closed_loop).abs() / 2.0
}

pub fn signed_polygon_area(closed_loop: &PolyLine) -> f64 {
  let mut signed_area = 0.0;
  let len = closed_loop.len();
  for i in 0..len {
    let j = (i + 1) % len;
    let p = closed_loop[i];
    let next_p = closed_loop[j];
    signed_area += (next_p.x - p.x) * (next_p.y + p.y);
  }
  signed_area
}

// pub fn poly_from_wire(wire: &Wire) -> PolyLine {
//   let mut polyline: PolyLine = wire.iter().map(|elem| elem.bounds.0 ).collect();
//   polyline.push(wire.first().unwrap().bounds.0); //XXX first elem may not start at zero
//   // let z = rand::random::<f64>() / -6.0;
//   // for p in &mut polyline {
//   //   p.z = z;
//   // }
//   polyline
// }

pub fn poly_from_wire(wire: &Wire) -> PolyLine {
  // let mut wire = wire.clone();
  // straighten_bounds(&mut wire);
  let mut polyline: PolyLine = wire.iter()
  .flat_map(|curve| {
    let poly = curve.cache.as_curve().tesselate(); //XXX cache -> base
    // assert!((curve.bounds.0.almost(poly[0]) && curve.bounds.1.almost(poly[1])) ||
      // (curve.bounds.1.almost(poly[0]) && curve.bounds.0.almost(poly[1])), "{:?} vs {:?}", curve.bounds, poly);
    let poly = if curve.bounds.0.almost(poly[0]) {
      poly
    } else {
      poly.into_iter().rev().collect()
    };
    let n = poly.len() - 1;
    poly.into_iter().take(n).collect::<PolyLine>()
  }).collect();
  polyline.push(wire.first().unwrap().bounds.0);
  // let z = rand::random::<f64>() / -6.0;
  // for p in &mut polyline {
  //   p.z = z;
  // }
  polyline
}

pub fn straighten_bounds(wire: &mut Wire) {
  let bounds = wire[0].bounds;
  let next_bounds = wire[1].bounds;
  let mut point = if bounds.0.almost(next_bounds.0) || bounds.0.almost(next_bounds.1) {
    bounds.1
  } else {
    bounds.0
  };
  for elem in wire {
    if elem.bounds.1.almost(point) {
      point = elem.bounds.0;
      elem.bounds = (elem.bounds.1, elem.bounds.0);
    } else {
      point = elem.bounds.1;
    }
  }
}

// // Returned loops are oriented counter-clockwise
// pub fn poly_from_wire(wire: &Vec<CurveType>) -> PolyLine {
//   if wire.len() == 1 {
//     panic!("PolyLine has length one {:?}", wire);
//   }
//   let mut polyline = vec![];
//   let mut iter = wire.iter().peekable();
//   while let Some(elem) = iter.next() {
//     let endpoints = elem.as_curve().endpoints();
//     if polyline.len() == 0 {
//       let next_elem = iter.peek().unwrap();
//       let next_endpoints = next_elem.as_curve().endpoints();
//       if endpoints.0.almost(next_endpoints.0) || endpoints.0.almost(next_endpoints.1) {
//         polyline.push(endpoints.1);
//         polyline.push(endpoints.0);
//       } else {
//         polyline.push(endpoints.0);
//         polyline.push(endpoints.1);
//       }
//     } else {
//       polyline.push(elem.as_curve().other_endpoint(polyline.last().unwrap()));
//     }
//   }
//   polyline
// }

pub fn trim(_elem: &CurveType, _cutters: &Vec<CurveType>, _p: Point3) {
  // let splits = split_element(elem, cutters);
  // splits.sort_by(|a, b| {
  //   let a = a.as_curve();
  //   p.distance(a.closest_point(p))
  // })
}


#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_data;

  fn make_trimmed(elems: Vec<CurveType>) -> Region {
    elems.into_iter().map(|elem| TrimmedCurve::new(elem)).collect()
  }

  #[test]
  fn compare_areas() {
    let rect = test_data::rectangle();
    let rect: Vec<CurveType> = rect.into_iter().map(|l| l.into_enum() ).collect();
    let rect_poly = poly_from_wire(&make_trimmed(rect));

    let reverse_rect = test_data::reverse_rectangle();
    let reverse_rect: Vec<CurveType> = reverse_rect.into_iter().map(|l| l.into_enum() ).collect();
    let reverse_rect_poly = poly_from_wire(&make_trimmed(reverse_rect));

    assert_eq!(signed_polygon_area(&rect_poly), -signed_polygon_area(&reverse_rect_poly));
  }

  #[test]
  fn rectangle_clockwise() {
    let rect = test_data::rectangle();
    let rect: Vec<CurveType> = rect.into_iter().map(|l| l.into_enum() ).collect();
    let rect_poly = poly_from_wire(&make_trimmed(rect));

    assert!(is_clockwise(&rect_poly));
  }

  #[test]
  fn angle_clockwise() {
    let angle = test_data::angle_right();

    assert!(clockwise(angle[0].points.0, angle[0].points.1, angle[1].points.1) < 0.0);
  }

  #[test]
  fn angle_anti_clockwise() {
    let angle = test_data::angle_left();

    assert!(clockwise(angle[0].points.0, angle[0].points.1, angle[1].points.1) > 0.0);
  }
}
