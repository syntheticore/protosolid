use crate::base::*;
use crate::Plane;
use crate::CurveType;
// use crate::log;

use itertools::Itertools;


#[derive(Debug, Clone)]
pub struct Axis {
  pub origin: Point3,
  pub direction: Vec3,
}

#[derive(Debug)]
pub enum PlaneError {
  Underdefined,
  Inconsistent,
}

impl std::fmt::Display for PlaneError {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match self {
      Self::Underdefined => write!(f, "Input was underdefined {:?}", self),
      Self::Inconsistent => write!(f, "Inputs did not match {:?}", self),
    }
  }
}

impl std::error::Error for PlaneError {}

impl From<PlaneError> for String {
  fn from(error: PlaneError) -> Self {
    error.into()
  }
}


// Find suitable points in wire to build a matching plane
pub fn plane_from_points(points: &Vec<Point3>) -> Result<Plane, PlaneError> {
  let points: Vec<Point3> = points.iter().cloned().unique_by(|p|
    format!("{:?}", p) //XXX Create wrapper for Point3 that implements Ord
  ).collect();
  //XXX use points with greatest distance as start points
  let v1 = (points[1] - points[0]).normalize();
  // if let Some(p3) = points.iter().skip(3).min_by(|p1, p2| {
  if let Some(p3) = points.iter().skip(2).min_by(|p1, p2| {
    let v2 = (*p1 - points[0]).normalize();
    let v3 = (*p2 - points[0]).normalize();
    v1.dot(v2).abs().partial_cmp(&v1.dot(v3).abs()).unwrap()
  }) {
    if v1.almost((p3 - points[0]).normalize()) {
      Err(PlaneError::Underdefined)
    } else {
      Ok(Plane::from_triangle(
        *p3,
        points[1],
        points[0],
      ))
    }
  } else { Err(PlaneError::Underdefined) }
}

pub fn plane_from_curves(elems: &Vec<CurveType>) -> Result<Plane, PlaneError> {
  for elem in elems.iter() {
    if let CurveType::Circle(circle) = elem {
      return Ok(Plane::from_normal(circle.center, circle.normal))
    }
  }
  let points = elems.iter().map(|curve|
    tuple2_to_vec(curve.as_curve().endpoints())
  ).collect::<Vec<Vec<Point3>>>().concat();
  let plane = plane_from_points(&points)?;
  if points.iter().all(|p| plane.contains_point(*p) ) {
    Ok(plane)
  } else {
    Err(PlaneError::Inconsistent)
  }
}

pub fn transform_from_location_and_normal(origin: Point3, normal: Vec3) -> Matrix4 {
  let up = Vec3::new(0.0, 0.0, 1.0);
  let dot = normal.dot(up);
  let x_axis = if dot.abs().almost(1.0) {
    Vec3::new(1.0, 0.0, 0.0) * dot.signum()
  } else {
    normal.cross(up).normalize()
  };
  let y_axis = normal.cross(x_axis);
  Matrix4::from_cols(
    x_axis.extend(0.0),
    y_axis.extend(0.0),
    normal.extend(0.0),
    origin.to_vec().extend(1.0)
  )
}


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
