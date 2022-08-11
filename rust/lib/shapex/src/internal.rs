use std::rc::{Rc};
use std::cell::RefCell;

pub use crate::base::*;


pub fn rc<T>(arg: T) -> Rc<RefCell<T>> {
  Rc::new(RefCell::new(arg))
}

pub fn tuple2_to_vec<T>(tuple: (T, T)) -> Vec<T> {
  vec![tuple.0, tuple.1]
}

pub fn sort_tuple2<T: PartialOrd>(from: T, to: T) -> (T, T) {
  if from <= to {
    (from, to)
  } else {
    (to, from)
  }
}

pub fn is_between(value: f64, start: f64, end: f64) -> bool {
  (start - EPSILON <= value && value <= end + EPSILON) || (end - EPSILON <= value && value <= start + EPSILON)
}


#[allow(unused_macros)]
macro_rules! almost_eq {
  ($a:expr, $b:expr) => {
    if !$a.almost($b) {
      panic!("\n\n{:?} != {:?}\n\n", $a, $b);
    }
  }
}

#[allow(unused_imports)]
pub(crate) use almost_eq;


#[allow(unused_macros)]
macro_rules! log {
  ( $( $t:tt )* ) => {
    web_sys::console::log_1(&format!( $( $t )* ).into());
  }
}

#[allow(unused_imports)]
pub(crate) use log;


#[cfg(feature = "rayon")]
macro_rules! parallel {
  ($a:expr) => { $a.par_iter() }
}

#[cfg(not(feature = "rayon"))]
macro_rules! parallel {
  ($a:expr) => { $a.iter() }
}

pub(crate) use parallel;
