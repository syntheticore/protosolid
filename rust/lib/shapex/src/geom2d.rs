use earcutr;

use crate::internal::*;
use crate::wire::*;
use crate::mesh::*;


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
  dot * cross.signum()
}

pub fn is_clockwise(closed_loop: &PolyLine) -> bool {
  signed_polygon_area(&closed_loop) < 0.0
}

pub fn polygon_area(closed_loop: &PolyLine) -> f64 {
  signed_polygon_area(&closed_loop).abs()
}

pub fn signed_polygon_area(closed_loop: &PolyLine) -> f64 {
  let mut signed_area = 0.0;
  let len = closed_loop.len();
  for i in 0..len {
    let j = (i + 1) % len;
    let p = closed_loop[i];
    let next_p = closed_loop[j];
    signed_area += p.x * next_p.y - next_p.x * p.y;
  }
  signed_area / 2.0
}

pub fn tesselate_polygon(vertices: PolyLine, holes: Vec<usize>) -> Mesh {
  // #[cfg(debug_assertions)]
  // assert!(!is_clockwise(&vertices));
  let flat_vertices: Vec<f64> = vertices.iter().flat_map(|v| vec![v.x, v.y] ).collect();
  let faces: Vec<usize> = earcutr::earcut(&flat_vertices, &holes, 2);
  let mut normals = Vec::with_capacity(vertices.len());
  for _ in 0..faces.len() {
    normals.push(Vec3::unit_z());
  }
  Mesh {
    vertices,
    faces,
    normals,
  }
}


#[cfg(test)]
mod tests {
use super::*;
  use crate::test_data;
  use crate::test_data::make_generic;
  use crate::test_data::make_wire;
  use crate::surface::SurfaceArea;

  #[test]
  fn compare_areas() {
    let rect_poly = &make_wire(make_generic(test_data::rectangle())).tesselate();
    let reverse_rect_poly = rect_poly.iter().rev().cloned().collect();
    assert_eq!(signed_polygon_area(&rect_poly), -signed_polygon_area(&reverse_rect_poly));
  }

  #[test]
  fn rectangle_area() {
    let rect_poly = &make_wire(make_generic(test_data::rectangle()));
    assert_eq!(4.0, rect_poly.area());
  }

  #[test]
  fn rectangle_clockwise() {
    let rect_poly = &make_wire(make_generic(test_data::rectangle())).tesselate();
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

  #[test]
  fn angle_straight() {
    let angle = test_data::angle_straight();
    assert_eq!(clockwise(angle[0].points.0, angle[0].points.1, angle[1].points.1), 0.0);
  }
}

