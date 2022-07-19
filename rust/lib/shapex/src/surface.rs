use serde::{Serialize, Deserialize};

use crate::base::*;
use crate::transform::*;
use crate::curve::*;
use crate::mesh::*;
use crate::geom2d;
use crate::geom3d;

pub mod intersection;
pub use intersection::SurfaceIntersection;
pub use intersection::CurveSurfaceIntersection;

// use crate::log;


pub trait Surface: Transformable {
  fn sample(&self, u: f64, v: f64) -> Point3;
  fn unsample(&self, p: Point3) -> (f64, f64);
  fn normal_at(&self, u: f64, v: f64) -> Vec3;
  fn tesselate(&self, profile: &Profile) -> Mesh;
  fn flip(&mut self); //XXX use Face::flip_normal instead

  fn tesselate_fixed(&self, u_res: usize, v_res: usize, _profile: &Profile) -> Mesh {
    let mut vertices: Vec<Point3> = vec![];
    let mut vertex_normals: Vec<Vec3> = vec![];
    let mut faces: Vec<usize> = vec![];
    let u_steps = u_res + 1;
    for j in 0..=v_res {
      let v = j as f64 / v_res as f64;
      for i in 0..=u_res {
        let u = i as f64 / u_res as f64;
        let vertex = self.sample(u, v);
        let normal = self.normal_at(u, v);
        vertices.push(vertex);
        vertex_normals.push(normal);
        if j == 0 || i == 0 { continue }
        // Triangle
        faces.push(j * u_steps + (i - 1) );
        faces.push((j - 1) * u_steps + i);
        faces.push(j * u_steps + i);
        // Triangle
        faces.push(j * u_steps + (i - 1) );
        faces.push((j - 1) * u_steps + (i - 1) );
        faces.push((j - 1) * u_steps + i);
      }
    }
    let normals = faces.iter().map(|index| vertex_normals[*index] ).collect();
    Mesh {
      vertices,
      faces,
      normals,
    }
  }
}

impl core::fmt::Debug for dyn Surface {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "Foo")
  }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SurfaceType {
  Planar(Plane),
  Cylindrical(CylindricalSurface),
  Spline(SplineSurface),
}

impl SurfaceType {
  pub fn as_surface(&self) -> &dyn Surface {
    match self {
      Self::Planar(plane) => plane,
      Self::Cylindrical(surf) => surf,
      Self::Spline(surf) => surf,
    }
  }

  pub fn as_surface_mut(&mut self) -> &mut dyn Surface {
    match self {
      Self::Planar(plane) => plane,
      Self::Cylindrical(surf) => surf,
      Self::Spline(surf) => surf,
    }
  }

  pub fn intersect(&self, other: &Self) -> SurfaceIntersection {
    match self {
      // Plane
      SurfaceType::Planar(plane) => match other {
        SurfaceType::Planar(surface) => intersection::plane_plane(plane, surface),
        SurfaceType::Cylindrical(_surface) => SurfaceIntersection::None,
        SurfaceType::Spline(_surface) => SurfaceIntersection::None,
      },

      // CylindricalSurface
      SurfaceType::Cylindrical(_surface) => match other {
        SurfaceType::Planar(_surface) => SurfaceIntersection::None,
        SurfaceType::Cylindrical(_surface) => SurfaceIntersection::None,
        SurfaceType::Spline(_surface) => SurfaceIntersection::None,
      },

      // SplineSurface
      SurfaceType::Spline(_surface) => match other {
        SurfaceType::Planar(_surface) => SurfaceIntersection::None,
        SurfaceType::Cylindrical(_surface) => SurfaceIntersection::None,
        SurfaceType::Spline(_surface) => SurfaceIntersection::None,
      },
    }
  }
}


impl CurveType {
  pub fn intersect_surface(&self, other: &SurfaceType) -> CurveSurfaceIntersection {
    match self {
      // Line
      CurveType::Line(line) => match other {
        SurfaceType::Planar(surface) => intersection::line_plane(line, surface),
        SurfaceType::Cylindrical(_surface) => CurveSurfaceIntersection::None,
        SurfaceType::Spline(_surface) => CurveSurfaceIntersection::None,
      },

      // Arc
      CurveType::Arc(_arc) => match other {
        SurfaceType::Planar(_surface) => CurveSurfaceIntersection::None,
        SurfaceType::Cylindrical(_surface) => CurveSurfaceIntersection::None,
        SurfaceType::Spline(_surface) => CurveSurfaceIntersection::None,
      },

      // Circle
      CurveType::Circle(_circle) => match other {
        SurfaceType::Planar(_surface) => CurveSurfaceIntersection::None,
        SurfaceType::Cylindrical(_surface) => CurveSurfaceIntersection::None,
        SurfaceType::Spline(_surface) => CurveSurfaceIntersection::None,
      },

      // Bezier Spline
      CurveType::Spline(_spline) => match other {
        SurfaceType::Planar(_surface) => CurveSurfaceIntersection::None,
        SurfaceType::Cylindrical(_surface) => CurveSurfaceIntersection::None,
        SurfaceType::Spline(_surface) => CurveSurfaceIntersection::None,
      },
    }
  }
}


#[derive(Debug)]
pub struct TrimmedSurface {
  pub base: SurfaceType,
  pub bounds: Profile,
}

impl TrimmedSurface {
  pub fn new(surf: SurfaceType, outer_bounds: Wire) -> Self {
    Self {
      base: surf,
      bounds: vec![outer_bounds],
    }
  }

  pub fn area(&self) -> f64 {
    0.0 //XXX
  }

  //XXX pub fn on_surface(&self, u: f64, v: f64) -> bool
}

impl Meshable for TrimmedSurface {
  fn tesselate(&self) -> Mesh {
    self.base.as_surface().tesselate(&self.bounds)
  }
}


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
    let pre_v = (p3 - p1).normalize();
    let normal = u.cross(pre_v).normalize();
    Self {
      origin: p1,
      u,
      v: u.cross(normal),
    }
  }

  pub fn from_normal(origin: Point3, normal: Vec3) -> Self {
    let m = geom3d::transform_from_location_and_normal(origin, normal);
    Self {
      origin,
      u: m.transform_vector(Vec3::new(1.0, 0.0, 0.0)),
      v: m.transform_vector(Vec3::new(0.0, 1.0, 0.0)),
    }
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

  pub fn into_enum(self) -> SurfaceType {
    SurfaceType::Planar(self)
  }
}

impl Surface for Plane {
  fn sample(&self, u: f64, v: f64) -> Point3 {
    self.origin + self.u * u + self.v * v
  }

  fn unsample(&self, p: Point3) -> (f64, f64) {
    let p_local = self.as_transform().invert().unwrap().transform_point(p);
    (p_local.x, p_local.y)
  }

  fn normal_at(&self, _u: f64, _v: f64) -> Vec3 {
    self.normal()
  }

  fn tesselate(&self, profile: &Profile) -> Mesh {
    let mut local_profile = profile.clone();
    let trans = self.as_transform();
    let trans_inv = trans.invert().unwrap();
    for wire in &mut local_profile {
      for curve in wire.iter_mut() {
        curve.transform(&trans_inv);
      }
    }
    let mut mesh = geom2d::tesselate_profile(&local_profile, self.normal());
    mesh.transform(&trans);
    mesh
  }

  fn flip(&mut self) {
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


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CylindricalSurface {
  pub plane: Plane,
  pub radius: f64,
  pub height: f64,
  pub bounds: (f64, f64),
}

impl CylindricalSurface {
  pub fn new(radius: f64, height: f64) -> Self {
    Self {
      plane: Plane::default(),
      radius,
      height,
      bounds: (0.0, 1.0),
    }
  }

  pub fn from_axis(origin: Point3, axis: Vec3, radius: f64) -> Self {
    Self {
      plane: Plane::from_normal(origin, axis),
      radius,
      height: axis.magnitude(),
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
    let p = self.plane.sample(u.sin() * self.radius, u.cos() * self.radius);
    p + (self.plane.normal() * self.height * v)
  }

  fn unsample(&self, _p: Point3) -> (f64, f64) {
    todo!()
  }

  fn normal_at(&self, u: f64, _v: f64) -> Vec3 {
    (self.sample(u, 0.0) - self.plane.origin).normalize()
  }

  fn tesselate(&self, profile: &Profile) -> Mesh {
    self.tesselate_fixed(80, 1, profile)
  }

  fn flip(&mut self) {
    self.bounds = (self.bounds.1, self.bounds.0);
  }
}

impl Transformable for CylindricalSurface {
  fn transform(&mut self, transform: &Matrix4) {
    self.plane.transform(transform);
  }
}


#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplineSurface {
  pub degree: (usize, usize),
  pub controls: Vec<Vec<Point3>>,
  pub knots: (Vec<f64>, Vec<f64>),
}

impl SplineSurface {
  pub fn tabulated(spline: &Spline, vec: Vec3) -> Self {
    let mut other_spline = spline.clone();
    other_spline.translate(vec);
    Self {
      degree: (spline.degree, 1),
      controls: vec![
        other_spline.controls,
        spline.controls.clone(),
      ],
      knots: (other_spline.knots, vec![0.0, 0.0, 1.0, 1.0]),
    }
  }

  fn get_basis_function(degree: usize, t: f64, knots: &Vec<f64>) -> Vec<f64> {
    // Remap t to actual curve range
    let low = knots[degree];
    let high = knots[knots.len() - degree - 1];
    let t = low + t * (high - low);
    // Find knot interval that contains t
    let span = (degree..knots.len() - 1).find(|&i| t <= knots[i + 1] ).unwrap();
    let n = knots.len() - 1;
    let mut basis = vec![0.0; n];
    basis[span] = 1.0;
    for k in 1..=degree {
      let base = (span - k).max(0);
      let delta = knots[base + k] - knots[base];
      let max = if span + k < n { span } else { n - k - 1 };
      let mut a = Self::inv_or_zero(delta) * (t - knots[base]);
      for i in base..=max {
        let delta = knots[i + k + 1] - knots[i + 1];
        let b = Self::inv_or_zero(delta) * (knots[i + k + 1] - t);
        basis[i] = a * basis[i] + b * basis[i + 1];
        a = 1.0 - b;
      }
    }
    basis.truncate(n - degree);
    basis
  }

  fn inv_or_zero(delta: f64) -> f64 {
    if delta.almost(0.0) {
      0.0
    } else {
      1.0 / delta
    }
  }

  fn tesselation_steps(&self, degree: usize, num_cvs: usize) -> usize {
    if degree == 1 {
      1
    } else {
      (num_cvs - 1) * 20
    }
  }

  pub fn into_enum(self) -> SurfaceType {
    SurfaceType::Spline(self)
  }
}

impl Surface for SplineSurface {
  fn sample(&self, u: f64, v: f64) -> Point3 {
    let basis_u = Self::get_basis_function(self.degree.0, u, &self.knots.0);
    let basis_v = Self::get_basis_function(self.degree.1, v, &self.knots.1);
    self.controls.iter().zip(&basis_v).fold(Point3::origin(), |acc, (row, bu)| {
      row.iter().zip(&basis_u).fold(acc, |acc, (cv, bv)| {
        acc + cv.to_vec() * (bu * bv)
      })
    })
  }

  fn unsample(&self, _p: Point3) -> (f64, f64) {
    todo!()
  }

  fn normal_at(&self, _u: f64, _v: f64) -> Vec3 {
    Vec3::unit_x()
  }

  fn tesselate(&self, profile: &Profile) -> Mesh {
    self.tesselate_fixed(
      self.tesselation_steps(self.degree.0, self.controls[0].len()),
      self.tesselation_steps(self.degree.1, self.controls.len()),
      profile
    )
  }

  fn flip(&mut self) {}
}

impl Transformable for SplineSurface {
  fn transform(&mut self, transform: &Matrix4) {
    for row in  &mut self.controls {
      for p in row {
        *p = transform.transform_point(*p);
      }
    }
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
  // use crate::test_data::make_generic;
  // use crate::test_data::make_region;

  #[test]
  fn plane_normal() {
    let mut plane = Plane::new();
    assert_eq!(plane.normal(), Vec3::new(0.0, 0.0, 1.0));
    plane.flip();
    assert_eq!(plane.normal(), Vec3::new(0.0, 0.0, -1.0));
  }

  #[test]
  fn cylinder_normal() {
    let cylinder = CylindricalSurface::new(1.0, 1.0);
    almost_eq(cylinder.normal_at(0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
    almost_eq(cylinder.normal_at(0.5, 0.0), Vec3::new(0.0, -1.0, 0.0));
    almost_eq(cylinder.normal_at(0.25, 0.0), Vec3::new(1.0, 0.0, 0.0));
    almost_eq(cylinder.normal_at(0.75, 0.0), Vec3::new(-1.0, 0.0, 0.0));
  }

  #[test]
  fn plane_transform1() {
    let p = Point3::new(0.0, 0.0, 20.0);
    let plane = Plane {
      origin: Point3::new(-7.071067811865475, 7.0710678118654755, 0.0),
      u: Vec3::new(0.0, 0.0, 1.0),
      v: Vec3::new(-0.7071067811865475, 0.7071067811865476, 0.0),
    };
    let trans = plane.as_transform();
    let p = trans.transform_point(p);
    almost_eq(p.z, 0.0);
  }

  #[test]
  fn plane_transform2() {
    let dist = 6.0;
    let vec = Vec3::new(0.4, 0.5, 1.0).normalize() * dist;
    println!("input vector {:#?}", vec);
    let plane = Plane::from_normal(Point3::new(1.0, 2.0, 3.0), vec.normalize());
    let normal = plane.normal() * dist;
    println!("normal {:#?}", normal);
    let gen_normal = plane.as_transform().transform_vector(Vec3::new(0.0, 0.0, dist));
    println!("generated normal {:#?}", gen_normal);
    almost_eq(vec, normal);
    almost_eq(normal, gen_normal);
  }
}
