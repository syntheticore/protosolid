use crate::base::*;
use crate::curve::*;

pub fn crossing_lines() -> Vec<Line> {
  vec![
    Line::new(Point3::new(-0.5, 0.0, 0.0), Point3::new(0.5, 0.0, 0.0)),
    Line::new(Point3::new(0.0, -0.5, 0.0), Point3::new(0.0, 0.5, 0.0)),
  ]
}

pub fn parallel_lines() -> Vec<Line> {
  vec![
    Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)),
    Line::new(Point3::new(0.0, 0.5, 0.0), Point3::new(1.0, 0.5, 0.0)),
  ]
}

pub fn overlapping_lines() -> Vec<Line> {
  vec![
    Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)),
    Line::new(Point3::new(0.5, 0.0, 0.0), Point3::new(1.5, 0.0, 0.0)),
  ]
}

pub fn t_section() -> Vec<Line> {
  vec![
    Line::new(Point3::new(-1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)),
    Line::new(Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 0.0, 0.0)),
  ]
}

pub fn angle_left() -> Vec<Line> {
  vec![
    Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)),
    Line::new(Point3::new(0.0, 1.0, 0.0), Point3::new(-1.0, 1.0, 0.0)),
  ]
}

pub fn angle_right() -> Vec<Line> {
  vec![
    Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)),
    Line::new(Point3::new(0.0, 1.0, 0.0), Point3::new(1.0, 1.0, 0.0)),
  ]
}

pub fn angle_straight() -> Vec<Line> {
  vec![
    Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)),
    Line::new(Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 2.0, 0.0)),
  ]
}

pub fn rectangle() -> Vec<Line> {
  let upper_left = Point3::new(-1.0, 1.0, 0.0);
  let upper_right = Point3::new(1.0, 1.0, 0.0);
  let lower_right = Point3::new(1.0, -1.0, 0.0);
  let lower_left = Point3::new(-1.0, -1.0, 0.0);
  vec![
    Line::new(upper_left, upper_right),
    Line::new(upper_right, lower_right),
    Line::new(lower_right, lower_left),
    Line::new(lower_left, upper_left),
  ]
}

pub fn reverse_rectangle() -> Vec<Line> {
  let upper_left = Point3::new(-1.0, 1.0, 0.0);
  let upper_right = Point3::new(1.0, 1.0, 0.0);
  let lower_right = Point3::new(1.0, -1.0, 0.0);
  let lower_left = Point3::new(-1.0, -1.0, 0.0);
  vec![
    Line::new(upper_left, lower_left),
    Line::new(lower_left, lower_right),
    Line::new(lower_right, upper_right),
    Line::new(upper_right, upper_left),
  ]
}

pub fn crossing_rectangle() -> Vec<Line> {
  let displacement = Vec3::new(0.5, 0.5, 0.0);
  let mut rect = rectangle();
  // Displace lower line
  rect[2].points.0 += displacement;
  rect[2].points.1 += displacement;
  // Displace left line
  rect[3].points.0 += displacement;
  rect[3].points.1 += displacement;
  rect
}

pub fn arc_rectangle() -> Vec<CurveType> {
  let upper_left = Point3::new(-1.0, 1.0, 0.0);
  let upper_right = Point3::new(1.0, 1.0, 0.0);
  let lower_right = Point3::new(1.0, -1.0, 0.0);
  let lower_left = Point3::new(-1.0, -1.0, 0.0);
  vec![
    Arc::from_points(
      upper_left,
      (upper_left + upper_right.to_vec()) / 2.0 + Vec3::new(0.0, 0.1, 0.0),
      upper_right
    ).unwrap().into_enum(),
    Line::new(upper_right, lower_right).into_enum(),
    Line::new(lower_right, lower_left).into_enum(),
    Line::new(lower_left, upper_left).into_enum(),
  ]
}

pub fn s_curve() -> BezierSpline {
  BezierSpline::new(vec![
    Point3::new(-1.5, -1.0, 0.0),
    Point3::new(-0.5, -1.0, 0.0),
    Point3::new(0.5, 1.0, 0.0),
    Point3::new(1.5, 1.0, 0.0),
  ])
}

pub fn make_generic<T: Curve>(elems: Vec<T>) -> Vec<CurveType> {
  elems.into_iter().map(|l| l.into_enum()).collect()
}

pub fn make_region(elems: Vec<CurveType>) -> Region {
  elems.into_iter().map(|elem| TrimmedCurve::new(elem)).collect()
}
