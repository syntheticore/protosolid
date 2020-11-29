use earcutr;

use crate::base::*;
use crate::PolyLine;
use crate::SketchElement;
use crate::mesh::Mesh;


pub fn cross_2d(vec1: Vec3, vec2: Vec3) -> f64 {
  vec1.x * vec2.y - vec1.y * vec2.x
}

pub fn tesselate_polygon(vertices: PolyLine) -> Mesh {
  let flat_vertices: Vec<f64> = vertices.iter().flat_map(|v| vec![v.x, v.y] ).collect();
  let triangles: Vec<usize> = earcutr::earcut(&flat_vertices, &vec![], 2);
  Mesh {
    vertices: vertices,
    faces: triangles,
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
  let mut iter = closed_loop.iter().peekable();
  while let Some(p) = iter.next() {
    let next_p = if let Some(next_p) = iter.peek() {
      next_p
    } else {
      &closed_loop[0]
    };
    signed_area += (next_p.x - p.x) * (next_p.y + p.y);
  }
  signed_area
}

pub fn poly_from_wire(loopy: Vec<SketchElement>) -> PolyLine {
  let mut polyline = vec![];
  let mut iter = loopy.iter().peekable();
  while let Some(elem) = iter.next() {
    let endpoints = elem.as_curve().endpoints();
    if polyline.len() == 0 {
      let next_elem = iter.peek().unwrap();
      let next_endpoints = next_elem.as_curve().endpoints();
      if endpoints.0.almost(next_endpoints.0) || endpoints.0.almost(next_endpoints.1) {
        polyline.push(endpoints.1);
        polyline.push(endpoints.0);
      } else {
        polyline.push(endpoints.0);
        polyline.push(endpoints.1);
      }
    } else {
      polyline.push(elem.as_curve().other_endpoint(polyline.last().unwrap()));
    }
  }
  let z = rand::random::<f64>() / -6.0;
  for p in &mut polyline {
    p.z = z;
  }
  polyline
}


#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_data;

  #[test]
  fn compare_areas() {
    let rect = test_data::rectangle();
    let rect: Vec<SketchElement> = rect.into_iter().map(|l| SketchElement::Line(l) ).collect();
    let rect_poly = poly_from_wire(rect);

    let reverse_rect = test_data::reverse_rectangle();
    let reverse_rect: Vec<SketchElement> = reverse_rect.into_iter().map(|l| SketchElement::Line(l) ).collect();
    let reverse_rect_poly = poly_from_wire(reverse_rect);

    assert_eq!(signed_polygon_area(&rect_poly), -signed_polygon_area(&reverse_rect_poly));
  }

  #[test]
  fn rectangle_clockwise() {
    let rect = test_data::rectangle();
    let rect: Vec<SketchElement> = rect.into_iter().map(|l| SketchElement::Line(l) ).collect();
    let rect_poly = poly_from_wire(rect.clone());

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
