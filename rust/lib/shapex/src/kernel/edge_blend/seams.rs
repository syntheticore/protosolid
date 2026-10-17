use super::super::periodic::SeamPlacement;
use super::super::PersistentId;
use super::*;

impl EulerTransaction<'_> {
    /// Keep chart cuts out of terminating vertex gaps while preparing a blend.
    /// Temporary subdivisions are rebound to the selection, then normalized
    /// back to real corners when the completed operation is published.
    pub(super) fn relocate_blend_endpoint_seams(
        &mut self,
        selected: &BTreeSet<EdgeId>,
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        let shells = selected
            .iter()
            .flat_map(|edge| self.working.edge_faces(*edge).unwrap())
            .map(|face| self.working.face(face).unwrap().shell)
            .collect::<BTreeSet<_>>();
        let seams = self
            .working
            .edges()
            .filter_map(|(id, edge)| {
                let faces = self.working.edge_faces(id).unwrap();
                (faces.len() == 1
                    && edge.coedges.len() == 2
                    && shells.contains(&self.working.face(faces[0]).unwrap().shell))
                .then(|| (id, faces[0], edge.vertices))
            })
            .collect::<Vec<_>>();
        'seams: for (seam, face, vertices) in seams {
            for vertex in vertices {
                if self.working.vertex_faces(vertex)?.len() <= 2 {
                    continue;
                }
                for boundary in self.working.vertex_edges(vertex)? {
                    if self.working.edge_faces(boundary)?.len() == 2 {
                        let edge = self.working.edge(boundary)?;
                        let use_id = edge
                            .coedges
                            .iter()
                            .find(|id| {
                                self.working
                                    .loop_(self.working.coedge(**id).unwrap().loop_id)
                                    .unwrap()
                                    .face
                                    == face
                            })
                            .copied();
                        if let Some(use_id) = use_id {
                            let coedge = self.working.coedge(use_id)?;
                            let target = coedge
                                .pcurve
                                .evaluate((coedge.range.start + coedge.range.end) * 0.5)?
                                .point;
                            if self.relocate_periodic_seam(
                                seam,
                                face,
                                target,
                                SeamPlacement::AllowBoundarySplits,
                                [
                                    kind.role(BlendRole::PeriodicSplitVertex),
                                    kind.role(BlendRole::PeriodicSplitEdge),
                                    kind.role(BlendRole::PeriodicSplitUse),
                                ],
                            )? {
                                continue 'seams;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

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

    /// Move the seam beyond a boundary that the blend would consume.
    fn relocate_periodic_blend_seam(
        &mut self,
        seam_id: EdgeId,
        face: FaceId,
        boundary: EdgeId,
        vertex: VertexId,
        kind: EdgeBlendKind,
    ) -> KernelResult<bool> {
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
            .point;
        self.relocate_periodic_seam(
            seam_id,
            face,
            target,
            SeamPlacement::AllowBoundarySplits,
            [
                kind.role(BlendRole::PeriodicSplitVertex),
                kind.role(BlendRole::PeriodicSplitEdge),
                kind.role(BlendRole::PeriodicSplitUse),
            ],
        )
    }
}
