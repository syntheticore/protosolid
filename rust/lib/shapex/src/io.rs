/// STEP file format (ISO 10303). Can store BREP data directly.
pub mod step;

/// STL file format. Stores raw polygonal meshes.
pub mod stl;

/// 3MF file format. Superior successor to STL, optimized for additive manufacturing software.
pub mod threemf;
