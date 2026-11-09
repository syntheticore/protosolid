use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use crate::Point2;

use super::{
    Coedge, CoedgeId, Diagnostic, DiagnosticKind, Face, FaceId, KernelError,
    KernelResult, LoopKind, ModelTolerance, PersistentId, ShellKind, Topology, VertexId,
};
use crate::geometry_error;

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
    fn topology(&mut self, entity: PersistentId, message: impl Into<String>) {
        self.error(Diagnostic::new(DiagnosticKind::TopologyInvariant, message).with_entity(entity));
    }
    fn geometry(&mut self, entity: PersistentId, message: impl Into<String>) {
        self.error(Diagnostic::new(DiagnosticKind::GeometryInvariant, message).with_entity(entity));
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
    let mut report = validate_structure(topology, tolerance, None);
    validate_physical_volumes(topology, tolerance, &mut report);
    report
}

/// Faces whose geometry a validation covers; `None` covers every face.
struct GeometryScope(Option<BTreeSet<FaceId>>);

impl GeometryScope {
    /// Every face that uses an entity named in `touched`, or every face.
    fn new(topology: &Topology, touched: Option<&BTreeSet<PersistentId>>) -> Self {
        let Some(touched) = touched else {
            return Self(None);
        };
        let uses_touched = |face: &Face| -> KernelResult<bool> {
            if touched.contains(&face.name) {
                return Ok(true);
            }
            for loop_id in &face.loops {
                if touched.contains(&topology.loop_(*loop_id)?.name) {
                    return Ok(true);
                }
                for id in topology.loop_coedges(*loop_id)? {
                    let coedge = topology.coedge(id)?;
                    let edge = topology.edge(coedge.edge)?;
                    if touched.contains(&coedge.name)
                        || touched.contains(&edge.name)
                        || edge.vertices.iter().any(|vertex| {
                            topology.vertex(*vertex).is_ok_and(|vertex| touched.contains(&vertex.name))
                        })
                    {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        };
        // A face whose boundary cannot be traversed is reported structurally
        // and validated geometrically as well.
        Self(Some(
            topology
                .faces()
                .filter(|(_, face)| uses_touched(face).unwrap_or(true))
                .map(|(id, _)| id)
                .collect(),
        ))
    }

    fn contains(&self, face: FaceId) -> bool {
        self.0.as_ref().is_none_or(|faces| faces.contains(&face))
    }

    fn contains_coedge(&self, topology: &Topology, coedge: &Coedge) -> bool {
        self.0.is_none() || topology.loop_(coedge.loop_id).is_ok_and(|loop_| self.contains(loop_.face))
    }
}

/// Check published topology and geometry without discarding a newly computed
/// physical certificate. Commit performs certification once and retains it.
///
/// Structural checks always cover the whole topology. Geometric checks cover
/// only faces that use an entity in `touched` when given: every other face
/// was published unchanged by an earlier validation.
pub(crate) fn validate_structure(
    topology: &Topology,
    tolerance: ModelTolerance,
    touched: Option<&BTreeSet<PersistentId>>,
) -> ValidationReport {
    let mut report = ValidationReport::default();
    validate_names(topology, &mut report);
    validate_ownership(topology, &mut report);
    validate_loops(topology, &mut report);
    validate_edges(topology, &mut report);
    let outgoing = outgoing_coedges(topology);
    validate_vertices(topology, &outgoing, &mut report);
    validate_manifold_vertices(topology, &outgoing, &mut report);
    validate_shell_euler_characteristic(topology, &mut report);
    let scope = GeometryScope::new(topology, touched);
    validate_geometry(topology, tolerance, &scope, &mut report);
    validate_planar_face_regions(topology, tolerance, &scope, &mut report);
    validate_curved_face_boundaries(topology, tolerance, &scope, &mut report);
    report
}

/// Winding and edge incidence alone admit a curved face whose trim folds over
/// itself, for example a tilted cylinder cap crossing its bottom rim. Validate
/// the trim in the surface chart before publishing any operation's result.
fn validate_curved_face_boundaries(
    topology: &Topology,
    tolerance: ModelTolerance,
    scope: &GeometryScope,
    report: &mut ValidationReport,
) {
    for (id, face) in topology.faces() {
        if !scope.contains(id) || matches!(face.surface(), super::Surface3::Plane(_)) {
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
                            return Err(geometry_error("face boundary crosses or touches itself away from a shared vertex"));
                        }
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                for diagnostic in error.diagnostics() {
                    report.geometry(face.name, format!("invalid curved face boundary: {}", diagnostic.message));
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
    scope: &GeometryScope,
    report: &mut ValidationReport,
) {
    for (id, face) in topology.faces() {
        let super::Surface3::Plane(plane) = face.surface() else {
            continue;
        };
        if !scope.contains(id) {
            continue;
        }
        let e = plane.u().magnitude2();
        let f = plane.u().dot(plane.v());
        let g = plane.v().magnitude2();
        let minimum_metric = ((e + g - ((e - g).powi(2) + 4.0 * f * f).sqrt()) * 0.5).sqrt();
        let uv_tolerance = ModelTolerance {
            linear: tolerance.parameter.max(tolerance.linear / minimum_metric),
            ..tolerance
        };
        let validate = |analytic_circles: bool| -> KernelResult<()> {
            let mut outer = None;
            let mut holes = Vec::new();
            for loop_id in face.loops() {
                let segments = topology
                    .loop_coedges(*loop_id)?
                    .into_iter()
                    .map(|id| {
                        let coedge = topology.coedge(id)?;
                        let edge = topology.edge(coedge.edge())?;
                        if analytic_circles && matches!(edge.curve(), super::Curve3::Circle(_)) {
                            // Validate circular trims analytically even
                            // when sewing retained a reparameterized
                            // projection as the coedge's exact p-curve.
                            if let super::PlaneCurveProjection::Curve(curve) =
                                plane.project_curve(edge.curve(), coedge.range(), tolerance)? {
                                return Ok(curve);
                            }
                        }
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
                geometry_error("planar face has no outer boundary")
            })?;
            super::Profile::from_face_wires(outer, holes, uv_tolerance)?;
            Ok(())
        };
        // Analytic circle images are costly to validate, so only fall back to
        // them when the retained p-curves fail.
        let result = validate(false).or_else(|_| validate(true));
        if let Err(error) = result {
            for diagnostic in error.diagnostics() {
                report.geometry(face.name, format!("invalid planar face region: {}", diagnostic.message));
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

fn validate_names(topology: &Topology, report: &mut ValidationReport) {
    let mut seen = BTreeSet::new();
    for name in topology.names() {
        if !seen.insert(name) {
            report.topology(name, "persistent topology names must be unique within a model");
        }
    }
}

fn validate_ownership(topology: &Topology, report: &mut ValidationReport) {
    let mut shell_owners = BTreeMap::new();
    let mut face_owners = BTreeMap::new();
    let mut loop_owners = BTreeMap::new();
    for (body_id, body) in topology.bodies() {
        if body.shells.is_empty() {
            report.topology(body.name, "a body must contain a shell");
        }
        let mut unique = BTreeSet::new();
        for shell_id in &body.shells {
            if !unique.insert(*shell_id) {
                report.topology(body.name, "a body may not list the same shell more than once");
            }
            *shell_owners.entry(*shell_id).or_insert(0usize) += 1;
            match topology.shell(*shell_id) {
                Ok(shell) if shell.body == body_id => {}
                _ => report.topology(body.name, "body-to-shell ownership is not reciprocal"),
            }
        }
    }

    for (shell_id, shell) in topology.shells() {
        if shell_owners.get(&shell_id).copied() != Some(1) || topology.body(shell.body).is_err() {
            report.topology(shell.name, "every shell must be owned exactly once by its recorded body");
        }
        if shell.kind != ShellKind::Closed {
            report.topology(shell.name, "published bodies may not contain open shells");
        }
        let mut unique = BTreeSet::new();
        for face_id in &shell.faces {
            if !unique.insert(*face_id) {
                report.topology(shell.name, "a shell may not list the same face more than once");
            }
            *face_owners.entry(*face_id).or_insert(0usize) += 1;
            match topology.face(*face_id) {
                Ok(face) if face.shell == shell_id => {}
                _ => report.topology(shell.name, "shell-to-face ownership is not reciprocal"),
            }
        }
    }

    for (face_id, face) in topology.faces() {
        if face_owners.get(&face_id).copied() != Some(1) || topology.shell(face.shell).is_err() {
            report.topology(face.name, "every face must be owned exactly once by its recorded shell");
        }
        let outer_count = face
            .loops
            .iter()
            .filter_map(|id| topology.loop_(*id).ok())
            .filter(|loop_| loop_.kind == LoopKind::Outer)
            .count();
        if outer_count != 1 {
            report.topology(face.name, "every face must own exactly one outer loop");
        }
        let mut unique = BTreeSet::new();
        for loop_id in &face.loops {
            if !unique.insert(*loop_id) {
                report.topology(face.name, "a face may not list the same loop more than once");
            }
            *loop_owners.entry(*loop_id).or_insert(0usize) += 1;
            match topology.loop_(*loop_id) {
                Ok(loop_) if loop_.face == face_id => {}
                _ => report.topology(face.name, "face-to-loop ownership is not reciprocal"),
            }
        }
    }

    for (loop_id, loop_value) in topology.loops() {
        if loop_owners.get(&loop_id).copied() != Some(1) || topology.face(loop_value.face).is_err()
        {
            report.topology(loop_value.name, "every loop must be owned exactly once by its recorded face");
        }
    }
}

fn validate_loops(topology: &Topology, report: &mut ValidationReport) {
    let mut globally_reached = BTreeSet::new();
    for (loop_id, loop_) in topology.loops() {
        let Some(first) = loop_.first else {
            report.topology(loop_.name, "a published face loop may not be empty");
            continue;
        };

        let mut current = first;
        let mut visited = BTreeSet::<CoedgeId>::new();
        loop {
            if !visited.insert(current) {
                if current != first {
                    report.topology(loop_.name, "a coedge cycle repeated before returning to its loop start");
                }
                break;
            }
            if !globally_reached.insert(current) {
                report.topology(loop_.name, "a coedge may be reachable from exactly one loop cycle");
            }
            let coedge = match topology.coedge(current) {
                Ok(value) => value,
                Err(_) => {
                    report.topology(loop_.name, "loop references a missing coedge");
                    break;
                }
            };
            if coedge.loop_id != loop_id {
                report.topology(coedge.name, "coedge belongs to a different loop");
            }
            match topology.coedge(coedge.next) {
                Ok(next) if next.previous == current => {}
                _ => report.topology(coedge.name, "coedge next/previous links are not reciprocal"),
            }
            if let (Ok([_, end]), Ok([next_start, _])) = (
                topology.coedge_vertices(current),
                topology.coedge_vertices(coedge.next),
            ) {
                if end != next_start {
                    report.topology(coedge.name, "consecutive coedges do not share an endpoint vertex");
                }
            }
            current = coedge.next;
            if current == first {
                break;
            }
            if visited.len() > topology.coedges.len() {
                report.topology(loop_.name, "coedge loop does not terminate");
                break;
            }
        }
        let is_closed_single_edge = visited.len() == 1
            && topology
                .coedge(first)
                .and_then(|coedge| topology.edge(coedge.edge))
                .is_ok_and(|edge| edge.vertices[0] == edge.vertices[1]);
        if visited.len() < 2 && !is_closed_single_edge {
            report.topology(loop_.name, "a published boundary loop needs at least two coedges unless it is one closed edge");
        }
    }
    for (coedge_id, coedge) in topology.coedges() {
        if !globally_reached.contains(&coedge_id) {
            report.topology(coedge.name, "every coedge must be reachable from its recorded loop");
        }
    }
}

fn validate_edges(topology: &Topology, report: &mut ValidationReport) {
    for (edge_id, edge) in topology.edges() {
        if topology.vertex(edge.vertices[0]).is_err() || topology.vertex(edge.vertices[1]).is_err()
        {
            report.topology(edge.name, "edge endpoint handles must refer to live vertices");
        }
        if edge.coedges.is_empty() || edge.coedges.len() > 2 {
            report.topology(edge.name, "a manifold edge must have one or two coedge uses");
        }
        if edge.coedges.len() != 2 {
            report.topology(edge.name, "every edge of a closed shell needs two coedges");
        }
        let mut unique = BTreeSet::new();
        for coedge_id in &edge.coedges {
            if !unique.insert(*coedge_id) {
                report.topology(edge.name, "an edge may not list the same coedge more than once");
            }
            match topology.coedge(*coedge_id) {
                Ok(coedge) if coedge.edge == edge_id => {}
                _ => report.topology(edge.name, "edge-to-coedge ownership is not reciprocal"),
            }
        }
        if edge.coedges.len() == 2 {
            let first = topology.coedge(edge.coedges[0]);
            let second = topology.coedge(edge.coedges[1]);
            if let (Ok(first), Ok(second)) = (first, second) {
                if first.forward == second.forward {
                    report.topology(edge.name, "the two uses of a manifold edge must have opposite orientations");
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
                    report.topology(edge.name, "both uses of an edge must belong to the same shell");
                }
            }
        }
    }
    for (coedge_id, coedge) in topology.coedges() {
        match topology.edge(coedge.edge) {
            Ok(edge) if edge.coedges.contains(&coedge_id) => {}
            _ => report.topology(coedge.name, "coedge-to-edge ownership is not reciprocal"),
        }
    }
}

/// The coedges leaving each vertex, from one pass over the coedges.
fn outgoing_coedges(topology: &Topology) -> BTreeMap<VertexId, Vec<CoedgeId>> {
    let mut outgoing = BTreeMap::<VertexId, Vec<CoedgeId>>::new();
    for (id, _) in topology.coedges() {
        if let Ok([start, _]) = topology.coedge_vertices(id) {
            outgoing.entry(start).or_default().push(id);
        }
    }
    outgoing
}

fn validate_vertices(
    topology: &Topology,
    outgoing: &BTreeMap<VertexId, Vec<CoedgeId>>,
    report: &mut ValidationReport,
) {
    for (vertex_id, vertex) in topology.vertices() {
        if !outgoing.contains_key(&vertex_id) {
            report.topology(vertex.name, "a published body may not contain isolated vertices");
        }
    }
}

fn validate_manifold_vertices(
    topology: &Topology,
    outgoing: &BTreeMap<VertexId, Vec<CoedgeId>>,
    report: &mut ValidationReport,
) {
    for (vertex_id, vertex) in topology.vertices() {
        let mut shell = None;
        // Use edge-end darts rather than edge ids as link-graph vertices. A
        // closed edge has two distinct topological ends at the same vertex;
        // collapsing those ends to one edge id would incorrectly give its
        // artificial parameter-seam vertex degree four.
        let mut link = BTreeMap::<(super::EdgeId, u8), Vec<(super::EdgeId, u8)>>::new();
        for coedge_id in outgoing.get(&vertex_id).into_iter().flatten().copied() {
            let Ok(coedge) = topology.coedge(coedge_id) else {
                continue;
            };
            let Ok(loop_value) = topology.loop_(coedge.loop_id) else {
                continue;
            };
            let Ok(face) = topology.face(loop_value.face) else {
                continue;
            };
            if let Some(existing) = shell {
                if existing != face.shell {
                    report.topology(vertex.name, "a manifold vertex may not be shared by multiple shells");
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
            report.topology(vertex.name, "the incident face corners at a closed-shell vertex must form one two-regular link");
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
            report.topology(vertex.name, "the incident faces at a vertex form disconnected fans");
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
                while let Ok(coedge) = topology.coedge(current) {
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
            report.topology(shell.name, "closed orientable shell has an impossible Euler characteristic");
        }
    }
}

fn validate_geometry(
    topology: &Topology,
    tolerance: ModelTolerance,
    scope: &GeometryScope,
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
            report.geometry(vertex.name, "vertex geometry is not finite");
        }
    }

    for (_, edge) in topology.edges() {
        if !edge
            .curve
            .domain()
            .contains_range(edge.range, tolerance.parameter)
        {
            report.geometry(edge.name, "edge trim range lies outside its curve's natural domain");
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
                Diagnostic::new(DiagnosticKind::GeometryInvariant, "edge curve endpoints do not agree with its vertices within tolerance")
                    .with_entity(edge.name)
                    .with_accuracy(achieved, endpoint_error),
            );
        }
    }

    for (_, coedge) in topology.coedges() {
        if scope.contains_coedge(topology, coedge) {
            validate_coedge_geometry(topology, tolerance, scale, coedge, report);
        }
    }
    for (id, face) in topology.faces() {
        if scope.contains(id) {
            validate_face_loops(topology, tolerance, face, report);
        }
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
        report.geometry(coedge.name, "coedge trim range lies outside its p-curve's natural domain");
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
            Diagnostic::new(DiagnosticKind::GeometryInvariant, "coedge p-curve endpoints do not agree with face and edge geometry")
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
            report.geometry(coedge.name, "p-curve leaves its face surface's natural parameter domain");
            break;
        }
        match face.surface.evaluate(uv.x, uv.y) {
            Ok(value) if value.point.distance(edge_point) <= achieved => {}
            Ok(value) => {
                let interior_error = value.point.distance(edge_point);
                report.error(
                    Diagnostic::new(DiagnosticKind::GeometryInvariant, "edge and p-curve disagree in the interior of their trim ranges")
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
        let mut attempt = ValidationReport::default();
        let (mut principal_area, mut lifted_area, uncertainty) =
            loop_areas(topology, tolerance, face, first, &mut attempt, false);
        let orientation = face.orientation.sign();
        let correct = |area: f64, uncertainty: f64| match loop_value.kind {
            LoopKind::Outer => area * orientation > tolerance.parameter.powi(2) + uncertainty,
            LoopKind::Inner => area * orientation < -tolerance.parameter.powi(2) - uncertainty,
        };
        // Existing canonical primitive builders may retain a valid loop wholly
        // inside the principal chart. Boolean and shell rebuilds can instead
        // retain equivalent seam endpoints in adjacent chart copies. Accept the
        // principal representation when it already has the required winding;
        // otherwise use its continuous periodic lift.
        // Cancellation can leave a small area with the expected sign but an
        // uncertainty larger than the area itself. Refine those loops too.
        if !attempt.is_valid() || !(correct(principal_area, uncertainty) || correct(lifted_area, uncertainty)) {
            attempt = ValidationReport::default();
            (principal_area, lifted_area, _) = loop_areas(topology, tolerance, face, first, &mut attempt, true);
        }
        report.diagnostics.extend(attempt.diagnostics);
        if !(correct(principal_area, 0.0) || correct(lifted_area, 0.0)) {
            report.geometry(loop_value.name, "face loop has zero area or winding inconsistent with face orientation");
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
    precise: bool,
) -> (f64, f64, f64) {
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
    let mut uncertainty = 0.0;
    let mut previous_end = None;
    while let Ok(coedge) = topology.coedge(current) {
        let start = coedge.pcurve.evaluate(coedge.range.start);
        let end = coedge.pcurve.evaluate(coedge.range.end);
        let chart_shift = match (previous_end, &start) {
            (Some(previous), Ok(start)) => {
                let delta: crate::Vec2 = previous - start.point;
                crate::Vec2::new(seam_jump(delta.x, 0), seam_jump(delta.y, 1))
            }
            _ => crate::Vec2::new(0.0, 0.0),
        };
        let area = coedge.pcurve.area_contribution_estimate(coedge.range, tolerance.parameter, precise);
        match area {
            Ok((mut area, error)) => {
                uncertainty += error + f64::EPSILON * 8.0 * area.abs();
                principal_area += area;
                if let (Ok(start), Ok(end)) = (&start, &end) {
                    area += 0.5
                        * (chart_shift.x * (end.point.y - start.point.y)
                            - chart_shift.y * (end.point.x - start.point.x));
                    uncertainty += f64::EPSILON * 8.0 * (
                        chart_shift.x.abs() * (end.point.y.abs() + start.point.y.abs())
                        + chart_shift.y.abs() * (end.point.x.abs() + start.point.x.abs()));
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
                if end.distance2(start) > tolerance.parameter.powi(2) {
                    match collapsed_bridge(face, tolerance, end, start) {
                        // A surface pole can collapse a finite parameter-space
                        // interval to one topological vertex. Account for that
                        // implicit bridge in the loop winding without inventing
                        // a zero-length 3D edge, even if it spans a full turn.
                        Some(bridge_start) => {
                            let bridge = 0.5 * (end.x * bridge_start.y - bridge_start.x * end.y);
                            uncertainty += f64::EPSILON * 8.0
                                * (end.x.abs() * bridge_start.y.abs() + bridge_start.x.abs() * end.y.abs());
                            lifted_area += bridge;
                            principal_area += bridge;
                        }
                        None if !domains[0].equivalent(end.x, start.x, tolerance.parameter)
                            || !domains[1].equivalent(end.y, start.y, tolerance.parameter) =>
                            report.geometry(coedge.name, "consecutive p-curves are not continuous in parameter space or across a collapsed surface interval"),
                        None => {},
                    }
                }
            }
        }
        current = coedge.next;
        if current == first {
            break;
        }
    }
    (principal_area, lifted_area, uncertainty)
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Curve2, FaceOrientation, Kernel, NurbsCurve2, ParameterRange};
    use crate::{Point3, Vec3};

    // Reuse a primitive's topology to exercise loop validation in isolation;
    // these tests deliberately replace its cap's p-curves and winding.
    fn cap_loop(curves: Vec<Curve2>) -> (Topology, Face) {
        let mut kernel = Kernel::new(ModelTolerance::default()).unwrap();
        let mut operation = kernel.begin_operation();
        operation.cylinder(Point3::origin(), Vec3::unit_x(), Vec3::unit_y(), Vec3::unit_z()).unwrap();
        operation.commit().unwrap();
        let mut topology = kernel.topology().clone();
        let mut face = topology.faces().find(|(_, face)| matches!(face.surface(), super::super::Surface3::Plane(_)))
            .unwrap().1.clone();
        face.orientation = FaceOrientation::Forward;
        let loop_id = face.loops[0];
        let template = topology.coedge(topology.loop_(loop_id).unwrap().first.unwrap()).unwrap().clone();
        let ids = curves.into_iter().map(|pcurve| {
            let mut coedge = template.clone();
            coedge.pcurve = pcurve;
            coedge.range = ParameterRange::new(0.0, 1.0).unwrap();
            topology.coedges.insert(coedge)
        }).collect::<Vec<_>>();
        for (index, id) in ids.iter().enumerate() {
            let coedge = topology.coedges.get_mut(*id).unwrap();
            coedge.previous = ids[(index + ids.len() - 1) % ids.len()];
            coedge.next = ids[(index + 1) % ids.len()];
        }
        let loop_ = topology.loops.get_mut(loop_id).unwrap();
        loop_.first = Some(ids[0]);
        loop_.kind = LoopKind::Outer;
        (topology, face)
    }

    #[test]
    fn thin_rational_loop_refines_an_incorrect_cheap_winding() {
        let curve = |points, weight| NurbsCurve2::new(2, points, vec![1.0, weight, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap().into();
        let (topology, mut face) = cap_loop(vec![
            curve(vec![Point2::new(100.0, 100.0), Point2::new(100.2, 100.000001), Point2::new(101.0, 100.0)], 0.1),
            curve(vec![Point2::new(101.0, 100.0), Point2::new(100.8, 99.999999), Point2::new(100.0, 100.0)], 2.0),
        ]);
        let tolerance = ModelTolerance::default();
        let first = topology.loop_(face.loops[0]).unwrap().first.unwrap();
        let mut report = ValidationReport::default();
        let (cheap, _, uncertainty) = loop_areas(&topology, tolerance, &face, first, &mut report, false);
        let (precise, _, _) = loop_areas(&topology, tolerance, &face, first, &mut report, true);
        assert!(report.is_valid());
        // The cheap pass may resolve the sign itself, but must never report the
        // wrong sign outside its uncertainty.
        assert!((cheap < 0.0 || cheap < uncertainty) && precise < 0.0,
            "cheap {cheap}, uncertainty {uncertainty}, precise {precise}");
        validate_face_loops(&topology, tolerance, &face, &mut report);
        assert_eq!(report.diagnostics().len(), 1);
        assert!(report.diagnostics()[0].message.contains("winding inconsistent"));
        face.orientation = FaceOrientation::Reversed;
        let mut report = ValidationReport::default();
        validate_face_loops(&topology, tolerance, &face, &mut report);
        assert!(report.is_valid(), "{:?}", report.diagnostics());
    }

    #[test]
    fn precise_retry_reports_a_chart_gap_only_once() {
        let (topology, face) = cap_loop(vec![
            super::super::Line2::new(Point2::origin(), crate::Vec2::unit_x()).unwrap().into(),
        ]);
        let mut report = ValidationReport::default();
        validate_face_loops(&topology, ModelTolerance::default(), &face, &mut report);
        assert_eq!(report.diagnostics().iter().filter(|diagnostic|
            diagnostic.message.contains("consecutive p-curves are not continuous")).count(), 1);
    }
}
