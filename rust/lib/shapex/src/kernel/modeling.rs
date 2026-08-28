use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cgmath::prelude::*;

use crate::{Point2, Point3, Vec3};

use super::euler::CommittedOperation;
use super::physical::PhysicalCertificate;
use super::{
    BodyId, BooleanKind, ChangeKind, Circle2, Curve2, Curve3, Cylinder3, DiagnosticKind, EdgeId,
    FaceId, Kernel, KernelError, KernelResult, LoopKind, MassProperties, ModelTolerance,
    NurbsCurve2, OperationHistory, Origin, ParameterRange, PersistentId, Plane3, Profile,
    RigidTransform3, Surface3, Tessellate, TessellationOptions, TriangleMesh, TrimmedCurve2, Wire2,
};

#[derive(Clone, Copy)]
struct ConservativeBounds3 {
    minimum: Point3,
    maximum: Point3,
}

impl ConservativeBounds3 {
    fn empty() -> Self {
        Self {
            minimum: Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            maximum: Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    fn include(&mut self, point: Point3) {
        self.minimum.x = self.minimum.x.min(point.x);
        self.minimum.y = self.minimum.y.min(point.y);
        self.minimum.z = self.minimum.z.min(point.z);
        self.maximum.x = self.maximum.x.max(point.x);
        self.maximum.y = self.maximum.y.max(point.y);
        self.maximum.z = self.maximum.z.max(point.z);
    }

    fn separated(self, other: Self, tolerance: f64) -> bool {
        self.maximum.x < other.minimum.x - tolerance
            || other.maximum.x < self.minimum.x - tolerance
            || self.maximum.y < other.minimum.y - tolerance
            || other.maximum.y < self.minimum.y - tolerance
            || self.maximum.z < other.minimum.z - tolerance
            || other.maximum.z < self.minimum.z - tolerance
    }
}

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
        if self
            .conservative_bounds()
            .separated(other.conservative_bounds(), self.tolerance().linear * 8.0)
        {
            let result = match kind {
                BooleanKind::Union => {
                    combine_compounds(vec![self.clone(), other.clone()], self.tolerance())?
                }
                BooleanKind::Difference => self.clone(),
                BooleanKind::Intersection => combine_compounds(Vec::new(), self.tolerance())?,
            };
            let role = match kind {
                BooleanKind::Union => "boolean.union",
                BooleanKind::Difference => "boolean.difference",
                BooleanKind::Intersection => "boolean.intersection",
            };
            return result.with_boolean_parents(self, other, role);
        }
        let curved = self
            .faces()
            .chain(other.faces())
            .any(|face| !matches!(face.surface(), Surface3::Plane(_)));
        if curved {
            // Sewing an exactly shared boundary is both more specific and more
            // robust than rebuilding the cross-section arrangement. In
            // particular, the arrangement intentionally rejects coincident
            // curve intervals because their multiplicity is ambiguous in the
            // general case, while an adjacent solid union has an unambiguous
            // Euler construction.
            if kind == BooleanKind::Union {
                if let Some(result) = self.union_adjacent_exact(other)? {
                    return Ok(result);
                }
            }
            if let Some(result) = self.boolean_coaxial_prisms_exact(other, kind)? {
                return Ok(result);
            }
            if let Some(result) = self.boolean_parallel_cylinders_exact(other, kind)? {
                return Ok(result);
            }
            if let Some(result) = self.boolean_contained_cylinder_in_linear_prism(other, kind)? {
                return Ok(result);
            }
            return Err(KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "this curved Boolean does not yet have an analytic construction",
            ));
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

    /// A conservative analytic enclosure used only to prove separation. Circle
    /// extents are closed-form and positive-weight NURBS lie in the convex hull
    /// of their control points, so a reported gap cannot hide an intersection.
    fn conservative_bounds(&self) -> ConservativeBounds3 {
        let mut bounds = ConservativeBounds3::empty();
        for vertex in self.vertices() {
            bounds.include(vertex.point());
        }
        for edge in self.edges() {
            match edge.curve() {
                Curve3::Line(_) => {}
                Curve3::Circle(circle) => {
                    let x = circle.x_radius();
                    let y = circle.y_radius();
                    let amplitude = Vec3::new(x.x.hypot(y.x), x.y.hypot(y.y), x.z.hypot(y.z));
                    bounds.include(circle.center() - amplitude);
                    bounds.include(circle.center() + amplitude);
                }
                Curve3::Nurbs(curve) => {
                    for point in curve.control_points() {
                        bounds.include(*point);
                    }
                }
            }
        }
        for face in self.faces() {
            if let Surface3::Nurbs(surface) = face.surface() {
                for row in surface.control_points() {
                    for point in row {
                        bounds.include(*point);
                    }
                }
            }
        }
        bounds
    }

    fn boolean_coaxial_prisms_exact(
        &self,
        other: &Self,
        kind: BooleanKind,
    ) -> KernelResult<Option<Self>> {
        let axis = self
            .faces()
            .chain(other.faces())
            .find_map(|face| match face.surface() {
                Surface3::Cylinder(cylinder) => Some(cylinder.axis().normalize()),
                Surface3::Nurbs(surface)
                    if surface.degrees().1 == 1 && surface.control_points().len() == 2 =>
                {
                    let first = &surface.control_points()[0];
                    let second = &surface.control_points()[1];
                    let vector = second
                        .first()
                        .zip(first.first())
                        .map(|(top, bottom)| *top - *bottom)?;
                    let scale = vector.magnitude().max(1.0);
                    (vector.magnitude() > self.tolerance().linear
                        && first.iter().zip(second).all(|(bottom, top)| {
                            ((*top - *bottom) - vector).magnitude()
                                <= self.tolerance().linear_at_scale(scale) * 8.0
                        }))
                    .then(|| vector.normalize())
                }
                _ => None,
            });
        let Some(axis) = axis else {
            return Ok(None);
        };
        let bounds = |compound: &Self| {
            compound
                .vertices()
                .map(|vertex| axis.dot(vertex.point().to_vec()))
                .fold(
                    (f64::INFINITY, f64::NEG_INFINITY),
                    |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
                )
        };
        let first_bounds = bounds(self);
        let second_bounds = bounds(other);
        let scale = first_bounds
            .0
            .abs()
            .max(first_bounds.1.abs())
            .max(second_bounds.0.abs())
            .max(second_bounds.1.abs())
            .max(1.0);
        let linear = self.tolerance().linear_at_scale(scale) * 8.0;
        let overlap_start = first_bounds.0.max(second_bounds.0);
        let overlap_end = first_bounds.1.min(second_bounds.1);
        if overlap_end < overlap_start - linear {
            return Ok(Some(match kind {
                BooleanKind::Union => {
                    combine_compounds(vec![self.clone(), other.clone()], self.tolerance())?
                        .with_boolean_parents(self, other, "boolean.union")?
                }
                BooleanKind::Difference => {
                    self.clone()
                        .with_boolean_parents(self, other, "boolean.difference")?
                }
                BooleanKind::Intersection => combine_compounds(Vec::new(), self.tolerance())?
                    .with_boolean_parents(self, other, "boolean.intersection")?,
            }));
        }
        if (overlap_end - overlap_start).abs() <= linear {
            return match kind {
                // A shared cap may still be sewable into one body; leave that
                // case to the adjacent Euler union path.
                BooleanKind::Union => Ok(None),
                BooleanKind::Difference => Ok(Some(self.clone().with_boolean_parents(
                    self,
                    other,
                    "boolean.difference",
                )?)),
                // The intersection has no three-dimensional measure and is
                // therefore empty in a solid-only kernel.
                BooleanKind::Intersection => Ok(Some(
                    combine_compounds(Vec::new(), self.tolerance())?.with_boolean_parents(
                        self,
                        other,
                        "boolean.intersection",
                    )?,
                )),
            };
        }
        let minimum = first_bounds.0.min(second_bounds.0);
        let base_plane = Plane3::from_origin_normal(Point3::from_vec(axis * minimum), axis)?;
        let Some(first_profile) = self.profile_on_parallel_cap(axis, first_bounds.0, base_plane)?
        else {
            return Ok(None);
        };
        let Some(second_profile) =
            other.profile_on_parallel_cap(axis, second_bounds.0, base_plane)?
        else {
            return Ok(None);
        };
        let mut levels = vec![
            first_bounds.0,
            first_bounds.1,
            second_bounds.0,
            second_bounds.1,
        ];
        levels.sort_by(f64::total_cmp);
        levels.dedup_by(|first, second| (*first - *second).abs() <= linear);
        let mut layers = Vec::new();
        for interval in levels.windows(2) {
            let start = interval[0];
            let end = interval[1];
            if end - start <= linear {
                continue;
            }
            let middle = (start + end) * 0.5;
            let first_active = middle > first_bounds.0 - linear && middle < first_bounds.1 + linear;
            let second_active =
                middle > second_bounds.0 - linear && middle < second_bounds.1 + linear;
            let profiles = match (first_active, second_active, kind) {
                (true, true, kind) => first_profile.boolean_planar(&second_profile, kind)?,
                (true, false, BooleanKind::Union | BooleanKind::Difference) => {
                    vec![first_profile.clone()]
                }
                (false, true, BooleanKind::Union) => vec![second_profile.clone()],
                _ => Vec::new(),
            };
            if profiles.is_empty() {
                continue;
            }
            let plane = Plane3::new(
                base_plane.origin() + axis * (start - minimum),
                base_plane.u(),
                base_plane.v(),
                self.tolerance(),
            )?;
            let pieces = profiles
                .into_iter()
                .map(|profile| profile.extrude(plane, axis * (end - start)))
                .collect::<KernelResult<Vec<_>>>()?;
            layers.push(combine_compounds(pieces, self.tolerance())?);
        }
        let mut layers = layers.into_iter();
        let mut result = layers
            .next()
            .unwrap_or(combine_compounds(Vec::new(), self.tolerance())?);
        for layer in layers {
            result = result.union(&layer)?;
        }
        let role = match kind {
            BooleanKind::Union => "boolean.union",
            BooleanKind::Difference => "boolean.difference",
            BooleanKind::Intersection => "boolean.intersection",
        };
        Ok(Some(result.with_boolean_parents(self, other, role)?))
    }

    fn profile_on_parallel_cap(
        &self,
        axis: Vec3,
        minimum: f64,
        target_plane: Plane3,
    ) -> KernelResult<Option<Profile>> {
        let tolerance = self.tolerance();
        let cap = self.faces().find(|face| {
            let Surface3::Plane(plane) = face.surface() else {
                return false;
            };
            if plane
                .normal()
                .angle(axis)
                .0
                .min(plane.normal().angle(-axis).0)
                > tolerance.angular
            {
                return false;
            }
            (axis.dot(plane.origin().to_vec()) - minimum).abs()
                <= tolerance.linear_at_scale(minimum.abs().max(1.0)) * 8.0
        });
        let Some(cap) = cap else {
            return Ok(None);
        };
        let Surface3::Plane(cap_plane) = cap.surface() else {
            unreachable!()
        };
        let map = |point: Point2| -> KernelResult<Point2> {
            let world = cap_plane.evaluate(point.x, point.y)?.point;
            let projected = target_plane.project(world)?.parameters;
            Ok(Point2::new(projected.0, projected.1))
        };
        let topology = self.state.topology();
        let mut outer = None;
        let mut holes = Vec::new();
        for loop_id in topology.face(cap.face)?.loops() {
            let mut segments = Vec::new();
            for coedge_id in topology.loop_coedges(*loop_id)? {
                let coedge = topology.coedge(coedge_id)?;
                let range = coedge.range();
                let start = map(coedge.pcurve().evaluate(range.start)?.point)?;
                let end = map(coedge.pcurve().evaluate(range.end)?.point)?;
                let segment = match coedge.pcurve() {
                    Curve2::Line(_) => TrimmedCurve2::line(start, end)?,
                    Curve2::Circle(_) => {
                        if start.distance(end) <= tolerance.linear {
                            let quarter = map(coedge
                                .pcurve()
                                .evaluate(range.start + range.span() * 0.25)?
                                .point)?;
                            let center = map(match coedge.pcurve() {
                                Curve2::Circle(circle) => circle.center(),
                                _ => unreachable!(),
                            })?;
                            let radius = center.distance(start);
                            let orientation = (start - center).x * (quarter - center).y
                                - (start - center).y * (quarter - center).x;
                            TrimmedCurve2::new(
                                Circle2::new(center, radius)?,
                                ParameterRange::new(
                                    0.0,
                                    orientation.signum() * std::f64::consts::TAU,
                                )?,
                            )?
                        } else {
                            let middle = map(coedge
                                .pcurve()
                                .evaluate((range.start + range.end) * 0.5)?
                                .point)?;
                            TrimmedCurve2::arc_through(start, middle, end)?
                        }
                    }
                    Curve2::Nurbs(curve) => TrimmedCurve2::new(
                        NurbsCurve2::new(
                            curve.degree(),
                            curve
                                .control_points()
                                .iter()
                                .copied()
                                .map(map)
                                .collect::<KernelResult<Vec<_>>>()?,
                            curve.weights().to_vec(),
                            curve.knots().to_vec(),
                        )?,
                        range,
                    )?,
                };
                segments.push(segment);
            }
            let wire = Wire2::new(segments, tolerance)?;
            match topology.loop_(*loop_id)?.kind() {
                LoopKind::Outer => outer = Some(wire),
                LoopKind::Inner => holes.push(wire),
            }
        }
        let Some(outer) = outer else {
            return Ok(None);
        };
        Profile::new(outer, holes, tolerance).map(Some)
    }

    fn boolean_parallel_cylinders_exact(
        &self,
        other: &Self,
        kind: BooleanKind,
    ) -> KernelResult<Option<Self>> {
        let (Some(first), Some(second)) = (
            self.single_cylindrical_prism(),
            other.single_cylindrical_prism(),
        ) else {
            return Ok(None);
        };
        let tolerance = self.tolerance();
        let axis = first.axis().normalize();
        if axis
            .angle(second.axis())
            .0
            .min(axis.angle(-second.axis()).0)
            > tolerance.angular
        {
            return Ok(None);
        }
        let axial_bounds = |compound: &Self| {
            compound
                .vertices()
                .map(|vertex| axis.dot(vertex.point().to_vec()))
                .fold(
                    (f64::INFINITY, f64::NEG_INFINITY),
                    |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
                )
        };
        let first_bounds = axial_bounds(self);
        let second_bounds = axial_bounds(other);
        let scale = first_bounds
            .0
            .abs()
            .max(first_bounds.1.abs())
            .max(second_bounds.0.abs())
            .max(second_bounds.1.abs())
            .max(1.0);
        let linear = tolerance.linear_at_scale(scale) * 8.0;
        if (first_bounds.0 - second_bounds.0).abs() > linear
            || (first_bounds.1 - second_bounds.1).abs() > linear
        {
            return Ok(None);
        }
        let minimum = (first_bounds.0 + second_bounds.0) * 0.5;
        let maximum = (first_bounds.1 + second_bounds.1) * 0.5;
        if maximum - minimum <= linear {
            return Ok(None);
        }
        let u = first.x_radius().normalize();
        let v = axis.cross(u).normalize();
        let base_origin = first.origin() + axis * (minimum - axis.dot(first.origin().to_vec()));
        let plane = Plane3::new(base_origin, u, v, tolerance)?;
        let center = |cylinder: Cylinder3| {
            let relative = cylinder.origin() - base_origin;
            Point2::new(relative.dot(u), relative.dot(v))
        };
        let first_center = center(first);
        let second_center = center(second);
        let first_radius = first.radius();
        let second_radius = second.radius();
        let delta = second_center - first_center;
        let distance = delta.magnitude();
        let circle_wire =
            |center, radius| Wire2::new([TrimmedCurve2::circle(center, radius)?], tolerance);
        if distance >= first_radius + second_radius - linear {
            return Ok(Some(match kind {
                BooleanKind::Union => {
                    combine_compounds(vec![self.clone(), other.clone()], tolerance)?
                }
                BooleanKind::Difference => self.clone(),
                BooleanKind::Intersection => combine_compounds(Vec::new(), tolerance)?
                    .with_boolean_parents(self, other, "boolean.intersection")?,
            }));
        }

        let first_inside_second = distance + first_radius <= second_radius + linear;
        let second_inside_first = distance + second_radius <= first_radius + linear;
        let (outer, holes) = if first_inside_second || second_inside_first {
            match kind {
                BooleanKind::Intersection => {
                    if first_inside_second {
                        (circle_wire(first_center, first_radius)?, Vec::new())
                    } else {
                        (circle_wire(second_center, second_radius)?, Vec::new())
                    }
                }
                BooleanKind::Union => {
                    if first_inside_second {
                        (circle_wire(second_center, second_radius)?, Vec::new())
                    } else {
                        (circle_wire(first_center, first_radius)?, Vec::new())
                    }
                }
                BooleanKind::Difference if first_inside_second => {
                    return Ok(Some(
                        combine_compounds(Vec::new(), tolerance)?.with_boolean_parents(
                            self,
                            other,
                            "boolean.difference",
                        )?,
                    ));
                }
                BooleanKind::Difference => (
                    circle_wire(first_center, first_radius)?,
                    vec![circle_wire(second_center, second_radius)?],
                ),
            }
        } else {
            let along = (first_radius * first_radius - second_radius * second_radius
                + distance * distance)
                / (2.0 * distance);
            let height = (first_radius * first_radius - along * along)
                .max(0.0)
                .sqrt();
            if height <= linear {
                return Ok(None);
            }
            let direction = delta / distance;
            let perpendicular = crate::Vec2::new(-direction.y, direction.x);
            let midpoint = first_center + direction * along;
            let first_hit = midpoint + perpendicular * height;
            let second_hit = midpoint - perpendicular * height;
            let arc = |center: Point2,
                       radius: f64,
                       start: Point2,
                       end: Point2,
                       other_center: Point2,
                       other_radius: f64,
                       want_inside: bool|
             -> KernelResult<TrimmedCurve2> {
                let start_parameter = (start.y - center.y).atan2(start.x - center.x);
                let end_parameter = (end.y - center.y).atan2(end.x - center.x);
                let counter_clockwise =
                    (end_parameter - start_parameter).rem_euclid(std::f64::consts::TAU);
                let middle_parameter = start_parameter + counter_clockwise * 0.5;
                let middle = center
                    + crate::Vec2::new(middle_parameter.cos(), middle_parameter.sin()) * radius;
                let inside = middle.distance(other_center) <= other_radius + linear;
                let end_parameter = if inside == want_inside {
                    start_parameter + counter_clockwise
                } else {
                    start_parameter - (std::f64::consts::TAU - counter_clockwise)
                };
                TrimmedCurve2::new(
                    Circle2::new(center, radius)?,
                    ParameterRange::new(start_parameter, end_parameter)?,
                )
            };
            let (first_inside, second_inside) = match kind {
                BooleanKind::Intersection => (true, true),
                BooleanKind::Union => (false, false),
                BooleanKind::Difference => (false, true),
            };
            (
                Wire2::new(
                    [
                        arc(
                            first_center,
                            first_radius,
                            first_hit,
                            second_hit,
                            second_center,
                            second_radius,
                            first_inside,
                        )?,
                        arc(
                            second_center,
                            second_radius,
                            second_hit,
                            first_hit,
                            first_center,
                            first_radius,
                            second_inside,
                        )?,
                    ],
                    tolerance,
                )?,
                Vec::new(),
            )
        };
        let result =
            Profile::new(outer, holes, tolerance)?.extrude(plane, axis * (maximum - minimum))?;
        let role = match kind {
            BooleanKind::Union => "boolean.union",
            BooleanKind::Difference => "boolean.difference",
            BooleanKind::Intersection => "boolean.intersection",
        };
        Ok(Some(result.with_boolean_parents(self, other, role)?))
    }

    fn with_boolean_parents(&self, first: &Self, second: &Self, role: &str) -> KernelResult<Self> {
        let tolerance = self.tolerance();
        let mut kernel = Kernel::new(tolerance)?;
        let bodies = self
            .solids
            .iter()
            .map(|solid| {
                kernel
                    .topology
                    .import_body(solid.state.topology(), solid.body)
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let outputs = body_entities(&kernel, &bodies)?;
        let mut sources = body_entities(
            &first.state,
            &first
                .solids
                .iter()
                .map(|solid| solid.body)
                .collect::<Vec<_>>(),
        )?;
        sources.extend(body_entities(
            &second.state,
            &second
                .solids
                .iter()
                .map(|solid| solid.body)
                .collect::<Vec<_>>(),
        )?);
        let mut history = OperationHistory::default();
        for (output, kind) in &outputs {
            history.derived(
                *output,
                sources
                    .iter()
                    .filter_map(|(source, source_kind)| (source_kind == kind).then_some(*source)),
                role,
            );
        }
        for (source, kind) in sources {
            let descendants = outputs
                .iter()
                .filter_map(|(output, output_kind)| (output_kind == &kind).then_some(*output))
                .collect::<Vec<_>>();
            history.changed(
                source,
                if descendants.len() > 1 {
                    ChangeKind::Split
                } else {
                    ChangeKind::Modified
                },
                descendants,
            );
        }
        let certificates = certify_bodies(&kernel, &bodies)?;
        Self::from_parts(
            kernel,
            super::identity::fresh_uuid(),
            bodies,
            history,
            certificates,
            [first, second],
        )
    }

    fn boolean_contained_cylinder_in_linear_prism(
        &self,
        other: &Self,
        kind: BooleanKind,
    ) -> KernelResult<Option<Self>> {
        let (cylinder_is_first, cylinder, prism) = match (
            self.single_cylindrical_prism(),
            other.single_cylindrical_prism(),
        ) {
            (Some(cylinder), None) => (true, cylinder, other),
            (None, Some(cylinder)) => (false, cylinder, self),
            _ => return Ok(None),
        };
        if prism
            .faces()
            .any(|face| !matches!(face.surface(), Surface3::Plane(_)))
        {
            return Ok(None);
        }
        let tolerance = self.tolerance();
        let axis = cylinder.axis().normalize();
        let bounds = |compound: &Self| {
            compound
                .vertices()
                .map(|vertex| axis.dot(vertex.point().to_vec()))
                .fold(
                    (f64::INFINITY, f64::NEG_INFINITY),
                    |(minimum, maximum), value| (minimum.min(value), maximum.max(value)),
                )
        };
        let cylinder_bounds = bounds(if cylinder_is_first { self } else { other });
        let prism_bounds = bounds(prism);
        let scale = cylinder_bounds
            .0
            .abs()
            .max(cylinder_bounds.1.abs())
            .max(prism_bounds.0.abs())
            .max(prism_bounds.1.abs())
            .max(1.0);
        let linear = tolerance.linear_at_scale(scale) * 8.0;
        if (cylinder_bounds.0 - prism_bounds.0).abs() > linear
            || (cylinder_bounds.1 - prism_bounds.1).abs() > linear
        {
            return Ok(None);
        }
        let minimum = (cylinder_bounds.0 + prism_bounds.0) * 0.5;
        let maximum = (cylinder_bounds.1 + prism_bounds.1) * 0.5;
        let base_origin = Point3::from_vec(axis * minimum);
        let plane = Plane3::from_origin_normal(base_origin, axis)?;
        let cap = prism
            .faces()
            .filter_map(|face| match face.surface() {
                Surface3::Plane(surface)
                    if surface
                        .normal()
                        .angle(axis)
                        .0
                        .min(surface.normal().angle(-axis).0)
                        <= tolerance.angular =>
                {
                    Some(face)
                }
                _ => None,
            })
            .min_by(|first, second| {
                let level = |face: &&FaceHandle| match face.surface() {
                    Surface3::Plane(surface) => axis.dot(surface.origin().to_vec()),
                    _ => unreachable!(),
                };
                level(first).total_cmp(&level(second))
            })
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "a linear prism has no planar cap",
                )
            })?;
        let topology = prism.state.topology();
        let mut outer = None;
        let mut holes = Vec::new();
        for loop_id in topology.face(cap.face)?.loops() {
            let segments = topology
                .loop_coedges(*loop_id)?
                .into_iter()
                .map(|coedge| {
                    let vertices = topology.coedge_vertices(coedge)?;
                    let start = plane
                        .project(topology.vertex(vertices[0])?.point())?
                        .parameters;
                    let end = plane
                        .project(topology.vertex(vertices[1])?.point())?
                        .parameters;
                    TrimmedCurve2::line(Point2::new(start.0, start.1), Point2::new(end.0, end.1))
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let wire = Wire2::new(segments, tolerance)?;
            match topology.loop_(*loop_id)?.kind() {
                LoopKind::Outer => outer = Some(wire),
                LoopKind::Inner => holes.push(wire),
            }
        }
        let outer = outer.ok_or_else(|| {
            KernelError::new(
                DiagnosticKind::TopologyInvariant,
                "a linear prism cap has no outer loop",
            )
        })?;
        let prism_profile = Profile::new(outer, holes, tolerance)?;
        if prism_profile
            .outer()
            .segments()
            .iter()
            .chain(
                prism_profile
                    .holes()
                    .iter()
                    .flat_map(|hole| hole.segments()),
            )
            .any(|segment| !matches!(segment.curve(), Curve2::Line(_)))
        {
            return Ok(None);
        }
        let center = plane.project(cylinder.origin())?.parameters;
        let center = Point2::new(center.0, center.1);
        let point_inside = |point: Point2, wire: &Wire2| -> KernelResult<bool> {
            let points = wire
                .segments()
                .iter()
                .map(TrimmedCurve2::start)
                .collect::<KernelResult<Vec<_>>>()?;
            let mut inside = false;
            for index in 0..points.len() {
                let first = points[index];
                let second = points[(index + 1) % points.len()];
                if (first.y > point.y) != (second.y > point.y)
                    && point.x
                        < (second.x - first.x) * (point.y - first.y) / (second.y - first.y)
                            + first.x
                {
                    inside = !inside;
                }
            }
            Ok(inside)
        };
        let boundary_clearance = |wire: &Wire2| -> KernelResult<f64> {
            wire.segments()
                .iter()
                .map(|segment| {
                    let start = segment.start()?;
                    let end = segment.end()?;
                    let direction = end - start;
                    let parameter =
                        ((center - start).dot(direction) / direction.magnitude2()).clamp(0.0, 1.0);
                    Ok(center.distance(start + direction * parameter))
                })
                .try_fold(f64::INFINITY, |minimum, distance| {
                    distance.map(|distance| minimum.min(distance))
                })
        };
        if !point_inside(center, prism_profile.outer())?
            || prism_profile
                .holes()
                .iter()
                .map(|hole| point_inside(center, hole))
                .collect::<KernelResult<Vec<_>>>()?
                .into_iter()
                .any(|inside| inside)
            || boundary_clearance(prism_profile.outer())? <= cylinder.radius() + linear
            || prism_profile
                .holes()
                .iter()
                .map(boundary_clearance)
                .collect::<KernelResult<Vec<_>>>()?
                .into_iter()
                .any(|clearance| clearance <= cylinder.radius() + linear)
        {
            return Ok(None);
        }
        let circle = Wire2::new(
            [TrimmedCurve2::circle(center, cylinder.radius())?],
            tolerance,
        )?;
        let height = maximum - minimum;
        let role = match kind {
            BooleanKind::Union => "boolean.union",
            BooleanKind::Difference => "boolean.difference",
            BooleanKind::Intersection => "boolean.intersection",
        };
        let result = match kind {
            BooleanKind::Union => prism_profile.extrude(plane, axis * height)?,
            BooleanKind::Intersection => {
                Profile::new(circle, [], tolerance)?.extrude(plane, axis * height)?
            }
            BooleanKind::Difference if cylinder_is_first => {
                combine_compounds(Vec::new(), tolerance)?
            }
            BooleanKind::Difference => {
                let mut result_holes = prism_profile.holes().to_vec();
                result_holes.push(circle);
                Profile::new(prism_profile.outer().clone(), result_holes, tolerance)?
                    .extrude(plane, axis * height)?
            }
        };
        Ok(Some(result.with_boolean_parents(self, other, role)?))
    }

    fn single_cylindrical_prism(&self) -> Option<Cylinder3> {
        let cylinders = self
            .faces()
            .filter_map(|face| match face.surface() {
                Surface3::Cylinder(cylinder) => Some(*cylinder),
                _ => None,
            })
            .collect::<Vec<_>>();
        let reference = cylinders.first().copied()?;
        let linear = self.tolerance().linear_at_scale(reference.radius()) * 8.0;
        (self
            .faces()
            .all(|face| matches!(face.surface(), Surface3::Plane(_) | Surface3::Cylinder(_)))
            && cylinders.iter().all(|candidate| {
                reference
                    .axis()
                    .normalize()
                    .cross(candidate.axis().normalize())
                    .magnitude()
                    <= self.tolerance().angular
                    && (candidate.radius() - reference.radius()).abs() <= linear
                    && (candidate.origin() - reference.origin())
                        .cross(reference.axis().normalize())
                        .magnitude()
                        <= linear
            }))
        .then_some(reference)
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
