use std::collections::VecDeque;

use crate::base::*;
use crate::curve::*;
use crate::surface::*;
use crate::geom2d::cross_2d;


#[derive(Debug, PartialEq)]
pub enum CurveIntersectionType {
  None,
  Touch(CurveIntersection), // Touching endpoints
  Pierce(Vec<CurveIntersection>), // Endpoint touching curve/surface
  Cross(Vec<CurveIntersection>), // Actual intersections
  Extended(Vec<CurveIntersection>), // Intersections outside geometric bounds
  Contained, // Overlap, Infinite intersections
}


#[derive(Debug, PartialEq)]
pub enum SurfaceIntersectionType {
  None,
  Touch(TrimmedCurve),
  Cross(TrimmedCurve),
  Extended(TrimmedCurve),
  Contained, // Overlap, Infinite intersections
}


#[derive(Debug, PartialEq)]
pub enum CurveSurfaceIntersectionType {
  None,
  Pierce(Vec<CurveIntersection>),
  Cross(Vec<CurveIntersection>),
  Extended(Vec<CurveIntersection>),
  Contained, // Overlap, Infinite intersections
}


#[derive(Debug, PartialEq)]
pub struct CurveIntersection {
  pub point: Point3,
  pub t: f64,
}

impl CurveIntersection {
  pub fn new(point: Point3, t: f64) -> Self {
    Self {
      point,
      t,
    }
  }
}


pub fn intersect(own: &CurveType, other: &CurveType) -> CurveIntersectionType {
  match own {
    // Line
    CurveType::Line(line) => match other {
      CurveType::Line(other) => line_line(line, other),
      CurveType::Circle(other) => line_circle(line, other),
      CurveType::Arc(_other) => CurveIntersectionType::None,
      CurveType::BezierSpline(other) => line_spline(line, other),
    },

    // Arc
    CurveType::Arc(_arc) => match other {
      CurveType::Line(_other) => CurveIntersectionType::None,
      CurveType::Circle(_other) => CurveIntersectionType::None,
      CurveType::Arc(_other) => CurveIntersectionType::None,
      CurveType::BezierSpline(_other) => CurveIntersectionType::None,
    },

    // Circle
    CurveType::Circle(circle) => match other {
      CurveType::Line(other) => line_circle(other, circle),
      CurveType::Circle(_other) => CurveIntersectionType::None,
      CurveType::Arc(_other) => CurveIntersectionType::None,
      CurveType::BezierSpline(_other) => CurveIntersectionType::None,
    },

    // Bezier Spline
    CurveType::BezierSpline(spline) => match other {
      CurveType::Line(other) => line_spline(other, spline), //XXX need to switch return values
      CurveType::Circle(_other) => CurveIntersectionType::None,
      CurveType::Arc(_other) => CurveIntersectionType::None,
      CurveType::BezierSpline(_other) => CurveIntersectionType::None,
    },
  }
}


// https://stackoverflow.com/questions/563198/how-do-you-detect-where-two-line-segments-intersect
pub fn line_line(own: &Line, other: &Line) -> CurveIntersectionType {
  let r = own.points.1 - own.points.0;
  let s = other.points.1 - other.points.0;
  let u_numerator = cross_2d(other.points.0 - own.points.0, r);
  let denominator = cross_2d(r, s);

  // Lines are colinear
  if u_numerator.almost(0.0) && denominator.almost(0.0) {
    // Lines overlap (All point differences in either direction have same sign)
    let overlap = ![
      (other.points.0.x - own.points.0.x < 0.0),
      (other.points.0.x - own.points.1.x < 0.0),
      (other.points.1.x - own.points.0.x < 0.0),
      (other.points.1.x - own.points.1.x < 0.0),
    ].windows(2).all(|w| w[0] == w[1]) || ![
      (other.points.0.y - own.points.0.y < 0.0),
      (other.points.0.y - own.points.1.y < 0.0),
      (other.points.1.y - own.points.0.y < 0.0),
      (other.points.1.y - own.points.1.y < 0.0),
    ].windows(2).all(|w| w[0] == w[1]);
    return if overlap {
      CurveIntersectionType::Contained
    } else {
      CurveIntersectionType::None
    }
  }

  // Lines touch at endpoints
  if own.points.0.almost(other.points.0) || own.points.0.almost(other.points.1){
    return CurveIntersectionType::Touch(CurveIntersection::new(own.points.0, 0.0))
  } else if own.points.1.almost(other.points.0) || own.points.1.almost(other.points.1) {
    return CurveIntersectionType::Touch(CurveIntersection::new(own.points.1, 1.0))
  }

  // Lines are parallel
  if denominator.almost(0.0) {
    return CurveIntersectionType::None;
  }

  // Lines cross
  let t = cross_2d(other.points.0 - own.points.0, s) / denominator;
  let u = u_numerator / denominator;
  let do_cross = (t >= 0.0) && (t <= 1.0) && (u >= 0.0) && (u <= 1.0);
  let intersection_point = own.points.0 + r * t;
  if do_cross {
    if t.almost(0.0) || t.almost(1.0) || u.almost(0.0) || u.almost(1.0) {
      CurveIntersectionType::Pierce(vec![CurveIntersection::new(intersection_point, t)])
    } else {
      CurveIntersectionType::Cross(vec![CurveIntersection::new(intersection_point, t)])
    }
  } else {
    //XXX Should be None for 3d lines
    CurveIntersectionType::Extended(vec![CurveIntersection::new(intersection_point, t)])
  }
}

pub fn line_spline(line: &Line, spline: &BezierSpline) -> CurveIntersectionType {
  let spline_end_points = spline.endpoints();
  // Curves touch at endpoints
  return if line.points.0.almost(spline_end_points.0) || line.points.0.almost(spline_end_points.1) {
    CurveIntersectionType::Touch(CurveIntersection::new(line.points.0, 0.0))

  } else if line.points.1.almost(spline_end_points.0) || line.points.1.almost(spline_end_points.1) {
    CurveIntersectionType::Touch(CurveIntersection::new(line.points.1, 1.0))

  } else {
    CurveIntersectionType::None
  }
}

pub fn line_circle(line: &Line, circle: &Circle) -> CurveIntersectionType {
  let direction = line.points.1 - line.points.0;
  let f = line.points.0 - circle.center;
  let a = direction.dot(direction);
  let b = f.dot(direction) * 2.0;
  let c = f.dot(f) - (circle.radius * circle.radius);

  let discriminant = b * b - 4.0 * a * c;
  if discriminant < 0.0 {
    CurveIntersectionType::None
  } else {
    let discriminant = discriminant.sqrt();

    let t1 = (-b - discriminant) / (2.0 * a);
    let t2 = (-b + discriminant) / (2.0 * a);

    let crossed = (t1 >= 0.0 && t1 <= 1.0, t2 >= 0.0 && t2 <= 1.0);

    let mut intersections = VecDeque::from(vec![
      CurveIntersection::new(line.sample(t1), t1),
      CurveIntersection::new(line.sample(t2), t2),
    ]);

    if crossed.0 || crossed.1 {
      if !crossed.0 { intersections.pop_front(); } else if !crossed.1 { intersections.pop_back(); }
      CurveIntersectionType::Cross(intersections.into_iter().collect())
    } else {
      CurveIntersectionType::Extended(intersections.into_iter().collect())
    }
  }
}

pub fn line_plane(line: &Line, plane: &Plane) -> CurveSurfaceIntersectionType {
  let n = plane.normal();
  let u = line.points.1 - line.points.0;
  let n_dot_u = n.dot(u);
  if n_dot_u <= EPSILON {
    // Line is parallel to plane
    if plane.contains_point(line.points.0) {
      // Line lies completely on plane
      CurveSurfaceIntersectionType::Contained
    } else {
      CurveSurfaceIntersectionType::None
    }
  } else {
    let s = n.dot(plane.origin - line.points.0) / n_dot_u;
    let p = line.points.0 + u * s;
    if s >= 0.0 && s <= 1.0 {
      // Line segment intersects plane
      if s == 0.0 || s == 1.0 {
        CurveSurfaceIntersectionType::Pierce(vec![CurveIntersection::new(p, s)])
      } else {
        CurveSurfaceIntersectionType::Cross(vec![CurveIntersection::new(p, s)])
      }
    } else {
      // The ray along the given line intersects plane
      CurveSurfaceIntersectionType::Extended(vec![CurveIntersection::new(p, s)])
    }
  }
}


#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_data;

  #[test]
  fn crossing_lines() {
    let lines = test_data::crossing_lines();
    let hit = line_line(&lines[0], &lines[1]);
    assert_eq!(hit, CurveIntersectionType::Cross(vec![
      CurveIntersection::new(Point3::origin(), 0.5)
    ]));
  }

  #[test]
  fn parallel_lines() {
    let lines = test_data::parallel_lines();
    let hit = line_line(&lines[0], &lines[1]);
    assert_eq!(hit, CurveIntersectionType::None);
  }

  #[test]
  fn overlapping_lines() {
    let lines = test_data::overlapping_lines();
    let hit = line_line(&lines[0], &lines[1]);
    assert_eq!(hit, CurveIntersectionType::Contained);
  }

  #[test]
  fn touching_lines() {
    let lines = test_data::rectangle();
    let hit = line_line(&lines[0], &lines[1]);
    assert_eq!(hit, CurveIntersectionType::Touch(
      CurveIntersection::new(Point3::new(1.0, 1.0, 0.0), 1.0)
    ));
  }

  #[test]
  fn circle_cross() {
    let circle = Circle::new(Point3::origin(), 1.0);
    let line = Line::new(Point3::new(-2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0));
    let hit = line_circle(&line, &circle);
    assert_eq!(hit, CurveIntersectionType::Cross(vec![
      CurveIntersection::new(Point3::new(-1.0, 0.0, 0.0), 0.25),
      CurveIntersection::new(Point3::new(1.0, 0.0, 0.0), 0.75),
    ]));
  }

  #[test]
  fn circle_pass() {
    let circle = Circle::new(Point3::origin(), 1.0);
    let line = Line::new(Point3::new(-2.0, 2.0, 0.0), Point3::new(2.0, 2.0, 0.0));
    let hit = line_circle(&line, &circle);
    assert_eq!(hit, CurveIntersectionType::None);
  }
}
