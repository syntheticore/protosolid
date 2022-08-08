use crate::curve::*;
use crate::geom2d::cross_2d;


/// Type of intersection between two curves.

#[derive(Debug, PartialEq)]
pub enum CurveIntersectionType {
  /// Touching endpoints
  Touch(CurveIntersection),
  /// Endpoint touching other curve
  Pierce(CurveIntersection),
  /// Actual intersection
  Cross(CurveIntersection),
  /// Intersection outside geometric bounds
  Extended(CurveIntersection),
  /// Overlap, Infinite intersections
  Contained,
}

impl CurveIntersectionType {
  pub fn get_intersection(&self, include_extended: bool) -> Option<&CurveIntersection> {
    match self {
      Self::Contained
      => None,

      Self::Touch(isect)
      | Self::Pierce(isect)
      | Self::Cross(isect)
      => Some(isect),

      Self::Extended(isect)
      => if include_extended {
        Some(isect)
      } else {
        None
      }
    }
  }

  /// Retrieve intersection if it would split the first element
  pub fn get_splitting_intersection(&self) -> Option<&CurveIntersection> {
    match self {
      Self::Contained
      | Self::Touch(_)
      | Self::Extended(_)
      => None,

      Self::Cross(isect)
      => Some(isect),

      | Self::Pierce(isect)
      => if isect.direction { // Are we piercing or being pierced?
        None
      } else {
        Some(isect)
      },
    }
  }

  pub fn get_point(&self, include_extended: bool) -> Option<Point3> {
    self.get_intersection(include_extended).map(|isect| isect.point )
  }

  pub fn invert(&mut self) -> &Self {
    match self {
      Self::Touch(isect)
      | Self::Pierce(isect)
      | Self::Cross(isect)
      | Self::Extended(isect)
      => isect.invert(),
      _ => {}
    };
    self
  }
}


/// Geometric intersection between two curves.
/// * `point` - Point of intersection
/// * `t1` - Parameter on first curve
/// * `t2` - Parameter on second curve
/// * `direction` - Determines which way the Pierce and Extended variants are to be interpreted

#[derive(Debug, Clone, PartialEq)]
pub struct CurveIntersection {
  pub point: Point3,
  pub t1: f64,
  pub t2: f64,
  pub direction: bool
}

impl CurveIntersection {
  pub fn new(point: Point3, t1: f64, t2: f64) -> Self {
    Self {
      point,
      t1,
      t2,
      direction: true,
    }
  }

  pub fn invert(&mut self) {
    (self.t1, self.t2) = (self.t2, self.t1);
    self.direction = !self.direction
  }
}


// https://stackoverflow.com/questions/563198/how-do-you-detect-where-two-line-segments-intersect
pub fn line_line(own: &Line, other: &Line) -> Option<CurveIntersectionType> {
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
      Some(CurveIntersectionType::Contained)
    } else {
      None
    }
  }

  // Lines touch at endpoints
  if own.points.0.almost(other.points.0) {
    return Some(CurveIntersectionType::Touch(CurveIntersection::new(own.points.0, 0.0, 0.0)))

  } else if own.points.0.almost(other.points.1) {
    return Some(CurveIntersectionType::Touch(CurveIntersection::new(own.points.0, 0.0, 1.0)))

  } else if own.points.1.almost(other.points.0) {
    return Some(CurveIntersectionType::Touch(CurveIntersection::new(own.points.1, 1.0, 0.0)))

  } else if own.points.1.almost(other.points.1) {
    return Some(CurveIntersectionType::Touch(CurveIntersection::new(own.points.1, 1.0, 1.0)))
  }

  // Lines are parallel
  if denominator.almost(0.0) {
    return None;
  }

  // Lines cross
  let t = cross_2d(other.points.0 - own.points.0, s) / denominator;
  let u = u_numerator / denominator;
  let do_cross = (t >= 0.0) && (t <= 1.0) && (u >= 0.0) && (u <= 1.0);
  let intersection_point = own.points.0 + r * t;
  if do_cross {
    let first_at_end = t.almost(0.0) || t.almost(1.0);
    let second_at_end = u.almost(0.0) || u.almost(1.0);
    if first_at_end || second_at_end {
      let mut isect = CurveIntersection::new(intersection_point, t, u);
      isect.direction = first_at_end;
      Some(CurveIntersectionType::Pierce(isect))
    } else {
      Some(CurveIntersectionType::Cross(CurveIntersection::new(intersection_point, t, u)))
    }
  } else {
    Some(CurveIntersectionType::Extended(CurveIntersection::new(intersection_point, t, u)))
  }
}


pub fn line_spline(line: &Line, spline: &Spline) -> Vec<CurveIntersectionType> {
  let spline_end_points = spline.endpoints();
  let mut intersections = vec![];
  // Curves touch at endpoints
  if line.points.0.almost(spline_end_points.0) {
    intersections.push(CurveIntersectionType::Touch(CurveIntersection::new(line.points.0, 0.0, 0.0)))

  } else if line.points.0.almost(spline_end_points.1) {
    intersections.push(CurveIntersectionType::Touch(CurveIntersection::new(line.points.0, 0.0, 1.0)))

  } else if line.points.1.almost(spline_end_points.0) {
    intersections.push(CurveIntersectionType::Touch(CurveIntersection::new(line.points.1, 1.0, 0.0)))

  } else if line.points.1.almost(spline_end_points.1) {
    intersections.push(CurveIntersectionType::Touch(CurveIntersection::new(line.points.1, 1.0, 1.0)))
  }
  intersections
}


pub fn line_circle(line: &Line, circle: &Circle) -> Vec<CurveIntersectionType> {
  let direction = line.points.1 - line.points.0;
  let f = line.points.0 - circle.plane.origin;
  let a = direction.dot(direction);
  let b = f.dot(direction) * 2.0;
  let c = f.dot(f) - (circle.radius.powf(2.0));
  let mut intersections = vec![];
  let discriminant = b * b - 4.0 * a * c;
  if discriminant >= 0.0 { // No intersection for negative discriminant
    let discriminant = discriminant.sqrt();

    let t1 = (-b - discriminant) / (2.0 * a);
    let t2 = (-b + discriminant) / (2.0 * a);

    let crossed = (t1 >= 0.0 && t1 <= 1.0, t2 >= 0.0 && t2 <= 1.0);

    let p1 = line.sample(t1);
    let p2 = line.sample(t2);
    let isect1 = CurveIntersection::new(p1, t1, circle.unsample(p1));
    let isect2 = CurveIntersection::new(p2, t2, circle.unsample(p2));

    intersections.push(if crossed.0 {
      CurveIntersectionType::Cross(isect1)
    } else {
      CurveIntersectionType::Extended(isect1)
    });
    intersections.push(if crossed.1 {
      CurveIntersectionType::Cross(isect2)
    } else {
      CurveIntersectionType::Extended(isect2)
    });
  }
  intersections
}


#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_data;

  #[test]
  fn crossing_lines() {
    let lines = test_data::crossing_lines();
    let hit = line_line(&lines[0], &lines[1]);
    assert_eq!(hit, Some(CurveIntersectionType::Cross(
      CurveIntersection::new(Point3::origin(), 0.5, 0.5)
    )));
  }

  #[test]
  fn parallel_lines() {
    let lines = test_data::parallel_lines();
    let hit = line_line(&lines[0], &lines[1]);
    assert_eq!(hit, None);
  }

  #[test]
  fn overlapping_lines() {
    let lines = test_data::overlapping_lines();
    let hit = line_line(&lines[0], &lines[1]);
    assert_eq!(hit, Some(CurveIntersectionType::Contained));
  }

  #[test]
  fn touching_lines() {
    let lines = test_data::rectangle();
    let hit = line_line(&lines[0], &lines[1]);
    assert_eq!(hit, Some(CurveIntersectionType::Touch(
      CurveIntersection::new(Point3::new(1.0, 1.0, 0.0), 1.0, 0.0)
    )));
  }

  #[test]
  fn circle_cross() {
    let circle = Circle::new(Point3::origin(), 1.0);
    let line = Line::new(Point3::new(-2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0));
    let hit = line_circle(&line, &circle);
    assert_eq!(hit, vec![
      CurveIntersectionType::Cross(CurveIntersection::new(Point3::new(-1.0, 0.0, 0.0), 0.25, 0.5)),
      CurveIntersectionType::Cross(CurveIntersection::new(Point3::new(1.0, 0.0, 0.0), 0.75, 0.0)),
    ]);
  }

  #[test]
  fn circle_pass() {
    let circle = Circle::new(Point3::origin(), 1.0);
    let line = Line::new(Point3::new(-2.0, 2.0, 0.0), Point3::new(2.0, 2.0, 0.0));
    let hit = line_circle(&line, &circle);
    assert_eq!(hit, vec![]);
  }
}
