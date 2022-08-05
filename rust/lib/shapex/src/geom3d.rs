use std::convert::From;

use serde::{Serialize, Deserialize};
use itertools::Itertools;

use crate::internal::*;
use crate::CurveType;
use crate::Transformable;


/// Three dimensional axis through a point.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Axis {
  pub origin: Point3,
  pub direction: Vec3,
}

impl Axis {
  pub fn new(origin: Point3, direction: Vec3) -> Self {
    Self {
      origin,
      direction: direction.normalize(),
    }
  }

  pub fn from_points(points: (Point3, Point3)) -> Self {
    Self {
      origin: points.0,
      direction: (points.1 - points.0).normalize(),
    }
  }

  pub fn as_transform(&self) -> Matrix4 {
    transform_from_location_and_normal(self.origin, self.direction)
  }

  pub fn closest_point(&self, p: Point3) -> Point3 {
    let t = (p - self.origin).dot(self.direction);
    self.origin + self.direction * t
  }

  pub fn flip(&mut self) {
    self.direction = -self.direction;
  }
}

impl Transformable for Axis {
  fn transform(&mut self, transform: &Matrix4) {
    self.origin = transform.transform_point(self.origin);
    self.direction = transform.transform_vector(self.direction);
  }
}

impl From<&Plane> for Axis {
  fn from(plane: &Plane) -> Self {
    Self {
      origin: plane.origin,
      direction: plane.normal(),
    }
  }
}


/// Three dimensional plane with a defined rotation around its normal.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plane {
  pub origin: Point3,
  pub u: Vec3,
  pub v: Vec3,
}

impl Default for Plane {
  fn default() -> Self {
    Self::new()
  }
}

impl Plane {
  pub fn new() -> Self {
    Self {
      origin: Point3::origin(),
      u: Vec3::new(1.0, 0.0, 0.0),
      v: Vec3::new(0.0, 1.0, 0.0),
    }
  }

  pub fn from_point(p: Point3) -> Self {
    Self {
      origin: p,
      u: Vec3::new(1.0, 0.0, 0.0),
      v: Vec3::new(0.0, 1.0, 0.0),
    }
  }

  pub fn from_triangle(p1: Point3, p2: Point3, p3: Point3) -> Self {
    let u = (p2 - p1).normalize();
    let pre_v = p3 - p1;
    let normal = u.cross(pre_v);
    Self {
      origin: p1,
      u,
      v: u.cross(normal).normalize(),
    }
  }

  pub fn from_normal(origin: Point3, normal: Vec3) -> Self {
    let m = transform_from_location_and_normal(origin, normal);
    Self {
      origin,
      u: m.transform_vector(Vec3::new(1.0, 0.0, 0.0)),
      v: m.transform_vector(Vec3::new(0.0, 1.0, 0.0)),
    }
  }

  pub fn sample(&self, u: f64, v: f64) -> Point3 {
    self.origin + self.u * u + self.v * v
  }

  pub fn unsample(&self, p: Point3) -> (f64, f64) {
    let p_local = self.as_transform().invert().unwrap().transform_point(p);
    (p_local.x, p_local.y)
  }

  pub fn d(&self) -> f64 {
    self.normal().dot(self.origin.to_vec())
  }

  pub fn normal(&self) -> Vec3 {
    self.u.cross(self.v)
  }

  pub fn contains_point(&self, p: Point3) -> bool {
    self.origin.almost(p) ||
    self.normal().dot((p - self.origin).normalize()).abs().almost(0.0)
  }

  // https://github.com/xibyte/jsketcher/blob/master/modules/geom/euclidean.ts
  // export function perpendicularVector(v) {
  //   v = vec.normalize(v);
  //   return IDENTITY_BASIS3.map(axis => vec.cross(axis, v)).sort((a, b) => vec.lengthSq(b) - vec.lengthSq(a))[0];
  // }

  pub fn as_transform(&self) -> Matrix4 {
    Matrix4::from_cols(
      self.u.extend(0.0),
      self.v.extend(0.0),
      self.normal().extend(0.0),
      self.origin.to_vec().extend(1.0)
    )
  }

  pub fn flip(&mut self) {
    self.v = -self.v;
  }
}

impl Transformable for Plane {
  fn transform(&mut self, transform: &Matrix4) {
    self.origin = transform.transform_point(self.origin);
    self.u = transform.transform_vector(self.u);
    self.v = transform.transform_vector(self.v);
  }
}

impl From<&Axis> for Plane {
  fn from(axis: &Axis) -> Self {
    Self::from_normal(axis.origin, axis.direction)
  }
}

impl From<&Matrix4> for Plane {
  fn from(m: &Matrix4) -> Self {
    Self {
      origin: m.transform_point(Point3::origin()),
      u: m.transform_vector(Vec3::new(1.0, 0.0, 0.0)),
      v: m.transform_vector(Vec3::new(0.0, 1.0, 0.0)),
    }
  }
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
      return Ok(circle.plane.clone())
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

pub fn transform_from_location_and_normal(origin: Point3, mut normal: Vec3) -> Matrix4 {
  normal = normal.normalize();
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

pub fn rotation_about_axis(axis: &Axis, angle: Deg<f64>) -> Matrix4 {
  let translation = Matrix4::from_translation(axis.origin.to_vec());
  let rotation = Matrix4::from_axis_angle(axis.direction, angle);
  translation * rotation * translation.invert().unwrap()
}


#[cfg(test)]
mod tests {
  // use super::*;
  // use crate::test_data;
}
