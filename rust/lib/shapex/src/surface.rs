use crate::base::*;
use crate::curve::*;
use crate::curve::intersection::CurveIntersection;
use crate::geom2d;
use crate::geom3d::*;
use crate::mesh::Mesh;


pub trait Surface: Transformable {
  fn sample(&self, u: f64, v: f64) -> Point3;
  fn normal_at(&self, u: f64, v: f64) -> Vec3;
  fn tesselate(&self, resolution: i32, bounds: &Wire) -> Mesh;
  fn flip(&mut self);
}

impl core::fmt::Debug for dyn Surface {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Foo")
  }
}


#[derive(Debug, Clone)]
pub enum SurfaceType {
  Planar(Plane),
  Cylindrical(CylindricalSurface),
}

impl SurfaceType {
  pub fn as_surface(&self) -> &dyn Surface {
    match self {
      Self::Planar(plane) => plane,
      Self::Cylindrical(surf) => surf,
    }
  }

  pub fn as_surface_mut(&mut self) -> &mut dyn Surface {
    match self {
      Self::Planar(plane) => plane,
      Self::Cylindrical(surf) => surf,
    }
  }
}


#[derive(Debug)]
pub struct TrimmedSurface {
  pub base: SurfaceType,
  pub bounds: Vec<Wire>,
}

impl TrimmedSurface {
  pub fn new(surf: SurfaceType, outer_bounds: Wire) -> Self {
    Self {
      base: surf,
      bounds: vec![outer_bounds],
    }
  }

  pub fn tesselate(&self) -> Mesh {
    let wire = &self.bounds[0];
    self.base.as_surface().tesselate(10, wire)
  }
}


#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
  pub origin: Point3,
  pub u: Vec3,
  pub v: Vec3,
}

impl Plane {
  pub fn new() -> Self {
    Self {
      origin: Point3::new(0.0, 0.0, 0.0),
      u: Vec3::new(1.0, 0.0, 0.0),
      v: Vec3::new(0.0, 1.0, 0.0),
    }
  }

  pub fn from_triangle(p1: Point3, p2: Point3, p3: Point3) -> Self {
    let u = (p2 - p1).normalize();
    let pre_v = p3 - p1;
    let normal = u.cross(pre_v).normalize();
    Self {
      origin: p1,
      u,
      v: u.cross(normal),
    }
  }

  pub fn normal(&self) -> Vec3 {
    self.u.cross(self.v)
  }

  pub fn intersect_line(&self, line: (Point3, Point3)) -> CurveIntersection {
    let n = self.normal();
    let u = line.1 - line.0;
    let n_dot_u = n.dot(u);
    if n_dot_u <= EPSILON {
      // Line is parallel to this plane
      if self.contains_point(line.0) {
        // Line lies completely on this plane
        CurveIntersection::Contained
      } else {
        CurveIntersection::None
      }
    } else {
      let s = n.dot(self.origin - line.0) / n_dot_u;
      let p = line.0 + u * s;
      if s >= 0.0 && s <= 1.0 {
        // Line segment intersects this plane
        if s == 0.0 || s == 1.0 {
          CurveIntersection::Pierce(vec![p])
        } else {
          CurveIntersection::Cross(vec![p])
        }
      } else {
        // The ray along the given line intersects this plane
        CurveIntersection::Extended(vec![p])
      }
    }
  }

  pub fn contains_point(&self, p: Point3) -> bool {
    self.normal().dot(p - self.origin) <= EPSILON
  }

  #[allow(dead_code)]
  fn normal_to_uv(normal: Vec3) -> (Vec3, Vec3) {
    let u = normal.cross(Vec3::unit_z()).normalize();
    let v = normal.cross(u);
    (u, v)
  }

  // https://math.stackexchange.com/questions/1956699/getting-a-transformation-matrix-from-a-normal-vector
  pub fn as_transform(&self) -> Transform {
    Transform::from_matrix(Matrix4::from_cols(
      self.u.extend(1.0),
      self.v.extend(1.0),
      self.normal().extend(1.0),
      Vec4::unit_w()
    ).transpose())
  }

  pub fn into_enum(self) -> SurfaceType {
    SurfaceType::Planar(self)
  }
}

impl Surface for Plane {
  fn sample(&self, u: f64, v: f64) -> Point3 {
    self.origin + self.u * u + self.v * v
  }

  fn normal_at(&self, _u: f64, _v: f64) -> Vec3 {
    self.normal()
  }

  fn tesselate(&self, _resolution: i32, bounds: &Wire) -> Mesh {
    let mut bounds = bounds.clone();
    let trans = self.as_transform();
    let lay_flat = trans.invert();
    for curve in &mut bounds {
      curve.transform(&lay_flat)
    }
    let polyline = geom2d::poly_from_wire(&bounds);
    let mut mesh = geom2d::tesselate_polygon(polyline, self.normal());
    mesh.transform(&trans);
    mesh
  }

  fn flip(&mut self) {
    self.v = -self.v;
  }
}

impl Transformable for Plane {
  fn transform(&mut self, transform: &Transform) {
    self.origin = transform.apply(self.origin);
    self.u = transform.apply_vec(self.u);
    self.v = transform.apply_vec(self.v);
  }
}


#[derive(Debug, Clone, PartialEq)]
pub struct CylindricalSurface {
  pub origin: Point3,
  pub radius: f64,
  pub direction: Vec3,
  pub bounds: (f64, f64),
}

impl CylindricalSurface {
  pub fn new() -> Self {
    Self {
      origin: Point3::new(0.0, 0.0, 0.0),
      radius: 1.0,
      direction: Vec3::new(0.0, 0.0, 1.0),
      bounds: (0.0, 1.0),
    }
  }

  pub fn into_enum(self) -> SurfaceType {
    SurfaceType::Cylindrical(self)
  }
}

impl Surface for CylindricalSurface {
  fn sample(&self, u: f64, v: f64) -> Point3 {
    let u = u * std::f64::consts::PI * 2.0;
    Point3::new(
      self.origin.x + u.sin() * self.radius,
      self.origin.y + u.cos() * self.radius,
      self.origin.z + self.direction.z * v,
    )
  }

  fn normal_at(&self, u: f64, _v: f64) -> Vec3 {
    (self.sample(u, 0.0) - self.origin).normalize()
  }

  fn tesselate(&self, resolution: i32, _bounds: &Wire) -> Mesh {
    for u in 0..resolution {
      let u = u as f64 / resolution as f64;
      let u = self.bounds.0 + u * (self.bounds.1 - self.bounds.0);
      for v in 0..resolution {
        let v = v as f64 / resolution as f64;
        self.sample(u, v);
      }
    }
    Mesh::default()
  }

  fn flip(&mut self) {
    self.bounds = (self.bounds.1, self.bounds.0);
  }
}

impl Transformable for CylindricalSurface {
  fn transform(&mut self, transform: &Transform) {
    self.origin = transform.apply(self.origin);
    self.direction = transform.apply_vec(self.direction);
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn plane_normal() {
    let plane = Plane::new();
    assert_eq!(plane.normal(), Vec3::new(0.0, 0.0, 1.0));
  }
}
