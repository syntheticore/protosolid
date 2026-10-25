use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use cgmath::prelude::*;

use crate::{Point3, Vec3};

use super::edge_blend::{BlendOutcome, BlendStaging};
use super::transaction::CommittedOperation;
use super::physical::PhysicalCertificate;
use super::{
    BodyId, BooleanKind, ChangeKind, DerivationRole, EdgeId, EntityId, FaceId, Kernel, KernelError,
    KernelResult, MassProperties, ModelTolerance, OperationHistory, Origin, PersistentId, Plane3,
    Profile, Reflection3, RigidTransform3, Tessellate, TessellationOptions, Topology, TriangleMesh,
    invalid,
};
use crate::{geometry_error, topology_error, unsupported};

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
    entries: BTreeMap<(EntityKind, PersistentId, uuid::Uuid), LineageEntry>,
}

impl Lineage {
    fn root(
        current: &BTreeMap<PersistentId, EntityKind>,
        revisions: &BTreeMap<PersistentId, uuid::Uuid>,
    ) -> Self {
        Self {
            entries: current
                .iter()
                .map(|(name, kind)| {
                    (
                        (*kind, *name, revisions[name]),
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
        revisions: &BTreeMap<PersistentId, uuid::Uuid>,
    ) -> Self {
        let parents = parents.into_iter().collect::<Vec<_>>();
        if parents.is_empty() {
            return Self::root(current, revisions);
        }

        let generated = generated_index(history);
        let mut entries = BTreeMap::<(EntityKind, PersistentId, uuid::Uuid), LineageEntry>::new();
        for parent in parents {
            for ((kind, ancestor, revision), previous) in &parent.entries {
                let entry = entries.entry((*kind, *ancestor, *revision)).or_default();
                for source in &previous.descendants {
                    let direct = direct_descendants(*kind, *source, history, current, &generated);
                    // Per-solid operations share the compound's ancestry. A
                    // branch without this entity must not mark a surviving
                    // sibling branch modified when the results are combined.
                    if direct.descendants.is_empty() {
                        continue;
                    }
                    entry.changed |= previous.changed || direct.changed;
                    entry.merged |= previous.merged || direct.merged;
                    entry.descendants.extend(direct.descendants);
                }
            }
        }

        for (current_name, kind) in current {
            entries.insert(
                (*kind, *current_name, revisions[current_name]),
                LineageEntry {
                    descendants: BTreeSet::from([*current_name]),
                    changed: false,
                    merged: false,
                },
            );
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

fn semantic_edge(edge: &EdgeHandle) -> bool {
    let Ok(faces) = edge.connected_faces() else {
        return true;
    };
    // Visibility follows geometry, independent of the operation ancestry.
    // One-face periodic seams and subdivisions of the same support stay hidden.
    match faces.as_slice() {
        [_] => false,
        [first, second] => {
            !same_surface_support(first.surface(), second.surface(), edge.state.tolerance())
        }
        _ => true,
    }
}

fn same_surface_support(
    first: &super::Surface3,
    second: &super::Surface3,
    tolerance: ModelTolerance,
) -> bool {
    match (first, second) {
        (super::Surface3::Plane(first), super::Surface3::Plane(second)) => {
            let first_normal = first.normal();
            let second_normal = second.normal();
            first_normal.cross(second_normal).magnitude() <= tolerance.angular
                && (second.origin() - first.origin()).dot(first_normal).abs() <= tolerance.linear
        }
        (super::Surface3::Cylinder(first), super::Surface3::Cylinder(second)) => {
            let first_axis = first.axis().normalize();
            let second_axis = second.axis().normalize();
            first_axis.cross(second_axis).magnitude() <= tolerance.angular
                && (second.origin() - first.origin())
                    .cross(first_axis)
                    .magnitude()
                    <= tolerance.linear
                && (first.radius() - second.radius()).abs() <= tolerance.linear
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
            changed: change_kind != ChangeKind::Unchanged,
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
    surface_properties: Arc<OnceLock<KernelResult<super::SurfaceProperties>>>,
}

impl FaceHandle {
    pub fn persistent_id(&self) -> PersistentId {
        self.name
    }
    // A published kernel is never mutated, so its faces outlive every handle.
    fn entity(&self) -> &super::Face {
        self.state.topology().face(self.face).expect("handle references its own immutable snapshot")
    }
    pub fn surface(&self) -> &super::Surface3 {
        self.entity().surface()
    }
    pub fn orientation(&self) -> super::FaceOrientation {
        self.entity().orientation()
    }
    pub fn surface_properties(&self) -> KernelResult<super::SurfaceProperties> {
        self.surface_properties.get_or_init(|| {
            super::physical::face_surface_properties(self.state.topology(), self.face, self.state.tolerance())
        }).clone()
    }
    pub fn area(&self) -> KernelResult<f64> {
        self.surface_properties().map(|properties| properties.area)
    }
    pub fn centroid(&self) -> KernelResult<Point3> {
        self.surface_properties().map(|properties| properties.centroid)
    }
    pub(crate) fn topology_id(&self) -> FaceId {
        self.face
    }
    pub(crate) fn parameter_bounds(&self) -> KernelResult<[super::ParameterRange; 2]> {
        let boundary = super::trim::face_boundary_curves(self.state.topology(), self.face, self.state.tolerance())?;
        super::trim::face_parameter_bounds(&boundary, self.state.tolerance())
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
}

impl EdgeHandle {
    pub fn persistent_id(&self) -> PersistentId {
        self.name
    }
    pub fn curve(&self) -> &super::Curve3 {
        self.state.topology().edge(self.edge).expect("handle references its own immutable snapshot").curve()
    }
    pub fn range(&self) -> KernelResult<super::ParameterRange> {
        Ok(self.state.topology().edge(self.edge)?.range())
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

/// How a shell's offset faces meet at its edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellJoin {
    /// Extend the offset supports until they intersect.
    Sharp,
    /// Join the offset supports with blends of the shell thickness.
    Rounded,
}

mod sealed {
    pub trait Sealed {}
}

/// Entity types accepted by [`Compound::resolve`] and provenance queries.
#[doc(hidden)]
pub trait ResolvableEntity: sealed::Sealed + Clone + PartialEq {
    fn persistent_name(&self) -> PersistentId;
    fn revision(&self) -> uuid::Uuid;
    fn resolve_in(&self, compound: &Compound) -> Resolution<Self>;
    fn resolve_in_solid(&self, solid: &Solid) -> Resolution<Self>;
    /// The solid's handles of this entity kind by persistent name.
    fn index_in(solid: &Solid) -> BTreeMap<PersistentId, &Self>;
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
            fn index_in(solid: &Solid) -> BTreeMap<PersistentId, &Self> {
                solid.$iterator().map(|entity| (entity.name, entity)).collect()
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

/// The side of a split plane whose material is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitKeep {
    /// Material opposite the plane normal.
    Left,
    /// Material along the plane normal.
    Right,
    Both,
}

/// The second measurement of an asymmetric chamfer; angles are in radians.
#[derive(Debug, Clone, Copy)]
pub enum ChamferMode {
    TwoDistances(f64),
    DistanceAngle(f64),
}

#[derive(Clone)]
pub(crate) struct ChamferSettings {
    pub mode: ChamferMode,
    pub reference_faces: BTreeMap<PersistentId, PersistentId>,
}

/// Controls rolling-ball edge fillet construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilletOptions {
    pub radius: f64,
    pub follow_tangent_edges: bool,
}

impl FilletOptions {
    pub fn new(radius: f64) -> Self {
        Self {
            radius,
            follow_tangent_edges: true,
        }
    }

    pub fn with_tangent_propagation(mut self, enabled: bool) -> Self {
        self.follow_tangent_edges = enabled;
        self
    }
}

impl Solid {
    pub fn persistent_id(&self) -> PersistentId {
        self.state.topology().body(self.body).unwrap().name()
    }
    pub(crate) fn topology_id(&self) -> BodyId {
        self.body
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

    /// Whether an edge separates distinct supporting surfaces.
    pub fn is_semantic_edge(&self, edge: &EdgeHandle) -> bool {
        self.edges()
            .any(|candidate| candidate.persistent_id() == edge.persistent_id())
            && semantic_edge(edge)
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
                geometry_error("rigidly transforming a solid produced no solid")
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
        kernel: impl Into<Arc<Kernel>>,
        operation: uuid::Uuid,
        bodies: Vec<BodyId>,
        history: OperationHistory,
        certificates: BTreeMap<BodyId, PhysicalCertificate>,
        parents: impl IntoIterator<Item = &'a Compound>,
    ) -> KernelResult<Self> {
        let kernel = kernel.into();
        let parents = parents.into_iter().collect::<Vec<_>>();
        let current_entities = body_entities(&kernel, &bodies)?;
        let current_names = current_entities.keys().copied().collect::<BTreeSet<_>>();
        let revisions = Arc::new(compose_revisions(
            &parents,
            &history,
            &current_names,
            operation,
        )?);
        let lineage = Arc::new(Lineage::compose(
            parents.iter().map(|parent| Arc::clone(&parent.lineage)),
            &history,
            &current_entities,
            &revisions,
        ));
        let state = kernel;
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
    pub(crate) fn topology(&self) -> &Topology {
        self.state.topology()
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

    /// Generator roles follow authored provenance through patch welding.
    pub fn creation_roles<T: ResolvableEntity>(&self, entity: &T) -> Vec<super::CreationRole> {
        let mut pending = vec![entity.persistent_name()];
        let mut visited = BTreeSet::new();
        let mut roles = Vec::new();
        while let Some(name) = pending.pop() {
            if !visited.insert(name) { continue; }
            if let Some(authored) = self.history.construction_roles.get(&name) {
                for role in authored {
                    if !roles.contains(role) { roles.push(role.clone()); }
                }
                continue;
            }
            match self.history.origin(name) {
                Some(Origin::Created { role: Some(role) })
                    if !roles.contains(role) => { roles.push(role.clone()); }
                Some(Origin::Derived { sources, .. }) => pending.extend(sources),
                _ => {},
            }
        }
        roles
    }

    pub(crate) fn with_construction_roles_from(self, tools: &[&Self], limits: &[(PersistentId, super::CreationRole)]) -> Self {
        let mut roles = BTreeMap::<PersistentId, Vec<super::CreationRole>>::new();
        for tool in tools {
            for face in tool.faces() {
                let source_roles = tool.creation_roles(face);
                let outputs = match self.resolve(face) {
                    Resolution::Preserved(face) | Resolution::Modified(face) | Resolution::Merged(face) => vec![face],
                    Resolution::Split(faces) => faces,
                    _ => Vec::new(),
                };
                if source_roles.is_empty() { continue; }
                for output in outputs {
                    let target = roles.entry(output.persistent_id()).or_default();
                    for role in &source_roles {
                        if !target.contains(role) { target.push(role.clone()); }
                    }
                }
            }
        }
        for (face, role) in limits { roles.insert(*face, vec![role.clone()]); }
        self.edit_history(|history| history.construction_roles.extend(roles))
    }

    /// Amend the operation history shared by this compound and its solids.
    fn edit_history(mut self, edit: impl FnOnce(&mut OperationHistory)) -> Self {
        edit(Arc::make_mut(&mut self.history));
        for solid in &mut self.solids {
            solid.history = Arc::clone(&self.history);
        }
        self
    }

    /// Whether an edge separates distinct supporting surfaces.
    pub fn is_semantic_edge(&self, edge: &EdgeHandle) -> bool {
        self.edges()
            .any(|candidate| candidate.persistent_id() == edge.persistent_id())
            && semantic_edge(edge)
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
        if transform == RigidTransform3::identity() {
            return self.unchanged();
        }
        self.mapped(|kernel, body| kernel.transform_body(body, transform))
    }

    /// Mirror every solid across a plane while retaining transitive entity lineage.
    pub fn mirrored(&self, plane: Plane3) -> KernelResult<Self> {
        let reflection = Reflection3::new(plane);
        self.mapped(|kernel, body| kernel.reflect_body(body, reflection))
    }

    fn mapped(
        &self,
        map: impl Fn(&mut Kernel, BodyId) -> KernelResult<OperationHistory>,
    ) -> KernelResult<Self> {
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
            map(&mut kernel, *body)?;
            history.absorb(kernel.topology.reidentify_body(*body, super::identity::fresh_uuid())?);
        }
        let certificates = certify_bodies(&kernel, &bodies)?;
        let result = Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            history,
            certificates,
            [self],
        )?;
        Ok(result.with_construction_roles_from(&[self], &[]))
    }

    pub fn boolean(&self, other: &Self, kind: BooleanKind) -> KernelResult<Self> {
        ensure_compatible_tolerances(self.state.tolerance(), other.state.tolerance())?;
        if self.solids.len() == 1 && other.solids.len() == 1 {
            return self.boolean_single(other, kind);
        }
        match kind {
            BooleanKind::Union => self.union_components_iterative(other),
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

    fn union_components_iterative(&self, other: &Self) -> KernelResult<Self> {
        let mut components = Vec::<Self>::new();
        for solid in self.solids.iter().chain(&other.solids) {
            let mut candidate = solid.to_compound();
            let mut index = 0;
            while index < components.len() {
                let united = components[index].boolean_single(&candidate, BooleanKind::Union)?;
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

    fn boolean_single(&self, other: &Self, kind: BooleanKind) -> KernelResult<Self> {
        debug_assert_eq!(self.solids.len(), 1);
        debug_assert_eq!(other.solids.len(), 1);
        // Bounds are an optional shortcut here. Unsupported enclosures must
        // still go through the exact Boolean, not make the operation fail.
        let separated = match (super::bounds::compound(self), super::bounds::compound(other)) {
            (Ok(first), Ok(second)) => first.separated(second, self.tolerance().linear * 8.0),
            _ => false,
        };
        if separated {
            let result = match kind {
                BooleanKind::Union => {
                    combine_compounds(vec![self.clone(), other.clone()], self.tolerance())?
                }
                BooleanKind::Difference => self.clone(),
                BooleanKind::Intersection => combine_compounds(Vec::new(), self.tolerance())?,
            };
            return result.with_operation_parents(&[self, other], DerivationRole::Boolean(kind));
        }
        super::boolean::boolean_brep(self, other, kind)
    }

    /// Publish a committed operation attributed to the compounds it consumed.
    pub(crate) fn from_operation_with_parents(
        kernel: Kernel,
        committed: CommittedOperation,
        parents: &[&Compound],
        role: DerivationRole,
    ) -> KernelResult<Self> {
        let CommittedOperation {
            operation,
            bodies,
            history,
            certificates,
        } = committed;
        let history = attribute_to_parents(&kernel, &bodies, &history, parents, role)?;
        Self::from_parts(kernel, operation, bodies, history, certificates, parents.iter().copied())
    }

    /// Attribute an already published compound to `parents`, for results built
    /// without a transaction.
    pub(crate) fn with_operation_parents(&self, parents: &[&Self], role: DerivationRole) -> KernelResult<Self> {
        let bodies = self
            .solids
            .iter()
            .map(|solid| solid.body)
            .collect::<Vec<_>>();
        let history = attribute_to_parents(&self.state, &bodies, &self.history, parents, role)?;
        let certificates = bodies
            .iter()
            .copied()
            .zip(self.solids.iter().map(|solid| PhysicalCertificate {
                properties: solid.mass_properties(),
            }))
            .collect();
        Self::from_parts(
            Arc::clone(&self.state),
            super::identity::fresh_uuid(),
            bodies,
            history,
            certificates,
            parents.iter().copied(),
        )
    }

    pub(crate) fn with_creation_roles(
        self,
        roles: impl IntoIterator<Item = (PersistentId, super::CreationRole)>,
    ) -> Self {
        self.edit_history(|history| {
            history
                .construction_roles
                .extend(roles.into_iter().map(|(name, role)| (name, vec![role])));
        })
    }

    pub(crate) fn with_patch_roles(self, tokens: &BTreeMap<PersistentId, super::CreationRole>) -> Self {
        self.edit_history(|history| history.record_patch_roles(tokens))
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

    /// Group `selection` by owning solid as that solid's own handles. Rejects a
    /// selection that is empty (unless allowed), repeats an entity, or names
    /// entities outside this compound revision with `message`.
    fn partition_selection<E: ResolvableEntity>(
        &self,
        selection: &[E],
        allow_empty: bool,
        message: &str,
    ) -> KernelResult<Vec<Vec<E>>> {
        let mut index = BTreeMap::new();
        for (position, solid) in self.solids.iter().enumerate() {
            index.extend(E::index_in(solid).into_iter().map(|(name, own)| (name, (position, own))));
        }
        let mut groups = vec![Vec::new(); self.solids.len()];
        let mut seen = BTreeSet::new();
        for entity in selection {
            let name = entity.persistent_name();
            match index.get(&name) {
                Some((position, own)) if **own == *entity && seen.insert(name) => {
                    groups[*position].push((*own).clone());
                }
                _ => return Err(invalid(message)),
            }
        }
        if selection.is_empty() && !allow_empty {
            return Err(invalid(message));
        }
        Ok(groups)
    }

    /// Run `operation` on every solid with its group of the selection. With
    /// `skip_unselected`, solids owning none of the selection pass through.
    /// A single solid keeps the operation's own history instead of a combination.
    fn per_solid<E: ResolvableEntity>(
        &self,
        groups: Vec<Vec<E>>,
        skip_unselected: bool,
        operation: impl Fn(&Compound, &[E]) -> KernelResult<Compound>,
    ) -> KernelResult<Compound> {
        if self.solids.len() == 1 {
            return operation(self, &groups[0]);
        }
        let mut results = Vec::with_capacity(self.solids.len());
        for (solid, local) in self.solids.iter().zip(groups) {
            let body = solid.to_compound();
            results.push(if skip_unselected && local.is_empty() {
                body
            } else {
                operation(&body, &local)?
            });
        }
        combine_compounds(results, self.tolerance())
    }

    /// Remove a feature and extend its surviving support surfaces to close it.
    pub fn dissolve_faces(&self, faces: &[FaceHandle]) -> KernelResult<Self> {
        let groups = self.partition_selection(faces, false, "dissolve needs distinct faces from this compound revision")?;
        self.per_solid(groups, true, super::features::dissolve_faces)
    }

    /// Offset selected faces along their outward normals, extending adjacent
    /// supports and adjusting tangent blends to keep each solid closed.
    pub fn offset_faces(&self, faces: &[FaceHandle], distance: f64) -> KernelResult<Self> {
        if !distance.is_finite() {
            return Err(invalid("offset distance must be finite"));
        }
        let groups = self.partition_selection(faces, false, "offset needs distinct faces from this compound revision")?;
        if distance == 0.0 {
            return self.unchanged();
        }
        if distance.abs() <= self.tolerance().linear {
            return Err(invalid("offset distance must exceed model tolerance"));
        }
        self.per_solid(groups, true, |body, local| super::features::offset_faces(body, local, distance))
    }

    pub fn shell(&self, openings: &[FaceHandle], distance: f64) -> KernelResult<Self> {
        self.shell_with_join(openings, distance, ShellJoin::Rounded)
    }

    /// Offset a skin with either intersected (sharp) or thickness-radius joins.
    pub fn shell_with_join(
        &self,
        openings: &[FaceHandle],
        distance: f64,
        join: ShellJoin,
    ) -> KernelResult<Self> {
        if !distance.is_finite() || distance.abs() <= self.tolerance().linear {
            return Err(invalid("shell thickness must be finite and exceed model tolerance"));
        }
        let groups = self.partition_selection(openings, true, "shell openings must be distinct faces from this compound revision")?;
        self.per_solid(groups, false, |body, local| {
            if local.len() == body.faces().count() {
                return Err(invalid("shelling cannot remove every face of a solid"));
            }
            super::features::shell(body, local, distance, join)
        })
    }

    pub fn fillet(&self, edges: &[EdgeHandle], radius: f64) -> KernelResult<Self> {
        self.fillet_with_options(edges, FilletOptions::new(radius))
    }

    pub fn fillet_with_options(
        &self,
        edges: &[EdgeHandle],
        options: FilletOptions,
    ) -> KernelResult<Self> {
        self.blend_edges(
            edges,
            options.radius,
            options.follow_tangent_edges,
            super::edge_blend::EdgeBlendKind::Fillet,
            None,
        )
    }

    fn blend_edges(
        &self,
        edges: &[EdgeHandle],
        size: f64,
        follow_tangent_edges: bool,
        kind: super::edge_blend::EdgeBlendKind,
        chamfer: Option<&ChamferSettings>,
    ) -> KernelResult<Self> {
        if !size.is_finite() || size <= self.tolerance().linear {
            return Err(invalid("edge blend size must be finite and exceed model tolerance"));
        }
        let groups = self.partition_selection(edges, false, "edge blend needs distinct edges from this compound revision")?;
        if self.solids.len() > 1 {
            return self.per_solid(groups, true, |body, local| {
                body.blend_edges(local, size, follow_tangent_edges, kind, chamfer)
            });
        }
        let selected = edges
            .iter()
            .map(EdgeHandle::persistent_id)
            .collect::<BTreeSet<_>>();

        let mut kernel = Kernel::new(self.tolerance())?;
        let mut transaction = kernel.begin_operation();
        transaction.import_body(self.state.topology(), self.solids[0].body)?;
        let index = transaction.working.entity_index();
        let imported_edges = selected
            .iter()
            .map(|name| match index.get(name) {
                Some(EntityId::Edge(edge)) => Ok(*edge),
                _ => Err(topology_error("imported edge blend edge could not be reidentified")),
            })
            .collect::<KernelResult<Vec<_>>>()?;
        match transaction.blend_edges(&imported_edges, size, follow_tangent_edges, kind, chamfer)? {
            BlendOutcome::Built => {}
            BlendOutcome::Staged(BlendStaging::MixedTrihedral { concave_edges }) => {
                return self.blend_setback_network(edges, size, &concave_edges, kind, chamfer);
            }
            // Sequencing is only honored for a corner the user selected entirely.
            BlendOutcome::Staged(BlendStaging::CrossingCorner { pair }) => {
                if !pair.iter().all(|name| selected.contains(name)) {
                    return Err(KernelError::from_diagnostic(
                        super::Diagnostic::new(
                            super::DiagnosticKind::UnsupportedGeometry,
                            "blends crossing a four-face corner must be built in sequence",
                        )
                        .with_entity(pair[0])
                        .with_entity(pair[1]),
                    ));
                }
                return self.blend_crossing_corner(edges, size, follow_tangent_edges, &pair, kind, chamfer);
            }
        }
        let committed = transaction.finish()?;
        Self::from_operation(kernel, committed, [self])
    }

    fn blend_setback_network(
        &self,
        edges: &[EdgeHandle],
        size: f64,
        concave_edges: &[PersistentId],
        kind: super::edge_blend::EdgeBlendKind,
        chamfer: Option<&ChamferSettings>,
    ) -> KernelResult<Self> {
        // Each stage creates smooth continuation edges that the following stage must consume to
        // enter the already-rounded corner. This is internal continuation, not user-facing
        // selection propagation: the original sharp-edge selection remains exactly `edges`.
        let concave = concave_edges.iter().copied().collect::<BTreeSet<_>>();
        // Concave rails create the setback; the convex network then consumes
        // its smooth continuations. This ordering is independent of selection
        // order and applies to any number of mixed corners.
        let stages = [true, false].map(|concavity| {
            edges.iter()
                .filter(|edge| concave.contains(&edge.persistent_id()) == concavity)
                .collect::<Vec<_>>()
        });
        self.blend_stages(&stages, size, true, kind, chamfer)
    }

    fn blend_crossing_corner(
        &self,
        edges: &[EdgeHandle],
        size: f64,
        follow_tangent_edges: bool,
        corner_edges: &[PersistentId],
        kind: super::edge_blend::EdgeBlendKind,
        chamfer: Option<&ChamferSettings>,
    ) -> KernelResult<Self> {
        // The second blend ends on the surface of the first. Its rails reach
        // the first blend's end boundary only when the first blend is the
        // wider one there, so try either edge last.
        let mut first_error = None;
        for last in corner_edges {
            let stages = [false, true].map(|is_last| {
                edges.iter()
                    .filter(|edge| (edge.persistent_id() == *last) == is_last)
                    .collect::<Vec<_>>()
            });
            match self.blend_stages(&stages, size, follow_tangent_edges, kind, chamfer) {
                Ok(result) => return Ok(result),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        Err(first_error.expect("a crossing corner names two edges"))
    }

    /// Blend each group of `edges` in turn, reidentifying later groups and
    /// their chamfer reference faces after each preceding blend.
    fn blend_stages(
        &self,
        stages: &[Vec<&EdgeHandle>],
        size: f64,
        follow_tangent_edges: bool,
        kind: super::edge_blend::EdgeBlendKind,
        chamfer: Option<&ChamferSettings>,
    ) -> KernelResult<Self> {
        let mut result = self.clone();
        for stage_edges in stages {
            let resolved = stage_edges.iter()
                .map(|edge| match result.resolve(*edge) {
                    Resolution::Preserved(edge)
                    | Resolution::Modified(edge)
                    | Resolution::Merged(edge) => Ok(edge),
                    Resolution::Split(mut descendants) if descendants.len() == 1 => {
                        Ok(descendants.remove(0))
                    }
                    _ => Err(unsupported("staged blend could not reidentify an incident edge after the preceding blend")),
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let settings = chamfer.map(|settings| {
                let reference_faces = stage_edges.iter().zip(&resolved).map(|(source, edge)| {
                    let reference = self.faces().find(|face| face.persistent_id() == settings.reference_faces[&source.persistent_id()]).unwrap();
                    let faces = match result.resolve(reference) {
                        Resolution::Preserved(face) | Resolution::Modified(face) | Resolution::Merged(face) => vec![face],
                        Resolution::Split(faces) => faces,
                        _ => Vec::new(),
                    };
                    let reference = edge.connected_faces()?.into_iter().find(|face| faces.contains(face))
                        .ok_or_else(|| topology_error("staged chamfer lost its reference face"))?;
                    Ok((edge.persistent_id(), reference.persistent_id()))
                }).collect::<KernelResult<_>>()?;
                Ok::<_, KernelError>(ChamferSettings { mode: settings.mode, reference_faces })
            }).transpose()?;
            result = result.blend_edges(&resolved, size, follow_tangent_edges, kind, settings.as_ref())?;
        }
        Ok(result)
    }

    /// Publish an identity operation while sharing topology, certificates and lineage.
    pub(crate) fn unchanged(&self) -> KernelResult<Self> {
        let mut names = Vec::new();
        for solid in &self.solids {
            names.extend(self.topology().body_entity_names(solid.body)?);
        }
        Ok(self
            .clone()
            .edit_history(|history| *history = history.identity(names)))
    }

    pub fn combined(&self, others: &[Self]) -> KernelResult<Self> {
        if others.is_empty() {
            return self.unchanged();
        }
        let parents = std::iter::once(self)
            .chain(others.iter())
            .collect::<Vec<_>>();
        let mut kernel = Kernel::new(self.state.tolerance())?;
        let mut bodies = Vec::new();
        let mut history = OperationHistory::default();
        let mut certificates = BTreeMap::new();
        for parent in &parents {
            ensure_compatible_tolerances(self.state.tolerance(), parent.state.tolerance())?;
            for solid in &parent.solids {
                let body = kernel
                    .topology
                    .import_body(solid.state.topology(), solid.body)?;
                bodies.push(body);
                certificates.insert(
                    body,
                    PhysicalCertificate {
                        properties: solid.mass_properties(),
                    },
                );
                history.absorb(
                    parent
                        .history
                        .identity(parent.topology().body_entity_names(solid.body)?),
                );
            }
        }
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            history,
            certificates,
            parents,
        )
    }

    /// The same solids under another tolerance policy. Entities keep the
    /// tolerances they were certified with, so a model sewn at a coarser
    /// precision remains valid next to models of the finer one.
    pub(crate) fn with_tolerance(&self, tolerance: ModelTolerance) -> KernelResult<Self> {
        let mut kernel = Kernel::new(tolerance)?;
        let mut bodies = Vec::new();
        let mut history = OperationHistory::default();
        let mut certificates = BTreeMap::new();
        for solid in &self.solids {
            let body = kernel.topology.import_body(solid.state.topology(), solid.body)?;
            bodies.push(body);
            certificates.insert(body, PhysicalCertificate { properties: solid.mass_properties() });
            history.absorb(self.history.identity(self.topology().body_entity_names(solid.body)?));
        }
        Self::from_parts(kernel, super::identity::fresh_uuid(), bodies, history, certificates, [self])
    }

    pub fn chamfer(&self, edges: &[EdgeHandle], distance: f64) -> KernelResult<Self> {
        self.chamfer_with_tangent_propagation(edges, distance, false)
    }

    pub fn chamfer_with_tangent_propagation(
        &self,
        edges: &[EdgeHandle],
        distance: f64,
        follow_tangent_edges: bool,
    ) -> KernelResult<Self> {
        self.blend_edges(
            edges,
            distance,
            follow_tangent_edges,
            super::edge_blend::EdgeBlendKind::Chamfer,
            None,
        )
    }

    pub fn chamfer_with_options(
        &self,
        edges: &[EdgeHandle],
        reference_faces: &[FaceHandle],
        distance: f64,
        mode: ChamferMode,
        follow_tangent_edges: bool,
    ) -> KernelResult<Self> {
        let valid = match mode {
            ChamferMode::TwoDistances(value) => {
                value.is_finite() && value > self.tolerance().linear
            }
            ChamferMode::DistanceAngle(value) => {
                value.is_finite() && value > 0.0 && value < std::f64::consts::FRAC_PI_2
            }
        };
        if !valid || reference_faces.len() != edges.len() {
            return Err(invalid("chamfer needs a positive second distance or an angle between 0 and 90 degrees, and one reference face per edge"));
        }
        let settings = ChamferSettings {
            mode,
            reference_faces: edges
                .iter()
                .zip(reference_faces)
                .map(|(edge, face)| (edge.persistent_id(), face.persistent_id()))
                .collect(),
        };
        self.blend_edges(
            edges,
            distance,
            follow_tangent_edges,
            super::edge_blend::EdgeBlendKind::Chamfer,
            Some(&settings),
        )
    }
    pub fn draft(&self, faces: &[FaceHandle], plane: Plane3, angle: f64) -> KernelResult<Self> {
        let groups = self.partition_selection(faces, false, "draft needs distinct faces from this compound revision")?;
        self.per_solid(groups, true, |body, local| super::features::draft(body, local, plane, angle))
    }

    pub fn split(&self, plane: Plane3, keep: SplitKeep) -> KernelResult<Self> {
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
        let mut history = OperationHistory::default();
        let mut certificates = BTreeMap::new();
        for solid in &self.solids {
            for name in self.topology().body_entity_names(solid.body)? {
                if removed.contains(&solid.body) {
                    history.changed(name, ChangeKind::Deleted, []);
                } else {
                    history.changed(name, ChangeKind::Unchanged, [name]);
                }
            }
            if !removed.contains(&solid.body) {
                let body = kernel
                    .topology
                    .import_body(solid.state.topology(), solid.body)?;
                bodies.push(body);
                certificates.insert(
                    body,
                    PhysicalCertificate {
                        properties: solid.mass_properties(),
                    },
                );
            }
        }
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            history,
            certificates,
            [self],
        )
    }

    /// Transform selected face supports and reconstruct their shared boundary.
    /// Tangent blends retain their radius where possible; rounded ends between
    /// parallel walls resize with the wall spacing.
    pub fn move_faces(
        &self,
        faces: &[FaceHandle],
        transform: RigidTransform3,
    ) -> KernelResult<Self> {
        let groups = self.partition_selection(faces, false, "move needs distinct faces from this compound revision")?;
        self.per_solid(groups, true, |body, local| super::features::move_faces(body, local, transform))
    }

    /// Replace a face's support and extend or trim its neighbors to meet it.
    /// The replacement supplies geometry only; other solids are retained.
    pub fn replace_face(
        &self,
        target: &FaceHandle,
        replacement: &FaceHandle,
    ) -> KernelResult<Self> {
        let target = std::slice::from_ref(target);
        let groups = self.partition_selection(target, false, "replacement target face does not belong to this compound revision")?;
        self.per_solid(groups, true, |body, local| {
            super::features::replace_face(body, &local[0], replacement)
        })
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
    kind: EntityKind,
    source: PersistentId,
    revision: uuid::Uuid,
    find: impl Fn(PersistentId) -> Option<T>,
) -> Resolution<T> {
    let Some(lineage) = lineage_graph.entries.get(&(kind, source, revision)) else {
        return Resolution::Unrelated;
    };
    let descendants = lineage
        .descendants
        .iter()
        .filter_map(|output| find(*output))
        .collect::<Vec<_>>();
    match descendants.len() {
        0 => Resolution::Deleted,
        1 if !lineage.changed => Resolution::Preserved(descendants[0].clone()),
        1 if lineage.merged => Resolution::Merged(descendants[0].clone()),
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
            geometry_error("published solid is missing its physical certificate")
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
    let name = state.topology().face(face)?.name();
    let revision = revisions[&name];
    Ok(FaceHandle {
        state,
        revisions,
        face,
        name,
        revision,
        surface_properties: Arc::new(OnceLock::new()),
    })
}

fn build_edge_handle(
    state: Arc<Kernel>,
    edge: EdgeId,
    revisions: Arc<BTreeMap<PersistentId, uuid::Uuid>>,
) -> KernelResult<EdgeHandle> {
    let name = state.topology().edge(edge)?.name();
    let revision = revisions[&name];
    Ok(EdgeHandle {
        state,
        revisions,
        edge,
        name,
        revision,
    })
}

/// Attribute an operation's outputs to the parent compounds it consumed.
/// Consumes operator-authored correspondence; never matches output geometry.
fn attribute_to_parents(
    kernel: &Kernel,
    bodies: &[BodyId],
    history: &OperationHistory,
    parents: &[&Compound],
    role: DerivationRole,
) -> KernelResult<OperationHistory> {
    let outputs = body_entities(kernel, bodies)?;
    let mut sources = BTreeMap::new();
    for parent in parents {
        for solid in &parent.solids {
            sources.extend(body_entities(&parent.state, &[solid.body])?);
        }
    }
    let mut descendants = BTreeMap::<PersistentId, BTreeMap<PersistentId, bool>>::new();
    let mut attributed = OperationHistory::default();
    attributed.construction_roles = history.construction_roles.clone();
    for (output, kind) in &outputs {
        let mut records = history
            .construction
            .get(output)
            .cloned()
            .unwrap_or_default();
        if sources.get(output) == Some(kind) && !records.iter().any(|r| r.name == *output) {
            let unchanged = matches!(
                history.change(*output),
                None | Some((ChangeKind::Unchanged, _))
            );
            records.push(super::history::EntitySource {
                name: *output,
                unchanged,
            });
        }
        records.retain(|record| sources.get(&record.name) == Some(kind));
        for record in &records {
            descendants
                .entry(record.name)
                .or_default()
                .insert(*output, record.unchanged);
        }
        if records.is_empty() {
            attributed.created(*output);
        } else {
            attributed.derived(*output, records.iter().map(|r| r.name), role);
            attributed.record_construction(*output, records);
        }
    }
    for source in sources.keys() {
        let outputs = descendants.remove(source).unwrap_or_default();
        let kind = match outputs.len() {
            0 => ChangeKind::Deleted,
            1 => {
                let (output, unchanged) = outputs.first_key_value().unwrap();
                if *unchanged {
                    ChangeKind::Unchanged
                } else if attributed.construction[output].len() > 1 {
                    ChangeKind::Merged
                } else {
                    ChangeKind::Modified
                }
            }
            _ => ChangeKind::Split,
        };
        attributed.changed(*source, kind, outputs.keys().copied());
    }
    Ok(attributed)
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
) -> KernelResult<BTreeMap<PersistentId, uuid::Uuid>> {
    // A solid split off with `to_compound` shares its source compound's
    // revisions, including stale ones of sibling solids. Inherit only names a
    // parent owns, so an untouched sibling cannot revert a modified revision
    // when per-solid results are combined.
    let mut inherited = BTreeMap::new();
    for parent in parents {
        let bodies = parent.solids.iter().map(|solid| solid.body).collect::<Vec<_>>();
        for name in body_entities(&parent.state, &bodies)?.into_keys() {
            inherited.insert(name, parent.revisions[&name]);
        }
    }
    let mut revisions = current
        .iter()
        .map(|name| (*name, inherited.get(name).copied().unwrap_or(operation)))
        .collect::<BTreeMap<_, _>>();
    for output in history.origins().keys() {
        if let Some(revision) = revisions.get_mut(output) {
            *revision = operation;
        }
    }
    for (source, (kind, outputs)) in history.changes() {
        if *kind != ChangeKind::Unchanged {
            for name in std::iter::once(source).chain(outputs) {
                if let Some(revision) = revisions.get_mut(name) {
                    *revision = operation;
                }
            }
        }
    }
    // Only an explicit retention record can carry a revision to a fresh name.
    for (source, (kind, outputs)) in history.changes() {
        if *kind == ChangeKind::Unchanged {
            if let Some(previous) = inherited.get(source) {
                for output in outputs {
                    if let Some(revision) = revisions.get_mut(output) {
                        *revision = *previous;
                    }
                }
            }
        }
    }
    Ok(revisions)
}

fn certify_bodies(
    kernel: &Kernel,
    bodies: &[BodyId],
) -> KernelResult<BTreeMap<BodyId, PhysicalCertificate>> {
    super::physical::certify_bodies(kernel.topology(), kernel.tolerance(), bodies)
        .map_err(KernelError::from_diagnostics)
}

fn ensure_compatible_tolerances(
    first: super::ModelTolerance,
    second: super::ModelTolerance,
) -> KernelResult<()> {
    if first != second {
        return Err(invalid("modeling operands must use the same tolerance policy"));
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
