//! The invariant-preserving BREP kernel.
//!
//! This module is independent from document models, user interfaces,
//! serialization transports, and platform bindings. Topology changes are made
//! transactionally through Euler operators and become visible only after complete
//! topological and physical validation.

mod arena;
mod boolean;
mod domain;
mod error;
mod euler;
mod features;
mod geometry;
mod history;
mod identity;
mod integration;
mod intersection;
mod mass_properties;
mod modeling;
mod naming;
mod physical;
mod polyhedron;
mod profile;
mod tessellation;
mod tolerance;
mod topology;
mod transform;
mod validation;
mod wire;

#[cfg(test)]
mod tests;

pub use arena::Handle;
pub use boolean::BooleanKind;
pub use domain::{DomainBounds, ParameterDomain, ParameterRange};
pub use error::{Diagnostic, DiagnosticKind, KernelError, KernelResult};
pub use euler::{
    EulerTransaction, KefResult, KemrResult, KevResult, KfmrhResult, MefInput, MefResult,
    MekrResult, MevInput, MevResult, MfkrhResult, MvfsResult,
};
pub use geometry::{
    Axis3, Circle2, Circle3, Cone3, Curve2, Curve2Evaluation, Curve3, CurveEvaluation, Cylinder3,
    Frame3, Line2, Line3, NurbsCurve2, NurbsCurve3, NurbsSurface3, OffsetSurface3, Plane3,
    Projection1, Projection2, Sphere3, Surface3, SurfaceCurvature, SurfaceEvaluation,
    SurfaceIntersectionCurve, Torus3,
};
pub use history::{ChangeKind, CreationRole, OperationHistory, Origin, ProfileBoundary};
pub use intersection::{
    CurveCurveHit, CurveSurfaceHit, IntersectionKind, IntersectionSet, SurfaceSurfaceCurve,
};
pub use mass_properties::MassProperties;
pub use modeling::{
    Compound, EdgeHandle, FaceHandle, Resolution, ResolvableEntity, Solid, VertexHandle,
};
pub use naming::PersistentId;
pub use polyhedron::PolyhedralMesh;
pub use profile::Profile;
pub use tessellation::{Polyline, Tessellate, TessellationOptions, TriangleMesh};
pub use tolerance::ModelTolerance;
pub use topology::{
    Body, BodyId, Coedge, CoedgeId, Edge, EdgeId, EntityId, Face, FaceId, FaceOrientation, Loop,
    LoopId, LoopKind, Shell, ShellId, ShellKind, Topology, Vertex, VertexId,
};
pub use transform::RigidTransform3;
pub use validation::ValidationReport;
pub use wire::{TrimmedCurve2, Wire2};

/// Owns one mutable BREP state for advanced Euler-level use.
#[derive(Debug)]
pub struct Kernel {
    tolerance: ModelTolerance,
    topology: Topology,
}

impl Kernel {
    pub fn new(tolerance: ModelTolerance) -> KernelResult<Self> {
        tolerance.validate()?;
        Ok(Self {
            tolerance,
            topology: Topology::new(),
        })
    }

    pub fn tolerance(&self) -> ModelTolerance {
        self.tolerance
    }

    pub fn topology(&self) -> &Topology {
        &self.topology
    }

    /// Re-run the complete publication invariant suite.
    pub fn validate(&self) -> ValidationReport {
        validation::validate(
            &self.topology,
            self.tolerance,
            validation::ValidationMode::Published,
        )
    }

    /// Begin an isolated Euler transaction with an automatically generated
    /// operation identity. Dropping it rolls back; committing validates and
    /// atomically publishes it.
    pub fn begin_operation(&mut self) -> EulerTransaction<'_> {
        EulerTransaction::new(self, identity::fresh_uuid())
    }
}
