use earcutr;

use crate::base::*;
use crate::curve::*;
use crate::intersection::*;
use crate::mesh::Mesh;


pub fn cross_2d(vec1: Vec3, vec2: Vec3) -> f64 {
  vec1.x * vec2.y - vec1.y * vec2.x
}

// Check if two line segments turn clockwise
// Returns values < 0 when clockwise, > 0 when anti-clockwise and 0 when segments are colinear
pub fn clockwise(p1: Point3, p2: Point3, p3: Point3) -> f64 {
  let v1 = (p2 - p1).normalize(); //OPT
  let v2 = (p3 - p2).normalize();
  let v3 = (p3 - p1).normalize();
  // Cross product changes sign with clockwiseness,
  // but doesn't show if angle is steeper or shallower than 90 degrees
  // (symmetric between front and back)
  let cross = cross_2d(v1, v3);
  // Dot product is "left/right" symmetric,
  // but negative for steep angles and positive for shallow angles
  let dot = (v1.dot(v2) - 1.0).abs() / 2.0; // Range shallow to steep => 0 -> 1
  dot * if cross >= 0.0 { 1.0 } else { -1.0 }
}

pub fn is_clockwise(closed_loop: &PolyLine) -> bool {
  signed_polygon_area(&closed_loop) > 0.0
}

pub fn polygon_area(closed_loop: &PolyLine) -> f64 {
  signed_polygon_area(&closed_loop).abs() / 2.0
}

fn signed_polygon_area(closed_loop: &PolyLine) -> f64 {
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

pub fn point_in_wire(p: Point3, wire: &Wire) -> bool {
  let ray = Line::new(p, p + Vec3::unit_x() * MAX_FLOAT).into_enum();
  let mut num_hits = 0;
  for elem in wire {
    num_hits += match intersect(&ray, &elem.cache) {
      CurveIntersectionType::Pierce(hits) |
      CurveIntersectionType::Cross(hits)
        => hits.len(),
      _ => 0,
    }
  }
  num_hits % 2 != 0
}

pub fn wire_in_wire(wire: &Wire, other: &Wire) -> bool {
  wire.iter().all(|elem| point_in_wire(elem.bounds.0, other))
}

pub fn tesselate_polygon(vertices: PolyLine, holes: Vec<usize>, normal: Vec3) -> Mesh {
  // #[cfg(debug_assertions)]
  // assert!(!is_clockwise(&vertices));
  let flat_vertices: Vec<f64> = vertices.iter().flat_map(|v| vec![v.x, v.y] ).collect();
  let faces: Vec<usize> = earcutr::earcut(&flat_vertices, &holes, 2);
  let mut normals = Vec::with_capacity(vertices.len());
  for _ in 0..faces.len() {
    normals.push(normal); //XXX Normals really used?
  }
  Mesh {
    vertices,
    faces,
    normals,
  }
}

pub fn tesselate_profile(profile: &Profile, normal: Vec3) -> Mesh {
  let poly_rings: Vec<PolyLine> = profile.iter().map(|wire| {
    tesselate_wire(wire)
  }).collect();
  let mut i = 0;
  let mut holes = Vec::with_capacity(poly_rings.len());
  for ring in &poly_rings {
    i += ring.len();
    holes.push(i);
  }
  holes.pop();
  let vertices: Vec<Point3> = poly_rings.into_iter().flatten().collect();
  tesselate_polygon(vertices, holes, normal)
}

pub fn tesselate_wire(wire: &Wire) -> PolyLine {
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

pub fn poly_from_wire(wire: &Wire) -> PolyLine {
  let mut polyline: PolyLine = wire.iter().map(|curve| curve.bounds.0 ).collect();
  polyline.push(wire[0].bounds.0);
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
  use crate::transform::*;

  fn make_wire(elems: Vec<CurveType>) -> Region {
    elems.into_iter().map(|elem| TrimmedCurve::new(elem)).collect()
  }

  fn make_generic<T: Curve>(elems: Vec<T>) -> Vec<CurveType> {
    elems.into_iter().map(|l| l.into_enum()).collect()
  }

  fn make_rect() -> Wire {
    let rect = test_data::rectangle();
    make_wire(make_generic(rect))
  }

  #[test]
  fn compare_areas() {
    let rect_poly = tesselate_wire(&make_rect());
    let reverse_rect_poly = rect_poly.iter().rev().cloned().collect();
    assert_eq!(signed_polygon_area(&rect_poly), -signed_polygon_area(&reverse_rect_poly));
  }

  #[test]
  fn rectangle_clockwise() {
    let rect_poly = tesselate_wire(&make_rect());
    assert!(is_clockwise(&rect_poly));
  }

  #[test]
  fn point_in_wire() {
    let rect = make_rect();
    assert!(super::point_in_wire(Point3::origin(), &rect));
    assert!(!super::point_in_wire(Point3::new(10.0, 0.0, 0.0), &rect));
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

  #[test]
  fn angle_straight() {
    let angle = test_data::angle_straight();
    assert_eq!(clockwise(angle[0].points.0, angle[0].points.1, angle[1].points.1), 0.0);
  }

  #[test]
  fn circle_in_rect() {
    let circle = make_wire(make_generic(vec![Circle::new(Point3::origin(), 0.5)]));
    let rect = make_rect();
    assert!(super::wire_in_wire(&circle, &rect));
    assert!(!super::wire_in_wire(&rect, &circle));
  }

  #[test]
  fn circle_in_circle() {
    let circle = make_wire(make_generic(vec![
      Circle::new(Point3::new(-27.0, 3.0, 0.0), 68.97340462273907)
    ]));
    let inner_circle = Circle::new(Point3::new(-1.0, 27.654544570311774, 0.0), 15.53598031475424);
    let inner_circle = make_wire(make_generic(vec![inner_circle]));
    println!("{:?}", inner_circle);
    assert!(super::wire_in_wire(&inner_circle, &circle));
    assert!(!super::wire_in_wire(&circle, &inner_circle));
  }

  #[test]
  fn point_in_circle() {
    let circle = make_wire(make_generic(vec![Circle::new(Point3::origin(), 20.0)]));
    assert!(super::point_in_wire(Point3::origin(), &circle));
  }

  #[test]
  fn rect_in_rect() {
    let rect = make_rect();
    let mut inner_rect = test_data::rectangle();
    for line in &mut inner_rect {
      line.scale(0.5);
    }
    let inner_rect = make_wire(make_generic(inner_rect));
    assert!(super::wire_in_wire(&inner_rect, &rect));
    assert!(!super::wire_in_wire(&rect, &inner_rect));
  }
}
