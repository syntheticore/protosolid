mod document;
mod component;
mod sketch;
mod feature;
mod references;

pub use document::*;
pub use component::*;
pub use sketch::*;
pub use feature::*;
pub use references::*;

pub mod io;

pub use uuid::Uuid;

pub use shapex;


#[macro_export] macro_rules! log {
  ( $( $t:tt )* ) => {
    web_sys::console::log_1(&format!( $( $t )* ).into());
  }
}
