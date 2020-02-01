// extern crate num;

use std;
use std::ops;
// use std::cmp;

#[derive(Copy, Clone, Debug, Default)]
pub struct Vec3 {
  pub x: f64,
  pub y: f64,
  pub z: f64
}

impl ops::Add for Vec3 {
  type Output = Self;

  fn add(self, other: Self) -> Self {
    Self {
      x: self.x + other.x,
      y: self.y + other.y,
      z: self.z + other.z
    }
  }
}

impl ops::Sub for Vec3 {
  type Output = Self;

  fn sub(self, other: Self) -> Self {
    Self {
      x: self.x - other.x,
      y: self.y - other.y,
      z: self.z - other.z
    }
  }
}

impl ops::Mul<f64> for Vec3 {
  type Output = Self;

  fn mul(self, scalar: f64) -> Self {
    Self {
      x: self.x * scalar,
      y: self.y * scalar,
      z: self.z * scalar
    }
  }
}

impl ops::Div<f64> for Vec3 {
  type Output = Self;

  fn div(self, scalar: f64) -> Self {
    self * (1.0 / scalar)
  }
}

impl Vec3 {
  pub fn new(x: f64, y: f64, z: f64) -> Self {
    Self {
      x: x,
      y: y,
      z: z
    }
  }

  pub fn origin() -> Self {
    Self {x: 0.0, y: 0.0, z: 0.0}
  }

  pub fn equals(self, other: Self) -> bool {
    (self.x - other.x).abs() <= std::f64::EPSILON &&
    (self.y - other.y).abs() <= std::f64::EPSILON &&
    (self.z - other.z).abs() <= std::f64::EPSILON
  }

  pub fn cross(&self, _other: &Self) -> Self {
    Self {
      x: 0.0,
      y: 0.0,
      z: 0.0
    }
  }

  pub fn dot(&self, _other: &Self) -> Self {
    Self {
      x: 0.0,
      y: 0.0,
      z: 0.0
    }
  }

  pub fn angle(&self, _other: &Self) -> f64{
    0.0
  }

  pub fn distance(self, other: Self) -> f64{
    (other - self).length()
  }

  pub fn length_squared(&self) -> f64 {
    self.x.powf(2.0) + self.y.powf(2.0) + self.z.powf(2.0)
  }

  pub fn length(&self) -> f64 {
    self.length_squared().sqrt()
  }

  pub fn invert(&self) -> Self {
    Self {
      x: -self.x,
      y: -self.y,
      z: -self.z
    }
  }

  pub fn normalize(self) -> Self {
    self / self.length()
  }

  pub fn project_on_vector(&self, _other: &Self) {

  }

  pub fn project_on_plane(&self, _normal: &Self) {

  }

  pub fn reflect(&self, _normal: &Self) {

  }

  pub fn lerp(self, other: Self, alpha: f64) -> Self {
    self + (other - self) * alpha
  }
}
