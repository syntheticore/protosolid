use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cgmath::prelude::*;

use crate::{Point3, Vec3};

use super::euler::CommittedOperation;
use super::physical::PhysicalCertificate;
use super::{
    BodyId, BooleanKind, ChangeKind, DiagnosticKind, EdgeId, FaceId, Kernel, KernelError,
    KernelResult, MassProperties, ModelTolerance, OperationHistory, Origin, PersistentId, Plane3,
    Profile, RigidTransform3, Surface3, Tessellate, TessellationOptions, TriangleMesh,
};

#[derive(Debug, Clone, Default)]
struct LineageEntry {
    descendants: BTreeSet<PersistentId>,
    changed: bool,
    merged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EntityKind {
    Vertex,
    Edge,
    Face,
}

/// Compact, transitive ancestry for every entity that has contributed to the
/// current result. It stores names and relationships only, never old geometry.
#[derive(Debug, Clone, Default)]
struct Lineage {
    entries: BTreeMap<(EntityKind, PersistentId), LineageEntry>,
}

impl Lineage {
    fn root(current: &BTreeMap<PersistentId, EntityKind>) -> Self {
        Self {
            entries: current
                .iter()
                .map(|(name, kind)| {
                    (
                        (*kind, *name),
                        LineageEntry {
                            descendants: BTreeSet::from([*name]),
                            changed: false,
                            merged: false,
                        },
                    )
                })
                .collect(),
        }
    }

    fn compose(
        parents: impl IntoIterator<Item = Arc<Lineage>>,
        history: &OperationHistory,
        current: &BTreeMap<PersistentId, EntityKind>,
    ) -> Self {
        let parents = parents.into_iter().collect::<Vec<_>>();
        if parents.is_empty() {
            return Self::root(current);
        }

        let generated = generated_index(history);
        let mut entries = BTreeMap::<(EntityKind, PersistentId), LineageEntry>::new();
        for parent in parents {
            for ((kind, ancestor), previous) in &parent.entries {
                let entry = entries.entry((*kind, *ancestor)).or_default();
                entry.changed |= previous.changed;
                entry.merged |= previous.merged;
                for source in &previous.descendants {
                    let direct = direct_descendants(*kind, *source, history, current, &generated);
                    entry.changed |= direct.changed;
                    entry.merged |= direct.merged;
                    entry.descendants.extend(direct.descendants);
                }
            }
        }

        for (current_name, kind) in current {
            entries
                .entry((*kind, *current_name))
                .or_insert_with(|| LineageEntry {
                    descendants: BTreeSet::from([*current_name]),
                    changed: false,
                    merged: false,
                });
        }
        Self { entries }
    }
}

fn generated_index(history: &OperationHistory) -> BTreeMap<PersistentId, BTreeSet<PersistentId>> {
    let mut generated = BTreeMap::<PersistentId, BTreeSet<PersistentId>>::new();
    for (output, origin) in history.origins() {
        if let Origin::Derived { sources, .. } = origin {
            for source in sources {
                generated.entry(*source).or_default().insert(*output);
            }
        }
    }
    generated
}

fn semantic_roots(
    history: &OperationHistory,
    entity: PersistentId,
    visiting: &mut BTreeSet<PersistentId>,
) -> BTreeSet<PersistentId> {
    if !visiting.insert(entity) {
        return BTreeSet::from([entity]);
    }
    let roots = match history.origin(entity) {
        Some(Origin::Derived { sources, .. }) if !sources.is_empty() => sources
            .iter()
            .flat_map(|source| semantic_roots(history, *source, visiting))
            .collect(),
        _ => BTreeSet::from([entity]),
    };
    visiting.remove(&entity);
    roots
}

fn semantic_edge(history: &OperationHistory, edge: &EdgeHandle) -> bool {
    let Ok(faces) = edge.connected_faces() else {
        return true;
    };
    if faces.len() != 2 {
        return true;
    }
    let first = semantic_roots(history, faces[0].persistent_id(), &mut BTreeSet::new());
    let second = semantic_roots(history, faces[1].persistent_id(), &mut BTreeSet::new());
    first.is_disjoint(&second)
}

fn same_curve_support(
    first: &super::Curve3,
    second: &super::Curve3,
    tolerance: ModelTolerance,
) -> bool {
    match (first, second) {
        (super::Curve3::Circle(first), super::Curve3::Circle(second)) => {
            first.center().distance(second.center()) <= tolerance.linear
                && (first.radius() - second.radius()).abs() <= tolerance.linear
                && first
                    .normal()
                    .dot(second.normal())
                    .abs()
                    .clamp(-1.0, 1.0)
                    .acos()
                    <= tolerance.angular
        }
        _ => first == second,
    }
}

fn direct_descendants(
    kind: EntityKind,
    source: PersistentId,
    history: &OperationHistory,
    current: &BTreeMap<PersistentId, EntityKind>,
    generated: &BTreeMap<PersistentId, BTreeSet<PersistentId>>,
) -> LineageEntry {
    if let Some((change_kind, outputs)) = history.change(source) {
        return LineageEntry {
            descendants: outputs
                .iter()
                .copied()
                .filter(|output| current.get(output) == Some(&kind))
                .collect(),
            changed: true,
            merged: change_kind == ChangeKind::Merged,
        };
    }
    if let Some(outputs) = generated.get(&source) {
        let descendants = outputs
            .iter()
            .copied()
            .filter(|output| current.get(output) == Some(&kind))
            .collect::<BTreeSet<_>>();
        if !descendants.is_empty() {
            let merged = descendants.iter().any(|output| {
                matches!(
                    history.origin(*output),
                    Some(Origin::Derived { sources, .. }) if sources.len() > 1
                )
            });
            return LineageEntry {
                descendants,
                changed: true,
                merged,
            };
        }
    }
    LineageEntry {
        descendants: (current.get(&source) == Some(&kind))
            .then_some(source)
            .into_iter()
            .collect(),
        changed: current.get(&source) != Some(&kind),
        merged: false,
    }
}

/// A cloneable face reference tied to an immutable BREP snapshot.
#[derive(Debug, Clone)]
pub struct FaceHandle {
    state: Arc<Kernel>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
    face: FaceId,
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
    pub fn boundary_edges(&self) -> KernelResult<Vec<EdgeHandle>> {
        self.state
            .topology()
            .face_edges(self.face)?
            .into_iter()
            .map(|edge| {
                build_edge_handle(Arc::clone(&self.state), edge, Arc::clone(&self.revisions))
            })
            .collect()
    }
    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<TriangleMesh> {
        <Self as Tessellate>::tessellate(self, options)
    }
    pub(crate) fn normal_at(
        &self,
        point: Point3,
        tolerance: ModelTolerance,
    ) -> KernelResult<crate::Vec3> {
        let projection = self.surface.project(point, tolerance)?;
        let normal = self
            .surface
            .normal(projection.parameters.0, projection.parameters.1)?;
        Ok(match self.state.topology().face(self.face)?.orientation() {
            super::FaceOrientation::Forward => normal,
            super::FaceOrientation::Reversed => -normal,
        })
    }
}

impl PartialEq for FaceHandle {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.revision == other.revision
    }
}
impl Eq for FaceHandle {}

impl Tessellate for FaceHandle {
    type Output = TriangleMesh;

    fn tessellate(&self, options: TessellationOptions) -> KernelResult<Self::Output> {
        self.state.tessellate_face(self.face, options)
    }
}

/// A cloneable edge reference tied to an immutable BREP snapshot.
#[derive(Debug, Clone)]
pub struct EdgeHandle {
    state: Arc<Kernel>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
    edge: EdgeId,
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
    pub fn endpoints(&self) -> KernelResult<[Point3; 2]> {
        let topology = self.state.topology();
        let vertices = topology.edge(self.edge)?.vertices();
        Ok([
            topology.vertex(vertices[0])?.point(),
            topology.vertex(vertices[1])?.point(),
        ])
    }
    pub fn connected_faces(&self) -> KernelResult<Vec<FaceHandle>> {
        self.state
            .topology()
            .edge_faces(self.edge)?
            .into_iter()
            .map(|face| {
                build_face_handle(Arc::clone(&self.state), face, Arc::clone(&self.revisions))
            })
            .collect()
    }
    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<super::Polyline> {
        <Self as Tessellate>::tessellate(self, options)
    }
}

impl PartialEq for EdgeHandle {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.revision == other.revision
    }
}
impl Eq for EdgeHandle {}

impl Tessellate for EdgeHandle {
    type Output = super::Polyline;

    fn tessellate(&self, options: TessellationOptions) -> KernelResult<Self::Output> {
        self.state.tessellate_edge(self.edge, options)
    }
}

/// A cloneable vertex reference tied to an immutable BREP snapshot.
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

/// Resolution of an ancestor entity in the current result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution<T> {
    Preserved(T),
    Modified(T),
    Split(Vec<T>),
    Merged(T),
    Deleted,
    Unrelated,
}

mod sealed {
    pub trait Sealed {}
}

/// Entity types accepted by [`Compound::resolve`] and provenance queries.
#[doc(hidden)]
pub trait ResolvableEntity: sealed::Sealed + Clone {
    fn persistent_name(&self) -> PersistentId;
    fn revision(&self) -> uuid::Uuid;
    fn resolve_in(&self, compound: &Compound) -> Resolution<Self>;
    fn resolve_in_solid(&self, solid: &Solid) -> Resolution<Self>;
}

macro_rules! impl_resolvable {
    ($type:ty, $iterator:ident, $kind:ident) => {
        impl sealed::Sealed for $type {}
        impl ResolvableEntity for $type {
            fn persistent_name(&self) -> PersistentId {
                self.name
            }
            fn revision(&self) -> uuid::Uuid {
                self.revision
            }
            fn resolve_in(&self, compound: &Compound) -> Resolution<Self> {
                resolve_entity_from(
                    &compound.lineage,
                    &compound.revisions,
                    EntityKind::$kind,
                    self.name,
                    self.revision,
                    |name| {
                        compound
                            .$iterator()
                            .find(|entity| entity.name == name)
                            .cloned()
                    },
                )
            }
            fn resolve_in_solid(&self, solid: &Solid) -> Resolution<Self> {
                resolve_entity_from(
                    &solid.lineage,
                    &solid.revisions,
                    EntityKind::$kind,
                    self.name,
                    self.revision,
                    |name| {
                        solid
                            .$iterator()
                            .find(|entity| entity.name == name)
                            .cloned()
                    },
                )
            }
        }
    };
}

impl_resolvable!(FaceHandle, faces, Face);
impl_resolvable!(EdgeHandle, edges, Edge);
impl_resolvable!(VertexHandle, vertices, Vertex);

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
    lineage: Arc<Lineage>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
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
    pub fn resolve<T: ResolvableEntity>(&self, entity: &T) -> Resolution<T> {
        entity.resolve_in_solid(self)
    }
    pub fn origin<T: ResolvableEntity>(&self, entity: &T) -> Option<&Origin> {
        self.history.origin(entity.persistent_name())
    }

    /// Whether an edge separates semantic faces rather than merely subdividing
    /// a faceted construction surface.
    pub fn is_semantic_edge(&self, edge: &EdgeHandle) -> bool {
        self.edges()
            .any(|candidate| candidate.persistent_id() == edge.persistent_id())
            && semantic_edge(&self.history, edge)
    }
    pub fn faces_generated_from<T: ResolvableEntity>(&self, source: &T) -> Vec<FaceHandle> {
        generated_names(&self.history, source.persistent_name())
            .filter_map(|name| self.faces.iter().find(|face| face.name == name).cloned())
            .collect()
    }
    pub fn edges_generated_from<T: ResolvableEntity>(&self, source: &T) -> Vec<EdgeHandle> {
        generated_names(&self.history, source.persistent_name())
            .filter_map(|name| self.edges.iter().find(|edge| edge.name == name).cloned())
            .collect()
    }
    pub fn to_compound(&self) -> Compound {
        Compound {
            state: Arc::clone(&self.state),
            solids: vec![self.clone()],
            history: Arc::clone(&self.history),
            lineage: Arc::clone(&self.lineage),
            revisions: Arc::clone(&self.revisions),
            properties: self.properties,
        }
    }
    pub fn union(&self, other: &Solid) -> KernelResult<Compound> {
        self.to_compound().union(&other.to_compound())
    }
    pub fn difference(&self, other: &Solid) -> KernelResult<Compound> {
        self.to_compound().difference(&other.to_compound())
    }
    pub fn intersection(&self, other: &Solid) -> KernelResult<Compound> {
        self.to_compound().intersection(&other.to_compound())
    }
    pub fn transformed(&self, transform: RigidTransform3) -> KernelResult<Solid> {
        self.to_compound()
            .transformed(transform)?
            .solids
            .into_iter()
            .next()
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "rigidly transforming a solid produced no solid",
                )
            })
    }
    pub fn tessellate(&self, options: TessellationOptions) -> KernelResult<TriangleMesh> {
        <Self as Tessellate>::tessellate(self, options)
    }
}

impl Tessellate for Solid {
    type Output = TriangleMesh;

    fn tessellate(&self, options: TessellationOptions) -> KernelResult<Self::Output> {
        self.state.tessellate_body(self.body, options)
    }
}

/// Immutable result of a modeling operation, containing zero or more solids.
#[derive(Debug, Clone)]
pub struct Compound {
    state: Arc<Kernel>,
    solids: Vec<Solid>,
    history: Arc<OperationHistory>,
    lineage: Arc<Lineage>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
    properties: MassProperties,
}

impl Compound {
    pub(crate) fn from_operation<'a>(
        kernel: Kernel,
        committed: CommittedOperation,
        parents: impl IntoIterator<Item = &'a Compound>,
    ) -> KernelResult<Self> {
        let CommittedOperation {
            operation,
            bodies,
            history,
            certificates,
        } = committed;
        Self::from_parts(kernel, operation, bodies, history, certificates, parents)
    }

    fn from_parts<'a>(
        kernel: Kernel,
        operation: uuid::Uuid,
        bodies: Vec<BodyId>,
        history: OperationHistory,
        certificates: BTreeMap<BodyId, PhysicalCertificate>,
        parents: impl IntoIterator<Item = &'a Compound>,
    ) -> KernelResult<Self> {
        let parents = parents.into_iter().collect::<Vec<_>>();
        let current_entities = body_entities(&kernel, &bodies)?;
        let current_names = current_entities.keys().copied().collect::<BTreeSet<_>>();
        let lineage = Arc::new(Lineage::compose(
            parents.iter().map(|parent| Arc::clone(&parent.lineage)),
            &history,
            &current_entities,
        ));
        let revisions = Arc::new(compose_revisions(
            &parents,
            &history,
            &current_names,
            operation,
        ));
        let state = Arc::new(kernel);
        let history = Arc::new(history);
        let mut solids = Vec::with_capacity(bodies.len());
        for body in bodies {
            solids.push(build_solid(
                Arc::clone(&state),
                body,
                Arc::clone(&history),
                Arc::clone(&lineage),
                Arc::clone(&revisions),
                &certificates,
            )?);
        }
        let properties = combine_properties(solids.iter().map(Solid::mass_properties));
        Ok(Self {
            state,
            solids,
            history,
            lineage,
            revisions,
            properties,
        })
    }

    pub fn solids(&self) -> impl ExactSizeIterator<Item = &Solid> {
        self.solids.iter()
    }
    pub(crate) fn tolerance(&self) -> ModelTolerance {
        self.state.tolerance()
    }
    pub(crate) fn face_name(&self, face: FaceId) -> KernelResult<PersistentId> {
        Ok(self.state.topology().face(face)?.name())
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
        <Self as Tessellate>::tessellate(self, options)
    }

    pub fn resolve<T: ResolvableEntity>(&self, entity: &T) -> Resolution<T> {
        entity.resolve_in(self)
    }

    pub fn origin<T: ResolvableEntity>(&self, entity: &T) -> Option<&Origin> {
        self.history.origin(entity.persistent_name())
    }

    /// Whether an edge separates semantic faces rather than merely subdividing
    /// a faceted construction surface.
    pub fn is_semantic_edge(&self, edge: &EdgeHandle) -> bool {
        self.edges()
            .any(|candidate| candidate.persistent_id() == edge.persistent_id())
            && semantic_edge(&self.history, edge)
    }

    pub fn faces_generated_from<T: ResolvableEntity>(&self, source: &T) -> Vec<FaceHandle> {
        generated_names(&self.history, source.persistent_name())
            .filter_map(|name| self.faces().find(|face| face.name == name).cloned())
            .collect()
    }

    pub fn edges_generated_from<T: ResolvableEntity>(&self, source: &T) -> Vec<EdgeHandle> {
        generated_names(&self.history, source.persistent_name())
            .filter_map(|name| self.edges().find(|edge| edge.name == name).cloned())
            .collect()
    }

    /// Rigidly transform every solid while retaining transitive entity lineage.
    pub fn transformed(&self, transform: RigidTransform3) -> KernelResult<Self> {
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let mut bodies = Vec::with_capacity(self.solids.len());
        for solid in &self.solids {
            bodies.push(
                kernel
                    .topology
                    .import_body(solid.state.topology(), solid.body)?,
            );
        }
        let mut history = OperationHistory::default();
        for body in &bodies {
            history.absorb(kernel.transform_body(*body, transform)?);
        }
        let certificates = certify_bodies(&kernel, &bodies)?;
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            history,
            certificates,
            [self],
        )
    }

    pub fn boolean(&self, other: &Self, kind: BooleanKind) -> KernelResult<Self> {
        ensure_compatible_tolerances(self.state.tolerance(), other.state.tolerance())?;
        if self.solids.len() == 1 && other.solids.len() == 1 {
            return self.boolean_single(other, kind);
        }
        match kind {
            BooleanKind::Union => {
                let mut components = Vec::<Self>::new();
                for solid in self.solids.iter().chain(&other.solids) {
                    let mut candidate = solid.to_compound();
                    let mut index = 0;
                    while index < components.len() {
                        let united =
                            components[index].boolean_single(&candidate, BooleanKind::Union)?;
                        if united.solids.len() == 1 {
                            candidate = united;
                            components.remove(index);
                            index = 0;
                        } else {
                            index += 1;
                        }
                    }
                    components.push(candidate);
                }
                combine_compounds(components, self.state.tolerance())
            }
            BooleanKind::Intersection => {
                let mut results = Vec::new();
                for first in &self.solids {
                    for second in &other.solids {
                        let result = first
                            .to_compound()
                            .boolean_single(&second.to_compound(), BooleanKind::Intersection)?;
                        if !result.solids.is_empty() {
                            results.push(result);
                        }
                    }
                }
                combine_compounds(results, self.state.tolerance())
            }
            BooleanKind::Difference => {
                let mut results = Vec::new();
                for first in &self.solids {
                    let mut pieces = vec![first.to_compound()];
                    for tool in &other.solids {
                        let mut next = Vec::new();
                        for piece in pieces {
                            for piece_solid in &piece.solids {
                                let result = piece_solid
                                    .to_compound()
                                    .boolean_single(&tool.to_compound(), BooleanKind::Difference)?;
                                if !result.solids.is_empty() {
                                    next.push(result);
                                }
                            }
                        }
                        pieces = next;
                    }
                    results.extend(pieces);
                }
                combine_compounds(results, self.state.tolerance())
            }
        }
    }

    fn boolean_single(&self, other: &Self, kind: BooleanKind) -> KernelResult<Self> {
        debug_assert_eq!(self.solids.len(), 1);
        debug_assert_eq!(other.solids.len(), 1);
        let curved = self
            .faces()
            .chain(other.faces())
            .any(|face| !matches!(face.surface(), Surface3::Plane(_)));
        if curved {
            if kind == BooleanKind::Union {
                if let Some(result) = self.union_adjacent_exact(other)? {
                    return Ok(result);
                }
            }
            let first = super::features::facet(self)?;
            let second = super::features::facet(other)?;
            return first.boolean_faceted_single(&second, kind);
        }
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let first = kernel
            .topology
            .import_body(self.state.topology(), self.solids[0].body)?;
        let second = kernel
            .topology
            .import_body(other.state.topology(), other.solids[0].body)?;
        let mut transaction = kernel.begin_operation();
        transaction.boolean(first, second, kind)?;
        let committed = transaction.finish()?;
        Self::from_operation(kernel, committed, [self, other])
    }

    fn boolean_faceted_single(&self, other: &Self, kind: BooleanKind) -> KernelResult<Self> {
        debug_assert_eq!(self.solids.len(), 1);
        debug_assert_eq!(other.solids.len(), 1);
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let first = kernel
            .topology
            .import_body(self.state.topology(), self.solids[0].body)?;
        let second = kernel
            .topology
            .import_body(other.state.topology(), other.solids[0].body)?;
        let mut transaction = kernel.begin_operation();
        transaction.boolean_from_facets(first, second, kind)?;
        let committed = transaction.finish()?;
        Self::from_operation(kernel, committed, [self, other])
    }

    fn union_adjacent_exact(&self, other: &Self) -> KernelResult<Option<Self>> {
        let equal_faces = super::features::coincident_opposite_face_names(self, other)?;
        let patch_faces = if equal_faces.is_none() {
            super::features::coincident_opposite_face_patch_names(self, other)?
        } else {
            None
        };
        let Some((target_is_first, target_name, source_name, whole_face)) = equal_faces
            .map(|(first, second)| (true, first, second, true))
            .or_else(|| patch_faces.map(|(first, target, source)| (first, target, source, false)))
        else {
            return Ok(None);
        };
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let first = kernel
            .topology
            .import_body(self.state.topology(), self.solids[0].body)?;
        let second = kernel
            .topology
            .import_body(other.state.topology(), other.solids[0].body)?;
        let target_face = kernel
            .topology
            .faces()
            .find_map(|(id, face)| (face.name() == target_name).then_some(id))
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::MissingEntity,
                    "imported adjacent union lost its target face",
                )
            })?;
        let source_face = kernel
            .topology
            .faces()
            .find_map(|(id, face)| (face.name() == source_name).then_some(id))
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::MissingEntity,
                    "imported adjacent union lost its source face",
                )
            })?;
        let (target, source) = if target_is_first {
            (first, second)
        } else {
            (second, first)
        };
        let mut transaction = kernel.begin_operation();
        if whole_face {
            transaction.sew_bodies_across_faces(target, source, target_face, source_face)?;
        } else {
            transaction.sew_body_across_face_patch(target, source, target_face, source_face)?;
        }
        let committed = transaction.finish()?;
        Self::from_operation(kernel, committed, [self, other]).map(Some)
    }

    pub fn union(&self, other: &Self) -> KernelResult<Self> {
        self.boolean(other, BooleanKind::Union)
    }
    pub fn difference(&self, other: &Self) -> KernelResult<Self> {
        self.boolean(other, BooleanKind::Difference)
    }
    pub fn intersection(&self, other: &Self) -> KernelResult<Self> {
        self.boolean(other, BooleanKind::Intersection)
    }

    pub fn shell(&self, openings: &[FaceHandle], distance: f64) -> KernelResult<Self> {
        if self.solids.len() > 1 {
            let selected = openings
                .iter()
                .map(FaceHandle::persistent_id)
                .collect::<BTreeSet<_>>();
            let mut results = Vec::new();
            for solid in &self.solids {
                let names = solid
                    .faces()
                    .map(FaceHandle::persistent_id)
                    .collect::<BTreeSet<_>>();
                let local = openings
                    .iter()
                    .filter(|face| names.contains(&face.persistent_id()))
                    .cloned()
                    .collect::<Vec<_>>();
                debug_assert!(local
                    .iter()
                    .all(|face| selected.contains(&face.persistent_id())));
                results.push(solid.to_compound().shell(&local, distance)?);
            }
            return combine_compounds(results, self.state.tolerance());
        }
        super::features::shell(self, openings, distance)
    }

    pub fn fillet(&self, edges: &[EdgeHandle], radius: f64) -> KernelResult<Self> {
        if self.solids.len() > 1 {
            let mut results = Vec::new();
            for solid in &self.solids {
                let names = solid
                    .edges()
                    .map(EdgeHandle::persistent_id)
                    .collect::<BTreeSet<_>>();
                let local = edges
                    .iter()
                    .filter(|edge| names.contains(&edge.persistent_id()))
                    .cloned()
                    .collect::<Vec<_>>();
                if local.is_empty() {
                    results.push(solid.to_compound());
                } else {
                    results.push(solid.to_compound().fillet(&local, radius)?);
                }
            }
            return combine_compounds(results, self.state.tolerance());
        }
        let selected_supports = edges
            .iter()
            .map(|edge| {
                edge.connected_faces().map(|faces| {
                    (
                        edge.curve().clone(),
                        faces
                            .into_iter()
                            .map(|face| face.persistent_id())
                            .collect::<BTreeSet<_>>(),
                    )
                })
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let expanded = self
            .edges()
            .filter_map(|candidate| {
                let faces = candidate
                    .connected_faces()
                    .ok()?
                    .into_iter()
                    .map(|face| face.persistent_id())
                    .collect::<BTreeSet<_>>();
                selected_supports
                    .iter()
                    .any(|(curve, selected_faces)| {
                        same_curve_support(candidate.curve(), curve, self.state.tolerance())
                            && (matches!(
                                (candidate.curve(), curve),
                                (super::Curve3::Circle(_), super::Curve3::Circle(_))
                            ) || &faces == selected_faces)
                    })
                    .then(|| candidate.clone())
            })
            .collect::<Vec<_>>();
        super::features::fillet(self, &expanded, radius)
    }

    pub fn combined(&self, others: &[Self]) -> KernelResult<Self> {
        let parents = std::iter::once(self)
            .chain(others.iter())
            .collect::<Vec<_>>();
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let mut bodies = Vec::new();
        for parent in &parents {
            ensure_compatible_tolerances(self.state.tolerance(), parent.state.tolerance())?;
            for solid in &parent.solids {
                bodies.push(
                    kernel
                        .topology
                        .import_body(solid.state.topology(), solid.body)?,
                );
            }
        }
        let certificates = certify_bodies(&kernel, &bodies)?;
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            OperationHistory::default(),
            certificates,
            parents,
        )
    }

    pub fn split(&self, plane: Plane3, keep: &str) -> KernelResult<Self> {
        if self.solids.len() > 1 {
            let results = self
                .solids
                .iter()
                .map(|solid| solid.to_compound().split(plane, keep))
                .collect::<KernelResult<Vec<_>>>()?;
            return combine_compounds(results, self.state.tolerance());
        }
        super::features::split(self, plane, keep)
    }

    pub fn without_solids(&self, removed: &[Solid]) -> KernelResult<Self> {
        let removed = removed
            .iter()
            .map(|solid| solid.body)
            .collect::<BTreeSet<_>>();
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let mut bodies = Vec::new();
        for solid in &self.solids {
            if !removed.contains(&solid.body) {
                bodies.push(
                    kernel
                        .topology
                        .import_body(solid.state.topology(), solid.body)?,
                );
            }
        }
        let certificates = certify_bodies(&kernel, &bodies)?;
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            OperationHistory::default(),
            certificates,
            [self],
        )
    }

    pub fn move_faces(
        &self,
        faces: &[FaceHandle],
        transform: RigidTransform3,
    ) -> KernelResult<Self> {
        if self.solids.len() > 1 {
            let mut results = Vec::new();
            for solid in &self.solids {
                let names = solid
                    .faces()
                    .map(FaceHandle::persistent_id)
                    .collect::<BTreeSet<_>>();
                let local = faces
                    .iter()
                    .filter(|face| names.contains(&face.persistent_id()))
                    .cloned()
                    .collect::<Vec<_>>();
                if local.is_empty() {
                    results.push(solid.to_compound());
                } else {
                    results.push(solid.to_compound().move_faces(&local, transform)?);
                }
            }
            return combine_compounds(results, self.state.tolerance());
        }
        super::features::move_faces(self, faces, transform)
    }

    pub fn replace_face(
        &self,
        target: &FaceHandle,
        replacement: &FaceHandle,
    ) -> KernelResult<Self> {
        if self.solids.len() > 1 {
            let mut found = false;
            let mut results = Vec::new();
            for solid in &self.solids {
                let contains = solid
                    .faces()
                    .any(|face| face.persistent_id() == target.persistent_id());
                if contains {
                    found = true;
                    results.push(solid.to_compound().replace_face(target, replacement)?);
                } else {
                    results.push(solid.to_compound());
                }
            }
            if !found {
                return Err(KernelError::new(
                    DiagnosticKind::InvalidInput,
                    "replacement target face does not belong to the compound",
                ));
            }
            return combine_compounds(results, self.state.tolerance());
        }
        super::features::replace_face(self, target, replacement)
    }
}

impl Tessellate for Compound {
    type Output = TriangleMesh;

    fn tessellate(&self, options: TessellationOptions) -> KernelResult<Self::Output> {
        let mut mesh = TriangleMesh::default();
        for solid in &self.solids {
            mesh.append(solid.tessellate(options)?);
        }
        Ok(mesh)
    }
}

fn combine_compounds(
    mut compounds: Vec<Compound>,
    tolerance: ModelTolerance,
) -> KernelResult<Compound> {
    if compounds.is_empty() {
        return Compound::from_parts(
            Kernel::new(tolerance)?,
            super::identity::fresh_uuid(),
            Vec::new(),
            OperationHistory::default(),
            BTreeMap::new(),
            [],
        );
    }
    let first = compounds.remove(0);
    first.combined(&compounds)
}

fn resolve_entity_from<T: Clone>(
    lineage_graph: &Lineage,
    revisions: &BTreeMap<PersistentId, uuid::Uuid>,
    kind: EntityKind,
    source: PersistentId,
    revision: uuid::Uuid,
    find: impl Fn(PersistentId) -> Option<T>,
) -> Resolution<T> {
    let Some(lineage) = lineage_graph.entries.get(&(kind, source)) else {
        return Resolution::Unrelated;
    };
    let descendants = lineage
        .descendants
        .iter()
        .filter_map(|output| find(*output))
        .collect::<Vec<_>>();
    match descendants.len() {
        0 => Resolution::Deleted,
        1 if lineage.merged => Resolution::Merged(descendants[0].clone()),
        1 if !lineage.changed && revisions.get(&source).copied() == Some(revision) => {
            Resolution::Preserved(descendants[0].clone())
        }
        1 => Resolution::Modified(descendants[0].clone()),
        _ => Resolution::Split(descendants),
    }
}

fn generated_names(
    history: &OperationHistory,
    source: PersistentId,
) -> impl Iterator<Item = PersistentId> + '_ {
    history
        .origins()
        .iter()
        .filter_map(move |(output, origin)| {
            matches!(origin, Origin::Derived { sources, .. } if sources.contains(&source))
                .then_some(*output)
        })
}

impl Profile {
    /// Extrude this profile into an independently owned, certified compound.
    pub fn extrude(&self, plane: Plane3, vector: Vec3) -> KernelResult<Compound> {
        let mut kernel = Kernel::new(self.tolerance())?;
        let mut transaction = kernel.begin_operation();
        transaction.extrude_curve_profile(plane, self, vector)?;
        let committed = transaction.finish()?;
        Compound::from_operation(kernel, committed, [])
    }
}

fn build_solid(
    state: Arc<Kernel>,
    body: BodyId,
    history: Arc<OperationHistory>,
    lineage: Arc<Lineage>,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
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
        .map(|face| build_face_handle(Arc::clone(&state), face, Arc::clone(&revisions)))
        .collect::<KernelResult<Vec<_>>>()?;
    let edges = edge_ids
        .into_iter()
        .map(|edge| build_edge_handle(Arc::clone(&state), edge, Arc::clone(&revisions)))
        .collect::<KernelResult<Vec<_>>>()?;
    let vertices = vertex_ids
        .into_iter()
        .map(|vertex| {
            let name = topology.vertex(vertex)?.name();
            Ok(VertexHandle {
                name,
                revision: revisions[&name],
                point: topology.vertex(vertex)?.point(),
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
        lineage,
        revisions,
    })
}

fn build_face_handle(
    state: Arc<Kernel>,
    face: FaceId,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
) -> KernelResult<FaceHandle> {
    let topology = state.topology();
    let entity = topology.face(face)?;
    let name = entity.name();
    let surface = entity.surface().clone();
    let revision = revisions[&name];
    Ok(FaceHandle {
        state,
        revisions,
        face,
        name,
        revision,
        surface,
    })
}

fn build_edge_handle(
    state: Arc<Kernel>,
    edge: EdgeId,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
) -> KernelResult<EdgeHandle> {
    let topology = state.topology();
    let entity = topology.edge(edge)?;
    let name = entity.name();
    let curve = entity.curve().clone();
    let revision = revisions[&name];
    Ok(EdgeHandle {
        state,
        revisions,
        edge,
        name,
        revision,
        curve,
    })
}

fn body_entities(
    kernel: &Kernel,
    bodies: &[BodyId],
) -> KernelResult<BTreeMap<PersistentId, EntityKind>> {
    let mut entities = BTreeMap::new();
    for body in bodies {
        for shell in kernel.topology().body(*body)?.shells() {
            for face in kernel.topology().shell(*shell)?.faces() {
                entities.insert(kernel.topology().face(*face)?.name(), EntityKind::Face);
                for edge in kernel.topology().face_edges(*face)? {
                    entities.insert(kernel.topology().edge(edge)?.name(), EntityKind::Edge);
                    for vertex in kernel.topology().edge(edge)?.vertices() {
                        entities
                            .insert(kernel.topology().vertex(vertex)?.name(), EntityKind::Vertex);
                    }
                }
            }
        }
    }
    Ok(entities)
}

fn compose_revisions(
    parents: &[&Compound],
    history: &OperationHistory,
    current: &BTreeSet<PersistentId>,
    operation: uuid::Uuid,
) -> BTreeMap<PersistentId, uuid::Uuid> {
    let generated = generated_index(history);
    current
        .iter()
        .copied()
        .map(|name| {
            let explicitly_changed = history.changes().contains_key(&name)
                || history
                    .changes()
                    .values()
                    .any(|(_, outputs)| outputs.contains(&name))
                || generated.values().any(|outputs| outputs.contains(&name));
            let inherited = (!explicitly_changed)
                .then(|| {
                    parents
                        .iter()
                        .find_map(|parent| parent.revisions.get(&name).copied())
                })
                .flatten();
            (name, inherited.unwrap_or(operation))
        })
        .collect()
}

fn certify_bodies(
    kernel: &Kernel,
    bodies: &[BodyId],
) -> KernelResult<BTreeMap<BodyId, PhysicalCertificate>> {
    let all = super::physical::certify_topology(kernel.topology(), kernel.tolerance())
        .map_err(KernelError::from_diagnostics)?;
    bodies
        .iter()
        .copied()
        .map(|body| {
            all.get(&body)
                .cloned()
                .map(|value| (body, value))
                .ok_or_else(|| {
                    KernelError::new(
                        DiagnosticKind::GeometryInvariant,
                        "published solid is missing its physical certificate",
                    )
                })
        })
        .collect()
}

fn ensure_compatible_tolerances(
    first: super::ModelTolerance,
    second: super::ModelTolerance,
) -> KernelResult<()> {
    if first != second {
        return Err(KernelError::new(
            DiagnosticKind::InvalidInput,
            "modeling operands must use the same tolerance policy",
        ));
    }
    Ok(())
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
