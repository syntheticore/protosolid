use crate::base::*;
use crate::curve::*;


pub fn intersect(own: &SketchElement, other: &SketchElement) -> Intersection {
  match own {
    // Line
    SketchElement::Line(line) => match other {
      SketchElement::Line(other) => line_line(line, other),
      SketchElement::Circle(_other) => Intersection::None,
      SketchElement::Arc(_other) => Intersection::None,
      SketchElement::BezierSpline(_other) => Intersection::None,
    },

    // Arc
    SketchElement::Circle(_circle) => match other {
      SketchElement::Line(_other) => Intersection::None,
      SketchElement::Circle(_other) => Intersection::None,
      SketchElement::Arc(_other) => Intersection::None,
      SketchElement::BezierSpline(_other) => Intersection::None,
    },

    // Circle
    SketchElement::Arc(_arc) => match other {
      SketchElement::Line(_other) => Intersection::None,
      SketchElement::Circle(_other) => Intersection::None,
      SketchElement::Arc(_other) => Intersection::None,
      SketchElement::BezierSpline(_other) => Intersection::None,
    },

    // Bezier Spline
    SketchElement::BezierSpline(_spline) => match other {
      SketchElement::Line(_other) => Intersection::None,
      SketchElement::Circle(_other) => Intersection::None,
      SketchElement::Arc(_other) => Intersection::None,
      SketchElement::BezierSpline(_other) => Intersection::None,
    },
  }
}

// https://stackoverflow.com/questions/563198/how-do-you-detect-where-two-line-segments-intersect
pub fn line_line(own: &Line, other: &Line) -> Intersection {
  let r = own.points.1 - own.points.0;
  let s = other.points.1 - other.points.0;
  let u_numerator = cross_2d(other.points.0 - own.points.0, r);
  let denominator = cross_2d(r, s);
  // Lines are colinear
  if u_numerator == 0.0 && denominator == 0.0 {
    // Lines touch at endpoints
    if own.points.0 == other.points.0 || own.points.0 == other.points.1 {
      return Intersection::Touch(own.points.0)
    } else if own.points.1 == other.points.0 || own.points.1 == other.points.1 {
      return Intersection::Touch(own.points.1)
    }
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
      Intersection::Contained
    } else {
      Intersection::None
    }
  }
  if denominator == 0.0 {
    // Lines are paralell
    return Intersection::None;
  }
  // Lines cross
  let t = cross_2d(other.points.0 - own.points.0, s) / denominator;
  let u = u_numerator / denominator;
  let do_cross = (t >= 0.0) && (t <= 1.0) && (u >= 0.0) && (u <= 1.0);
  let intersection_point = own.points.0 + r * t;
  if do_cross {
    Intersection::Hit(vec![intersection_point])
  } else {
    Intersection::Extended(vec![intersection_point])
  }
}


#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_data;

  // #[test]
  // fn crossing_lines() {
  //   let lines = test_data::crossing_lines();
  //   let hit = intersection::line_line(&lines.0, &lines.1);
  //   assert_eq!(hit, Intersection::Hit(vec![Point3::new(0.0, 0.0, 0.0)]));
  // }

  // #[test]
  // fn parallel_lines() {
  //   let lines = test_data::parallel_lines();
  //   let hit = intersection::line_line(&lines.0, &lines.1);
  //   assert_eq!(hit, Intersection::None);
  // }

  #[test]
  fn overlapping_lines() {
    let lines = test_data::overlapping_lines();
    let hit = intersection::line_line(&lines.0, &lines.1);
    assert_eq!(hit, Intersection::Contained);
  }
}
