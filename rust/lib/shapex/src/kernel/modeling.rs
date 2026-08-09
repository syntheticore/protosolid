use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cgmath::prelude::*;

use crate::{Point3, Vec3};

use super::euler::CommittedOperation;
use super::physical::PhysicalCertificate;
use super::{
    BodyId, ChangeKind, DiagnosticKind, Kernel, KernelError, KernelResult, MassProperties,
    OperationHistory, PersistentId, Plane3, Profile, TessellationOptions, TriangleMesh,
};

/// A lightweight, cloneable reference to a face in an immutable BREP snapshot.
#[derive(Debug, Clone)]
pub struct FaceHandle {
    name: PersistentId,
    revision: uuid::Uuid,
    surface: super::Surface3,
}

impl FaceHandle {
    pub fn persistent_id(&self) -> PersistentId {
        self.name
    }
    pub fn surface(&self) -> &super::Surface3 {
        &self.surface
    }
}

impl PartialEq for FaceHandle {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.revision == other.revision
    }
}
impl Eq for FaceHandle {}

/// A lightweight reference to an edge in an immutable BREP snapshot.
#[derive(Debug, Clone)]
pub struct EdgeHandle {
    name: PersistentId,
    revision: uuid::Uuid,
    curve: super::Curve3,
}

impl EdgeHandle {
    pub fn persistent_id(&self) -> PersistentId {
        self.name
    }
    pub fn curve(&self) -> &super::Curve3 {
        &self.curve
    }
}

impl PartialEq for EdgeHandle {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.revision == other.revision
    }
}
impl Eq for EdgeHandle {}

/// A lightweight reference to a vertex in an immutable BREP snapshot.
#[derive(Debug, Clone)]
pub struct VertexHandle {
    name: PersistentId,
    revision: uuid::Uuid,
    point: Point3,
}

impl VertexHandle {
    pub fn persistent_id(&self) -> PersistentId {
        self.name
    }
    pub fn point(&self) -> Point3 {
        self.point
    }
}

impl PartialEq for VertexHandle {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.revision == other.revision
    }
}
impl Eq for VertexHandle {}

/// Resolution of an entity from an input BREP into an operation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution<T> {
    Preserved(T),
    Modified(T),
    Split(Vec<T>),
    Merged(T),
    Deleted,
}

mod sealed {
    pub trait Sealed {}
}

/// Entity types that can be resolved through a compound's topology history.
#[doc(hidden)]
pub trait ResolvableEntity: sealed::Sealed + Clone {
    fn resolve_in(&self, compound: &Compound) -> Resolution<Self>;
}

impl sealed::Sealed for FaceHandle {}
impl ResolvableEntity for FaceHandle {
    fn resolve_in(&self, compound: &Compound) -> Resolution<Self> {
        resolve_entity(compound, self.name, self.revision, |compound, name| {
            compound.faces().find(|face| face.name == name).cloned()
        })
    }
}

impl sealed::Sealed for EdgeHandle {}
impl ResolvableEntity for EdgeHandle {
    fn resolve_in(&self, compound: &Compound) -> Resolution<Self> {
        resolve_entity(compound, self.name, self.revision, |compound, name| {
            compound.edges().find(|edge| edge.name == name).cloned()
        })
    }
}

impl sealed::Sealed for VertexHandle {}
impl ResolvableEntity for VertexHandle {
    fn resolve_in(&self, compound: &Compound) -> Resolution<Self> {
        resolve_entity(compound, self.name, self.revision, |compound, name| {
            compound
                .vertices()
                .find(|vertex| vertex.name == name)
                .cloned()
        })
    }
}

/// One connected, physically certified material volume.
#[derive(Debug, Clone)]
pub struct Solid {
    state: Arc<Kernel>,
    body: BodyId,
    properties: MassProperties,
    faces: Vec<FaceHandle>,
    edges: Vec<EdgeHandle>,
    vertices: Vec<VertexHandle>,
    history: Arc<OperationHistory>,
}

impl Solid {
    pub fn volume(&self) -> f64 {
        self.properties.volume
    }
    pub fn surface_area(&self) -> f64 {
        self.properties.surface_area
    }
    pub fn centroid(&self) -> Point3 {
        self.properties.centroid
    }
    pub fn mass_properties(&self) -> MassProperties {
        self.properties
    }
    pub fn faces(&self) -> impl ExactSizeIterator<Item = &FaceHandle> {
        self.faces.iter()
    }
    pub fn edges(&self) -> impl ExactSizeIterator<Item = &EdgeHandle> {
        self.edges.iter()
    }
    pub fn vertices(&self) -> impl ExactSizeIterator<Item = &VertexHandle> {
        self.vertices.iter()
    }
    pub fn topology_history(&self) -> &OperationHistory {
        &self.history
    }
    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<TriangleMesh> {
        self.state.tessellate_body(self.body, options)
    }
}

/// Immutable result of a modeling operation, containing zero or more solids.
#[derive(Debug, Clone)]
pub struct Compound {
    operation: uuid::Uuid,
    solids: Vec<Solid>,
    history: Arc<OperationHistory>,
    properties: MassProperties,
}

impl Compound {
    pub(crate) fn from_operation(
        kernel: Kernel,
        committed: CommittedOperation,
    ) -> KernelResult<Self> {
        let CommittedOperation {
            operation,
            bodies,
            history,
            certificates,
        } = committed;
        let state = Arc::new(kernel);
        let history = Arc::new(history);
        let mut solids = Vec::with_capacity(bodies.len());
        for body in bodies {
            solids.push(build_solid(
                Arc::clone(&state),
                body,
                operation,
                Arc::clone(&history),
                &certificates,
            )?);
        }
        let properties = combine_properties(solids.iter().map(Solid::mass_properties));
        Ok(Self {
            operation,
            solids,
            history,
            properties,
        })
    }

    pub fn solids(&self) -> impl ExactSizeIterator<Item = &Solid> {
        self.solids.iter()
    }
    pub fn volume(&self) -> f64 {
        self.properties.volume
    }
    pub fn surface_area(&self) -> f64 {
        self.properties.surface_area
    }
    pub fn centroid(&self) -> Point3 {
        self.properties.centroid
    }
    pub fn mass_properties(&self) -> MassProperties {
        self.properties
    }
    pub fn topology_history(&self) -> &OperationHistory {
        &self.history
    }
    pub fn faces(&self) -> impl Iterator<Item = &FaceHandle> {
        self.solids.iter().flat_map(Solid::faces)
    }
    pub fn edges(&self) -> impl Iterator<Item = &EdgeHandle> {
        self.solids.iter().flat_map(Solid::edges)
    }
    pub fn vertices(&self) -> impl Iterator<Item = &VertexHandle> {
        self.solids.iter().flat_map(Solid::vertices)
    }

    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<TriangleMesh> {
        let mut mesh = TriangleMesh::default();
        for solid in &self.solids {
            mesh.append(solid.tessellate(options)?);
        }
        Ok(mesh)
    }

    pub fn resolve<T: ResolvableEntity>(&self, entity: &T) -> Resolution<T> {
        entity.resolve_in(self)
    }
}

fn resolve_entity<T: Clone>(
    compound: &Compound,
    source: PersistentId,
    revision: uuid::Uuid,
    find: impl Fn(&Compound, PersistentId) -> Option<T>,
) -> Resolution<T> {
    if revision == compound.operation {
        return find(compound, source)
            .map(Resolution::Preserved)
            .unwrap_or(Resolution::Deleted);
    }
    if let Some((kind, outputs)) = compound.history.change(source) {
        let descendants = outputs
            .iter()
            .filter_map(|output| find(compound, *output))
            .collect::<Vec<_>>();
        return match kind {
            ChangeKind::Modified => descendants
                .into_iter()
                .next()
                .map(Resolution::Modified)
                .unwrap_or(Resolution::Deleted),
            ChangeKind::Split => {
                if descendants.is_empty() {
                    Resolution::Deleted
                } else {
                    Resolution::Split(descendants)
                }
            }
            ChangeKind::Merged => descendants
                .into_iter()
                .next()
                .map(Resolution::Merged)
                .unwrap_or(Resolution::Deleted),
            ChangeKind::Deleted => Resolution::Deleted,
        };
    }
    find(compound, source)
        .map(Resolution::Modified)
        .unwrap_or(Resolution::Deleted)
}

impl Profile {
    /// Extrude this profile into an independently owned, certified compound.
    /// Operation and topology identities are generated automatically.
    pub fn extrude(&self, plane: Plane3, vector: Vec3) -> KernelResult<Compound> {
        let mut kernel = Kernel::new(self.tolerance())?;
        let mut transaction = kernel.begin_operation();
        transaction.extrude_curve_profile(plane, self, vector)?;
        let committed = transaction.finish()?;
        Compound::from_operation(kernel, committed)
    }
}

fn build_solid(
    state: Arc<Kernel>,
    body: BodyId,
    operation: uuid::Uuid,
    history: Arc<OperationHistory>,
    certificates: &BTreeMap<BodyId, PhysicalCertificate>,
) -> KernelResult<Solid> {
    let properties = certificates
        .get(&body)
        .map(|certificate| certificate.properties)
        .ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::GeometryInvariant,
                "published solid is missing its physical certificate",
            )
        })?;
    let topology = state.topology();
    let mut face_ids = BTreeSet::new();
    let mut edge_ids = BTreeSet::new();
    let mut vertex_ids = BTreeSet::new();
    for shell in topology.body(body)?.shells() {
        for face in topology.shell(*shell)?.faces() {
            face_ids.insert(*face);
            for edge in topology.face_edges(*face)? {
                edge_ids.insert(edge);
                vertex_ids.extend(topology.edge(edge)?.vertices());
            }
        }
    }
    let faces = face_ids
        .into_iter()
        .map(|id| {
            let name = topology.face(id)?.name();
            Ok(FaceHandle {
                name,
                revision: operation,
                surface: topology.face(id)?.surface().clone(),
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let edges = edge_ids
        .into_iter()
        .map(|id| {
            let name = topology.edge(id)?.name();
            Ok(EdgeHandle {
                name,
                revision: operation,
                curve: topology.edge(id)?.curve().clone(),
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    let vertices = vertex_ids
        .into_iter()
        .map(|id| {
            let name = topology.vertex(id)?.name();
            Ok(VertexHandle {
                name,
                revision: operation,
                point: topology.vertex(id)?.point(),
            })
        })
        .collect::<KernelResult<Vec<_>>>()?;
    Ok(Solid {
        state,
        body,
        properties,
        faces,
        edges,
        vertices,
        history,
    })
}

fn combine_properties(properties: impl IntoIterator<Item = MassProperties>) -> MassProperties {
    let properties = properties.into_iter().collect::<Vec<_>>();
    let volume = properties.iter().map(|value| value.volume).sum::<f64>();
    let surface_area = properties
        .iter()
        .map(|value| value.surface_area)
        .sum::<f64>();
    let centroid = if volume > 0.0 {
        Point3::from_vec(
            properties
                .iter()
                .map(|value| value.centroid.to_vec() * value.volume)
                .fold(Vec3::new(0.0, 0.0, 0.0), |sum, value| sum + value)
                / volume,
        )
    } else {
        Point3::new(0.0, 0.0, 0.0)
    };
    let mut inertia = [[0.0; 3]; 3];
    for value in &properties {
        let offset = value.centroid - centroid;
        let squared = offset.magnitude2();
        for (row, inertia_row) in inertia.iter_mut().enumerate() {
            for (column, inertia_value) in inertia_row.iter_mut().enumerate() {
                *inertia_value += value.inertia[row][column]
                    + value.volume
                        * if row == column {
                            squared - [offset.x, offset.y, offset.z][row].powi(2)
                        } else {
                            -[offset.x, offset.y, offset.z][row]
                                * [offset.x, offset.y, offset.z][column]
                        };
            }
        }
    }
    MassProperties {
        volume,
        signed_volume: volume,
        surface_area,
        centroid,
        inertia,
    }
}
