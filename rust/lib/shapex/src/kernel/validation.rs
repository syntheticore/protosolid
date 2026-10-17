use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::Point2;

use super::{
    Coedge, CoedgeId, Diagnostic, DiagnosticKind, Face, FaceId, KernelError,
    KernelResult, LoopKind, ModelTolerance, PersistentId, ShellKind, Topology,
};

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

/// Prove that every body of `topology` bounds a closed, geometrically
/// consistent and physically valid volume.
pub(crate) fn validate(topology: &Topology, tolerance: ModelTolerance) -> ValidationReport {
    let mut report = validate_structure(topology, tolerance);
    validate_physical_volumes(topology, tolerance, &mut report);
    report
}

/// Check published topology and geometry without discarding a newly computed
/// physical certificate. Commit performs certification once and retains it.
pub(crate) fn validate_structure(topology: &Topology, tolerance: ModelTolerance) -> ValidationReport {
    let mut report = ValidationReport::default();
    validate_names(topology, &mut report);
    validate_ownership(topology, &mut report);
    validate_loops(topology, &mut report);
    validate_edges(topology, &mut report);
    validate_vertices(topology, &mut report);
    validate_manifold_vertices(topology, &mut report);
    validate_shell_euler_characteristic(topology, &mut report);
    validate_geometry(topology, tolerance, &mut report);
    validate_planar_face_regions(topology, tolerance, &mut report);
    validate_curved_face_boundaries(topology, tolerance, &mut report);
    report
}

/// Winding and edge incidence alone admit a curved face whose trim folds over
/// itself, for example a tilted cylinder cap crossing its bottom rim. Validate
/// the trim in the surface chart before publishing any operation's result.
fn validate_curved_face_boundaries(
    topology: &Topology,
    tolerance: ModelTolerance,
    report: &mut ValidationReport,
) {
    for (_, face) in topology.faces() {
        if matches!(face.surface(), super::Surface3::Plane(_)) {
            continue;
        }
        for loop_id in face.loops() {
            let result = (|| -> KernelResult<()> {
                let mut segments = Vec::new();
                let mut previous: Option<Point2> = None;
                let uses = topology.loop_coedges(*loop_id)?;
                for id in &uses {
                    let coedge = topology.coedge(*id)?;
                    let mut pcurve = coedge.pcurve().clone();
                    if let Some(end) = previous {
                        let start = pcurve.evaluate(coedge.range().start)?.point;
                        let shift = super::whole_periods(face.surface().domains(), end - start);
                        if shift.magnitude2() > 0.0 {
                            pcurve = super::trim::translate_pcurve(&pcurve, shift, tolerance)?;
                        }
                    }
                    previous = Some(pcurve.evaluate(coedge.range().end)?.point);
                    segments.push(super::TrimmedCurve2::new(pcurve, coedge.range())?);
                }
                for first in 0..segments.len() {
                    for second in first + 1..segments.len() {
                        let a = topology.coedge(uses[first])?;
                        let b = topology.coedge(uses[second])?;
                        // Two uses of one seam are the chart cut, not two
                        // independently intersecting material boundaries.
                        if a.edge() == b.edge() {
                            continue;
                        }
                        let intersection = super::wire::curve_pair_intersection(
                            &segments[first], &segments[second], tolerance,
                        )?;
                        let vertices_a = topology.coedge_vertices(uses[first])?;
                        let vertices_b = topology.coedge_vertices(uses[second])?;
                        let endpoints_a = [segments[first].start()?, segments[first].end()?];
                        let endpoints_b = [segments[second].start()?, segments[second].end()?];
                        let represented = |point: Point2| {
                            (0..2).any(|i| (0..2).any(|j| {
                                vertices_a[i] == vertices_b[j]
                                    && endpoints_a[i].distance(point) <= tolerance.linear * 8.0
                                    && endpoints_b[j].distance(point) <= tolerance.linear * 8.0
                            }))
                        };
                        let spatial = |coedge: &Coedge, parameter| -> KernelResult<crate::Point3> {
                            let edge = topology.edge(coedge.edge())?;
                            Ok(edge.curve().evaluate(coedge.edge_parameter(edge, parameter))?.point)
                        };
                        let mut crossing = false;
                        for hit in &intersection.hits {
                            if represented(hit.point) { continue; }
                            // UV charts can have very different scales from
                            // model units. A near miss in UV is a crossing only
                            // when the retained spatial curves also meet.
                            let first = spatial(a, hit.first_parameter)?;
                            let second = spatial(b, hit.second_parameter)?;
                            let scale = first.to_vec().magnitude().max(second.to_vec().magnitude());
                            crossing |= first.distance(second) <= tolerance.linear_at_scale(scale) * 8.0;
                        }
                        if intersection.coincident || crossing {
                            return Err(KernelError::new(DiagnosticKind::GeometryInvariant,
                                "face boundary crosses or touches itself away from a shared vertex"));
                        }
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                for diagnostic in error.diagnostics() {
                    report.error(Diagnostic::new(
                        DiagnosticKind::GeometryInvariant,
                        format!("invalid curved face boundary: {}", diagnostic.message),
                    ).with_entity(face.name));
                }
            }
        }
    }
}

/// A closed edge graph and correctly wound loops do not guarantee a valid
/// trimmed face: a hole can cross or lie outside its outer boundary. Validate
/// exact planar p-curves even when the rest of the body contains curved faces.
fn validate_planar_face_regions(
    topology: &Topology,
    tolerance: ModelTolerance,
    report: &mut ValidationReport,
) {
    for (_, face) in topology.faces() {
        let super::Surface3::Plane(plane) = face.surface() else {
            continue;
        };
        let e = plane.u().magnitude2();
        let f = plane.u().dot(plane.v());
        let g = plane.v().magnitude2();
        let minimum_metric = ((e + g - ((e - g).powi(2) + 4.0 * f * f).sqrt()) * 0.5).sqrt();
        let uv_tolerance = ModelTolerance {
            linear: tolerance.parameter.max(tolerance.linear / minimum_metric),
            ..tolerance
        };
        let result = (|| -> KernelResult<()> {
            let mut outer = None;
            let mut holes = Vec::new();
            for loop_id in face.loops() {
                let segments = topology
                    .loop_coedges(*loop_id)?
                    .into_iter()
                    .map(|id| {
                        let coedge = topology.coedge(id)?;
                        super::TrimmedCurve2::new(coedge.pcurve().clone(), coedge.range())
                    })
                    .collect::<KernelResult<Vec<_>>>()?;
                let wire = super::Wire2::new(segments, uv_tolerance)?;
                match topology.loop_(*loop_id)?.kind() {
                    LoopKind::Outer => outer = Some(wire),
                    LoopKind::Inner => holes.push(wire),
                }
            }
            let outer = outer.ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::GeometryInvariant,
                    "planar face has no outer boundary",
                )
            })?;
            super::Profile::new(outer, holes, uv_tolerance)?;
            Ok(())
        })();
        if let Err(error) = result {
            for diagnostic in error.diagnostics() {
                report.error(
                    Diagnostic::new(
                        DiagnosticKind::GeometryInvariant,
                        format!("invalid planar face region: {}", diagnostic.message),
                    )
                    .with_entity(face.name),
                );
            }
        }
    }
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

fn validate_ownership(topology: &Topology, report: &mut ValidationReport) {
    let mut shell_owners = BTreeMap::new();
    let mut face_owners = BTreeMap::new();
    let mut loop_owners = BTreeMap::new();
    for (body_id, body) in topology.bodies() {
        if body.shells.is_empty() {
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
        if shell.kind != ShellKind::Closed {
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

fn validate_loops(topology: &Topology, report: &mut ValidationReport) {
    let mut globally_reached = BTreeSet::new();
    for (loop_id, loop_) in topology.loops() {
        let Some(first) = loop_.first else {
            report.error(
                Diagnostic::new(
                    DiagnosticKind::TopologyInvariant,
                    "a published face loop may not be empty",
                )
                .with_entity(loop_.name),
            );
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
        if visited.len() < 2 && !is_closed_single_edge {
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

fn validate_edges(topology: &Topology, report: &mut ValidationReport) {
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
        if edge.coedges.len() != 2 {
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

fn validate_vertices(topology: &Topology, report: &mut ValidationReport) {
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
        if outgoing_uses.is_empty() {
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

fn validate_geometry(topology: &Topology, tolerance: ModelTolerance, report: &mut ValidationReport) {
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
        validate_coedge_geometry(topology, tolerance, scale, coedge, report);
    }
    for (_, face) in topology.faces() {
        validate_face_loops(topology, tolerance, face, report);
    }
}

/// Validate one face's boundary geometry and loop winding in isolation.
pub(crate) fn validate_face(
    topology: &Topology,
    tolerance: ModelTolerance,
    face_id: FaceId,
) -> KernelResult<()> {
    let mut report = ValidationReport::default();
    let scale = topology
        .vertices()
        .map(|(_, vertex)| vertex.point.to_vec().magnitude())
        .fold(1.0_f64, f64::max);
    let face = topology.face(face_id)?;
    for loop_id in &face.loops {
        for coedge in topology.loop_coedges(*loop_id)? {
            validate_coedge_geometry(topology, tolerance, scale, topology.coedge(coedge)?, &mut report);
        }
    }
    validate_face_loops(topology, tolerance, face, &mut report);
    report.into_result()
}

/// Check that a coedge's p-curve lies on its face and follows its edge.
fn validate_coedge_geometry(
    topology: &Topology,
    tolerance: ModelTolerance,
    scale: f64,
    coedge: &Coedge,
    report: &mut ValidationReport,
) {
    let Ok(loop_) = topology.loop_(coedge.loop_id) else {
        return;
    };
    let Ok(face) = topology.face(loop_.face) else {
        return;
    };
    let Ok(edge) = topology.edge(coedge.edge) else {
        return;
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
        return;
    }
    let vertices = if coedge.forward {
        edge.vertices
    } else {
        [edge.vertices[1], edge.vertices[0]]
    };
    let report_error = |report: &mut ValidationReport, error: KernelError, entity| {
        for diagnostic in error.diagnostics() {
            report.error(diagnostic.clone().with_entity(entity));
        }
    };
    let surface_point = |report: &mut ValidationReport, parameter: f64| {
        let uv = match coedge.pcurve.evaluate(parameter) {
            Ok(value) => value.point,
            Err(error) => {
                report_error(report, error, coedge.name);
                return None;
            }
        };
        match face.surface.evaluate(uv.x, uv.y) {
            Ok(value) => Some(value.point),
            Err(error) => {
                report_error(report, error, face.name);
                None
            }
        }
    };
    let Some(surface_start) = surface_point(report, coedge.range.start) else {
        return;
    };
    let Some(surface_end) = surface_point(report, coedge.range.end) else {
        return;
    };
    let Ok(start) = topology.vertex(vertices[0]) else {
        return;
    };
    let Ok(end) = topology.vertex(vertices[1]) else {
        return;
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
    let domains = face.surface.domains();
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
                report_error(report, error, edge.name);
                break;
            }
        };
        let uv = match coedge.pcurve.evaluate(pcurve_parameter) {
            Ok(value) => value.point,
            Err(error) => {
                report_error(report, error, coedge.name);
                break;
            }
        };
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
                report_error(report, error, face.name);
                break;
            }
        }
    }
}

/// Check that every loop of `face` is continuous in its parameter chart and
/// winds consistently with the face orientation.
fn validate_face_loops(
    topology: &Topology,
    tolerance: ModelTolerance,
    face: &Face,
    report: &mut ValidationReport,
) {
    for loop_id in &face.loops {
        let Ok(loop_value) = topology.loop_(*loop_id) else {
            continue;
        };
        let Some(first) = loop_value.first else {
            continue;
        };
        let (principal_area, lifted_area) = loop_areas(topology, tolerance, face, first, report);
        let orientation = face.orientation.sign();
        let correct = |area: f64| match loop_value.kind {
            LoopKind::Outer => area * orientation > tolerance.parameter.powi(2),
            LoopKind::Inner => area * orientation < -tolerance.parameter.powi(2),
        };
        // Existing canonical primitive builders may retain a valid loop wholly
        // inside the principal chart. Boolean and shell rebuilds can instead
        // retain equivalent seam endpoints in adjacent chart copies. Accept the
        // principal representation when it already has the required winding;
        // otherwise use its continuous periodic lift.
        if !(correct(principal_area) || correct(lifted_area)) {
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

/// The signed parameter-space area of the loop starting at `first`, both in
/// its principal chart and continuously lifted across periodic seams.
/// Reports consecutive p-curves that are not continuous.
fn loop_areas(
    topology: &Topology,
    tolerance: ModelTolerance,
    face: &Face,
    first: CoedgeId,
    report: &mut ValidationReport,
) -> (f64, f64) {
    let domains = face.surface.domains();
    // A whole-period jump between consecutive uses, within parameter tolerance.
    let seam_jump = |value: f64, axis: usize| {
        let candidate = domains[axis].whole_periods(value);
        if (value - candidate).abs() <= tolerance.parameter * 64.0 {
            candidate
        } else {
            0.0
        }
    };
    let mut current = first;
    let mut lifted_area = 0.0;
    let mut principal_area = 0.0;
    let mut previous_end = None;
    loop {
        let Ok(coedge) = topology.coedge(current) else {
            break;
        };
        let start = coedge.pcurve.evaluate(coedge.range.start);
        let end = coedge.pcurve.evaluate(coedge.range.end);
        let chart_shift = match (previous_end, &start) {
            (Some(previous), Ok(start)) => {
                let delta: crate::Vec2 = previous - start.point;
                crate::Vec2::new(seam_jump(delta.x, 0), seam_jump(delta.y, 1))
            }
            _ => crate::Vec2::new(0.0, 0.0),
        };
        match coedge
            .pcurve
            .signed_area_contribution(coedge.range, tolerance.parameter)
        {
            Ok(mut area) => {
                principal_area += area;
                if let (Ok(start), Ok(end)) = (&start, &end) {
                    area += 0.5
                        * (chart_shift.x * (end.point.y - start.point.y)
                            - chart_shift.y * (end.point.x - start.point.x));
                }
                lifted_area += area;
            }
            Err(error) => {
                for diagnostic in error.diagnostics() {
                    report.error(diagnostic.clone().with_entity(coedge.name));
                }
            }
        }
        if let Ok(end) = &end {
            previous_end = Some(end.point + chart_shift);
        }
        if let (Ok(next), Ok(end)) = (topology.coedge(coedge.next), &end) {
            if let Ok(start) = next.pcurve.evaluate(next.range.start) {
                let (end, start) = (end.point, start.point);
                if !domains[0].equivalent(end.x, start.x, tolerance.parameter)
                    || !domains[1].equivalent(end.y, start.y, tolerance.parameter)
                {
                    match collapsed_bridge(face, tolerance, end, start) {
                        // A surface pole can collapse a finite parameter-space
                        // interval to one topological vertex. Account for that
                        // implicit bridge in the loop winding without inventing
                        // a zero-length 3D edge.
                        Some(bridge_start) => {
                            let bridge = 0.5 * (end.x * bridge_start.y - bridge_start.x * end.y);
                            lifted_area += bridge;
                            principal_area += bridge;
                        }
                        None => report.error(
                            Diagnostic::new(
                                DiagnosticKind::GeometryInvariant,
                                "consecutive p-curves are not continuous in parameter space or across a collapsed surface interval",
                            )
                            .with_entity(coedge.name),
                        ),
                    }
                }
            }
        }
        current = coedge.next;
        if current == first {
            break;
        }
    }
    (principal_area, lifted_area)
}

/// When the parameter interval from `end` to `start` collapses to one point
/// of `face`'s surface, the start aligned to `end`'s periodic chart.
fn collapsed_bridge(
    face: &Face,
    tolerance: ModelTolerance,
    end: Point2,
    start: Point2,
) -> Option<Point2> {
    let end_value = face.surface.evaluate(end.x, end.y).ok()?;
    let start_value = face.surface.evaluate(start.x, start.y).ok()?;
    // Compare the short interval in one periodic chart. Raw representatives
    // near opposite sides of a seam can differ by a full turn plus harmless
    // numerical error in the other axis.
    let domains = face.surface.domains();
    let mut aligned_start = start;
    for (axis, derivative) in [end_value.du, end_value.dv].into_iter().enumerate() {
        if derivative.magnitude() > tolerance.linear {
            aligned_start[axis] = domains[axis].nearest(aligned_start[axis], end[axis]);
        }
    }
    let middle = Point2::from_vec((end.to_vec() + aligned_start.to_vec()) * 0.5);
    let middle_value = face.surface.evaluate(middle.x, middle.y).ok()?;
    let allowed = tolerance.linear_at_scale(scale_from_point(end_value.point));
    (end_value.point.distance(start_value.point) <= allowed
        && end_value.point.distance(middle_value.point) <= allowed)
        .then_some(aligned_start)
}

fn scale_from_point(point: crate::Point3) -> f64 {
    point.to_vec().magnitude().max(1.0)
}
