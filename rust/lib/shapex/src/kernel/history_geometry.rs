//! Exact retention checks for known source/output pairs. These checks never
//! establish ancestry or override an operation's explicit change records.

use super::{
    ChangeKind, Curve3, EntityId, KernelResult, OperationHistory, ParameterRange, PersistentId,
    Topology,
};
use cgmath::prelude::*;
use std::collections::BTreeMap;

pub(crate) fn entities(topology: &Topology) -> BTreeMap<PersistentId, EntityId> {
    topology
        .vertices()
        .map(|(id, value)| (value.name(), EntityId::Vertex(id)))
        .chain(
            topology
                .edges()
                .map(|(id, value)| (value.name(), EntityId::Edge(id))),
        )
        .chain(
            topology
                .coedges()
                .map(|(id, value)| (value.name(), EntityId::Coedge(id))),
        )
        .chain(
            topology
                .loops()
                .map(|(id, value)| (value.name(), EntityId::Loop(id))),
        )
        .chain(
            topology
                .faces()
                .map(|(id, value)| (value.name(), EntityId::Face(id))),
        )
        .chain(
            topology
                .shells()
                .map(|(id, value)| (value.name(), EntityId::Shell(id))),
        )
        .chain(
            topology
                .bodies()
                .map(|(id, value)| (value.name(), EntityId::Body(id))),
        )
        .collect()
}

impl OperationHistory {
    /// Complete retained/deleted names after an Euler transaction. Local Euler
    /// records remain authoritative; unchanged names must also retain their trims.
    pub(crate) fn complete_retained(
        &mut self,
        before: &Topology,
        after: &Topology,
    ) -> KernelResult<()> {
        let inputs = entities(before);
        let outputs = entities(after);
        for (source, input) in inputs {
            if self.change(source).is_some() {
                continue;
            }
            if let Some(output) = outputs.get(&source) {
                let kind = if same_entity(before, input, after, *output)? {
                    ChangeKind::Unchanged
                } else {
                    ChangeKind::Modified
                };
                self.changed(source, kind, [source]);
            } else {
                self.changed(source, ChangeKind::Deleted, []);
            }
        }
        Ok(())
    }
}

fn same_set<T: Copy>(
    first: &Topology,
    a: &[T],
    second: &Topology,
    b: &[T],
    entity: impl Fn(T) -> EntityId,
) -> KernelResult<bool> {
    if a.len() != b.len() {
        return Ok(false);
    }
    let mut used = vec![false; b.len()];
    for input in a {
        let mut matched = false;
        for (index, output) in b.iter().enumerate() {
            if !used[index] && same_entity(first, entity(*input), second, entity(*output))? {
                used[index] = true;
                matched = true;
                break;
            }
        }
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Compare a directed trimmed curve. Lines and circles allow changes of frame;
/// other curves must retain their exact representation and parameter interval.
/// Endpoint matching alone would confuse complementary arcs or different splines.
fn same_segment(
    a: &Curve3,
    ar: ParameterRange,
    b: &Curve3,
    br: ParameterRange,
) -> KernelResult<bool> {
    match (a, b) {
        (Curve3::Line(a), Curve3::Line(b)) => Ok(a.evaluate(ar.start)?.point
            == b.evaluate(br.start)?.point
            && a.evaluate(ar.end)?.point == b.evaluate(br.end)?.point),
        (Curve3::Circle(a), Curve3::Circle(b)) => {
            if a.center() != b.center()
                || a.radius() != b.radius()
                || ar.span().abs() != br.span().abs()
            {
                return Ok(false);
            }
            let an = a.normal() * ar.span().signum();
            let bn = b.normal() * br.span().signum();
            if an.dot(bn) <= 0.0 || an.cross(bn).magnitude2() != 0.0 {
                return Ok(false);
            }
            if ar.span().abs() == std::f64::consts::TAU {
                return Ok(true);
            }
            Ok(a.evaluate(ar.start)?.point == b.evaluate(br.start)?.point
                && a.evaluate(ar.end)?.point == b.evaluate(br.end)?.point)
        }
        _ => Ok(a == b && ar == br),
    }
}

pub(crate) fn same_entity(
    first: &Topology,
    a: EntityId,
    second: &Topology,
    b: EntityId,
) -> KernelResult<bool> {
    Ok(match (a, b) {
        (EntityId::Vertex(a), EntityId::Vertex(b)) => {
            first.vertex(a)?.point() == second.vertex(b)?.point()
        }
        (EntityId::Edge(a), EntityId::Edge(b)) => {
            let a = first.edge(a)?;
            let b = second.edge(b)?;
            same_segment(a.curve(), a.range(), b.curve(), b.range())?
                || same_segment(a.curve(), a.range(), b.curve(), b.range().reversed())?
        }
        (EntityId::Coedge(a), EntityId::Coedge(b)) => {
            let a = first.coedge(a)?;
            let b = second.coedge(b)?;
            let ae = first.edge(a.edge())?;
            let be = second.edge(b.edge())?;
            let ar = if a.is_forward() {
                ae.range()
            } else {
                ae.range().reversed()
            };
            let br = if b.is_forward() {
                be.range()
            } else {
                be.range().reversed()
            };
            same_segment(ae.curve(), ar, be.curve(), br)?
        }
        (EntityId::Loop(a), EntityId::Loop(b)) => {
            if first.loop_(a)?.kind() != second.loop_(b)?.kind() {
                return Ok(false);
            }
            let a = first.loop_coedges(a)?;
            let b = second.loop_coedges(b)?;
            if a.len() != b.len() {
                return Ok(false);
            }
            if a.is_empty() {
                return Ok(true);
            }
            for offset in 0..b.len() {
                let mut cyclic_match = true;
                for (index, input) in a.iter().enumerate() {
                    if !same_entity(
                        first,
                        EntityId::Coedge(*input),
                        second,
                        EntityId::Coedge(b[(index + offset) % b.len()]),
                    )? {
                        cyclic_match = false;
                        break;
                    }
                }
                if cyclic_match {
                    return Ok(true);
                }
            }
            false
        }
        (EntityId::Face(a), EntityId::Face(b)) => {
            let a = first.face(a)?;
            let b = second.face(b)?;
            let same_support = match (a.surface(), b.surface()) {
                (super::Surface3::Plane(ap), super::Surface3::Plane(bp)) => {
                    let same_direction = ap.normal().dot(bp.normal()) > 0.0;
                    ap.normal().cross(bp.normal()).magnitude2() == 0.0
                        && (ap.origin() - bp.origin()).dot(ap.normal()) == 0.0
                        && same_direction == (a.orientation() == b.orientation())
                }
                _ => a.surface() == b.surface() && a.orientation() == b.orientation(),
            };
            if !same_support {
                return Ok(false);
            }
            same_set(first, a.loops(), second, b.loops(), EntityId::Loop)?
        }
        (EntityId::Shell(a), EntityId::Shell(b)) => {
            let a = first.shell(a)?;
            let b = second.shell(b)?;
            a.kind() == b.kind() && same_set(first, a.faces(), second, b.faces(), EntityId::Face)?
        }
        (EntityId::Body(a), EntityId::Body(b)) => same_set(
            first,
            first.body(a)?.shells(),
            second,
            second.body(b)?.shells(),
            EntityId::Shell,
        )?,
        _ => false,
    })
}
