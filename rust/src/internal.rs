use std::rc::{Rc};
use std::cell::RefCell;


pub fn rc<T>(arg: T) -> Rc<RefCell<T>> {
  Rc::new(RefCell::new(arg))
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

