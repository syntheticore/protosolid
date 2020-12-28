use crate::geom2d::*;
use crate::curve::*;


#[derive(Debug, PartialEq)]
pub enum CurveIntersection {
  None,
  Touch(Point3), // Touching endpoints
  Pierce(Vec<Point3>), // Endpoint touching curve/surface
  Cross(Vec<Point3>), // Actual intersections
  Extended(Vec<Point3>), // Intersections outside geometric bounds
  Contained, // Overlap, Infinite intersections
}


pub fn intersect(own: &CurveType, other: &CurveType) -> CurveIntersection {
  match own {
    // Line
    CurveType::Line(line) => match other {
      CurveType::Line(other) => line_line(line, other),
      CurveType::Circle(_other) => CurveIntersection::None,
      CurveType::Arc(_other) => CurveIntersection::None,
      CurveType::BezierSpline(other) => line_spline(line, other),
    },

    // Arc
    CurveType::Circle(_circle) => match other {
      CurveType::Line(_other) => CurveIntersection::None,
      CurveType::Circle(_other) => CurveIntersection::None,
      CurveType::Arc(_other) => CurveIntersection::None,
      CurveType::BezierSpline(_other) => CurveIntersection::None,
    },

    // Circle
    CurveType::Arc(_arc) => match other {
      CurveType::Line(_other) => CurveIntersection::None,
      CurveType::Circle(_other) => CurveIntersection::None,
      CurveType::Arc(_other) => CurveIntersection::None,
      CurveType::BezierSpline(_other) => CurveIntersection::None,
    },

    // Bezier Spline
    CurveType::BezierSpline(spline) => match other {
      CurveType::Line(other) => line_spline(other, spline),
      CurveType::Circle(_other) => CurveIntersection::None,
      CurveType::Arc(_other) => CurveIntersection::None,
      CurveType::BezierSpline(_other) => CurveIntersection::None,
    },
  }
}

// https://stackoverflow.com/questions/563198/how-do-you-detect-where-two-line-segments-intersect
pub fn line_line(own: &Line, other: &Line) -> CurveIntersection {
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
      CurveIntersection::Contained
    } else {
      CurveIntersection::None
    }
  }
  // Lines touch at endpoints
  if own.points.0.almost(other.points.0) || own.points.0.almost(other.points.1) {
    return CurveIntersection::Touch(own.points.0)
  } else if own.points.1.almost(other.points.0) || own.points.1.almost(other.points.1) {
    return CurveIntersection::Touch(own.points.1)
  }
  if denominator.almost(0.0) {
    // Lines are paralell
    return CurveIntersection::None;
  }
  // Lines cross
  let t = cross_2d(other.points.0 - own.points.0, s) / denominator;
  let u = u_numerator / denominator;
  let do_cross = (t >= 0.0) && (t <= 1.0) && (u >= 0.0) && (u <= 1.0);
  let intersection_point = own.points.0 + r * t;
  if do_cross {
    if t.almost(0.0) || t.almost(1.0) || u.almost(0.0) || u.almost(1.0) {
      CurveIntersection::Pierce(vec![intersection_point])
    } else {
      CurveIntersection::Cross(vec![intersection_point])
    }
  } else {
    CurveIntersection::Extended(vec![intersection_point])
  }
}

pub fn line_spline(line: &Line, spline: &BezierSpline) -> CurveIntersection {
  let spline_end_points = spline.endpoints();
  // Curves touch at endpoints
  return if line.points.0.almost(spline_end_points.0) || line.points.0.almost(spline_end_points.1) {
    CurveIntersection::Touch(line.points.0)
  } else if line.points.1.almost(spline_end_points.0) || line.points.1.almost(spline_end_points.1) {
    CurveIntersection::Touch(line.points.1)
  } else {
    CurveIntersection::None
  }
}

pub fn line_circle(line: &Line, circle: &Circle) -> CurveIntersection {
  let direction = line.points.1 - line.points.0;
  let f = line.points.0 - circle.center;
  let a = direction.dot(direction);
  let b = f.dot(direction) * 2.0;
  let c = f.dot(f) - (circle.radius * circle.radius);

  let discriminant = b * b - 4.0 * a * c;
  if discriminant < 0.0 {
    CurveIntersection::None
  } else {
    let discriminant = discriminant.sqrt();

    let t1 = (-b - discriminant) / (2.0 * a);
    let t2 = (-b + discriminant) / (2.0 * a);

    let mut intersections = vec![];
    if t1 >= 0.0 && t1 <= 1.0 {
      intersections.push(line.points.0 + direction * t1);
    }

    if t2 >= 0.0 && t2 <= 1.0 {
      intersections.push(line.points.0 + direction * t2);
    }

    if intersections.len() > 0 {
      CurveIntersection::Cross(intersections)
    } else {
      CurveIntersection::None
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
    let hit = intersection::line_line(&lines[0], &lines[1]);
    assert_eq!(hit, CurveIntersection::Cross(vec![Point3::new(0.0, 0.0, 0.0)]));
  }

  #[test]
  fn parallel_lines() {
    let lines = test_data::parallel_lines();
    let hit = intersection::line_line(&lines[0], &lines[1]);
    assert_eq!(hit, CurveIntersection::None);
  }

  #[test]
  fn overlapping_lines() {
    let lines = test_data::overlapping_lines();
    let hit = intersection::line_line(&lines[0], &lines[1]);
    assert_eq!(hit, CurveIntersection::Contained);
  }

  #[test]
  fn touching_lines() {
    let lines = test_data::rectangle();
    let hit = intersection::line_line(&lines[0], &lines[1]);
    assert_eq!(hit, CurveIntersection::Touch(Point3::new(1.0, 1.0, 0.0)));
  }

  #[test]
  fn circle_cross() {
    let circle = Circle::new(Point3::new(0.0, 0.0, 0.0), 1.0);
    let line = Line::new(Point3::new(-2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0));
    let hit = intersection::line_circle(&line, &circle);
    assert_eq!(hit, CurveIntersection::Cross(vec![Point3::new(-1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)]));
  }

  #[test]
  fn circle_pass() {
    let circle = Circle::new(Point3::new(0.0, 0.0, 0.0), 1.0);
    let line = Line::new(Point3::new(-2.0, 2.0, 0.0), Point3::new(2.0, 2.0, 0.0));
    let hit = intersection::line_circle(&line, &circle);
    assert_eq!(hit, CurveIntersection::None);
  }
}
