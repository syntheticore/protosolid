use std::rc::{Rc, Weak};
use std::cell::RefCell;
use std::fmt::Debug;

pub use crate::base::*;


pub type Ref<T> = Rc<RefCell<T>>;
pub type WeakRef<T> = Weak<RefCell<T>>;

pub fn rc<T>(arg: T) -> Rc<RefCell<T>> {
  Rc::new(RefCell::new(arg))
}

pub fn tuple2_to_vec<T>(tuple: (T, T)) -> Vec<T> {
  vec![tuple.0, tuple.1]
}

pub fn almost_eq<T: Almost + Debug + Copy>(first: T, second: T) {
  if !first.almost(second) {
    panic!("\n\n{:?} != {:?}\n\n", first, second);
  }
}

#[macro_export] macro_rules! log {
  ( $( $t:tt )* ) => {
    web_sys::console::log_1(&format!( $( $t )* ).into());
  }
}

#[cfg(feature = "rayon")]
macro_rules! parallel {
  ($a:expr) => { $a.par_iter() }
}

#[cfg(not(feature = "rayon"))]
macro_rules! parallel {
  ($a:expr) => { $a.iter() }
}

pub(crate) use parallel;
