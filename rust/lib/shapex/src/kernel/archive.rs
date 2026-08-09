use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Point2, Point3, Vec2, Vec3};

use super::topology::{Body, Coedge, Edge, Face, Loop, Shell, Vertex};
use super::{
    Circle2, Circle3, Cone3, Curve2, Curve3, Cylinder3, DiagnosticKind, FaceOrientation, Kernel,
    KernelError, KernelResult, Line2, Line3, LoopKind, ModelTolerance, NurbsCurve2, NurbsCurve3,
    NurbsSurface3, ParameterRange, PersistentId, Plane3, ShellKind, Sphere3, Surface3, Topology,
    Torus3,
};

const ARCHIVE_VERSION: u32 = 3;

/// Transport-neutral, serde-compatible representation of a complete kernel.
/// Import always reconstructs into private arenas and validates before publishing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelArchive {
    pub version: u32,
    pub tolerance: [f64; 4],
    pub vertices: Vec<ArchiveVertex>,
    pub edges: Vec<ArchiveEdge>,
    pub coedges: Vec<ArchiveCoedge>,
    pub loops: Vec<ArchiveLoop>,
    pub faces: Vec<ArchiveFace>,
    pub shells: Vec<ArchiveShell>,
    pub bodies: Vec<ArchiveBody>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveVertex {
    pub name: PersistentId,
    pub point: [f64; 3],
    pub tolerance: f64,
    pub outgoing: Option<PersistentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEdge {
    pub name: PersistentId,
    pub vertices: [PersistentId; 2],
    pub curve: ArchiveCurve3,
    pub range: [f64; 2],
    pub tolerance: f64,
    pub coedges: Vec<PersistentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveCoedge {
    pub name: PersistentId,
    pub edge: PersistentId,
    pub loop_name: PersistentId,
    pub forward: bool,
    pub pcurve: ArchiveCurve2,
    pub range: [f64; 2],
    pub next: PersistentId,
    pub previous: PersistentId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveLoop {
    pub name: PersistentId,
    pub face: PersistentId,
    pub kind: ArchiveLoopKind,
    pub first: Option<PersistentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveFace {
    pub name: PersistentId,
    pub shell: PersistentId,
    pub surface: ArchiveSurface3,
    pub orientation: ArchiveFaceOrientation,
    pub tolerance: f64,
    pub loops: Vec<PersistentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveShell {
    pub name: PersistentId,
    pub body: PersistentId,
    pub kind: ArchiveShellKind,
    pub faces: Vec<PersistentId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveBody {
    pub name: PersistentId,
    pub shells: Vec<PersistentId>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ArchiveLoopKind {
    Outer,
    Inner,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ArchiveFaceOrientation {
    Forward,
    Reversed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ArchiveShellKind {
    Open,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArchiveCurve2 {
    Line {
        origin: [f64; 2],
        direction: [f64; 2],
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
    Nurbs {
        degree: usize,
        control_points: Vec<[f64; 2]>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArchiveCurve3 {
    Line {
        origin: [f64; 3],
        direction: [f64; 3],
    },
    Circle {
        center: [f64; 3],
        x_radius: [f64; 3],
        y_radius: [f64; 3],
    },
    Nurbs {
        degree: usize,
        control_points: Vec<[f64; 3]>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArchiveSurface3 {
    Plane {
        origin: [f64; 3],
        u: [f64; 3],
        v: [f64; 3],
    },
    Cylinder {
        origin: [f64; 3],
        axis: [f64; 3],
        x_radius: [f64; 3],
        y_radius: [f64; 3],
    },
    Cone {
        apex: [f64; 3],
        axis: [f64; 3],
        x_radius: [f64; 3],
        y_radius: [f64; 3],
    },
    Sphere {
        center: [f64; 3],
        x_radius: [f64; 3],
        y_radius: [f64; 3],
        z_radius: [f64; 3],
    },
    Torus {
        center: [f64; 3],
        x_major: [f64; 3],
        y_major: [f64; 3],
        minor_radius: f64,
    },
    Nurbs {
        degrees: (usize, usize),
        control_points: Vec<Vec<[f64; 3]>>,
        weights: Vec<Vec<f64>>,
        u_knots: Vec<f64>,
        v_knots: Vec<f64>,
    },
}

impl Kernel {
    pub fn archive(&self) -> KernelArchive {
        KernelArchive::from_kernel(self)
    }

    pub fn from_archive(archive: KernelArchive) -> KernelResult<Self> {
        archive.into_kernel()
    }

    pub fn to_ron(&self) -> KernelResult<String> {
        ron::ser::to_string_pretty(&self.archive(), ron::ser::PrettyConfig::default()).map_err(
            |error| KernelError::new(DiagnosticKind::SerializationFailure, error.to_string()),
        )
    }

    pub fn from_ron(source: &str) -> KernelResult<Self> {
        let archive: KernelArchive = ron::from_str(source).map_err(|error| {
            KernelError::new(DiagnosticKind::SerializationFailure, error.to_string())
        })?;
        archive.into_kernel()
    }
}

impl KernelArchive {
    pub fn from_kernel(kernel: &Kernel) -> Self {
        let topology = kernel.topology();
        Self {
            version: ARCHIVE_VERSION,
            tolerance: [
                kernel.tolerance().linear,
                kernel.tolerance().relative,
                kernel.tolerance().angular,
                kernel.tolerance().parameter,
            ],
            vertices: topology
                .vertices()
                .map(|(_, value)| ArchiveVertex {
                    name: value.name(),
                    point: point3(value.point()),
                    tolerance: value.tolerance(),
                    outgoing: value
                        .outgoing()
                        .and_then(|id| topology.coedge(id).ok())
                        .map(|value| value.name()),
                })
                .collect(),
            edges: topology
                .edges()
                .map(|(_, value)| ArchiveEdge {
                    name: value.name(),
                    vertices: value.vertices().map(|id| {
                        topology
                            .vertex(id)
                            .map(|value| value.name())
                            .unwrap_or(value.name())
                    }),
                    curve: ArchiveCurve3::from_curve(value.curve()),
                    range: range(value.range()),
                    tolerance: value.tolerance(),
                    coedges: value
                        .coedges()
                        .iter()
                        .filter_map(|id| topology.coedge(*id).ok().map(|value| value.name()))
                        .collect(),
                })
                .collect(),
            coedges: topology
                .coedges()
                .map(|(_, value)| ArchiveCoedge {
                    name: value.name(),
                    edge: topology
                        .edge(value.edge())
                        .map(|edge| edge.name())
                        .unwrap_or(value.name()),
                    loop_name: topology
                        .loop_(value.loop_id())
                        .map(|loop_value| loop_value.name())
                        .unwrap_or(value.name()),
                    forward: value.is_forward(),
                    pcurve: ArchiveCurve2::from_curve(value.pcurve()),
                    range: range(value.range()),
                    next: topology
                        .coedge(value.next())
                        .map(|next| next.name())
                        .unwrap_or(value.name()),
                    previous: topology
                        .coedge(value.previous())
                        .map(|previous| previous.name())
                        .unwrap_or(value.name()),
                })
                .collect(),
            loops: topology
                .loops()
                .map(|(_, value)| ArchiveLoop {
                    name: value.name(),
                    face: topology
                        .face(value.face())
                        .map(|face| face.name())
                        .unwrap_or(value.name()),
                    kind: value.kind().into(),
                    first: value
                        .first()
                        .and_then(|id| topology.coedge(id).ok())
                        .map(|value| value.name()),
                })
                .collect(),
            faces: topology
                .faces()
                .map(|(_, value)| ArchiveFace {
                    name: value.name(),
                    shell: topology
                        .shell(value.shell())
                        .map(|shell| shell.name())
                        .unwrap_or(value.name()),
                    surface: ArchiveSurface3::from_surface(value.surface()),
                    orientation: value.orientation().into(),
                    tolerance: value.tolerance(),
                    loops: value
                        .loops()
                        .iter()
                        .filter_map(|id| topology.loop_(*id).ok().map(|value| value.name()))
                        .collect(),
                })
                .collect(),
            shells: topology
                .shells()
                .map(|(_, value)| ArchiveShell {
                    name: value.name(),
                    body: topology
                        .body(value.body())
                        .map(|body| body.name())
                        .unwrap_or(value.name()),
                    kind: value.kind().into(),
                    faces: value
                        .faces()
                        .iter()
                        .filter_map(|id| topology.face(*id).ok().map(|value| value.name()))
                        .collect(),
                })
                .collect(),
            bodies: topology
                .bodies()
                .map(|(_, value)| ArchiveBody {
                    name: value.name(),
                    shells: value
                        .shells()
                        .iter()
                        .filter_map(|id| topology.shell(*id).ok().map(|value| value.name()))
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn into_kernel(self) -> KernelResult<Kernel> {
        if self.version != ARCHIVE_VERSION {
            return Err(KernelError::new(
                DiagnosticKind::SerializationFailure,
                format!("unsupported Shapex archive version {}", self.version),
            ));
        }
        let tolerance = ModelTolerance::new(
            self.tolerance[0],
            self.tolerance[1],
            self.tolerance[2],
            self.tolerance[3],
        );
        tolerance.validate()?;
        let mut topology = Topology::new();
        let mut bodies = BTreeMap::new();
        for value in &self.bodies {
            unique_insert(
                &mut bodies,
                value.name,
                topology.bodies.insert(Body::new(value.name)),
            )?;
        }
        let mut shells = BTreeMap::new();
        for value in &self.shells {
            let body = required(&bodies, value.body, "shell body")?;
            unique_insert(
                &mut shells,
                value.name,
                topology
                    .shells
                    .insert(Shell::new(value.name, body, value.kind.into())),
            )?;
        }
        let mut faces = BTreeMap::new();
        for value in &self.faces {
            let shell = required(&shells, value.shell, "face shell")?;
            let surface = value.surface.to_surface(tolerance)?;
            unique_insert(
                &mut faces,
                value.name,
                topology.faces.insert(Face::new(
                    value.name,
                    shell,
                    surface,
                    value.orientation.into(),
                    value.tolerance,
                )),
            )?;
        }
        let mut loops = BTreeMap::new();
        for value in &self.loops {
            let face = required(&faces, value.face, "loop face")?;
            unique_insert(
                &mut loops,
                value.name,
                topology
                    .loops
                    .insert(Loop::new(value.name, face, value.kind.into())),
            )?;
        }
        let mut vertices = BTreeMap::new();
        for value in &self.vertices {
            unique_insert(
                &mut vertices,
                value.name,
                topology.vertices.insert(Vertex::new(
                    value.name,
                    to_point3(value.point),
                    value.tolerance,
                )),
            )?;
        }
        let mut edges = BTreeMap::new();
        for value in &self.edges {
            let endpoints = [
                required(&vertices, value.vertices[0], "edge vertex")?,
                required(&vertices, value.vertices[1], "edge vertex")?,
            ];
            unique_insert(
                &mut edges,
                value.name,
                topology.edges.insert(Edge::new(
                    value.name,
                    endpoints,
                    value.curve.to_curve(tolerance)?,
                    to_range(value.range)?,
                    value.tolerance,
                )),
            )?;
        }
        let mut coedges = BTreeMap::new();
        for value in &self.coedges {
            let edge = required(&edges, value.edge, "coedge edge")?;
            let loop_id = required(&loops, value.loop_name, "coedge loop")?;
            let pcurve = value.pcurve.to_curve()?;
            let range = to_range(value.range)?;
            let handle = topology.coedges.insert_with(|self_id| {
                Coedge::new(
                    value.name,
                    edge,
                    loop_id,
                    value.forward,
                    pcurve,
                    range,
                    self_id,
                )
            });
            unique_insert(&mut coedges, value.name, handle)?;
        }

        for value in &self.coedges {
            let id = required(&coedges, value.name, "coedge")?;
            let coedge = topology.coedges.get_mut(id)?;
            coedge.next = required(&coedges, value.next, "coedge next")?;
            coedge.previous = required(&coedges, value.previous, "coedge previous")?;
        }
        for value in &self.vertices {
            let id = required(&vertices, value.name, "vertex")?;
            topology.vertices.get_mut(id)?.outgoing = value
                .outgoing
                .map(|name| required(&coedges, name, "vertex outgoing"))
                .transpose()?;
        }
        for value in &self.edges {
            let id = required(&edges, value.name, "edge")?;
            topology.edges.get_mut(id)?.coedges = value
                .coedges
                .iter()
                .map(|name| required(&coedges, *name, "edge coedge"))
                .collect::<KernelResult<_>>()?;
        }
        for value in &self.loops {
            let id = required(&loops, value.name, "loop")?;
            topology.loops.get_mut(id)?.first = value
                .first
                .map(|name| required(&coedges, name, "loop first"))
                .transpose()?;
        }
        for value in &self.faces {
            let id = required(&faces, value.name, "face")?;
            topology.faces.get_mut(id)?.loops = value
                .loops
                .iter()
                .map(|name| required(&loops, *name, "face loop"))
                .collect::<KernelResult<_>>()?;
        }
        for value in &self.shells {
            let id = required(&shells, value.name, "shell")?;
            topology.shells.get_mut(id)?.faces = value
                .faces
                .iter()
                .map(|name| required(&faces, *name, "shell face"))
                .collect::<KernelResult<_>>()?;
        }
        for value in &self.bodies {
            let id = required(&bodies, value.name, "body")?;
            topology.bodies.get_mut(id)?.shells = value
                .shells
                .iter()
                .map(|name| required(&shells, *name, "body shell"))
                .collect::<KernelResult<_>>()?;
        }

        super::validation::validate(
            &topology,
            tolerance,
            super::validation::ValidationMode::Published,
        )
        .into_result()?;
        let kernel = Kernel {
            tolerance,
            topology,
        };
        kernel.validate().into_result()?;
        Ok(kernel)
    }
}

impl ArchiveCurve2 {
    fn from_curve(curve: &Curve2) -> Self {
        match curve {
            Curve2::Line(value) => Self::Line {
                origin: point2(value.origin()),
                direction: vec2(value.direction()),
            },
            Curve2::Circle(value) => Self::Circle {
                center: point2(value.center()),
                radius: value.radius(),
            },
            Curve2::Nurbs(value) => Self::Nurbs {
                degree: value.degree(),
                control_points: value.control_points().iter().copied().map(point2).collect(),
                weights: value.weights().to_vec(),
                knots: value.knots().to_vec(),
            },
        }
    }

    fn to_curve(&self) -> KernelResult<Curve2> {
        Ok(match self {
            Self::Line { origin, direction } => {
                Line2::new(to_point2(*origin), to_vec2(*direction))?.into()
            }
            Self::Circle { center, radius } => Circle2::new(to_point2(*center), *radius)?.into(),
            Self::Nurbs {
                degree,
                control_points,
                weights,
                knots,
            } => NurbsCurve2::new(
                *degree,
                control_points.iter().copied().map(to_point2).collect(),
                weights.clone(),
                knots.clone(),
            )?
            .into(),
        })
    }
}

impl ArchiveCurve3 {
    fn from_curve(curve: &Curve3) -> Self {
        match curve {
            Curve3::Line(value) => Self::Line {
                origin: point3(value.origin()),
                direction: vec3(value.direction()),
            },
            Curve3::Circle(value) => Self::Circle {
                center: point3(value.center()),
                x_radius: vec3(value.x_radius()),
                y_radius: vec3(value.y_radius()),
            },
            Curve3::Nurbs(value) => Self::Nurbs {
                degree: value.degree(),
                control_points: value.control_points().iter().copied().map(point3).collect(),
                weights: value.weights().to_vec(),
                knots: value.knots().to_vec(),
            },
        }
    }

    fn to_curve(&self, tolerance: ModelTolerance) -> KernelResult<Curve3> {
        Ok(match self {
            Self::Line { origin, direction } => {
                Line3::new(to_point3(*origin), to_vec3(*direction))?.into()
            }
            Self::Circle {
                center,
                x_radius,
                y_radius,
            } => Circle3::new(
                to_point3(*center),
                to_vec3(*x_radius),
                to_vec3(*y_radius),
                tolerance,
            )?
            .into(),
            Self::Nurbs {
                degree,
                control_points,
                weights,
                knots,
            } => NurbsCurve3::new(
                *degree,
                control_points.iter().copied().map(to_point3).collect(),
                weights.clone(),
                knots.clone(),
            )?
            .into(),
        })
    }
}

impl ArchiveSurface3 {
    fn from_surface(surface: &Surface3) -> Self {
        match surface {
            Surface3::Plane(value) => Self::Plane {
                origin: point3(value.origin()),
                u: vec3(value.u()),
                v: vec3(value.v()),
            },
            Surface3::Cylinder(value) => Self::Cylinder {
                origin: point3(value.origin()),
                axis: vec3(value.axis()),
                x_radius: vec3(value.x_radius()),
                y_radius: vec3(value.y_radius()),
            },
            Surface3::Cone(value) => Self::Cone {
                apex: point3(value.apex()),
                axis: vec3(value.axis()),
                x_radius: vec3(value.x_radius()),
                y_radius: vec3(value.y_radius()),
            },
            Surface3::Sphere(value) => Self::Sphere {
                center: point3(value.center()),
                x_radius: vec3(value.x_radius()),
                y_radius: vec3(value.y_radius()),
                z_radius: vec3(value.z_radius()),
            },
            Surface3::Torus(value) => Self::Torus {
                center: point3(value.center()),
                x_major: vec3(value.x_major()),
                y_major: vec3(value.y_major()),
                minor_radius: value.minor_radius(),
            },
            Surface3::Nurbs(value) => Self::Nurbs {
                degrees: value.degrees(),
                control_points: value
                    .control_points()
                    .iter()
                    .map(|row| row.iter().copied().map(point3).collect())
                    .collect(),
                weights: value.weights().to_vec(),
                u_knots: value.knots().0.to_vec(),
                v_knots: value.knots().1.to_vec(),
            },
        }
    }

    fn to_surface(&self, tolerance: ModelTolerance) -> KernelResult<Surface3> {
        Ok(match self {
            Self::Plane { origin, u, v } => {
                Plane3::new(to_point3(*origin), to_vec3(*u), to_vec3(*v), tolerance)?.into()
            }
            Self::Cylinder {
                origin,
                axis,
                x_radius,
                y_radius,
            } => Cylinder3::new(
                to_point3(*origin),
                to_vec3(*axis),
                to_vec3(*x_radius),
                to_vec3(*y_radius),
                tolerance,
            )?
            .into(),
            Self::Cone {
                apex,
                axis,
                x_radius,
                y_radius,
            } => Cone3::new(
                to_point3(*apex),
                to_vec3(*axis),
                to_vec3(*x_radius),
                to_vec3(*y_radius),
                tolerance,
            )?
            .into(),
            Self::Sphere {
                center,
                x_radius,
                y_radius,
                z_radius,
            } => Sphere3::new(
                to_point3(*center),
                to_vec3(*x_radius),
                to_vec3(*y_radius),
                to_vec3(*z_radius),
                tolerance,
            )?
            .into(),
            Self::Torus {
                center,
                x_major,
                y_major,
                minor_radius,
            } => Torus3::new(
                to_point3(*center),
                to_vec3(*x_major),
                to_vec3(*y_major),
                *minor_radius,
                tolerance,
            )?
            .into(),
            Self::Nurbs {
                degrees,
                control_points,
                weights,
                u_knots,
                v_knots,
            } => NurbsSurface3::new(
                *degrees,
                control_points
                    .iter()
                    .map(|row| row.iter().copied().map(to_point3).collect())
                    .collect(),
                weights.clone(),
                (u_knots.clone(), v_knots.clone()),
            )?
            .into(),
        })
    }
}

fn point2(value: Point2) -> [f64; 2] {
    [value.x, value.y]
}
fn vec2(value: Vec2) -> [f64; 2] {
    [value.x, value.y]
}
fn point3(value: Point3) -> [f64; 3] {
    [value.x, value.y, value.z]
}
fn vec3(value: Vec3) -> [f64; 3] {
    [value.x, value.y, value.z]
}
fn to_point2(value: [f64; 2]) -> Point2 {
    Point2::new(value[0], value[1])
}
fn to_vec2(value: [f64; 2]) -> Vec2 {
    Vec2::new(value[0], value[1])
}
fn to_point3(value: [f64; 3]) -> Point3 {
    Point3::new(value[0], value[1], value[2])
}
fn to_vec3(value: [f64; 3]) -> Vec3 {
    Vec3::new(value[0], value[1], value[2])
}
fn range(value: ParameterRange) -> [f64; 2] {
    [value.start, value.end]
}
fn to_range(value: [f64; 2]) -> KernelResult<ParameterRange> {
    ParameterRange::new(value[0], value[1])
}

fn unique_insert<T: Copy>(
    map: &mut BTreeMap<PersistentId, T>,
    name: PersistentId,
    value: T,
) -> KernelResult<()> {
    if map.insert(name, value).is_some() {
        return Err(archive_error("persistent entity name is duplicated"));
    }
    Ok(())
}

fn required<T: Copy>(
    map: &BTreeMap<PersistentId, T>,
    name: PersistentId,
    role: &str,
) -> KernelResult<T> {
    map.get(&name)
        .copied()
        .ok_or_else(|| archive_error(format!("archive references a missing {role}")))
}

fn archive_error(message: impl Into<String>) -> KernelError {
    KernelError::new(DiagnosticKind::SerializationFailure, message)
}

impl From<LoopKind> for ArchiveLoopKind {
    fn from(value: LoopKind) -> Self {
        match value {
            LoopKind::Outer => Self::Outer,
            LoopKind::Inner => Self::Inner,
        }
    }
}
impl From<ArchiveLoopKind> for LoopKind {
    fn from(value: ArchiveLoopKind) -> Self {
        match value {
            ArchiveLoopKind::Outer => Self::Outer,
            ArchiveLoopKind::Inner => Self::Inner,
        }
    }
}
impl From<FaceOrientation> for ArchiveFaceOrientation {
    fn from(value: FaceOrientation) -> Self {
        match value {
            FaceOrientation::Forward => Self::Forward,
            FaceOrientation::Reversed => Self::Reversed,
        }
    }
}
impl From<ArchiveFaceOrientation> for FaceOrientation {
    fn from(value: ArchiveFaceOrientation) -> Self {
        match value {
            ArchiveFaceOrientation::Forward => Self::Forward,
            ArchiveFaceOrientation::Reversed => Self::Reversed,
        }
    }
}
impl From<ShellKind> for ArchiveShellKind {
    fn from(value: ShellKind) -> Self {
        match value {
            ShellKind::Open => Self::Open,
            ShellKind::Closed => Self::Closed,
        }
    }
}
impl From<ArchiveShellKind> for ShellKind {
    fn from(value: ArchiveShellKind) -> Self {
        match value {
            ArchiveShellKind::Open => Self::Open,
            ArchiveShellKind::Closed => Self::Closed,
        }
    }
}
