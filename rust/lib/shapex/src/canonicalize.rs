//! Removal of representation boundaries that carry no material meaning:
//! partitions of one face support and degree-two subdivisions of one curve.

use std::collections::{BTreeMap, BTreeSet};

use cgmath::prelude::*;

use super::history::ChangeKind;
use super::topology::Edge;
use super::transaction::Transaction;
use super::{
    whole_periods, CoedgeId, Curve2, Curve3, EdgeId, KernelResult, LoopKind, ModelTolerance,
    ParameterRange, Surface3, SurfaceIntersectionCurve, VertexId,
};

/// A fully prepared join; discarded candidates leave topology untouched.
struct EdgeMerge {
    vertex: VertexId,
    survivor: EdgeId,
    removed: EdgeId,
    prepend: bool,
    curve: Curve3,
    range: ParameterRange,
    coedges: Vec<CoedgeMerge>,
}

struct CoedgeMerge {
    survivor: CoedgeId,
    removed: CoedgeId,
    pcurve: Curve2,
    range: ParameterRange,
}

/// Retain an exact curve and extend its trim across a compatible subdivision.
/// Non-analytic curves currently require identical supports and join parameters.
pub(super) fn same_domain_edge_range(
    first: &Edge,
    second: &Edge,
    vertex: VertexId,
    tol: ModelTolerance,
) -> KernelResult<Option<ParameterRange>> {
    let same = match (&first.curve, &second.curve) {
        (Curve3::Line(a), Curve3::Line(b)) => {
            a.direction()
                .normalize()
                .cross(b.direction().normalize())
                .magnitude()
                <= tol.angular
                && (a.origin() - b.origin())
                    .cross(a.direction().normalize())
                    .magnitude()
                    <= tol.linear
        }
        (Curve3::Circle(a), Curve3::Circle(b)) => {
            a.center().distance(b.center()) <= tol.linear
                && (a.radius() - b.radius()).abs() <= tol.linear
                && a.normal().cross(b.normal()).magnitude() <= tol.angular
        }
        (a, b) => a == b,
    };
    if !same {
        return Ok(None);
    }
    let prepend = first.vertices[0] == vertex;
    let first_join = if prepend {
        first.range.start
    } else {
        first.range.end
    };
    let second_join = if second.vertices[0] == vertex {
        second.range.start
    } else {
        second.range.end
    };
    let second_end = if second.vertices[0] == vertex {
        second.range.end
    } else {
        second.range.start
    };
    if !matches!(first.curve, Curve3::Line(_) | Curve3::Circle(_))
        && (first_join - second_join).abs() > tol.parameter
    {
        return Ok(None);
    }
    let a = first.curve.evaluate(first_join)?.first_derivative;
    let b = second.curve.evaluate(second_join)?.first_derivative;
    let scale = b.dot(a) / a.magnitude2();
    let extension = (second_end - second_join) * scale;
    if !extension.is_finite() || (extension.signum() == first.range.span().signum()) == prepend {
        return Ok(None);
    }
    let agrees = (0..=4).all(|i| {
        let fraction = i as f64 / 4.0;
        let a = first.curve.evaluate(first_join + extension * fraction);
        let b = second
            .curve
            .evaluate(second_join + (second_end - second_join) * fraction);
        matches!((a, b), (Ok(a), Ok(b)) if a.point.distance(b.point) <= tol.linear)
    });
    if !agrees {
        return Ok(None);
    }
    let range = ParameterRange::new(
        if prepend {
            first_join + extension
        } else {
            first.range.start
        },
        if prepend {
            first.range.end
        } else {
            first_join + extension
        },
    )?;
    if let Some(period) = first.curve.domain().period {
        if range.span().abs() > period + tol.parameter * 32.0 {
            return Ok(None);
        }
    }
    if !first.curve.domain().contains_range(range, tol.parameter) {
        return Ok(None);
    }
    Ok(Some(range))
}

/// Independent intersection charts need a splice; adjacent trims of one
/// implicit curve must keep their shared parameterization.
pub(super) fn intersection_curves_to_join<'a>(
    first: &'a Edge,
    second: &'a Edge,
    vertex: VertexId,
    tolerance: ModelTolerance,
) -> KernelResult<Option<(&'a SurfaceIntersectionCurve, &'a SurfaceIntersectionCurve)>> {
    let (Curve3::SurfaceIntersection(a), Curve3::SurfaceIntersection(b)) = (&first.curve, &second.curve) else {
        return Ok(None);
    };
    if first.curve == second.curve
        && same_domain_edge_range(first, second, vertex, tolerance)?.is_some()
    {
        return Ok(None);
    }
    Ok(Some((a, b)))
}

impl Transaction<'_> {
    /// Normalize redundant partitions when an operation may have created them.
    /// Removing face partitions can expose degree-two edge subdivisions.
    pub(crate) fn unify_same_domain(&mut self) -> KernelResult<()> {
        self.merge_periodic_face_partitions()?;
        self.unify_same_domain_edges()
    }

    /// Remove partition edges between patches in the same periodic surface
    /// chart. Keep actual seams (two uses on one face), distinct supports, and
    /// disconnected face regions intact.
    fn merge_periodic_face_partitions(&mut self) -> KernelResult<()> {
        let mut candidates = BTreeSet::new();
        for (id, edge) in self.working.edges() {
            if edge.coedges.len() != 2 {
                continue;
            }
            let face = self
                .working
                .loop_(self.working.coedge(edge.coedges[0])?.loop_id)?
                .face;
            if matches!(
                self.working.face(face)?.surface,
                Surface3::Cylinder(_)
                    | Surface3::Torus(_)
                    | Surface3::RollingBall(_)
                    | Surface3::Blend(_)
                    | Surface3::Ruled(_)
            ) {
                candidates.insert(id);
            }
        }
        // A merge deletes only its own edge, but may enable earlier candidates
        // whose faces have now been joined; rescan from the first after each.
        loop {
            let mut merged = None;
            for edge in &candidates {
                if self.merge_face_partition(*edge)? {
                    merged = Some(*edge);
                    break;
                }
            }
            match merged {
                Some(edge) => candidates.remove(&edge),
                None => return Ok(()),
            };
        }
    }

    /// Join adjacent patches of one exact support without changing material.
    /// Blend contacts may consume these representation boundaries just as
    /// they consume parameter seams within one face.
    pub(super) fn merge_face_partition(&mut self, edge: EdgeId) -> KernelResult<bool> {
        let value = self.working.edge(edge)?;
        if value.coedges.len() != 2 {
            return Ok(false);
        }
        let first_use = self.working.coedge(value.coedges[0])?;
        let second_use = self.working.coedge(value.coedges[1])?;
        let first_loop = self.working.loop_(first_use.loop_id)?;
        let second_loop = self.working.loop_(second_use.loop_id)?;
        if first_loop.face == second_loop.face
            || first_loop.kind != LoopKind::Outer
            || second_loop.kind != LoopKind::Outer
        {
            return Ok(false);
        }
        let first = self.working.face(first_loop.face)?;
        let second = self.working.face(second_loop.face)?;
        if first.surface != second.surface
            || first.orientation != second.orientation
            || second.loops.len() != 1
        {
            return Ok(false);
        }
        // Align the removed patch with the survivor using whole periods only.
        let delta = first_use.pcurve.evaluate(first_use.range.start)?.point
            - second_use.pcurve.evaluate(second_use.range.end)?.point;
        let offset = whole_periods(first.surface.domains(), delta);
        if (delta - offset).magnitude() > self.kernel.tolerance.parameter * 16.0 {
            return Ok(false);
        }
        let removed_face = second_loop.face;
        let removed_loop = second_use.loop_id;
        if offset.magnitude2() > 0.0 {
            for id in self.working.loop_coedges(removed_loop)? {
                let pcurve = super::trim::translate_pcurve(
                    &self.working.coedge(id)?.pcurve,
                    offset,
                    self.kernel.tolerance,
                )?;
                self.working.coedges.get_mut(id)?.pcurve = pcurve;
            }
        }
        let name = self.working.face(removed_face)?.name;
        let survivor = self.kef(edge, removed_face)?.surviving_face;
        self.history.changed(
            name,
            ChangeKind::Merged,
            [self.working.face(survivor)?.name],
        );
        Ok(true)
    }

    /// Remove degree-two subdivisions using a local incidence index. Only the
    /// surviving endpoints need reconsideration after each merge.
    pub(crate) fn unify_same_domain_edges(&mut self) -> KernelResult<()> {
        let mut incidence = BTreeMap::<VertexId, BTreeSet<EdgeId>>::new();
        for (id, edge) in self.working.edges() {
            for vertex in edge.vertices {
                incidence.entry(vertex).or_default().insert(id);
            }
        }
        let mut pending = incidence
            .iter()
            .filter_map(|(vertex, edges)| (edges.len() == 2).then_some(*vertex))
            .collect::<BTreeSet<_>>();
        while let Some(vertex) = pending.pop_first() {
            let Some(edges) = incidence.get(&vertex) else {
                continue;
            };
            if edges.len() != 2 {
                continue;
            }
            let mut edges = edges.iter().copied();
            let edges = [edges.next().unwrap(), edges.next().unwrap()];
            let Some(candidate) = self.prepare_edge_merge(vertex, edges)? else {
                continue;
            };
            let survivor = candidate.survivor;
            let removed = candidate.removed;
            let endpoints = self.apply_edge_merge(candidate)?;
            incidence.remove(&vertex);
            for endpoint in endpoints {
                let edges = incidence
                    .get_mut(&endpoint)
                    .expect("surviving endpoint is indexed");
                edges.remove(&removed);
                edges.insert(survivor);
                pending.insert(endpoint);
            }
        }
        Ok(())
    }

    pub(super) fn unify_edge_vertex(&mut self, vertex: VertexId) -> KernelResult<()> {
        let edges = self.working.vertex_edges(vertex)?;
        if let [first, second] = edges.as_slice() {
            if let Some(candidate) = self.prepare_edge_merge(vertex, [*first, *second])? {
                self.apply_edge_merge(candidate)?;
            }
        }
        Ok(())
    }

    /// Prepare both face uses before changing any connectivity. Geometrically
    /// equivalent supports are insufficient without compatible loop adjacency.
    fn prepare_edge_merge(
        &self,
        vertex: VertexId,
        edges: [EdgeId; 2],
    ) -> KernelResult<Option<EdgeMerge>> {
        let first = self.working.edge(edges[0])?;
        let second = self.working.edge(edges[1])?;
        if first.coedges.len() != 2
            || second.coedges.len() != 2
            || first.vertices[0] == first.vertices[1]
            || second.vertices[0] == second.vertices[1]
            || self.working.edge_faces(edges[0])?.len() != 2
            || self.working.edge_faces(edges[1])?.len() != 2
        {
            return Ok(None);
        }
        // The removed edge's use adjacent to each surviving use in its loop.
        let mate = |keep: &super::Coedge| {
            second.coedges.iter().copied().find(|other| {
                self.working.coedge(*other).is_ok_and(|value| {
                    value.loop_id == keep.loop_id && (keep.next == *other || keep.previous == *other)
                })
            })
        };
        if let Some((a, b)) = intersection_curves_to_join(first, second, vertex, self.kernel.tolerance)? {
            let prepend = first.vertices[0] == vertex;
            let other_range = if (second.vertices[0] == vertex) == prepend {
                second.range.reversed()
            } else {
                second.range
            };
            let joined = if prepend {
                b.joined(other_range, a, first.range)?
            } else {
                a.joined(first.range, b, other_range)?
            };
            let Some(curve) = joined else {
                return Ok(None);
            };
            let exact = curve.exact_pcurves();
            let mut coedges = Vec::new();
            for id in &first.coedges {
                let keep = self.working.coedge(*id)?;
                let Some(other) = mate(keep) else {
                    return Ok(None);
                };
                let surface = &self.working.face(self.working.loop_(keep.loop_id)?.face)?.surface;
                let pcurve = if surface == curve.first_surface() {
                    &exact[0]
                } else if surface == curve.second_surface() {
                    &exact[1]
                } else {
                    return Ok(None);
                };
                // Keep each use in its existing chart, including opposite
                // lifts of a periodic seam on the same support.
                let anchor = if prepend { 1.0 } else { 0.0 };
                let old_anchor = if prepend == keep.forward {
                    keep.range.end
                } else {
                    keep.range.start
                };
                let delta =
                    keep.pcurve.evaluate(old_anchor)?.point - pcurve.evaluate(anchor)?.point;
                coedges.push(CoedgeMerge {
                    survivor: *id,
                    removed: other,
                    pcurve: super::trim::translate_pcurve(
                        pcurve,
                        whole_periods(surface.domains(), delta),
                        self.kernel.tolerance,
                    )?,
                    range: if keep.forward {
                        curve.range()
                    } else {
                        curve.range().reversed()
                    },
                });
            }
            if coedges[0].removed == coedges[1].removed {
                return Ok(None);
            }
            return Ok(Some(EdgeMerge {
                vertex,
                survivor: edges[0],
                removed: edges[1],
                prepend,
                range: curve.range(),
                curve: curve.into(),
                coedges,
            }));
        }
        let Some(range) = same_domain_edge_range(first, second, vertex, self.kernel.tolerance)?
        else {
            return Ok(None);
        };
        let tol = self.kernel.tolerance;
        let mut coedges = Vec::new();
        for id in &first.coedges {
            let keep = self.working.coedge(*id)?;
            let Some(other) = mate(keep) else {
                break;
            };
            let map = |t: f64| {
                let fraction = (t - first.range.start) / first.range.span();
                keep.range.start
                    + keep.range.span()
                        * if keep.forward {
                            fraction
                        } else {
                            1.0 - fraction
                        }
            };
            let start = map(if keep.forward { range.start } else { range.end });
            let end = map(if keep.forward { range.end } else { range.start });
            let use_range = ParameterRange::new(start, end)?;
            let Ok(pcurve) = keep
                .pcurve
                .extended(ParameterRange::new(start.min(end), start.max(end))?)
            else {
                break;
            };
            let surface = &self
                .working
                .face(self.working.loop_(keep.loop_id)?.face)?
                .surface;
            let valid = (0..=16).all(|i| {
                let fraction = i as f64 / 16.0;
                let t = range.start + range.span() * fraction;
                let uv = pcurve.evaluate(map(t));
                let point = first.curve.evaluate(t);
                match (uv, point) {
                    (Ok(uv), Ok(point)) => {
                        surface.evaluate(uv.point.x, uv.point.y).is_ok_and(|value| {
                            value.point.distance(point.point)
                                <= tol.linear_at_scale(point.point.to_vec().magnitude()) * 8.0
                        })
                    }
                    _ => false,
                }
            });
            if !valid {
                break;
            }
            coedges.push(CoedgeMerge {
                survivor: *id,
                removed: other,
                pcurve,
                range: use_range,
            });
        }
        if coedges.len() != 2 || coedges[0].removed == coedges[1].removed {
            return Ok(None);
        }
        Ok(Some(EdgeMerge {
            vertex,
            survivor: edges[0],
            removed: edges[1],
            prepend: first.vertices[0] == vertex,
            curve: first.curve.clone(),
            range,
            coedges,
        }))
    }

    /// Apply a prepared edge join, retaining the surviving edge's identity.
    fn apply_edge_merge(&mut self, candidate: EdgeMerge) -> KernelResult<[VertexId; 2]> {
        let EdgeMerge {
            vertex,
            survivor,
            removed,
            prepend,
            curve,
            range,
            coedges,
        } = candidate;
        let removed_edge = self.working.edge(removed)?.clone();
        let other_vertex = removed_edge
            .vertices
            .iter()
            .copied()
            .find(|v| *v != vertex)
            .unwrap();
        for CoedgeMerge {
            survivor: keep_id,
            removed: remove_id,
            pcurve,
            range: use_range,
        } in coedges
        {
            let removed_use = self.working.coedge(remove_id)?.clone();
            self.working.coedges.get_mut(removed_use.previous)?.next = removed_use.next;
            self.working.coedges.get_mut(removed_use.next)?.previous = removed_use.previous;
            if self.working.loop_(removed_use.loop_id)?.first == Some(remove_id) {
                self.working.loops.get_mut(removed_use.loop_id)?.first = Some(keep_id);
            }
            let keep = self.working.coedges.get_mut(keep_id)?;
            keep.pcurve = pcurve;
            keep.range = use_range;
            let name = keep.name;
            self.working.coedges.remove(remove_id)?;
            self.history
                .changed(removed_use.name, ChangeKind::Merged, [name]);
            self.history.modified(name);
            self.record_loop_edit(removed_use.loop_id)?;
        }
        let edge = self.working.edges.get_mut(survivor)?;
        edge.vertices[if prepend { 0 } else { 1 }] = other_vertex;
        edge.curve = curve;
        edge.range = range;
        let name = edge.name;
        self.working.edges.remove(removed)?;
        self.history.merge_construction(removed_edge.name, name);
        self.history
            .changed(removed_edge.name, ChangeKind::Merged, [name]);
        self.history.changed(name, ChangeKind::Modified, [name]);
        let vertex_name = self.working.vertex(vertex)?.name;
        self.working.vertices.remove(vertex)?;
        self.history.changed(vertex_name, ChangeKind::Deleted, []);
        Ok(self.working.edge(survivor)?.vertices)
    }
}
