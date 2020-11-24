mod base;
mod curve;

pub use base::*;
pub use curve::*;

pub mod surface;
pub mod solid;
pub mod features;
pub mod geom2d;
pub mod geom3d;
pub mod mesh;

// #[cfg(test)]
pub mod test_data;
