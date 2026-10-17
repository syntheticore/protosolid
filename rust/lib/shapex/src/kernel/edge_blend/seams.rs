use super::super::trim::translate_pcurve;
use super::super::PersistentId;
use super::*;

impl EulerTransaction<'_> {
    /// Find boundary fragments that a blend contact would consume or reverse.
    /// Seams and redundant partitions at their far ends can be normalized
    /// before the network is prepared again.
    pub(super) fn consumed_blend_boundaries(
        &self,
        patches: &[BlendPatch],
    ) -> KernelResult<Vec<(EdgeId, VertexId)>> {
        let tolerance = self.kernel.tolerance;
        let mut consumed = Vec::new();
        for patch in patches {
            for end in &patch.ends {
                let BlendPatchEnd::Boundary(end) = end else {
                    continue;
                };
                for parent in 0..2 {
                    let boundary = self.working.coedge(end.adjacent_uses[parent])?.edge;
                    let edge = self.working.edge(boundary)?;
                    let endpoint = edge
                        .vertices
                        .iter()
                        .position(|vertex| *vertex == end.old_vertex)
                        .unwrap();
                    let mut parameter = edge
                        .curve
                        .project(end.contact_points[parent], tolerance)?
                        .parameter;
                    if let Some(period) = edge.curve.domain().period {
                        let anchor = if endpoint == 0 {
                            edge.range.start
                        } else {
                            edge.range.end
                        };
                        parameter += ((anchor - parameter) / period).round() * period;
                    }
                    let remaining = if endpoint == 0 {
                        edge.range.end - parameter
                    } else {
                        parameter - edge.range.start
                    };
                    if remaining * edge.range.span() > tolerance.parameter * edge.range.span().abs()
                    {
                        continue;
                    }
                    let far = edge.vertices[1 - endpoint];
                    consumed.push((boundary, far));
                }
            }
        }
        Ok(consumed)
    }

    pub(super) fn normalize_blend_boundary(
        &mut self,
        boundary: EdgeId,
        vertex: VertexId,
        kind: EdgeBlendKind,
    ) -> KernelResult<bool> {
        for seam in self.working.vertex_edges(vertex)? {
            let faces = self.working.edge_faces(seam)?;
            if faces.len() == 2 && self.merge_face_partition(seam)? {
                self.unify_edge_vertex(vertex)?;
                return Ok(true);
            }
            if faces.len() == 1
                && self.relocate_periodic_blend_seam(seam, faces[0], boundary, vertex, kind)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn rebind_blend_selection(
        &self,
        input_names: &[PersistentId],
        chamfer: Option<&ChamferSettings>,
    ) -> KernelResult<(BTreeSet<EdgeId>, Option<ChamferSettings>)> {
        let names = input_names
            .iter()
            .flat_map(|name| self.blend_descendant_names(*name))
            .collect::<BTreeSet<_>>();
        let selected = self
            .working
            .edges()
            .filter_map(|(id, edge)| names.contains(&edge.name).then_some(id))
            .collect();
        let mut settings = chamfer.cloned();
        if let Some(settings) = &mut settings {
            for (edge, face) in settings.reference_faces.clone() {
                let edges = self.blend_descendant_names(edge);
                let faces = self.blend_descendant_names(face);
                for (id, edge) in self
                    .working
                    .edges()
                    .filter(|(_, edge)| edges.contains(&edge.name))
                {
                    let face = self
                        .working
                        .edge_faces(id)?
                        .into_iter()
                        .find(|id| faces.contains(&self.working.face(*id).unwrap().name))
                        .unwrap();
                    settings
                        .reference_faces
                        .insert(edge.name, self.working.face(face)?.name);
                }
            }
        }
        Ok((selected, settings))
    }

    fn blend_descendant_names(&self, name: PersistentId) -> BTreeSet<PersistentId> {
        let mut names = BTreeSet::new();
        let mut pending = vec![name];
        while let Some(name) = pending.pop() {
            if names.insert(name) {
                pending.extend(self.history.outputs_for(name));
            }
        }
        names
    }

    fn split_blend_boundary(
        &mut self,
        edge_id: EdgeId,
        parameter: f64,
        kind: EdgeBlendKind,
    ) -> KernelResult<VertexId> {
        let edge = self.working.edge(edge_id)?.clone();
        let fraction = (parameter - edge.range.start) / edge.range.span();
        let name = self
            .names
            .next_derived(kind.role(BlendRole::PeriodicSplitVertex))?;
        let vertex = self.make_vertex(name, edge.curve.evaluate(parameter)?.point);
        self.history
            .derived(name, [edge.name], kind.role(BlendRole::PeriodicSplitVertex));
        let name = self
            .names
            .next_derived(kind.role(BlendRole::PeriodicSplitEdge))?;
        let second = self.make_edge(
            name,
            [vertex, edge.vertices[1]],
            edge.curve.clone(),
            ParameterRange::new(parameter, edge.range.end)?,
        )?;
        self.history
            .derived(name, [edge.name], kind.role(BlendRole::PeriodicSplitEdge));
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
            let name = self
                .names
                .next_derived(kind.role(BlendRole::PeriodicSplitUse))?;
            let added = self.make_coedge(
                name,
                second,
                old.loop_id,
                old.forward,
                old.pcurve.clone(),
                range,
            )?;
            self.history
                .derived(name, [old.name], kind.role(BlendRole::PeriodicSplitUse));
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
    fn relocate_periodic_blend_seam(
        &mut self,
        seam_id: EdgeId,
        face: FaceId,
        boundary: EdgeId,
        vertex: VertexId,
        kind: EdgeBlendKind,
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
        let period = surface.domains()[axis].period.unwrap();
        let other_axis = 1 - axis;
        let continuation = self.working.vertex_edges(vertex)?.into_iter().find(|edge| {
            *edge != boundary
                && *edge != seam_id
                && self
                    .working
                    .edge_faces(*edge)
                    .is_ok_and(|faces| faces.contains(&face))
        });
        let Some(continuation) = continuation else {
            return Ok(false);
        };
        let use_id = self
            .working
            .edge(continuation)?
            .coedges
            .iter()
            .copied()
            .find(|id| {
                self.working
                    .coedge(*id)
                    .is_ok_and(|value| self.working.loop_(value.loop_id).unwrap().face == face)
            })
            .unwrap();
        let coedge = self.working.coedge(use_id)?;
        let target = coedge
            .pcurve
            .evaluate((coedge.range.start + coedge.range.end) * 0.5)?
            .point[axis];
        // Removing the seam exposes two rings. Locate a unique crossing on
        // each before editing; a chart cut must stay inside the retained face.
        let mut rings = Vec::new();
        let bounds = face_parameter_bounds(
            &face_boundary_curves(&self.working, face, tolerance)?,
            tolerance,
        )?;
        for start in [first.next, second.next] {
            let mut current = start;
            let mut crossing = Vec::new();
            while !seam.coedges.contains(&current) {
                let value = self.working.coedge(current)?;
                let midpoint = value
                    .pcurve
                    .evaluate((value.range.start + value.range.end) * 0.5)?
                    .point[axis];
                let lifted = target + ((midpoint - target) / period).round() * period;
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
                    if fraction > tolerance.parameter && fraction < 1.0 - tolerance.parameter {
                        let edge = self.working.edge(value.edge)?;
                        let parameter = edge.range.start
                            + edge.range.span()
                                * if value.forward {
                                    fraction
                                } else {
                                    1.0 - fraction
                                };
                        crossing.push((value.edge, parameter));
                    }
                }
                current = value.next;
            }
            if crossing.len() != 1 {
                return Ok(false);
            }
            rings.push(crossing[0]);
        }
        let loops = self.kemr_unchecked(seam_id, seam.coedges[0])?;
        let mut outgoing = Vec::new();
        for (edge, parameter) in rings {
            let vertex = self.split_blend_boundary(edge, parameter, kind)?;
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
                offset[axis] = ((previous - uv[axis]) / period).round() * period;
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
        let relocated = self.mekr_unchecked(
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
