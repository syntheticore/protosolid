//! The invariant-preserving BREP kernel.
//!
//! Shapex is independent from document models, user interfaces,
//! serialization transports, and platform bindings. Every topology change is
//! made inside a [`transaction::Transaction`] and becomes visible only after
//! complete topological and physical validation.
//!
//! Operations build topology in one of two ways. Global operations (Booleans,
//! offsets, sweeps, lofts, face edits) produce exact trimmed face patches that
//! `sew` assembles into shells. Primitive construction and local edits (blends,
//! seam relocation, canonicalization) use the Euler operators of `euler` and
//! the low-level edits of `transaction`.

#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod arena;
mod boolean;
mod brep_classification;
mod bounds;
mod canonicalize;
mod domain;
mod edge_blend;
mod error;
mod euler;
mod extrude;
mod features;
mod geometry;
mod history;
mod identity;
mod integration;
mod intersection;
mod linalg;
mod mass_properties;
mod math;
mod modeling;
mod naming;
mod physical;
mod periodic;
mod planar;
mod polygon;
#[cfg(test)]
mod polyhedron;
mod primitives;
mod profile;
mod section;
mod sew;
mod step;
mod sweep;
mod sweep_limits;
mod sweeps;
mod tessellation;
mod tolerance;
mod topology;
mod transaction;
mod trim;
mod transform;
mod validation;
mod wire;

#[cfg(test)]
mod tests;

pub use math::*;
pub use arena::Handle;
pub use boolean::BooleanKind;
pub use domain::{DomainBounds, ParameterDomain, ParameterRange};
pub(crate) use domain::{box_center, nearest_uv, whole_periods};
pub use error::{Diagnostic, DiagnosticKind, KernelError, KernelResult};
pub(crate) use error::{geometry_error, invalid, topology_error, unsupported};
pub use sweep::SweptSurface3;
pub use geometry::{
    Axis3, Circle2, Circle3, Cone3, Curve2, Curve2Evaluation, Curve3, CurveEvaluation, Cylinder3,
    EdgeBlendSurface3, Frame3, Line2, Line3, NurbsCurve2, NurbsCurve3, NurbsSurface3,
    OffsetSurface3, Plane3, Projection1, Projection2, RevolvedSurface3, RollingBallSurface3,
    Sphere3, Surface3,
    SurfaceCurvature, SurfaceCurve3, SurfaceEvaluation, SurfaceIntersectionCurve,
    SurfaceProjectionPcurve2, Torus3,
};
pub use history::{
    BlendRole, ChangeKind, CreationRole, DerivationRole, OperationHistory, Origin,
    ProfileBoundary, SurfaceChart,
};
pub use intersection::{
    CurveCurveHit, CurveSurfaceHit, IntersectionKind, IntersectionSet, SurfaceSurfaceCurve,
};
pub use mass_properties::{MassProperties, SurfaceProperties};
pub use modeling::{
    ChamferMode, Compound, EdgeHandle, FaceHandle, FilletOptions, Resolution, ResolvableEntity,
    Solid, SplitKeep, VertexHandle,
};
pub use naming::PersistentId;
#[cfg(test)]
pub(crate) use polyhedron::PolyhedralMesh;
pub use profile::Profile;
pub use tessellation::{Polyline, Tessellate, TessellationOptions, TriangleMesh};
pub use tolerance::ModelTolerance;
pub use topology::{
    Body, BodyId, Coedge, CoedgeId, Edge, EdgeId, EntityId, Face, FaceId, FaceOrientation, Loop,
    LoopId, LoopKind, Shell, ShellId, ShellKind, Topology, Vertex, VertexId,
};
pub use step::{StepAssembly, StepLengthUnit, StepOccurrence, StepProduct};
pub use transform::RigidTransform3;
pub use validation::ValidationReport;
pub use wire::{TrimmedCurve2, Wire2};
pub use planar::{CurveProjection2, CurveIntersection2, CurveContact, PointClassification, Polyline2, OffsetCurve2};
pub use planar::{Path2, PathOffset2, OffsetOptions, JoinStyle};
pub use planar::{ProjectedCurve2, PlaneCurveProjection};
pub use planar::CurveApproximation2;
pub use planar::ProfileApproximation;
pub use planar::RigidTransform2;

/// Owns one BREP state. Results of modeling operations each own a kernel.
#[derive(Debug)]
pub struct Kernel {
    tolerance: ModelTolerance,
    topology: Topology,
    tessellation_cache: std::sync::Mutex<tessellation::TessellationCache>,
}

impl Kernel {
    pub fn new(tolerance: ModelTolerance) -> KernelResult<Self> {
        tolerance.validate()?;
        Ok(Self {
            tolerance,
            topology: Topology::new(),
            tessellation_cache: Default::default(),
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
        validation::validate(&self.topology, self.tolerance)
    }

    /// Begin an isolated transaction with an automatically generated operation
    /// identity. Dropping it rolls back; finishing validates and atomically
    /// publishes it.
    pub(crate) fn begin_operation(&mut self) -> transaction::Transaction<'_> {
        transaction::Transaction::new(self, identity::fresh_uuid())
    }
}
