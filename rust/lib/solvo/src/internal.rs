use std::rc::{Rc};
use std::cell::RefCell;


pub fn rc<T>(arg: T) -> Rc<RefCell<T>> {
  Rc::new(RefCell::new(arg))
}


pub fn tuple2_to_vec<T>(tuple: (T, T)) -> Vec<T> {
  vec![tuple.0, tuple.1]
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

