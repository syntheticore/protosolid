use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cgmath::prelude::*;

use crate::{Point3, Vec3};

use super::euler::CommittedOperation;
use super::physical::PhysicalCertificate;
use super::{
    BodyId, BooleanKind, ChangeKind, DiagnosticKind, EdgeId, EntityId, FaceId, Kernel,
    KernelError, KernelResult, MassProperties, ModelTolerance, OperationHistory, Origin,
    PersistentId, Plane3, Profile, RigidTransform3, Surface3, Tessellate, TessellationOptions,
    Topology, TriangleMesh,
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

fn semantic_edge(history: &OperationHistory, edge: &EdgeHandle) -> bool {
    let Ok(faces) = edge.connected_faces() else {
        return true;
    };
    // A two-use edge whose uses lie on one periodic face is a parameterization
    // seam, not a physical or user-selectable model edge.
    if faces.len() == 1 {
        return false;
    }
    if faces.len() != 2 {
        return true;
    }
    // A subdivision of one support is not a model edge, regardless of which
    // operation last touched the body. Operation history contains only the
    // current operation and can lose the common ancestry of untouched faces.
    if same_surface_support(
        faces[0].surface(),
        faces[1].surface(),
        edge.state.tolerance(),
    ) {
        return false;
    }
    let first_roots = history.source_roots(faces[0].persistent_id());
    let second_roots = history.source_roots(faces[1].persistent_id());
    if first_roots.is_disjoint(&second_roots) {
        return true;
    }
    // An exact Boolean deliberately gives all of its output faces the same
    // operation ancestry. Preserve creases between unlike analytical supports,
    // including perpendicular planes, without exposing the many edges of a
    // faceted feature reconstruction.
    if std::mem::discriminant(faces[0].surface()) != std::mem::discriminant(faces[1].surface()) {
        return true;
    }
    let exact_boolean_face = |face: &FaceHandle| {
        matches!(
            history.origin(face.persistent_id()),
            Some(Origin::Derived { role, .. })
                if matches!(
                    role.as_str(),
                    "boolean.union" | "boolean.difference" | "boolean.intersection"
                )
        )
    };
    // Chamfer faces deliberately inherit from their adjacent parent faces.
    // That shared ancestry is not a faceted reconstruction: the crease against
    // either parent (or a corner/cap patch) is a real model edge immediately.
    let chamfer_face = |face: &FaceHandle| {
        matches!(history.origin(face.persistent_id()),
            Some(Origin::Derived { role, .. }) if matches!(role.as_str(),
                "chamfer.blend.face" | "chamfer.vertex.face" | "chamfer.cap.face"))
    };
    match (faces[0].surface(), faces[1].surface()) {
        (Surface3::Plane(first), Surface3::Plane(second))
            if (exact_boolean_face(&faces[0]) && exact_boolean_face(&faces[1]))
                || chamfer_face(&faces[0])
                || chamfer_face(&faces[1]) =>
        {
            first.normal().cross(second.normal()).magnitude() > edge.state.tolerance().angular
        }
        // Faceted feature reconstructions deliberately share planar ancestry,
        // so their internal triangle edges remain hidden. Curved analytical
        // patches, however, can have the same surface representation while
        // lying on different supports (for example two adjacent NURBS patches
        // in a rolling blend). Their shared boundary is a semantic BREP edge.
        (Surface3::Plane(_), Surface3::Plane(_)) => false,
        (first, second) => !same_surface_support(first, second, edge.state.tolerance()),
    }
}

fn same_curve_support(
    first: &super::Curve3,
    second: &super::Curve3,
    tolerance: ModelTolerance,
) -> bool {
    match (first, second) {
        (super::Curve3::Line(first), super::Curve3::Line(second)) => {
            let first_direction = first.direction().normalize();
            let second_direction = second.direction().normalize();
            first_direction.cross(second_direction).magnitude() <= tolerance.angular
                && (second.origin() - first.origin())
                    .cross(first_direction)
                    .magnitude()
                    <= tolerance.linear
        }
        (super::Curve3::Circle(first), super::Curve3::Circle(second)) => {
            first.center().distance(second.center()) <= tolerance.linear
                && (first.radius() - second.radius()).abs() <= tolerance.linear
                // acos(dot) amplifies roundoff near parallel normals: even
                // identical circles can appear farther apart than the angular
                // tolerance and lose their edge lineage after a Boolean.
                && first.normal().cross(second.normal()).magnitude() <= tolerance.angular
        }
        (super::Curve3::Nurbs(first), super::Curve3::Nurbs(second)) => {
            let compatible = first.degree() == second.degree()
                && first.control_points().len() == second.control_points().len()
                && first.weights().len() == second.weights().len()
                && first.knots().len() == second.knots().len();
            if !compatible {
                return false;
            }
            let normalized_knots = |knots: &[f64]| {
                let start = knots[0];
                let span = knots[knots.len() - 1] - start;
                knots
                    .iter()
                    .map(|value| (value - start) / span)
                    .collect::<Vec<_>>()
            };
            let first_knots = normalized_knots(first.knots());
            let second_knots = normalized_knots(second.knots());
            let same = |reversed: bool| {
                let point = |index| {
                    if reversed {
                        second.control_points()[second.control_points().len() - 1 - index]
                    } else {
                        second.control_points()[index]
                    }
                };
                let weight = |index| {
                    if reversed {
                        second.weights()[second.weights().len() - 1 - index]
                    } else {
                        second.weights()[index]
                    }
                };
                let knot = |index| {
                    if reversed {
                        1.0 - second_knots[second_knots.len() - 1 - index]
                    } else {
                        second_knots[index]
                    }
                };
                let weight_scale = weight(0) / first.weights()[0];
                first
                    .control_points()
                    .iter()
                    .enumerate()
                    .all(|(index, value)| value.distance(point(index)) <= tolerance.linear)
                    && first.weights().iter().enumerate().all(|(index, value)| {
                        (value * weight_scale - weight(index)).abs() <= tolerance.parameter
                    })
                    && first_knots
                        .iter()
                        .enumerate()
                        .all(|(index, value)| (value - knot(index)).abs() <= tolerance.parameter)
            };
            same(false) || same(true)
        }
        _ => first == second,
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

fn trimmed_curves_overlap(
    first: &super::Curve3,
    first_range: super::ParameterRange,
    second: &super::Curve3,
    second_range: super::ParameterRange,
    tolerance: ModelTolerance,
) -> KernelResult<bool> {
    for (a, ar, b, br) in [
        (first, first_range, second, second_range),
        (second, second_range, first, first_range),
    ] {
        for parameter in [ar.start, (ar.start + ar.end) * 0.5, ar.end] {
            let projection = b.project_on_trim(a.evaluate(parameter)?.point, br, tolerance)?;
            if projection.distance <= tolerance.linear
                && projection.parameter > br.start.min(br.end) + tolerance.parameter
                && projection.parameter < br.start.max(br.end) - tolerance.parameter
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
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
    surface: super::Surface3,
}

impl FaceHandle {
    pub fn persistent_id(&self) -> PersistentId {
        self.name
    }
    pub fn surface(&self) -> &super::Surface3 {
        &self.surface
    }
    pub(crate) fn topology_id(&self) -> FaceId {
        self.face
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
        ));
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
        if transform == RigidTransform3::identity() {
            return self.unchanged();
        }
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
            let role = match kind {
                BooleanKind::Union => "boolean.union",
                BooleanKind::Difference => "boolean.difference",
                BooleanKind::Intersection => "boolean.intersection",
            };
            return result.with_boolean_parents(self, other, role);
        }
        super::boolean::boolean_brep(self, other, kind)
    }

    fn with_geometric_parents(
        &self,
        parents: &[&Self],
        role: &str,
        extra_origins: &BTreeMap<PersistentId, Vec<PersistentId>>,
    ) -> KernelResult<Self> {
        let tolerance = self.tolerance();
        let source_vertices = parents
            .iter()
            .flat_map(|parent| parent.vertices())
            .map(|vertex| (vertex.persistent_id(), vertex.point()))
            .collect::<BTreeMap<_, _>>();
        let edge_geometry = |edge: &EdgeHandle| {
            Ok((
                edge.persistent_id(),
                (
                    edge.curve().clone(),
                    edge.state.topology().edge(edge.edge)?.range(),
                    edge.connected_faces()?
                        .into_iter()
                        .map(|face| face.persistent_id())
                        .collect::<Vec<_>>(),
                ),
            ))
        };
        let source_edges = parents
            .iter()
            .flat_map(|parent| parent.edges())
            .map(edge_geometry)
            .collect::<KernelResult<BTreeMap<_, _>>>()?;
        let output_vertices = self
            .vertices()
            .map(|vertex| (vertex.persistent_id(), vertex.point()))
            .collect::<BTreeMap<_, _>>();
        let output_edges = self
            .edges()
            .map(edge_geometry)
            .collect::<KernelResult<BTreeMap<_, _>>>()?;
        let kernel = &self.state;
        let bodies = self
            .solids
            .iter()
            .map(|solid| solid.body)
            .collect::<Vec<_>>();
        let outputs = body_entities(&kernel, &bodies)?;
        let mut sources = BTreeMap::new();
        for parent in parents {
            sources.extend(body_entities(
                &parent.state,
                &parent
                    .solids
                    .iter()
                    .map(|solid| solid.body)
                    .collect::<Vec<_>>(),
            )?);
        }
        // Face builders carry their source correspondence through sewing and
        // merging. Supplied feature origins take precedence over all inference.
        let face_sources = self
            .faces()
            .map(|face| {
                let output = face.persistent_id();
                let ancestors = if let Some(origins) = extra_origins.get(&output) {
                    origins.iter().copied().collect()
                } else if sources.get(&output) == Some(&EntityKind::Face) {
                    BTreeSet::from([output])
                } else {
                    self.history
                        .source_roots(output)
                        .into_iter()
                        .filter(|name| sources.get(name) == Some(&EntityKind::Face))
                        .collect()
                };
                (output, ancestors)
            })
            .collect::<BTreeMap<_, _>>();
        let related =
            |kind: EntityKind, source: PersistentId, output: PersistentId| -> KernelResult<bool> {
                if let Some(origins) = extra_origins.get(&output) {
                    return Ok(origins.contains(&source));
                }
                Ok(match kind {
                    EntityKind::Vertex => {
                        source_vertices[&source].distance(output_vertices[&output])
                            <= tolerance.linear
                    }
                    EntityKind::Edge => {
                        let first = &source_edges[&source];
                        let second = &output_edges[&output];
                        // One neighboring face may be replaced by a Boolean cap while
                        // the other retains this boundary interval.
                        same_curve_support(&first.0, &second.0, tolerance)
                            && first.2.iter().any(|face| {
                                second
                                    .2
                                    .iter()
                                    .any(|output_face| face_sources[output_face].contains(face))
                            })
                            && trimmed_curves_overlap(
                                &first.0, first.1, &second.0, second.1, tolerance,
                            )?
                    }
                    EntityKind::Face => face_sources[&output].contains(&source),
                })
            };
        // Compare only established pairs, once, to distinguish retention from
        // modification. Exact comparison cannot accumulate tolerance drift.
        let output_geometry = super::history_geometry::entities(kernel.topology());
        let mut unchanged = BTreeSet::new();
        let mut descendants = BTreeMap::<PersistentId, BTreeSet<PersistentId>>::new();
        let mut contributors = BTreeMap::<PersistentId, BTreeSet<PersistentId>>::new();
        for parent in parents {
            let input_geometry = super::history_geometry::entities(parent.topology());
            for (source, kind) in &sources {
                let Some(input) = input_geometry.get(source) else {
                    continue;
                };
                for (output, output_kind) in &outputs {
                    if kind == output_kind && related(*kind, *source, *output)? {
                        descendants.entry(*source).or_default().insert(*output);
                        contributors.entry(*output).or_default().insert(*source);
                        if super::history_geometry::same_entity(
                            parent.topology(),
                            *input,
                            kernel.topology(),
                            output_geometry[output],
                        )? {
                            unchanged.insert((*source, *output));
                        }
                    }
                }
            }
        }
        let mut history = OperationHistory::default();
        for output in outputs.keys() {
            if let Some(sources) = extra_origins.get(output) {
                history.derived(*output, sources.iter().copied(), role);
            } else if let Some(sources) = contributors.get(output) {
                history.derived(*output, sources.iter().copied(), role);
            } else {
                history.created(*output);
            }
        }
        for source in sources.keys() {
            let descendants = descendants.remove(source).unwrap_or_default();
            let kind = match descendants.len() {
                0 => ChangeKind::Deleted,
                1 => {
                    let output = *descendants.first().unwrap();
                    if unchanged.contains(&(*source, output)) {
                        ChangeKind::Unchanged
                    } else if contributors[&output].len() > 1 {
                        ChangeKind::Merged
                    } else {
                        ChangeKind::Modified
                    }
                }
                _ => ChangeKind::Split,
            };
            history.changed(*source, kind, descendants);
        }
        // This operation only rebuilds stable names and lineage around an
        // unchanged, already-published exact BREP. Preserve its analytical
        // certificates instead of asking the primitive recognizers to
        // reclassify a composite rational blend.
        let certificates = bodies
            .iter()
            .copied()
            .zip(self.solids.iter().map(|solid| PhysicalCertificate {
                properties: solid.mass_properties(),
            }))
            .collect::<BTreeMap<_, _>>();
        Self::from_parts(
            Arc::clone(&self.state),
            super::identity::fresh_uuid(),
            bodies,
            history,
            certificates,
            parents.iter().copied(),
        )
    }

    pub(crate) fn with_boolean_parents(
        &self,
        first: &Self,
        second: &Self,
        role: &str,
    ) -> KernelResult<Self> {
        self.with_geometric_parents(&[first, second], role, &BTreeMap::new())
    }

    pub(crate) fn with_feature_parent(
        &self,
        parent: &Self,
        role: &str,
        extra_origins: &BTreeMap<PersistentId, Vec<PersistentId>>,
    ) -> KernelResult<Self> {
        self.with_geometric_parents(&[parent], role, extra_origins)
    }

    pub(crate) fn with_creation_roles(
        mut self,
        roles: impl IntoIterator<Item = (PersistentId, super::CreationRole)>,
    ) -> Self {
        let history = Arc::make_mut(&mut self.history);
        for (name, role) in roles {
            history.created_as(name, role);
        }
        for solid in &mut self.solids {
            solid.history = Arc::clone(&self.history);
        }
        self
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

    /// Offset selected faces along their outward normals, extending adjacent
    /// supports and adjusting tangent blends to keep each solid closed.
    pub fn offset_faces(&self, faces: &[FaceHandle], distance: f64) -> KernelResult<Self> {
        if !distance.is_finite() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "offset distance must be finite",
            ));
        }
        if faces.is_empty()
            || faces
                .iter()
                .any(|face| !self.faces().any(|available| *available == *face))
            || faces
                .iter()
                .map(FaceHandle::persistent_id)
                .collect::<BTreeSet<_>>()
                .len()
                != faces.len()
        {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "offset needs distinct faces from this compound revision",
            ));
        }
        if distance == 0.0 {
            return self.unchanged();
        }
        if distance.abs() <= self.tolerance().linear {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "offset distance must exceed model tolerance",
            ));
        }
        let mut results = Vec::new();
        for solid in &self.solids {
            let local = faces
                .iter()
                .filter(|face| solid.faces().any(|available| *available == **face))
                .cloned()
                .collect::<Vec<_>>();
            let body = solid.to_compound();
            results.push(if local.is_empty() {
                body
            } else {
                super::features::offset_faces(&body, &local, distance)?
            });
        }
        combine_compounds(results, self.tolerance())
    }

    pub fn shell(&self, openings: &[FaceHandle], distance: f64) -> KernelResult<Self> {
        self.shell_with_join(openings, distance, "rounded")
    }

    /// Offset a skin with either intersected (sharp) or thickness-radius joins.
    pub fn shell_with_join(
        &self,
        openings: &[FaceHandle],
        distance: f64,
        join_type: &str,
    ) -> KernelResult<Self> {
        if !matches!(join_type, "sharp" | "rounded") {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "shell join type must be sharp or rounded",
            ));
        }
        if !distance.is_finite() || distance.abs() <= self.tolerance().linear {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "shell thickness must be finite and exceed model tolerance",
            ));
        }
        let selected = openings
            .iter()
            .map(FaceHandle::persistent_id)
            .collect::<BTreeSet<_>>();
        if selected.len() != openings.len() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "shell opening selection contains duplicates",
            ));
        }
        let available = self
            .faces()
            .map(FaceHandle::persistent_id)
            .collect::<BTreeSet<_>>();
        if !selected.is_subset(&available) {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "shell opening faces do not belong to this compound revision",
            ));
        }
        if !available.is_empty() && selected == available {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "shelling cannot remove every face of a solid",
            ));
        }
        if self.solids.len() > 1 {
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
                results.push(
                    solid
                        .to_compound()
                        .shell_with_join(&local, distance, join_type)?,
                );
            }
            return combine_compounds(results, self.state.tolerance());
        }
        super::features::shell(self, openings, distance, join_type)
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
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "edge blend size must be finite and exceed model tolerance",
            ));
        }
        if edges.is_empty() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "edge blend needs at least one edge",
            ));
        }
        let selected = edges
            .iter()
            .map(EdgeHandle::persistent_id)
            .collect::<BTreeSet<_>>();
        if selected.len() != edges.len() {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "edge blend edge selection contains duplicates",
            ));
        }
        let available = self
            .edges()
            .map(|edge| edge.persistent_id())
            .collect::<BTreeSet<_>>();
        if !selected.is_subset(&available) {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "edge blend edges do not belong to this compound revision",
            ));
        }
        if self.solids.len() > 1 {
            let mut results = Vec::with_capacity(self.solids.len());
            for solid in &self.solids {
                let local = solid
                    .edges()
                    .filter(|edge| selected.contains(&edge.persistent_id()))
                    .cloned()
                    .collect::<Vec<_>>();
                let local_compound = solid.to_compound();
                results.push(if local.is_empty() {
                    local_compound
                } else {
                    local_compound.blend_edges(&local, size, follow_tangent_edges, kind, chamfer)?
                });
            }
            return combine_compounds(results, self.tolerance());
        }

        let mut kernel = Kernel::new(self.tolerance())?;
        kernel
            .topology
            .import_body(self.state.topology(), self.solids[0].body)?;
        let imported_edges = selected
            .iter()
            .map(|name| match kernel.topology.entity(*name) {
                Some(EntityId::Edge(edge)) => Ok(edge),
                _ => Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    "imported edge blend edge could not be reidentified",
                )),
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let mut transaction = kernel.begin_operation();
        if let Err(error) =
            transaction.blend_edges(&imported_edges, size, follow_tangent_edges, kind, chamfer)
        {
            let mixed_trihedral = (edges.len() == 3 && chamfer.is_none())
                .then(|| {
                    error.diagnostics().iter().find(|diagnostic| {
                        diagnostic.operation.as_deref() == Some("blend.mixed-trihedral-setback")
                    })
                })
                .flatten();
            if let Some(diagnostic) = mixed_trihedral {
                return self.blend_three_edge_setback(edges, size, &diagnostic.entities, kind);
            }
            return Err(error);
        }
        let committed = transaction.finish()?;
        Self::from_operation(kernel, committed, [self])
    }

    fn blend_three_edge_setback(
        &self,
        edges: &[EdgeHandle],
        size: f64,
        concave_edges: &[PersistentId],
        kind: super::edge_blend::EdgeBlendKind,
    ) -> KernelResult<Self> {
        // Each stage creates smooth continuation edges that the following stage must consume to
        // enter the already-rounded corner. This is internal continuation, not user-facing
        // selection propagation: the original sharp-edge selection remains exactly `edges`.
        let fallback_orders = [
            [[0, 1], [2, usize::MAX]],
            [[0, 2], [1, usize::MAX]],
            [[1, 2], [0, usize::MAX]],
            [[0, usize::MAX], [1, 2]],
            [[1, usize::MAX], [0, 2]],
            [[2, usize::MAX], [0, 1]],
        ];
        let concave = concave_edges.iter().copied().collect::<BTreeSet<_>>();
        let concave_indices = edges
            .iter()
            .enumerate()
            .filter_map(|(index, edge)| concave.contains(&edge.persistent_id()).then_some(index))
            .collect::<Vec<_>>();
        let minority = match concave_indices.as_slice() {
            [only] => Some(*only),
            [first, second] => (0..3).find(|index| index != first && index != second),
            _ => None,
        };
        let mut grouped_orders = Vec::with_capacity(fallback_orders.len());
        if let Some(singleton) = minority {
            let pair = (0..3)
                .filter(|index| *index != singleton)
                .collect::<Vec<_>>();
            grouped_orders.push([[singleton, usize::MAX], [pair[0], pair[1]]]);
        }
        for order in fallback_orders {
            if !grouped_orders.contains(&order) {
                grouped_orders.push(order);
            }
        }
        let mut last_error = None;
        for groups in grouped_orders {
            let mut result = self.clone();
            let mut failed = false;
            for group in groups {
                let resolved = group
                    .into_iter()
                    .filter(|index| *index != usize::MAX)
                    .map(|index| match result.resolve(&edges[index]) {
                        Resolution::Preserved(edge)
                        | Resolution::Modified(edge)
                        | Resolution::Merged(edge) => Ok(edge),
                        Resolution::Split(mut descendants) if descendants.len() == 1 => {
                            Ok(descendants.remove(0))
                        }
                        _ => Err(KernelError::new(
                            DiagnosticKind::UnsupportedGeometry,
                            "setback blend could not reidentify an incident edge after the preceding blend",
                        )),
                    })
                    .collect::<KernelResult<Vec<_>>>();
                match resolved
                    .and_then(|resolved| result.blend_edges(&resolved, size, true, kind, None))
                {
                    Ok(next) => result = next,
                    Err(error) => {
                        last_error = Some(error);
                        failed = true;
                        break;
                    }
                }
            }
            if !failed {
                return Ok(result);
            }
        }
        Err(last_error.unwrap_or_else(|| {
            KernelError::new(
                DiagnosticKind::UnsupportedGeometry,
                "mixed convex-concave setback blend has no valid construction order",
            )
        }))
    }

    /// Publish an identity operation while sharing topology, certificates and lineage.
    pub(crate) fn unchanged(&self) -> KernelResult<Self> {
        let mut result = self.clone();
        let mut names = Vec::new();
        for solid in &self.solids {
            names.extend(self.topology().body_entity_names(solid.body)?);
        }
        result.history = Arc::new(self.history.identity(names));
        for solid in &mut result.solids {
            solid.history = Arc::clone(&result.history);
        }
        Ok(result)
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
            return Err(KernelError::new(DiagnosticKind::InvalidInput,
                "chamfer needs a positive second distance or an angle between 0 and 90 degrees, and one reference face per edge"));
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
        if faces.is_empty()
            || faces
                .iter()
                .any(|face| !self.faces().any(|available| *available == *face))
            || faces
                .iter()
                .map(FaceHandle::persistent_id)
                .collect::<BTreeSet<_>>()
                .len()
                != faces.len()
        {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "draft needs distinct faces from this compound revision",
            ));
        }
        let mut results = Vec::new();
        for solid in &self.solids {
            let local = faces
                .iter()
                .filter(|face| solid.faces().any(|available| *available == **face))
                .cloned()
                .collect::<Vec<_>>();
            let body = solid.to_compound();
            results.push(if local.is_empty() {
                body
            } else {
                super::features::draft(&body, &local, plane, angle)?
            });
        }
        combine_compounds(results, self.tolerance())
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
        let selected = faces
            .iter()
            .map(FaceHandle::persistent_id)
            .collect::<BTreeSet<_>>();
        if faces.is_empty()
            || selected.len() != faces.len()
            || faces.iter().any(|face| !self.faces().any(|candidate| candidate == face))
        {
            return Err(KernelError::new(
                DiagnosticKind::InvalidInput,
                "move needs distinct faces from this compound revision",
            ));
        }
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
    let inherited = parents
        .iter()
        .flat_map(|parent| {
            parent
                .revisions
                .iter()
                .map(|(name, revision)| (*name, *revision))
        })
        .collect::<BTreeMap<_, _>>();
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
    revisions
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
