mod base;
mod curve;
mod surface;
mod solid;
mod mesh;

pub use base::*;
pub use curve::*;
pub use surface::*;
pub use solid::*;
pub use mesh::*;

pub mod geom2d;
pub mod geom3d;
pub mod features;

// #[cfg(test)]
pub mod test_data;
