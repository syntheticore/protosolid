// use crate::surface::*;


#[derive(Debug, PartialEq)]
pub enum SurfaceIntersection {
  None,
  Bounded(CurveType),
  Infinite(CurveType),
  Contained, // Overlap, Infinite intersections
}


#[cfg(test)]
mod tests {
  use super::*;
  use crate::test_data;
}
