use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use super::{
    CoedgeId, Diagnostic, DiagnosticKind, FaceOrientation, KernelError, KernelResult, LoopKind,
    ModelTolerance, PersistentId, ShellKind, Topology,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationMode {
    /// Euler workspace: connectivity must be internally consistent, but shells may
    /// be open and loops may temporarily contain Euler spikes.
    Construction,
    /// Published model: every body must bound a closed, geometrically consistent volume.
    Published,
    /// Published topology accompanied by a separately validated analytical
    /// physical certificate.
    PublishedWithCertificate,
}

impl ValidationMode {
    fn is_published(self) -> bool {
        matches!(self, Self::Published | Self::PublishedWithCertificate)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    fn error(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn into_result(self) -> KernelResult<()> {
        if self.is_valid() {
            return Ok(());
        }
        let mut diagnostics = self.diagnostics.into_iter();
        let Some(first) = diagnostics.next() else {
            return Ok(());
        };
        let mut error = KernelError::from_diagnostic(first);
        for diagnostic in diagnostics {
            error.push(diagnostic);
        }
        Err(error)
    }
}

pub(crate) fn validate(
    topology: &Topology,
    tolerance: ModelTolerance,
    mode: ValidationMode,
) -> ValidationReport {
    let mut report = ValidationReport::default();
    validate_names(topology, &mut report);
    validate_ownership(topology, mode, &mut report);
    validate_loops(topology, mode, &mut report);
    validate_edges(topology, mode, &mut report);
    validate_vertices(topology, mode, &mut report);
    if mode.is_published() {
        validate_manifold_vertices(topology, &mut report);
        validate_shell_euler_characteristic(topology, &mut report);
    }
    validate_geometry(topology, tolerance, mode, &mut report);
    if mode == ValidationMode::Published {
        validate_physical_volumes(topology, tolerance, &mut report);
    }
    report
}

fn validate_physical_volumes(
    topology: &Topology,
    tolerance: ModelTolerance,
    report: &mut ValidationReport,
) {
    if let Err(diagnostics) = super::physical::certify_topology(topology, tolerance) {
        for diagnostic in diagnostics {
            report.error(diagnostic);
        }
    }
}

fn duplicate_name(
    seen: &mut BTreeSet<PersistentId>,
    name: PersistentId,
    report: &mut ValidationReport,
) {
    if !seen.insert(name) {
        report.error(
            Diagnostic::new(
                DiagnosticKind::TopologyInvariant,
                "persistent topology names must be unique within a model",
            )
            .with_entity(name),
        );
    }
}

fn validate_names(topology: &Topology, report: &mut ValidationReport) {
    let mut seen = BTreeSet::new();
    for (_, value) in topology.vertices() {
        duplicate_name(&mut seen, value.name, report);
    }
    for (_, value) in topology.edges() {
        duplicate_name(&mut seen, value.name, report);
    }
    for (_, value) in topology.coedges() {
        duplicate_name(&mut seen, value.name, report);
    }
    for (_, value) in topology.loops() {
        duplicate_name(&mut seen, value.name, report);
    }
    for (_, value) in topology.faces() {
        duplicate_name(&mut seen, value.name, report);
    }
    for (_, value) in topology.shells() {
        duplicate_name(&mut seen, value.name, report);
    }
    for (_, value) in topology.bodies() {
        duplicate_name(&mut seen, value.name, report);
    }
}

fn validate_ownership(topology: &Topology, mode: ValidationMode, report: &mut ValidationReport) {
    let mut shell_owners = BTreeMap::new();
    let mut face_owners = BTreeMap::new();
    let mut loop_owners = BTreeMap::new();
    for (body_id, body) in topology.bodies() {
        if mode.is_published() && body.shells.is_empty() {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "a body must contain a shell",
                )
                .with_entity(body.name),
            );
        }
        let mut unique = BTreeSet::new();
        for shell_id in &body.shells {
            if !unique.insert(*shell_id) {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "a body may not list the same shell more than once",
                    )
                    .with_entity(body.name),
                );
            }
            *shell_owners.entry(*shell_id).or_insert(0usize) += 1;
            match topology.shell(*shell_id) {
                Ok(shell) if shell.body == body_id => {}
                _ => report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "body-to-shell ownership is not reciprocal",
                    )
                    .with_entity(body.name),
                ),
            }
        }
    }

    for (shell_id, shell) in topology.shells() {
        if shell_owners.get(&shell_id).copied() != Some(1) || topology.body(shell.body).is_err() {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "every shell must be owned exactly once by its recorded body",
                )
                .with_entity(shell.name),
            );
        }
        if mode.is_published() && shell.kind != ShellKind::Closed {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "published bodies may not contain open shells",
                )
                .with_entity(shell.name),
            );
        }
        let mut unique = BTreeSet::new();
        for face_id in &shell.faces {
            if !unique.insert(*face_id) {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "a shell may not list the same face more than once",
                    )
                    .with_entity(shell.name),
                );
            }
            *face_owners.entry(*face_id).or_insert(0usize) += 1;
            match topology.face(*face_id) {
                Ok(face) if face.shell == shell_id => {}
                _ => report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "shell-to-face ownership is not reciprocal",
                    )
                    .with_entity(shell.name),
                ),
            }
        }
    }

    for (face_id, face) in topology.faces() {
        if face_owners.get(&face_id).copied() != Some(1) || topology.shell(face.shell).is_err() {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "every face must be owned exactly once by its recorded shell",
                )
                .with_entity(face.name),
            );
        }
        let outer_count = face
            .loops
            .iter()
            .filter_map(|id| topology.loop_(*id).ok())
            .filter(|loop_| loop_.kind == LoopKind::Outer)
            .count();
        if outer_count != 1 {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "every face must own exactly one outer loop",
                )
                .with_entity(face.name),
            );
        }
        let mut unique = BTreeSet::new();
        for loop_id in &face.loops {
            if !unique.insert(*loop_id) {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "a face may not list the same loop more than once",
                    )
                    .with_entity(face.name),
                );
            }
            *loop_owners.entry(*loop_id).or_insert(0usize) += 1;
            match topology.loop_(*loop_id) {
                Ok(loop_) if loop_.face == face_id => {}
                _ => report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "face-to-loop ownership is not reciprocal",
                    )
                    .with_entity(face.name),
                ),
            }
        }
    }

    for (loop_id, loop_value) in topology.loops() {
        if loop_owners.get(&loop_id).copied() != Some(1) || topology.face(loop_value.face).is_err()
        {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "every loop must be owned exactly once by its recorded face",
                )
                .with_entity(loop_value.name),
            );
        }
    }
}

fn validate_loops(topology: &Topology, mode: ValidationMode, report: &mut ValidationReport) {
    let mut globally_reached = BTreeSet::new();
    for (loop_id, loop_) in topology.loops() {
        let Some(first) = loop_.first else {
            if mode.is_published() {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "a published face loop may not be empty",
                    )
                    .with_entity(loop_.name),
                );
            }
            continue;
        };

        let mut current = first;
        let mut visited = BTreeSet::<CoedgeId>::new();
        loop {
            if !visited.insert(current) {
                if current != first {
                    report.error(
                        Diagnostic::new(
                            DiagnosticKind::TopologyInvariant,
                            "a coedge cycle repeated before returning to its loop start",
                        )
                        .with_entity(loop_.name),
                    );
                }
                break;
            }
            if !globally_reached.insert(current) {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "a coedge may be reachable from exactly one loop cycle",
                    )
                    .with_entity(loop_.name),
                );
            }
            let coedge = match topology.coedge(current) {
                Ok(value) => value,
                Err(_) => {
                    report.error(
                        Diagnostic::new(
                            DiagnosticKind::TopologyInvariant,
                            "loop references a missing coedge",
                        )
                        .with_entity(loop_.name),
                    );
                    break;
                }
            };
            if coedge.loop_id != loop_id {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "coedge belongs to a different loop",
                    )
                    .with_entity(coedge.name),
                );
            }
            match topology.coedge(coedge.next) {
                Ok(next) if next.previous == current => {}
                _ => report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "coedge next/previous links are not reciprocal",
                    )
                    .with_entity(coedge.name),
                ),
            }
            if let (Ok([_, end]), Ok([next_start, _])) = (
                topology.coedge_vertices(current),
                topology.coedge_vertices(coedge.next),
            ) {
                if end != next_start {
                    report.error(
                        Diagnostic::new(
                            DiagnosticKind::TopologyInvariant,
                            "consecutive coedges do not share an endpoint vertex",
                        )
                        .with_entity(coedge.name),
                    );
                }
            }
            current = coedge.next;
            if current == first {
                break;
            }
            if visited.len() > topology.coedges.len() {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "coedge loop does not terminate",
                    )
                    .with_entity(loop_.name),
                );
                break;
            }
        }
        let is_closed_single_edge = visited.len() == 1
            && topology
                .coedge(first)
                .and_then(|coedge| topology.edge(coedge.edge))
                .is_ok_and(|edge| edge.vertices[0] == edge.vertices[1]);
        if mode.is_published() && visited.len() < 2 && !is_closed_single_edge {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "a published boundary loop needs at least two coedges unless it is one closed edge",
                )
                .with_entity(loop_.name),
            );
        }
    }
    for (coedge_id, coedge) in topology.coedges() {
        if !globally_reached.contains(&coedge_id) {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "every coedge must be reachable from its recorded loop",
                )
                .with_entity(coedge.name),
            );
        }
    }
}

fn validate_edges(topology: &Topology, mode: ValidationMode, report: &mut ValidationReport) {
    for (edge_id, edge) in topology.edges() {
        if topology.vertex(edge.vertices[0]).is_err() || topology.vertex(edge.vertices[1]).is_err()
        {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "edge endpoint handles must refer to live vertices",
                )
                .with_entity(edge.name),
            );
        }
        if edge.coedges.is_empty() || edge.coedges.len() > 2 {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "a manifold edge must have one or two coedge uses",
                )
                .with_entity(edge.name),
            );
        }
        if mode.is_published() && edge.coedges.len() != 2 {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "every edge of a closed shell needs two coedges",
                )
                .with_entity(edge.name),
            );
        }
        let mut unique = BTreeSet::new();
        for coedge_id in &edge.coedges {
            if !unique.insert(*coedge_id) {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "an edge may not list the same coedge more than once",
                    )
                    .with_entity(edge.name),
                );
            }
            match topology.coedge(*coedge_id) {
                Ok(coedge) if coedge.edge == edge_id => {}
                _ => report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "edge-to-coedge ownership is not reciprocal",
                    )
                    .with_entity(edge.name),
                ),
            }
        }
        if edge.coedges.len() == 2 {
            let first = topology.coedge(edge.coedges[0]);
            let second = topology.coedge(edge.coedges[1]);
            if let (Ok(first), Ok(second)) = (first, second) {
                if first.forward == second.forward {
                    report.error(
                        Diagnostic::new(
                            DiagnosticKind::TopologyInvariant,
                            "the two uses of a manifold edge must have opposite orientations",
                        )
                        .with_entity(edge.name),
                    );
                }
                let first_shell = topology
                    .loop_(first.loop_id)
                    .and_then(|loop_| topology.face(loop_.face))
                    .map(|face| face.shell);
                let second_shell = topology
                    .loop_(second.loop_id)
                    .and_then(|loop_| topology.face(loop_.face))
                    .map(|face| face.shell);
                if !matches!((first_shell, second_shell), (Ok(a), Ok(b)) if a == b) {
                    report.error(
                        Diagnostic::new(
                            DiagnosticKind::TopologyInvariant,
                            "both uses of an edge must belong to the same shell",
                        )
                        .with_entity(edge.name),
                    );
                }
            }
        }
    }
    for (coedge_id, coedge) in topology.coedges() {
        match topology.edge(coedge.edge) {
            Ok(edge) if edge.coedges.contains(&coedge_id) => {}
            _ => report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "coedge-to-edge ownership is not reciprocal",
                )
                .with_entity(coedge.name),
            ),
        }
    }
}

fn validate_vertices(topology: &Topology, mode: ValidationMode, report: &mut ValidationReport) {
    for (vertex_id, vertex) in topology.vertices() {
        let outgoing_uses: Vec<_> = topology
            .coedges()
            .filter_map(|(id, _)| {
                topology
                    .coedge_vertices(id)
                    .ok()
                    .and_then(|vertices| (vertices[0] == vertex_id).then_some(id))
            })
            .collect();
        if let Some(outgoing) = vertex.outgoing {
            match topology.coedge_vertices(outgoing) {
                Ok([start, _]) if start == vertex_id => {}
                _ => report.error(
                    Diagnostic::new(
                        DiagnosticKind::TopologyInvariant,
                        "vertex outgoing coedge does not originate at that vertex",
                    )
                    .with_entity(vertex.name),
                ),
            }
        } else if !outgoing_uses.is_empty() {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "an incident vertex must reference an outgoing coedge",
                )
                .with_entity(vertex.name),
            );
        }
        if mode.is_published() && outgoing_uses.is_empty() {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "a published body may not contain isolated vertices",
                )
                .with_entity(vertex.name),
            );
        }
    }
}

fn validate_manifold_vertices(topology: &Topology, report: &mut ValidationReport) {
    for (vertex_id, vertex) in topology.vertices() {
        let mut shell = None;
        // Use edge-end darts rather than edge ids as link-graph vertices. A
        // closed edge has two distinct topological ends at the same vertex;
        // collapsing those ends to one edge id would incorrectly give its
        // artificial parameter-seam vertex degree four.
        let mut link = BTreeMap::<(super::EdgeId, u8), Vec<(super::EdgeId, u8)>>::new();
        for (coedge_id, coedge) in topology.coedges() {
            let Ok([start, _]) = topology.coedge_vertices(coedge_id) else {
                continue;
            };
            if start != vertex_id {
                continue;
            }
            let Ok(loop_value) = topology.loop_(coedge.loop_id) else {
                continue;
            };
            let Ok(face) = topology.face(loop_value.face) else {
                continue;
            };
            if let Some(existing) = shell {
                if existing != face.shell {
                    report.error(
                        Diagnostic::new(
                            DiagnosticKind::TopologyInvariant,
                            "a manifold vertex may not be shared by multiple shells",
                        )
                        .with_entity(vertex.name),
                    );
                }
            } else {
                shell = Some(face.shell);
            }
            let Ok(previous) = topology.coedge(coedge.previous) else {
                continue;
            };
            let incoming = (previous.edge, if previous.forward { 1 } else { 0 });
            let outgoing = (coedge.edge, if coedge.forward { 0 } else { 1 });
            link.entry(incoming).or_default().push(outgoing);
            link.entry(outgoing).or_default().push(incoming);
        }
        if link.is_empty() {
            continue;
        }
        if link.values().any(|neighbors| neighbors.len() != 2) {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "the incident face corners at a closed-shell vertex must form one two-regular link",
                )
                .with_entity(vertex.name),
            );
            continue;
        }
        let Some(start) = link.keys().next().copied() else {
            continue;
        };
        let mut reached = BTreeSet::new();
        let mut pending = vec![start];
        while let Some(edge) = pending.pop() {
            if !reached.insert(edge) {
                continue;
            }
            if let Some(neighbors) = link.get(&edge) {
                pending.extend(neighbors.iter().copied());
            }
        }
        if reached.len() != link.len() {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "the incident faces at a vertex form disconnected fans",
                )
                .with_entity(vertex.name),
            );
        }
    }
}

fn validate_shell_euler_characteristic(topology: &Topology, report: &mut ValidationReport) {
    for (_, shell) in topology.shells() {
        if shell.kind != ShellKind::Closed {
            continue;
        }
        let mut edges = BTreeSet::new();
        let mut vertices = BTreeSet::new();
        let mut face_cells = 0isize;
        for face_id in &shell.faces {
            let Ok(face) = topology.face(*face_id) else {
                continue;
            };
            let inner_loops = face
                .loops
                .iter()
                .filter_map(|loop_id| topology.loop_(*loop_id).ok())
                .filter(|loop_value| loop_value.kind == LoopKind::Inner)
                .count();
            face_cells += 1 - inner_loops as isize;
            for loop_id in &face.loops {
                let Ok(loop_value) = topology.loop_(*loop_id) else {
                    continue;
                };
                let Some(first) = loop_value.first else {
                    continue;
                };
                let mut current = first;
                loop {
                    let Ok(coedge) = topology.coedge(current) else {
                        break;
                    };
                    edges.insert(coedge.edge);
                    if let Ok(edge) = topology.edge(coedge.edge) {
                        vertices.extend(edge.vertices);
                    }
                    current = coedge.next;
                    if current == first {
                        break;
                    }
                }
            }
        }
        let characteristic = vertices.len() as isize - edges.len() as isize + face_cells;
        if characteristic > 2 || characteristic % 2 != 0 {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "closed orientable shell has an impossible Euler characteristic",
                )
                .with_entity(shell.name),
            );
        }
    }
}

fn validate_geometry(
    topology: &Topology,
    tolerance: ModelTolerance,
    mode: ValidationMode,
    report: &mut ValidationReport,
) {
    let scale = topology
        .vertices()
        .map(|(_, vertex)| vertex.point.to_vec().magnitude())
        .fold(1.0_f64, f64::max);

    for (_, vertex) in topology.vertices() {
        if !vertex.point.x.is_finite()
            || !vertex.point.y.is_finite()
            || !vertex.point.z.is_finite()
            || !vertex.tolerance.is_finite()
            || vertex.tolerance < 0.0
        {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::GeometryInvariant,
                    "vertex geometry is not finite",
                )
                .with_entity(vertex.name),
            );
        }
    }

    for (_, edge) in topology.edges() {
        if !edge
            .curve
            .domain()
            .contains_range(edge.range, tolerance.parameter)
        {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::GeometryInvariant,
                    "edge trim range lies outside its curve's natural domain",
                )
                .with_entity(edge.name),
            );
            continue;
        }
        let Ok(start) = topology.vertex(edge.vertices[0]) else {
            continue;
        };
        let Ok(end) = topology.vertex(edge.vertices[1]) else {
            continue;
        };
        let curve_start = match edge.curve.evaluate(edge.range.start) {
            Ok(value) => value.point,
            Err(error) => {
                for diagnostic in error.diagnostics() {
                    report.error(diagnostic.clone().with_entity(edge.name));
                }
                continue;
            }
        };
        let curve_end = match edge.curve.evaluate(edge.range.end) {
            Ok(value) => value.point,
            Err(error) => {
                for diagnostic in error.diagnostics() {
                    report.error(diagnostic.clone().with_entity(edge.name));
                }
                continue;
            }
        };
        let achieved = edge
            .tolerance
            .max(start.tolerance)
            .max(end.tolerance)
            .max(tolerance.linear_at_scale(scale));
        let endpoint_error = curve_start
            .distance(start.point)
            .max(curve_end.distance(end.point));
        if endpoint_error > achieved {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::GeometryInvariant,
                    "edge curve endpoints do not agree with its vertices within tolerance",
                )
                .with_entity(edge.name)
                .with_accuracy(achieved, endpoint_error),
            );
        }
    }

    for (_, coedge) in topology.coedges() {
        let Ok(loop_) = topology.loop_(coedge.loop_id) else {
            continue;
        };
        let Ok(face) = topology.face(loop_.face) else {
            continue;
        };
        let Ok(edge) = topology.edge(coedge.edge) else {
            continue;
        };
        if !coedge
            .pcurve
            .domain()
            .contains_range(coedge.range, tolerance.parameter)
        {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::GeometryInvariant,
                    "coedge trim range lies outside its p-curve's natural domain",
                )
                .with_entity(coedge.name),
            );
            continue;
        }
        let vertices = if coedge.forward {
            edge.vertices
        } else {
            [edge.vertices[1], edge.vertices[0]]
        };
        let uv_start = match coedge.pcurve.evaluate(coedge.range.start) {
            Ok(value) => value.point,
            Err(error) => {
                for diagnostic in error.diagnostics() {
                    report.error(diagnostic.clone().with_entity(coedge.name));
                }
                continue;
            }
        };
        let uv_end = match coedge.pcurve.evaluate(coedge.range.end) {
            Ok(value) => value.point,
            Err(error) => {
                for diagnostic in error.diagnostics() {
                    report.error(diagnostic.clone().with_entity(coedge.name));
                }
                continue;
            }
        };
        let surface_start = match face.surface.evaluate(uv_start.x, uv_start.y) {
            Ok(value) => value.point,
            Err(error) => {
                for diagnostic in error.diagnostics() {
                    report.error(diagnostic.clone().with_entity(face.name));
                }
                continue;
            }
        };
        let surface_end = match face.surface.evaluate(uv_end.x, uv_end.y) {
            Ok(value) => value.point,
            Err(error) => {
                for diagnostic in error.diagnostics() {
                    report.error(diagnostic.clone().with_entity(face.name));
                }
                continue;
            }
        };
        let Ok(start) = topology.vertex(vertices[0]) else {
            continue;
        };
        let Ok(end) = topology.vertex(vertices[1]) else {
            continue;
        };
        let achieved = face
            .tolerance
            .max(edge.tolerance)
            .max(tolerance.linear_at_scale(scale));
        let endpoint_error = surface_start
            .distance(start.point)
            .max(surface_end.distance(end.point));
        if endpoint_error > achieved {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::GeometryInvariant,
                    "coedge p-curve endpoints do not agree with face and edge geometry",
                )
                .with_entity(coedge.name)
                .with_accuracy(achieved, endpoint_error),
            );
        }
        for sample in 1..8 {
            let fraction = sample as f64 / 8.0;
            let edge_parameter = if coedge.forward {
                edge.range.start + edge.range.span() * fraction
            } else {
                edge.range.end - edge.range.span() * fraction
            };
            let pcurve_parameter = coedge.range.start + coedge.range.span() * fraction;
            let edge_point = match edge.curve.evaluate(edge_parameter) {
                Ok(value) => value.point,
                Err(error) => {
                    for diagnostic in error.diagnostics() {
                        report.error(diagnostic.clone().with_entity(edge.name));
                    }
                    break;
                }
            };
            let uv = match coedge.pcurve.evaluate(pcurve_parameter) {
                Ok(value) => value.point,
                Err(error) => {
                    for diagnostic in error.diagnostics() {
                        report.error(diagnostic.clone().with_entity(coedge.name));
                    }
                    break;
                }
            };
            let domains = face.surface.domains();
            if !domains[0].contains(uv.x, tolerance.parameter)
                || !domains[1].contains(uv.y, tolerance.parameter)
            {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::GeometryInvariant,
                        "p-curve leaves its face surface's natural parameter domain",
                    )
                    .with_entity(coedge.name),
                );
                break;
            }
            match face.surface.evaluate(uv.x, uv.y) {
                Ok(value) if value.point.distance(edge_point) <= achieved => {}
                Ok(value) => {
                    let interior_error = value.point.distance(edge_point);
                    report.error(
                        Diagnostic::new(
                            DiagnosticKind::GeometryInvariant,
                            "edge and p-curve disagree in the interior of their trim ranges",
                        )
                        .with_entity(coedge.name)
                        .with_accuracy(achieved, interior_error),
                    );
                    break;
                }
                Err(error) => {
                    for diagnostic in error.diagnostics() {
                        report.error(diagnostic.clone().with_entity(face.name));
                    }
                    break;
                }
            }
        }
    }

    validate_face_parameter_loops(topology, tolerance, mode, report);
}

fn validate_face_parameter_loops(
    topology: &Topology,
    tolerance: ModelTolerance,
    mode: ValidationMode,
    report: &mut ValidationReport,
) {
    for (_, face) in topology.faces() {
        let domains = face.surface.domains();
        for loop_id in &face.loops {
            let Ok(loop_value) = topology.loop_(*loop_id) else {
                continue;
            };
            let Some(first) = loop_value.first else {
                continue;
            };
            let mut current = first;
            let mut signed_area = 0.0;
            loop {
                let Ok(coedge) = topology.coedge(current) else {
                    break;
                };
                match coedge
                    .pcurve
                    .signed_area_contribution(coedge.range, tolerance.parameter)
                {
                    Ok(area) => signed_area += area,
                    Err(error) => {
                        for diagnostic in error.diagnostics() {
                            report.error(diagnostic.clone().with_entity(coedge.name));
                        }
                    }
                }
                if let Ok(next) = topology.coedge(coedge.next) {
                    let end = coedge.pcurve.evaluate(coedge.range.end);
                    let start = next.pcurve.evaluate(next.range.start);
                    if let (Ok(end), Ok(start)) = (end, start) {
                        if !domains[0].equivalent(end.point.x, start.point.x, tolerance.parameter)
                            || !domains[1].equivalent(
                                end.point.y,
                                start.point.y,
                                tolerance.parameter,
                            )
                        {
                            let geometrically_continuous = face
                                .surface
                                .evaluate(end.point.x, end.point.y)
                                .and_then(|end_value| {
                                    face.surface.evaluate(start.point.x, start.point.y).map(
                                        |start_value| {
                                            end_value.point.distance(start_value.point)
                                                <= tolerance.linear_at_scale(scale_from_point(
                                                    end_value.point,
                                                ))
                                        },
                                    )
                                })
                                .unwrap_or(false);
                            if !geometrically_continuous {
                                report.error(
                                    Diagnostic::new(
                                        DiagnosticKind::GeometryInvariant,
                                        "consecutive p-curves are not continuous in parameter or geometric space",
                                    )
                                    .with_entity(coedge.name),
                                );
                            }
                        }
                    }
                }
                current = coedge.next;
                if current == first {
                    break;
                }
            }
            let orientation = match face.orientation {
                FaceOrientation::Forward => 1.0,
                FaceOrientation::Reversed => -1.0,
            };
            let oriented_area = signed_area * orientation;
            let correct_winding = match loop_value.kind {
                LoopKind::Outer => oriented_area > tolerance.parameter.powi(2),
                LoopKind::Inner => oriented_area < -tolerance.parameter.powi(2),
            };
            if mode.is_published() && !correct_winding {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::GeometryInvariant,
                        "face loop has zero area or winding inconsistent with face orientation",
                    )
                    .with_entity(loop_value.name),
                );
            }
        }
    }
}

fn scale_from_point(point: crate::Point3) -> f64 {
    point.to_vec().magnitude().max(1.0)
}
