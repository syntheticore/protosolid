//! Geometric modeling kernel.
//!
//! This crate allows for the creation, analysis and modification of parametric curves and surfaces, as well as manifold solid bodies.
//!
//! Solids are represented as a topological model of their bounding surfaces, known as boundary representation (BREP).
//! Shapex vaguely follows the implementation as described in the 1988's book 'An introduction to solid modeling' by Martti Mäntylä.
//! Specifically, topology modifications are accomplished using local operations called Euler operators, which are guaranteed to
//! maintain valid topology across a series of changes. This implies that non-manifold bodies cannot be represented using this model
//! and need to be avoided during intermediate modeling operations.
//!
//! Additionally, geometric conditions get checked and repaired to ensure they match a model's topology, such that operations always result in a
//! watertight, manufacturable volume. This way, models can be checked for physical properties, such as volume and weight.
//! All geometry types can be intersected with each other, yielding the most appropriate intersection geometry for each specific case.
//!
//! Shapex also includes a set of common high level modeling operations, typically used in CAD software.
//! This includes extrusion, revolution, loft, sweep, shelling, fillets and chamfers, as well as boolean set operations.
//!
//!
//! # Examples
//!
//! Extruding a cylinder from a circle and tessellating it for display:
//! ```
//! use shapex::*;
//!
//! fn cylinder_mesh(radius: f64, height: f64) -> Mesh {
//!   // Create circle and wrap it in a [CurveType] enum,
//!   // in order to pass it around as a generic curve.
//!   let circle = Circle::new(Point3::origin(), radius).into_enum();
//!
//!   // Wires combine curves to form a closed loop.
//!   // In this case, a single circle will suffice to close the wire.
//!   let curve = TrimmedCurve::new(circle);
//!   let wire = Wire::new(vec![curve]).unwrap();
//!
//!   // Profiles use coplanar wires to form enclosed regions.
//!   // Additional wires could be used to describe holes in the shape.
//!   let profile = Profile::new(Plane::new(), vec![wire]);
//!
//!   // Extrude profile to create solid body.
//!   let solid = features::extrude(&profile, height).unwrap();
//!   Ok(solid.tesselate())
//! }
//! ```

mod base;
mod transform;
mod curve;
mod surface;
mod solid;
mod mesh;
mod wire;

pub use base::*;
pub use transform::*;
pub use curve::*;
pub use surface::*;
pub use solid::*;
pub use mesh::*;
pub use wire::*;

/// Functions applicable to two-dimensional geometry only
#[doc(hidden)]
pub mod geom2d;

/// Serialization and import/export of common file formats
pub mod io;

#[doc(hidden)]
pub mod internal;

// #[cfg(test)]
#[doc(hidden)]
pub mod test_data;
