use crate::Point3;

use std::collections::{BTreeMap, BTreeSet};

use super::arena::Arena;
use super::{Curve2, Curve3, Handle, KernelResult, ParameterRange, PersistentId, Surface3};

pub type VertexId = Handle<Vertex>;
pub type EdgeId = Handle<Edge>;
pub type CoedgeId = Handle<Coedge>;
pub type LoopId = Handle<Loop>;
pub type FaceId = Handle<Face>;
pub type ShellId = Handle<Shell>;
pub type BodyId = Handle<Body>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityId {
    Vertex(VertexId),
    Edge(EdgeId),
    Coedge(CoedgeId),
    Loop(LoopId),
    Face(FaceId),
    Shell(ShellId),
    Body(BodyId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceOrientation {
    Forward,
    Reversed,
}

impl FaceOrientation {
    pub fn reversed(self) -> Self {
        match self {
            Self::Forward => Self::Reversed,
            Self::Reversed => Self::Forward,
        }
    }

    /// `1` when the material normal follows the surface normal, else `-1`.
    pub fn sign(self) -> f64 {
        match self {
            Self::Forward => 1.0,
            Self::Reversed => -1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopKind {
    Outer,
    Inner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Open,
    Closed,
}

#[derive(Debug, Clone)]
pub struct Vertex {
    pub(crate) name: PersistentId,
    pub(crate) point: Point3,
    pub(crate) tolerance: f64,
    pub(crate) outgoing: Option<CoedgeId>,
}

impl Vertex {
    pub fn name(&self) -> PersistentId {
        self.name
    }
    pub fn point(&self) -> Point3 {
        self.point
    }
    pub fn tolerance(&self) -> f64 {
        self.tolerance
    }
    pub fn outgoing(&self) -> Option<CoedgeId> {
        self.outgoing
    }
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub(crate) name: PersistentId,
    pub(crate) vertices: [VertexId; 2],
    pub(crate) curve: Curve3,
    pub(crate) range: ParameterRange,
    pub(crate) tolerance: f64,
    pub(crate) coedges: Vec<CoedgeId>,
}

impl Edge {
    pub fn name(&self) -> PersistentId {
        self.name
    }
    pub fn vertices(&self) -> [VertexId; 2] {
        self.vertices
    }
    pub fn curve(&self) -> &Curve3 {
        &self.curve
    }
    pub fn range(&self) -> ParameterRange {
        self.range
    }
    pub fn tolerance(&self) -> f64 {
        self.tolerance
    }
    pub fn coedges(&self) -> &[CoedgeId] {
        &self.coedges
    }

    /// Tangent directed from an endpoint into the retained edge interval.
    pub(crate) fn endpoint_tangent(&self, endpoint: usize) -> KernelResult<crate::Vec3> {
        let (parameter, sign) = if endpoint == 0 {
            (self.range.start, 1.0)
        } else {
            (self.range.end, -1.0)
        };
        Ok(self.curve.first_order(parameter)?.1 * (self.range.span().signum() * sign))
    }
}

#[derive(Debug, Clone)]
pub struct Coedge {
    pub(crate) name: PersistentId,
    pub(crate) edge: EdgeId,
    pub(crate) loop_id: LoopId,
    pub(crate) forward: bool,
    pub(crate) pcurve: Curve2,
    pub(crate) range: ParameterRange,
    pub(crate) next: CoedgeId,
    pub(crate) previous: CoedgeId,
}

impl Coedge {
    pub fn name(&self) -> PersistentId {
        self.name
    }
    pub fn edge(&self) -> EdgeId {
        self.edge
    }
    pub fn loop_id(&self) -> LoopId {
        self.loop_id
    }
    pub fn is_forward(&self) -> bool {
        self.forward
    }
    pub fn pcurve(&self) -> &Curve2 {
        &self.pcurve
    }
    pub fn range(&self) -> ParameterRange {
        self.range
    }
    pub fn next(&self) -> CoedgeId {
        self.next
    }
    pub fn previous(&self) -> CoedgeId {
        self.previous
    }

    /// The parameter on `edge`'s curve where this use's p-curve is at
    /// `parameter`. A use traverses its edge with an affine parameter
    /// correspondence, reversed when it runs against the edge.
    pub(crate) fn edge_parameter(&self, edge: &Edge, parameter: f64) -> f64 {
        let fraction = (parameter - self.range.start) / self.range.span();
        let fraction = if self.forward { fraction } else { 1.0 - fraction };
        edge.range.start + edge.range.span() * fraction
    }

    /// The p-curve parameter of this use at `edge`'s curve parameter; the
    /// inverse of [`Self::edge_parameter`].
    pub(crate) fn use_parameter(&self, edge: &Edge, parameter: f64) -> f64 {
        let fraction = (parameter - edge.range.start) / edge.range.span();
        let fraction = if self.forward { fraction } else { 1.0 - fraction };
        self.range.start + self.range.span() * fraction
    }
}

#[derive(Debug, Clone)]
pub struct Loop {
    pub(crate) name: PersistentId,
    pub(crate) face: FaceId,
    pub(crate) kind: LoopKind,
    pub(crate) first: Option<CoedgeId>,
}

impl Loop {
    pub fn name(&self) -> PersistentId {
        self.name
    }
    pub fn face(&self) -> FaceId {
        self.face
    }
    pub fn kind(&self) -> LoopKind {
        self.kind
    }
    pub fn first(&self) -> Option<CoedgeId> {
        self.first
    }
}

#[derive(Debug, Clone)]
pub struct Face {
    pub(crate) name: PersistentId,
    pub(crate) shell: ShellId,
    pub(crate) surface: Surface3,
    pub(crate) orientation: FaceOrientation,
    pub(crate) tolerance: f64,
    pub(crate) loops: Vec<LoopId>,
}

impl Face {
    pub fn name(&self) -> PersistentId {
        self.name
    }
    pub fn shell(&self) -> ShellId {
        self.shell
    }
    pub fn surface(&self) -> &Surface3 {
        &self.surface
    }
    pub fn orientation(&self) -> FaceOrientation {
        self.orientation
    }
    pub fn tolerance(&self) -> f64 {
        self.tolerance
    }
    /// The unit material normal at surface parameters `(u, v)`.
    pub fn oriented_normal(&self, u: f64, v: f64) -> KernelResult<crate::Vec3> {
        Ok(self.surface.normal(u, v)? * self.orientation.sign())
    }

    /// The unit material normal at the surface projection of `point`.
    pub(crate) fn oriented_normal_near(
        &self,
        point: Point3,
        tolerance: super::ModelTolerance,
    ) -> KernelResult<crate::Vec3> {
        let (u, v) = self.surface.project(point, tolerance)?.parameters;
        self.oriented_normal(u, v)
    }

    pub fn loops(&self) -> &[LoopId] {
        &self.loops
    }
}

#[derive(Debug, Clone)]
pub struct Shell {
    pub(crate) name: PersistentId,
    pub(crate) body: BodyId,
    pub(crate) kind: ShellKind,
    pub(crate) faces: Vec<FaceId>,
}

impl Shell {
    pub fn name(&self) -> PersistentId {
        self.name
    }
    pub fn body(&self) -> BodyId {
        self.body
    }
    pub fn kind(&self) -> ShellKind {
        self.kind
    }
    pub fn faces(&self) -> &[FaceId] {
        &self.faces
    }
}

#[derive(Debug, Clone)]
pub struct Body {
    pub(crate) name: PersistentId,
    pub(crate) shells: Vec<ShellId>,
}

impl Body {
    pub fn name(&self) -> PersistentId {
        self.name
    }
    pub fn shells(&self) -> &[ShellId] {
        &self.shells
    }
}

/// All BREP connectivity for a model. Mutation is restricted to transactions.
#[derive(Debug, Clone, Default)]
pub struct Topology {
    pub(crate) vertices: Arena<Vertex>,
    pub(crate) edges: Arena<Edge>,
    pub(crate) coedges: Arena<Coedge>,
    pub(crate) loops: Arena<Loop>,
    pub(crate) faces: Arena<Face>,
    pub(crate) shells: Arena<Shell>,
    pub(crate) bodies: Arena<Body>,
}

impl Topology {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertex(&self, id: VertexId) -> KernelResult<&Vertex> {
        self.vertices.get(id)
    }
    pub fn edge(&self, id: EdgeId) -> KernelResult<&Edge> {
        self.edges.get(id)
    }
    pub fn coedge(&self, id: CoedgeId) -> KernelResult<&Coedge> {
        self.coedges.get(id)
    }
    pub fn loop_(&self, id: LoopId) -> KernelResult<&Loop> {
        self.loops.get(id)
    }
    pub fn face(&self, id: FaceId) -> KernelResult<&Face> {
        self.faces.get(id)
    }
    pub fn shell(&self, id: ShellId) -> KernelResult<&Shell> {
        self.shells.get(id)
    }
    pub fn body(&self, id: BodyId) -> KernelResult<&Body> {
        self.bodies.get(id)
    }

    pub fn vertices(&self) -> impl Iterator<Item = (VertexId, &Vertex)> {
        self.vertices.iter()
    }
    pub fn edges(&self) -> impl Iterator<Item = (EdgeId, &Edge)> {
        self.edges.iter()
    }
    pub fn coedges(&self) -> impl Iterator<Item = (CoedgeId, &Coedge)> {
        self.coedges.iter()
    }
    pub fn loops(&self) -> impl Iterator<Item = (LoopId, &Loop)> {
        self.loops.iter()
    }
    pub fn faces(&self) -> impl Iterator<Item = (FaceId, &Face)> {
        self.faces.iter()
    }
    pub fn shells(&self) -> impl Iterator<Item = (ShellId, &Shell)> {
        self.shells.iter()
    }
    pub fn bodies(&self) -> impl Iterator<Item = (BodyId, &Body)> {
        self.bodies.iter()
    }

    /// Copy one complete body from another immutable topology into this one.
    ///
    /// Persistent names and exact geometry are preserved, while every arena
    /// handle is remapped. This is the internal composition primitive used by
    /// owned high-level modeling operations; callers never need to coordinate
    /// kernel-local [`BodyId`] values.
    pub(crate) fn import_body(
        &mut self,
        source: &Topology,
        source_body: BodyId,
    ) -> KernelResult<BodyId> {
        let source_body_value = source.body(source_body)?;
        let mut source_shells = Vec::new();
        let mut source_faces = Vec::new();
        let mut source_loops = Vec::new();
        let mut source_coedges = Vec::new();
        let mut source_edges = BTreeSet::new();
        let mut source_vertices = BTreeSet::new();

        for shell in source_body_value.shells() {
            source_shells.push(*shell);
            for face in source.shell(*shell)?.faces() {
                source_faces.push(*face);
                for loop_id in source.face(*face)?.loops() {
                    source_loops.push(*loop_id);
                    for coedge in source.loop_coedges(*loop_id)? {
                        source_coedges.push(coedge);
                        let edge = source.coedge(coedge)?.edge();
                        source_edges.insert(edge);
                        source_vertices.extend(source.edge(edge)?.vertices());
                    }
                }
            }
        }

        let mut vertices = BTreeMap::new();
        for old in source_vertices {
            let value = source.vertex(old)?;
            let new = self.vertices.insert(Vertex {
                name: value.name,
                point: value.point,
                tolerance: value.tolerance,
                outgoing: None,
            });
            vertices.insert(old, new);
        }

        let mut edges = BTreeMap::new();
        for old in source_edges {
            let value = source.edge(old)?;
            let new = self.edges.insert(Edge {
                name: value.name,
                vertices: [vertices[&value.vertices[0]], vertices[&value.vertices[1]]],
                curve: value.curve.clone(),
                range: value.range,
                tolerance: value.tolerance,
                coedges: Vec::new(),
            });
            edges.insert(old, new);
        }

        let body = self.bodies.insert(Body {
            name: source_body_value.name,
            shells: Vec::new(),
        });
        let mut shells = BTreeMap::new();
        for old in source_shells {
            let value = source.shell(old)?;
            let new = self.shells.insert(Shell {
                name: value.name,
                body,
                kind: value.kind,
                faces: Vec::new(),
            });
            shells.insert(old, new);
        }

        let mut faces = BTreeMap::new();
        for old in source_faces {
            let value = source.face(old)?;
            let new = self.faces.insert(Face {
                name: value.name,
                shell: shells[&value.shell],
                surface: value.surface.clone(),
                orientation: value.orientation,
                tolerance: value.tolerance,
                loops: Vec::new(),
            });
            faces.insert(old, new);
        }

        let mut loops = BTreeMap::new();
        for old in source_loops {
            let value = source.loop_(old)?;
            let new = self.loops.insert(Loop {
                name: value.name,
                face: faces[&value.face],
                kind: value.kind,
                first: None,
            });
            loops.insert(old, new);
        }

        let mut coedges = BTreeMap::new();
        for old in source_coedges.iter().copied() {
            let value = source.coedge(old)?;
            let edge = edges[&value.edge];
            let loop_id = loops[&value.loop_id];
            let new = self.coedges.insert_with(|placeholder| Coedge {
                name: value.name,
                edge,
                loop_id,
                forward: value.forward,
                pcurve: value.pcurve.clone(),
                range: value.range,
                next: placeholder,
                previous: placeholder,
            });
            coedges.insert(old, new);
        }

        for (old, new) in &coedges {
            let source_value = source.coedge(*old)?;
            let value = self.coedges.get_mut(*new)?;
            value.next = coedges[&source_value.next];
            value.previous = coedges[&source_value.previous];
        }
        for (old, new) in &loops {
            self.loops.get_mut(*new)?.first =
                source.loop_(*old)?.first.map(|first| coedges[&first]);
        }
        for (old, new) in &edges {
            self.edges.get_mut(*new)?.coedges = source
                .edge(*old)?
                .coedges
                .iter()
                .map(|coedge| coedges[coedge])
                .collect();
        }
        for (old, new) in &vertices {
            self.vertices.get_mut(*new)?.outgoing = source
                .vertex(*old)?
                .outgoing
                .and_then(|coedge| coedges.get(&coedge).copied());
        }
        for (old, new) in &faces {
            self.faces.get_mut(*new)?.loops = source
                .face(*old)?
                .loops
                .iter()
                .map(|loop_id| loops[loop_id])
                .collect();
        }
        for (old, new) in &shells {
            self.shells.get_mut(*new)?.faces = source
                .shell(*old)?
                .faces
                .iter()
                .map(|face| faces[face])
                .collect();
        }
        self.bodies.get_mut(body)?.shells = source_body_value
            .shells
            .iter()
            .map(|shell| shells[shell])
            .collect();
        Ok(body)
    }

    /// Resolve a persistent parametric name to the current generational handle.
    pub fn entity(&self, name: PersistentId) -> Option<EntityId> {
        self.vertices()
            .find_map(|(id, value)| (value.name == name).then_some(EntityId::Vertex(id)))
            .or_else(|| {
                self.edges()
                    .find_map(|(id, value)| (value.name == name).then_some(EntityId::Edge(id)))
            })
            .or_else(|| {
                self.coedges()
                    .find_map(|(id, value)| (value.name == name).then_some(EntityId::Coedge(id)))
            })
            .or_else(|| {
                self.loops()
                    .find_map(|(id, value)| (value.name == name).then_some(EntityId::Loop(id)))
            })
            .or_else(|| {
                self.faces()
                    .find_map(|(id, value)| (value.name == name).then_some(EntityId::Face(id)))
            })
            .or_else(|| {
                self.shells()
                    .find_map(|(id, value)| (value.name == name).then_some(EntityId::Shell(id)))
            })
            .or_else(|| {
                self.bodies()
                    .find_map(|(id, value)| (value.name == name).then_some(EntityId::Body(id)))
            })
    }

    pub fn coedge_vertices(&self, id: CoedgeId) -> KernelResult<[VertexId; 2]> {
        let coedge = self.coedge(id)?;
        let edge = self.edge(coedge.edge)?;
        Ok(if coedge.forward {
            edge.vertices
        } else {
            [edge.vertices[1], edge.vertices[0]]
        })
    }

    pub fn loop_coedges(&self, loop_id: LoopId) -> KernelResult<Vec<CoedgeId>> {
        let Some(first) = self.loop_(loop_id)?.first else {
            return Ok(Vec::new());
        };
        let mut result = Vec::new();
        let mut current = first;
        loop {
            result.push(current);
            current = self.coedge(current)?.next;
            if current == first {
                return Ok(result);
            }
            if result.len() > self.coedges.len() {
                return Err(super::KernelError::new(
                    super::DiagnosticKind::TopologyInvariant,
                    "coedge cycle does not terminate",
                ));
            }
        }
    }

    pub fn face_edges(&self, face_id: FaceId) -> KernelResult<Vec<EdgeId>> {
        let mut edges = BTreeSet::new();
        for loop_id in &self.face(face_id)?.loops {
            for coedge_id in self.loop_coedges(*loop_id)? {
                edges.insert(self.coedge(coedge_id)?.edge);
            }
        }
        Ok(edges.into_iter().collect())
    }

    pub fn face_vertices(&self, face_id: FaceId) -> KernelResult<Vec<VertexId>> {
        let mut vertices = BTreeSet::new();
        for edge_id in self.face_edges(face_id)? {
            vertices.extend(self.edge(edge_id)?.vertices);
        }
        Ok(vertices.into_iter().collect())
    }

    pub fn edge_faces(&self, edge_id: EdgeId) -> KernelResult<Vec<FaceId>> {
        let mut faces = BTreeSet::new();
        for coedge_id in &self.edge(edge_id)?.coedges {
            faces.insert(self.loop_(self.coedge(*coedge_id)?.loop_id)?.face);
        }
        Ok(faces.into_iter().collect())
    }

    pub fn vertex_edges(&self, vertex_id: VertexId) -> KernelResult<Vec<EdgeId>> {
        self.vertex(vertex_id)?;
        Ok(self
            .edges()
            .filter_map(|(id, edge)| edge.vertices.contains(&vertex_id).then_some(id))
            .collect())
    }

    pub fn vertex_faces(&self, vertex_id: VertexId) -> KernelResult<Vec<FaceId>> {
        let mut faces = BTreeSet::new();
        for edge_id in self.vertex_edges(vertex_id)? {
            faces.extend(self.edge_faces(edge_id)?);
        }
        Ok(faces.into_iter().collect())
    }

    pub(crate) fn body_entity_names(
        &self,
        body_id: BodyId,
    ) -> KernelResult<BTreeSet<PersistentId>> {
        let mut names = BTreeSet::new();
        let body = self.body(body_id)?;
        names.insert(body.name);
        for shell_id in &body.shells {
            let shell = self.shell(*shell_id)?;
            names.insert(shell.name);
            for face_id in &shell.faces {
                let face = self.face(*face_id)?;
                names.insert(face.name);
                for loop_id in &face.loops {
                    let loop_value = self.loop_(*loop_id)?;
                    names.insert(loop_value.name);
                    let Some(first) = loop_value.first else {
                        continue;
                    };
                    let mut current = first;
                    loop {
                        let coedge = self.coedge(current)?;
                        names.insert(coedge.name);
                        let edge = self.edge(coedge.edge)?;
                        names.insert(edge.name);
                        names.insert(self.vertex(edge.vertices[0])?.name);
                        names.insert(self.vertex(edge.vertices[1])?.name);
                        current = coedge.next;
                        if current == first {
                            break;
                        }
                    }
                }
            }
        }
        Ok(names)
    }

    /// An independently owned transformed body must be composable with its
    /// source and other copies. Fresh opaque names retain exact correspondence.
    pub(crate) fn reidentify_body(&mut self, body: BodyId, namespace: uuid::Uuid) -> KernelResult<super::OperationHistory> {
        let names = self.body_entity_names(body)?;
        let mut history = super::OperationHistory::default();
        for (local, old) in names.into_iter().enumerate() {
            let new = PersistentId { namespace, local: local as u64 };
            match self.entity(old).unwrap() {
                EntityId::Vertex(id) => self.vertices.get_mut(id)?.name = new,
                EntityId::Edge(id) => self.edges.get_mut(id)?.name = new,
                EntityId::Coedge(id) => self.coedges.get_mut(id)?.name = new,
                EntityId::Loop(id) => self.loops.get_mut(id)?.name = new,
                EntityId::Face(id) => self.faces.get_mut(id)?.name = new,
                EntityId::Shell(id) => self.shells.get_mut(id)?.name = new,
                EntityId::Body(id) => self.bodies.get_mut(id)?.name = new,
            }
            history.changed(old, super::ChangeKind::Modified, [new]);
            history.derived(new, [old], super::DerivationRole::Transform);
        }
        Ok(history)
    }
}

// Constructors are intentionally confined to Euler code.
impl Vertex {
    pub(crate) fn new(name: PersistentId, point: Point3, tolerance: f64) -> Self {
        Self {
            name,
            point,
            tolerance,
            outgoing: None,
        }
    }
}
impl Edge {
    pub(crate) fn new(
        name: PersistentId,
        vertices: [VertexId; 2],
        curve: Curve3,
        range: ParameterRange,
        tolerance: f64,
    ) -> Self {
        Self {
            name,
            vertices,
            curve,
            range,
            tolerance,
            coedges: Vec::new(),
        }
    }
}
impl Coedge {
    pub(crate) fn new(
        name: PersistentId,
        edge: EdgeId,
        loop_id: LoopId,
        forward: bool,
        pcurve: Curve2,
        range: ParameterRange,
        placeholder: CoedgeId,
    ) -> Self {
        Self {
            name,
            edge,
            loop_id,
            forward,
            pcurve,
            range,
            next: placeholder,
            previous: placeholder,
        }
    }
}
impl Loop {
    pub(crate) fn new(name: PersistentId, face: FaceId, kind: LoopKind) -> Self {
        Self {
            name,
            face,
            kind,
            first: None,
        }
    }
}
impl Face {
    pub(crate) fn new(
        name: PersistentId,
        shell: ShellId,
        surface: Surface3,
        orientation: FaceOrientation,
        tolerance: f64,
    ) -> Self {
        Self {
            name,
            shell,
            surface,
            orientation,
            tolerance,
            loops: Vec::new(),
        }
    }
}
impl Shell {
    pub(crate) fn new(name: PersistentId, body: BodyId, kind: ShellKind) -> Self {
        Self {
            name,
            body,
            kind,
            faces: Vec::new(),
        }
    }
}
impl Body {
    pub(crate) fn new(name: PersistentId) -> Self {
        Self {
            name,
            shells: Vec::new(),
        }
    }
}
