//! STEP (ISO 10303-21, AP214) exchange of BREP solids.

mod fit;
mod parser;
mod reader;
mod writer;

use super::{Compound, KernelResult, ModelTolerance, RigidTransform3};

impl Compound {
    /// Write every solid as one AP214 part in millimetres. `timestamp` is
    /// recorded in the file header.
    pub fn to_step(&self, name: &str, timestamp: &str) -> KernelResult<String> {
        writer::write(self, name, timestamp)
    }

    /// Read every BREP solid of a STEP file, placed by its assembly
    /// structure and converted to millimetres.
    pub fn from_step(text: &str, tolerance: ModelTolerance) -> KernelResult<Self> {
        reader::read(text, tolerance)
    }
}

/// The product structure of a STEP file: parts and assemblies, each defined
/// once and placed by its occurrences in other products.
#[derive(Debug, Clone, Default)]
pub struct StepAssembly {
    pub products: Vec<StepProduct>,
    /// Products that no other product places.
    pub roots: Vec<usize>,
    /// The written length unit; read assemblies are in millimetres.
    pub unit: StepLengthUnit,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StepLengthUnit {
    #[default]
    Millimetre,
    Inch,
}

impl StepLengthUnit {
    fn millimetres(self) -> f64 {
        match self {
            Self::Millimetre => 1.0,
            Self::Inch => 25.4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StepProduct {
    pub name: String,
    /// The product's own solids in its own coordinates.
    pub bodies: Option<Compound>,
    /// RGB surface colours in [0, 1] of the solids in `bodies` order.
    /// Missing entries are uncoloured.
    pub colors: Vec<Option<[f64; 3]>>,
    pub occurrences: Vec<StepOccurrence>,
}

/// A placement of another product in a product's coordinates.
#[derive(Debug, Clone)]
pub struct StepOccurrence {
    pub product: usize,
    pub placement: RigidTransform3,
}

impl StepAssembly {
    /// Write every product once, with its occurrences as AP214 assembly
    /// usages, in the assembly's unit.
    pub fn to_step(&self, timestamp: &str) -> KernelResult<String> {
        writer::write_assembly(self, timestamp)
    }

    /// Read the product structure of a STEP file in millimetres. A file
    /// without products becomes one unnamed root product.
    pub fn from_step(text: &str, tolerance: ModelTolerance) -> KernelResult<Self> {
        reader::read_assembly(text, tolerance)
    }
}

#[cfg(test)]
mod tests;
