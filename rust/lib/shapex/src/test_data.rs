use crate::base::*;
use crate::curve::*;

// pub fn crossing_lines() -> (Line, Line) {
//   (
//     Line::new(Point3::new(-0.5, 0.0, 0.0), Point3::new(0.5, 0.0, 0.0)),
//     Line::new(Point3::new(0.0, 0.0, -0.5), Point3::new(0.0, 0.0, 0.5)),
//   )
// }

pub fn parallel_lines() -> (Line, Line) {
  (
    Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)),
    Line::new(Point3::new(0.0, 0.0, 0.5), Point3::new(1.0, 0.0, 0.5)),
  )
}

pub fn overlapping_lines() -> (Line, Line) {
  (
    Line::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)),
    Line::new(Point3::new(0.5, 0.0, 0.0), Point3::new(1.5, 0.0, 0.0)),
  )
}
