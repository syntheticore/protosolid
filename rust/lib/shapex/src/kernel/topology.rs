use crate::Point3;

use std::collections::BTreeSet;

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

/// All BREP connectivity for a model. Mutation is restricted to Euler operators.
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
