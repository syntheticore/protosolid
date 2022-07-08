mod base;
mod transform;
mod curve;
mod surface;
mod solid;
mod mesh;

pub use base::*;
pub use transform::*;
pub use curve::*;
pub use surface::*;
pub use solid::*;
pub use mesh::*;

pub mod geom2d;
pub mod geom3d;
pub mod intersection;
pub mod io;

pub use intersection::CurveIntersectionType;
pub use intersection::SurfaceIntersectionType;
pub use intersection::CurveSurfaceIntersectionType;
pub use intersection::CurveIntersection;

// #[cfg(test)]
pub mod test_data;
