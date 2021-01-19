use crate::base::*;
use crate::curve::*;
use crate::curve::intersection::CurveIntersection;
use crate::geom2d;
use crate::geom3d::*;
use crate::mesh::Mesh;


pub trait Surface: Transformable {
  fn sample(&self, u: f64, v: f64) -> Point3;
  // fn unsample(&self, p: Point3) -> (f64, f64);
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
    self.base.as_surface().tesselate(80, wire)
  }

  pub fn area(&self) -> f64 {
    0.0 //XXX
  }

  //XXX pub fn on_surface(&self, u: f64, v: f64) -> bool
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

  // https://github.com/xibyte/jsketcher/blob/master/modules/geom/euclidean.ts
  // export function perpendicularVector(v) {
  //   v = vec.normalize(v);
  //   return IDENTITY_BASIS3.map(axis => vec.cross(axis, v)).sort((a, b) => vec.lengthSq(b) - vec.lengthSq(a))[0];
  // }

  #[allow(dead_code)]
  fn normal_to_uv(normal: Vec3) -> (Vec3, Vec3) {
    let u = normal.cross(Vec3::unit_z()).normalize();
    let v = normal.cross(u);
    (u, v)
  }

  // https://math.stackexchange.com/questions/1956699/getting-a-transformation-matrix-from-a-normal-vector
  pub fn as_transform(&self) -> Transform {
    Transform::from_matrix(Matrix4::from_cols(
      self.u.extend(self.origin.x),
      self.v.extend(self.origin.y),
      self.normal().extend(self.origin.z),
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
    // let mut mesh = geom2d::tesselate_polygon(polyline, Vec3::new(0.0, 0.0, 1.0).normalize());
    let mut mesh = geom2d::tesselate_polygon(polyline, self.normal());
    mesh.transform(&trans);
    mesh
    // Mesh::default()
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
  pub fn new(radius: f64) -> Self {
    Self {
      origin: Point3::new(0.0, 0.0, 0.0),
      direction: Vec3::new(0.0, 0.0, 1.0),
      radius,
      bounds: (0.0, 1.0),
    }
  }

  pub fn into_enum(self) -> SurfaceType {
    SurfaceType::Cylindrical(self)
  }
}

impl Surface for CylindricalSurface {
  fn sample(&self, u: f64, v: f64) -> Point3 {
    let u = self.bounds.0 + u * (self.bounds.1 - self.bounds.0);
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
    let mut vertices: Vec<Point3> = vec![];
    let mut faces: Vec<usize> = vec![];
    let mut normals: Vec<Vec3> = vec![];
    let mut iter = (0..=resolution).peekable();
    while let Some(i) = iter.next() {
      let u = i as f64 / resolution as f64;
      let upper_left = self.sample(u, 1.0);
      let lower_left = self.sample(u, 0.0);
      vertices.push(lower_left);
      vertices.push(upper_left);
      let normal = self.normal_at(u, 0.0);
      if let Some(&next_i) = iter.peek() {
        let next_u = next_i as f64 / resolution as f64;
        let next_normal = self.normal_at(next_u, 0.0);
        let i = i as usize * 2;
        // Triangle
        faces.push(i);
        faces.push(i + 1);
        faces.push(i + 2);
        normals.push(normal);
        normals.push(normal);
        normals.push(next_normal);
        // Triangle
        faces.push(i + 1);
        faces.push(i + 3);
        faces.push(i + 2);
        normals.push(normal);
        normals.push(next_normal);
        normals.push(next_normal);
      }
    }
    Mesh {
      vertices,
      faces,
      normals,
    }
    // Mesh::default()
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


// EllipticalSurface
// ConicSurface
// EllipticalConicSurface
// SphericalSurface
// ToricSurface
// NurbsSurface


#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn plane_normal() {
    let plane = Plane::new();
    assert_eq!(plane.normal(), Vec3::new(0.0, 0.0, 1.0));
  }

  #[test]
  fn cylinder_normal() {
    let cylinder = CylindricalSurface::new(1.0);
    almost_eq(cylinder.normal_at(0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
    almost_eq(cylinder.normal_at(0.5, 0.0), Vec3::new(0.0, -1.0, 0.0));
    almost_eq(cylinder.normal_at(0.25, 0.0), Vec3::new(1.0, 0.0, 0.0));
    almost_eq(cylinder.normal_at(0.75, 0.0), Vec3::new(-1.0, 0.0, 0.0));
  }
}
