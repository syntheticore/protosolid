use crate::surface::*;

// use crate::log;


#[derive(Debug, PartialEq)]
pub enum SurfaceIntersectionType {
  Touch(CurveType),
  Cross(CurveType),
  Extended(CurveType),
  Contained, // Overlap, Infinite intersections
}

impl SurfaceIntersectionType {
  pub fn get_line(&self) -> Option<&Line> {
    match self {
      Self::Contained
      => None,

      Self::Touch(curve)
      | Self::Cross(curve)
      | Self::Extended(curve)
      => {
        if let CurveType::Line(line) = curve {
          Some(line)
        } else { None }
      },
    }
  }
}


#[derive(Debug, PartialEq)]
pub enum CurveSurfaceIntersectionType {
  Pierce(CurveSurfaceIntersection),
  Cross(CurveSurfaceIntersection),
  Extended(CurveSurfaceIntersection),
  Contained, // Overlap, Infinite intersections
}

impl CurveSurfaceIntersectionType {
  pub fn get_intersection(&self, include_extended: bool) -> Option<&CurveSurfaceIntersection> {
    match self {
      Self::Contained
      => None,

      Self::Pierce(isect)
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

  pub fn get_point(&self, include_extended: bool) -> Option<Point3> {
    self.get_intersection(include_extended).map(|isect| isect.point )
  }
}


#[derive(Debug, Clone, PartialEq)]
pub struct CurveSurfaceIntersection {
  pub point: Point3,
  pub t: f64,
  // pub u: f64,
  // pub v: f64,
}

impl CurveSurfaceIntersection {
  pub fn new(point: Point3, t: f64) -> Self {
    Self {
      point,
      t,
      // u, v,
    }
  }
}


#[allow(dead_code)]
pub fn line_plane(line: &Line, plane: &Plane) -> Option<CurveSurfaceIntersectionType> {
  let n = plane.normal();
  let u = line.points.1 - line.points.0;
  let n_dot_u = n.dot(u);
  if n_dot_u.almost(0.0) {
    // Line is parallel to plane
    if plane.contains_point(line.points.0) {
      // Line lies completely on plane
      Some(CurveSurfaceIntersectionType::Contained)
    } else {
      None
    }
  } else {
    let s = n.dot(plane.origin - line.points.0) / n_dot_u;
    let p = line.points.0 + u * s;
    if s >= 0.0 && s <= 1.0 {
      // Line segment intersects plane
      if s.almost(0.0) || s.almost(1.0) {
        Some(CurveSurfaceIntersectionType::Pierce(CurveSurfaceIntersection::new(p, s)))
      } else {
        Some(CurveSurfaceIntersectionType::Cross(CurveSurfaceIntersection::new(p, s)))
      }
    } else {
      // The ray along the given line intersects plane
      Some(CurveSurfaceIntersectionType::Extended(CurveSurfaceIntersection::new(p, s)))
    }
  }
}


pub fn plane_plane(plane: &Plane, other: &Plane) -> Option<SurfaceIntersectionType> {
  let normal = plane.normal();
  let other_normal = other.normal();
  let third_normal = normal.cross(other_normal);
  let det = third_normal.magnitude2();
  if det.almost(0.0) {
    // Planes are parallel
    if plane.contains_point(other.origin) {
      Some(SurfaceIntersectionType::Contained)
    } else {
      None
    }
  } else {
    let p = Point3::from_vec((other_normal.cross(third_normal) * plane.d() + third_normal.cross(normal) * other.d()) / det);
    Some(SurfaceIntersectionType::Cross(Line::new(p, p + third_normal).into_enum()))
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn plane_intersection() {
    let plane = Plane::from_normal(Point3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
    let other = Plane::from_normal(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
    let isect = plane_plane(&plane, &other);
    println!("{:#?}", isect);
    if let Some(SurfaceIntersectionType::Cross(curve)) = isect {
      if let CurveType::Line(line) = curve {
        let points = line.endpoints();
        almost_eq(points.1 - points.0, Vec3::new(-1.0, 0.0, 0.0));
        assert!(tuple2_to_vec(points).iter().all(|p| plane.contains_point(*p) && other.contains_point(*p) ));
      } else { panic!("Intersection was no line") }
    } else { panic!("No intersection detected") }
  }
}
