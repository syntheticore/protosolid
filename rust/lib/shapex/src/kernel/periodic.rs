use std::collections::{BTreeMap, BTreeSet};

use crate::{Point2, Vec2};

use super::canonicalize::same_domain_edge_range;
use super::transaction::Transaction;
use super::history::{ChangeKind, DerivationRole};
use super::trim::{face_boundary_curves, face_parameter_bounds, translate_pcurve};
use super::wire::curve_pair_intersection;
use super::{
    Curve2, Curve3, EdgeId, FaceId, KernelResult, Line2, ParameterRange, TrimmedCurve2, VertexId,
};

pub(super) enum SeamPlacement {
    PreserveBoundaries,
    AllowBoundarySplits,
}

impl Transaction<'_> {
    fn continuous_boundary_edges(
        &self,
        vertex: VertexId,
        edges: [EdgeId; 2],
    ) -> KernelResult<bool> {
        let first = self.working.edge(edges[0])?;
        let second = self.working.edge(edges[1])?;
        let faces = self.working.edge_faces(edges[0])?;
        if first.vertices[0] == first.vertices[1]
            || second.vertices[0] == second.vertices[1]
            || faces.len() != 2
            || faces.into_iter().collect::<BTreeSet<_>>()
                != self.working.edge_faces(edges[1])?.into_iter().collect()
        {
            return Ok(false);
        }
        if let (Curve3::SurfaceIntersection(a), Curve3::SurfaceIntersection(b)) =
            (&first.curve, &second.curve)
        {
            let prepend = first.vertices[0] == vertex;
            let other_range = if (second.vertices[0] == vertex) == prepend {
                second.range.reversed()
            } else {
                second.range
            };
            return Ok(if prepend {
                b.joined(other_range, a, first.range)?
            } else {
                a.joined(first.range, b, other_range)?
            }
            .is_some());
        }
        Ok(same_domain_edge_range(first, second, vertex, self.kernel.tolerance)?.is_some())
    }

    /// A representation seam must not divide an otherwise continuous boundary.
    /// Anchor it at a real boundary corner before publishing any operation.
    pub(super) fn normalize_periodic_edge_subdivisions(&mut self) -> KernelResult<()> {
        loop {
            let mut seams = Vec::new();
            for (id, edge) in self.working.edges() {
                let faces = self.working.edge_faces(id)?;
                if faces.len() == 1 && edge.coedges.len() == 2 {
                    seams.push((id, faces[0], edge.vertices));
                }
            }
            let mut relocated = false;
            for (seam, face, endpoints) in seams {
                let mut boundary = BTreeMap::<VertexId, BTreeSet<EdgeId>>::new();
                for edge_id in self.working.face_edges(face)? {
                    if self.working.edge_faces(edge_id)?.len() != 2 {
                        continue;
                    }
                    for vertex in self.working.edge(edge_id)?.vertices {
                        boundary.entry(vertex).or_default().insert(edge_id);
                    }
                }
                let mut split = false;
                for vertex in endpoints {
                    let Some(edges) = boundary.get(&vertex) else {
                        continue;
                    };
                    if edges.len() == 2 {
                        let mut edges = edges.iter().copied();
                        let pair = [edges.next().unwrap(), edges.next().unwrap()];
                        let continuous = self.continuous_boundary_edges(vertex, pair)?;
                        split |= continuous;
                    }
                }
                if !split {
                    continue;
                }
                let mut corners = Vec::new();
                for (vertex, edges) in boundary {
                    if endpoints.contains(&vertex) || edges.len() != 2 {
                        continue;
                    }
                    let mut edges = edges.into_iter();
                    if !self.continuous_boundary_edges(
                        vertex,
                        [edges.next().unwrap(), edges.next().unwrap()],
                    )? {
                        corners.push(vertex);
                    }
                }
                corners.sort_by_key(|vertex| self.working.vertex(*vertex).unwrap().name);
                for vertex in corners {
                    let use_id = self
                        .working
                        .vertex_edges(vertex)?
                        .into_iter()
                        .flat_map(|edge| self.working.edge(edge).unwrap().coedges.clone())
                        .find(|id| {
                            self.working.coedge_vertices(*id).unwrap()[0] == vertex
                                && self
                                    .working
                                    .loop_(self.working.coedge(*id).unwrap().loop_id)
                                    .unwrap()
                                    .face
                                    == face
                        })
                        .unwrap();
                    let coedge = self.working.coedge(use_id)?;
                    let target = coedge.pcurve.evaluate(coedge.range.start)?.point;
                    if self.relocate_periodic_seam(
                        seam,
                        face,
                        target,
                        SeamPlacement::PreserveBoundaries,
                        [
                            DerivationRole::PeriodicSeamVertex,
                            DerivationRole::PeriodicSeamEdge,
                            DerivationRole::PeriodicSeamUse,
                        ],
                    )? {
                        relocated = true;
                        break;
                    }
                }
                if relocated {
                    break;
                }
            }
            if !relocated {
                return Ok(());
            }
        }
    }

    fn split_periodic_boundary(
        &mut self,
        edge_id: EdgeId,
        parameter: f64,
        roles: [DerivationRole; 3],
    ) -> KernelResult<VertexId> {
        let edge = self.working.edge(edge_id)?.clone();
        let fraction = (parameter - edge.range.start) / edge.range.span();
        let name = self.names.next_derived(roles[0])?;
        let vertex = self.make_vertex(name, edge.curve.evaluate(parameter)?.point);
        self.history.derived(name, [edge.name], roles[0]);
        let name = self.names.next_derived(roles[1])?;
        let second = self.make_edge(
            name,
            [vertex, edge.vertices[1]],
            edge.curve.clone(),
            ParameterRange::new(parameter, edge.range.end)?,
        )?;
        self.history.derived(name, [edge.name], roles[1]);
        self.working.edges.get_mut(edge_id)?.vertices[1] = vertex;
        self.working.edges.get_mut(edge_id)?.range.end = parameter;
        for id in edge.coedges {
            let old = self.working.coedge(id)?.clone();
            let split = old.range.start
                + old.range.span()
                    * if old.forward {
                        fraction
                    } else {
                        1.0 - fraction
                    };
            let range = if old.forward {
                ParameterRange::new(split, old.range.end)?
            } else {
                ParameterRange::new(old.range.start, split)?
            };
            let name = self.names.next_derived(roles[2])?;
            let added = self.make_coedge(
                name,
                second,
                old.loop_id,
                old.forward,
                old.pcurve.clone(),
                range,
            )?;
            self.history.derived(name, [old.name], roles[2]);
            if old.forward {
                self.working.coedges.get_mut(id)?.range.end = split;
                self.insert_loop_coedge(id, old.next, added)?;
            } else {
                self.working.coedges.get_mut(id)?.range.start = split;
                self.insert_loop_coedge(old.previous, id, added)?;
            }
            self.history.modified(old.name);
        }
        for vertex in [edge.vertices[0], vertex, edge.vertices[1]] {
            self.repair_outgoing(vertex)?;
        }
        self.history
            .changed(edge.name, ChangeKind::Split, [edge.name, name]);
        Ok(vertex)
    }

    /// Relocate a constant-parameter seam between two winding boundary rings.
    /// This uses the surface's periodic chart, not a world axis or a particular
    /// solid type. Other faces retain their exact curves when a ring is split.
    pub(super) fn relocate_periodic_seam(
        &mut self,
        seam_id: EdgeId,
        face: FaceId,
        target: Point2,
        placement: SeamPlacement,
        roles: [DerivationRole; 3],
    ) -> KernelResult<bool> {
        let tolerance = self.kernel.tolerance;
        let surface = self.working.face(face)?.surface.clone();
        let seam = self.working.edge(seam_id)?.clone();
        if seam.coedges.len() != 2 {
            return Ok(false);
        }
        let first = self.working.coedge(seam.coedges[0])?;
        let second = self.working.coedge(seam.coedges[1])?;
        if first.loop_id != second.loop_id {
            return Ok(false);
        }
        let Curve2::Line(line) = &first.pcurve else {
            return Ok(false);
        };
        let Some(axis) = surface
            .domains()
            .iter()
            .enumerate()
            .find_map(|(axis, domain)| {
                (domain.period.is_some() && line.direction()[axis].abs() <= tolerance.parameter)
                    .then_some(axis)
            })
        else {
            return Ok(false);
        };
        let domain = surface.domains()[axis];
        let period = domain.period.unwrap();
        let other_axis = 1 - axis;
        let target_uv = target;
        let target = target[axis];
        let turns = (target - line.origin()[axis]) / period;
        if (turns - turns.round()).abs() * period <= tolerance.parameter {
            return Ok(false);
        }
        // Removing the seam exposes two rings. A nonmonotone ring may cross
        // the proposed cut more than once: prefer the requested boundary
        // point, and reject any other crossings between the chosen endpoints.
        let mut rings = Vec::new();
        let mut crossings = Vec::new();
        let bounds = face_parameter_bounds(
            &face_boundary_curves(&self.working, face, tolerance)?,
            tolerance,
        )?;
        for start in [first.next, second.next] {
            let mut current = start;
            let mut crossing = Vec::<(EdgeId, f64, Option<VertexId>, f64)>::new();
            while !seam.coedges.contains(&current) {
                let value = self.working.coedge(current)?;
                let midpoint = value.pcurve.evaluate(value.range.middle())?.point[axis];
                let lifted = domain.nearest(target, midpoint);
                let mut origin = Point2::new(0.0, 0.0);
                origin[axis] = lifted;
                let mut direction = Vec2::new(0.0, 0.0);
                direction[other_axis] = 1.0;
                let cut = TrimmedCurve2::new(Line2::new(origin, direction)?, bounds[other_axis])?;
                let segment = TrimmedCurve2::new(value.pcurve.clone(), value.range)?;
                let intersection = curve_pair_intersection(&segment, &cut, tolerance)?;
                if intersection.coincident {
                    return Ok(false);
                }
                for hit in intersection.hits {
                    let parameter = hit.first_parameter;
                    let fraction = (parameter - value.range.start) / value.range.span();
                    if fraction >= -tolerance.parameter && fraction <= 1.0 + tolerance.parameter {
                        let edge = self.working.edge(value.edge)?;
                        let parameter = edge.range.start
                            + edge.range.span()
                                * if value.forward {
                                    fraction
                                } else {
                                    1.0 - fraction
                                };
                        let endpoint = if fraction.abs() <= tolerance.parameter * 64.0 {
                            Some(edge.vertices[usize::from(!value.forward)])
                        } else if (fraction - 1.0).abs() <= tolerance.parameter * 64.0 {
                            Some(edge.vertices[usize::from(value.forward)])
                        } else {
                            None
                        };
                        if endpoint.is_none() || !crossing.iter().any(|hit| hit.2 == endpoint) {
                            let transverse =
                                value.pcurve.evaluate(hit.first_parameter)?.point[other_axis];
                            crossing.push((value.edge, parameter, endpoint, transverse));
                        }
                    }
                }
                current = value.next;
            }
            let selected = if crossing.len() == 1 {
                crossing[0]
            } else if let Some(hit) = crossing
                .iter()
                .find(|hit| (hit.3 - target_uv[other_axis]).abs() <= tolerance.parameter * 64.0)
            {
                *hit
            } else {
                return Ok(false);
            };
            crossings.extend(crossing);
            rings.push(selected);
        }
        let low = rings[0].3.min(rings[1].3);
        let high = rings[0].3.max(rings[1].3);
        // Publishing a chart cut may reanchor a closed edge, but must not
        // transfer an artificial subdivision to another open boundary.
        if matches!(placement, SeamPlacement::PreserveBoundaries)
            && rings.iter().any(|(edge, _, vertex, _)| {
                let edge = self.working.edge(*edge).unwrap();
                vertex.is_none() && edge.vertices[0] != edge.vertices[1]
            })
        {
            return Ok(false);
        }
        if crossings.iter().any(|hit| {
            hit.3 > low + tolerance.parameter * 64.0 && hit.3 < high - tolerance.parameter * 64.0
        }) {
            return Ok(false);
        }
        let loops = self.kemr(seam_id, seam.coedges[0])?;
        let mut outgoing = Vec::new();
        for (edge, parameter, vertex, _) in rings {
            let vertex = match vertex {
                Some(vertex) => vertex,
                None => self.split_periodic_boundary(edge, parameter, roles)?,
            };
            let id = self
                .working
                .vertex_edges(vertex)?
                .into_iter()
                .flat_map(|edge| self.working.edge(edge).unwrap().coedges.clone())
                .find(|id| {
                    self.working.coedge_vertices(*id).unwrap()[0] == vertex
                        && self
                            .working
                            .loop_(self.working.coedge(*id).unwrap().loop_id)
                            .unwrap()
                            .face
                            == face
                })
                .unwrap();
            outgoing.push(id);
        }
        outgoing.sort_by_key(|id| self.working.coedge(*id).unwrap().loop_id != loops.outer_loop);
        let mut anchor = target;
        let mut endpoints = Vec::new();
        for start in &outgoing {
            let mut current = *start;
            let start_uv = self
                .working
                .coedge(current)?
                .pcurve
                .evaluate(self.working.coedge(current)?.range.start)?
                .point;
            let mut previous = anchor;
            loop {
                let value = self.working.coedge(current)?.clone();
                let uv = value.pcurve.evaluate(value.range.start)?.point;
                let mut offset = Vec2::new(0.0, 0.0);
                offset[axis] = domain.whole_periods(previous - uv[axis]);
                let pcurve = translate_pcurve(&value.pcurve, offset, tolerance)?;
                previous = pcurve.evaluate(value.range.end)?.point[axis];
                self.working.coedges.get_mut(current)?.pcurve = pcurve;
                self.history.modified(value.name);
                current = value.next;
                if current == *start {
                    break;
                }
            }
            let mut uv = start_uv;
            uv[axis] = anchor;
            endpoints.push(uv);
            anchor = previous;
        }
        let range = ParameterRange::new(0.0, 1.0)?;
        let mut direction = Vec2::new(0.0, 0.0);
        direction[other_axis] = endpoints[1][other_axis] - endpoints[0][other_axis];
        let mut origin = endpoints[0];
        origin[axis] = endpoints[1][axis];
        let forward: Curve2 = Line2::new(origin, direction)?.into();
        origin[axis] = endpoints[0][axis];
        let reverse: Curve2 = Line2::new(origin, direction)?.into();
        let curve = surface.boundary_curve(forward.clone(), range, tolerance)?;
        let relocated = self.mekr(
            outgoing[0],
            outgoing[1],
            curve,
            range,
            forward,
            reverse,
            range,
        )?;
        let name = self.working.edge(relocated.edge)?.name;
        self.history
            .changed(seam.name, ChangeKind::Modified, [name]);
        for vertex in seam.vertices {
            self.unify_edge_vertex(vertex)?;
        }
        Ok(true)
    }
}
