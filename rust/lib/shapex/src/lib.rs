//! Shapex is an invariant-preserving boundary-representation CAD kernel for Rust.
//!
//! The core crate owns exact geometry, manifold topology, Euler operators,
//! persistent entity identity, transitive operation provenance, tessellation,
//! mass properties, and physical-volume certification. It contains no
//! UI, document model, JavaScript, WebAssembly, or platform binding code.
//!
//! Finished bodies enter a [`Kernel`] only through a transaction that passes both
//! topological and geometric publication checks. Algorithms that are not yet
//! certified return a structured [`KernelError`] instead of publishing a best-effort
//! or physically invalid result.

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "legacy"), doc = include_str!("../README.md"))]

pub mod kernel;
#[cfg(not(feature = "legacy"))]
mod math;

#[cfg(not(feature = "legacy"))]
pub use kernel::{
    Circle2, Compound, CreationRole, Curve2, EdgeHandle, FaceHandle, FilletOptions, Frame3, Line2,
    MassProperties, Plane3, Polyline, Profile, ProfileBoundary, Resolution, Solid, Tessellate,
    TessellationOptions, TriangleMesh, TrimmedCurve2, VertexHandle, Wire2,
};
pub use kernel::{Kernel, KernelError, KernelResult, ModelTolerance};
#[cfg(not(feature = "legacy"))]
pub use math::*;

// The pre-v2 API is isolated behind an explicit compatibility feature while
// downstream application code migrates. It is not part of the standalone kernel.
#[cfg(feature = "legacy")]
mod base;
#[cfg(feature = "legacy")]
mod curve;
#[cfg(feature = "legacy")]
mod internal;
#[cfg(feature = "legacy")]
mod mesh;
#[cfg(feature = "legacy")]
mod solid;
#[cfg(feature = "legacy")]
mod surface;
#[cfg(feature = "legacy")]
mod transform;
#[cfg(feature = "legacy")]
mod wire;

#[cfg(feature = "legacy")]
pub use base::*;
#[cfg(feature = "legacy")]
pub use curve::*;
#[cfg(feature = "legacy")]
pub use mesh::*;
#[cfg(feature = "legacy")]
pub use solid::*;
#[cfg(feature = "legacy")]
pub use surface::*;
#[cfg(feature = "legacy")]
pub use transform::*;
#[cfg(feature = "legacy")]
pub use wire::*;

#[cfg(feature = "legacy")]
#[doc(hidden)]
pub mod geom2d;
#[cfg(feature = "legacy")]
pub mod io;
#[cfg(feature = "legacy")]
#[doc(hidden)]
pub mod test_data;
