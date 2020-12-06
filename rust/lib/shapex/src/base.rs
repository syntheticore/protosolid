use std::rc::{Rc, Weak};
use std::cell::RefCell;

pub use cgmath::prelude::*;

pub type Vec2 = cgmath::Vector2<f64>;
pub type Vec3 = cgmath::Vector3<f64>;
pub type Vec4 = cgmath::Vector4<f64>;
pub type Point2 = cgmath::Point2<f64>;
pub type Point3 = cgmath::Point3<f64>;
pub type Matrix4 = cgmath::Matrix4<f64>;


pub const EPSILON: f64 = core::f64::EPSILON * 1000.0;

pub trait Almost {
  // const EPS: f64 = core::f64::EPSILON * 100000.0;
  fn almost(&self, other: Self) -> bool;
}

impl Almost for Point3 {
  fn almost(&self, other: Self) -> bool {
    (self.x - other.x).abs() < EPSILON &&
    (self.y - other.y).abs() < EPSILON &&
    (self.z - other.z).abs() < EPSILON
  }
}

impl Almost for f64 {
  fn almost(&self, other: Self) -> bool {
    (self - other).abs() < EPSILON
  }
}


pub fn tuple2_to_vec<T>(tuple: (T, T)) -> Vec<T> {
  vec![tuple.0, tuple.1]
}


pub type Ref<T> = Rc<RefCell<T>>;
pub type WeakRef<T> = Weak<RefCell<T>>;

pub fn rc<T>(arg: T) -> Rc<RefCell<T>> {
  Rc::new(RefCell::new(arg))
}
