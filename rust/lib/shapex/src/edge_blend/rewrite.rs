//! Replace the blended edges by their prepared patches: contact vertices,
//! rolling, cap and junction faces, and removal of the consumed topology.

use super::*;
use crate::{invalid, topology_error};

impl Transaction<'_> {
    pub(super) fn rewrite_blend_network(
        &mut self,
        patches: &[BlendPatch],
        junctions: &BTreeMap<VertexId, BlendJunction>,
        junction_surfaces: &BTreeMap<VertexId, Surface3>,
        closed_seams: &BTreeSet<VertexId>,
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        let first_patch = patches.first().ok_or_else(|| {
            invalid("blend network is empty")
        })?;
        let shell = self.working.face(first_patch.spine.faces[0])?.shell;
        for patch in patches {
            for face in patch.spine.faces {
                if self.working.face(face)?.shell != shell {
                    return Err(topology_error("blend network crosses shell boundaries"));
                }
            }
        }
        let (termination_gaps, crossing_gaps) = self.termination_gaps(patches)?;
        let mut rewrite = BlendRewrite {
            termination_gaps,
            crossing_gaps,
            ..Default::default()
        };
        self.create_contact_vertices(&mut rewrite, patches, junctions, kind)?;
        self.retarget_terminated_rails(&rewrite, patches, junctions)?;
        self.retarget_junction_seams(&rewrite, patches, junctions)?;
        self.create_junction_faces(&mut rewrite, shell, junctions, junction_surfaces, kind)?;
        self.create_cap_faces(&mut rewrite, shell, patches, kind)?;
        for patch in patches {
            self.create_blend_face(&mut rewrite, shell, patch, junctions, kind)?;
        }
        self.orient_junction_uses(&rewrite)?;
        self.close_rim_corners(&mut rewrite, patches, junctions, junction_surfaces, kind)?;
        self.close_blend_loops(&mut rewrite)?;
        let modified_faces = self.remove_blended_topology(&rewrite, patches)?;
        self.merge_transition_faces(&rewrite, closed_seams)?;
        for face in modified_faces {
            let name = self.working.face(face)?.name;
            self.history.changed(name, ChangeKind::Modified, [name]);
        }
        // Removing construction face joins exposes their degree-two rail
        // vertices. Consume those subdivisions in the same transaction.
        self.unify_same_domain()?;
        let body = self.working.shell(shell)?.body;
        self.touch(body);
        self.composes_imported_topology = true;
        Ok(())
    }

    /// Capture every external loop gap before any endpoint changes.
    fn termination_gaps(
        &self,
        patches: &[BlendPatch],
    ) -> KernelResult<(TerminationGaps, TerminationGaps)> {
        let mut termination_gaps = BTreeMap::new();
        let mut crossing_gaps = BTreeMap::new();
        for patch in patches {
            for endpoint_index in 0..2 {
                let BlendPatchEnd::Boundary(endpoint) = &patch.ends[endpoint_index] else {
                    continue;
                };
                let edge = |parent: usize| -> KernelResult<EdgeId> {
                    Ok(match endpoint.tangent_termination {
                        Some((seam, index)) if index == parent => seam,
                        _ => self.working.coedge(endpoint.adjacent_uses[parent])?.edge,
                    })
                };
                let edges = [edge(0)?, edge(1)?];
                let key = (patch.spine.edge.name, endpoint.old_vertex);
                if let Some((seam, second_face)) = endpoint.crossing {
                    // The end boundary runs through the first support up to the
                    // seam, and through the second from the seam onwards.
                    termination_gaps.insert(
                        key,
                        self.termination_gap(
                            [edges[0], seam],
                            endpoint.terminating_face,
                            endpoint.old_vertex,
                        )?,
                    );
                    crossing_gaps.insert(
                        key,
                        self.termination_gap([seam, edges[1]], second_face, endpoint.old_vertex)?,
                    );
                } else {
                    termination_gaps.insert(
                        key,
                        self.termination_gap(edges, endpoint.terminating_face, endpoint.old_vertex)?,
                    );
                }
            }
        }
        Ok((termination_gaps, crossing_gaps))
    }

    /// The consecutive uses of `edges` in `face` around `vertex`, and whether
    /// the use of the first edge precedes the gap.
    fn termination_gap(
        &self,
        edges: [EdgeId; 2],
        face: FaceId,
        vertex: VertexId,
    ) -> KernelResult<(CoedgeId, CoedgeId, bool)> {
        let mut uses = Vec::with_capacity(2);
        for edge in edges {
            let terminating_use = self
                .working
                .edge(edge)?
                .coedges
                .iter()
                .copied()
                .find(|coedge| {
                    self.working
                        .coedge(*coedge)
                        .and_then(|value| self.working.loop_(value.loop_id))
                        .map(|loop_value| loop_value.face == face)
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    topology_error("blend boundary rail has no terminating-face use")
                })?;
            uses.push(terminating_use);
        }
        let first = self.working.coedge_vertices(uses[0])?;
        let second = self.working.coedge_vertices(uses[1])?;
        let gap = if first[1] == vertex && second[0] == vertex {
            (uses[0], uses[1], true)
        } else if second[1] == vertex && first[0] == vertex {
            (uses[1], uses[0], false)
        } else {
            return Err(topology_error("blend terminating-face uses do not form one vertex gap"));
        };
        if self.working.coedge(gap.0)?.next != gap.1 {
            return Err(topology_error("blend terminating-face gap is not consecutive"));
        }
        Ok(gap)
    }

    /// Create the vertex at each contact of a blend with its parent faces,
    /// reusing a boundary vertex where the contact consumes a whole edge.
    fn create_contact_vertices(
        &mut self,
        rewrite: &mut BlendRewrite,
        patches: &[BlendPatch],
        junctions: &BTreeMap<VertexId, BlendJunction>,
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        let tolerance = self.kernel.tolerance;
        let contact_vertices = &mut rewrite.contact_vertices;
        let collapsed_edges = &mut rewrite.collapsed_edges;
        for patch in patches {
            for endpoint_index in 0..2 {
                let old_vertex = patch.spine.edge.vertices[endpoint_index];
                for parent_index in 0..2 {
                    let face = patch.spine.faces[parent_index];
                    let key =
                        Self::contact_vertex_key(patch, endpoint_index, parent_index, junctions);
                    if contact_vertices.contains_key(&key) {
                        continue;
                    }
                    let point = patch.contact_points[endpoint_index][parent_index];
                    if let BlendPatchEnd::Boundary(endpoint) = &patch.ends[endpoint_index] {
                        let edge_id = self
                            .working
                            .coedge(endpoint.adjacent_uses[parent_index])?
                            .edge;
                        let edge = self.working.edge(edge_id)?;
                        let opposite = if edge.vertices[0] == old_vertex {
                            edge.vertices[1]
                        } else {
                            edge.vertices[0]
                        };
                        if !endpoint.cap.is_some_and(|(free, _)| free == parent_index)
                            && opposite != old_vertex
                            && point.distance(self.working.vertex(opposite)?.point)
                                <= tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0))
                        {
                            // The new contact consumes this whole boundary edge.
                            // Reuse its far vertex so the remaining faces stay sewn.
                            contact_vertices.insert(key, opposite);
                            collapsed_edges.insert(edge_id, opposite);
                            continue;
                        }
                    }
                    let name = self.names.next_derived(kind.role(BlendRole::ContactVertex))?;
                    let vertex = self.make_vertex(name, point);
                    self.history.derived(
                        name,
                        [
                            self.working.vertex(old_vertex)?.name,
                            self.working.face(face)?.name,
                        ],
                        kind.role(BlendRole::ContactVertex),
                    );
                    self.history.inherit_construction_tags(name, self.working.vertex(old_vertex)?.name);
                    contact_vertices.insert(key, vertex);
                }
                if let (
                    BlendPatchEnd::Boundary(BlendEndpoint {
                        crossing: Some((seam, second_face)),
                        ..
                    }),
                    Some((point, _)),
                ) = (&patch.ends[endpoint_index], &patch.crossings[endpoint_index])
                {
                    let name = self.names.next_derived(kind.role(BlendRole::ContactVertex))?;
                    let vertex = self.make_vertex(name, *point);
                    self.history.derived(
                        name,
                        [
                            self.working.vertex(old_vertex)?.name,
                            self.working.edge(*seam)?.name,
                        ],
                        kind.role(BlendRole::ContactVertex),
                    );
                    contact_vertices.insert((old_vertex, *second_face, Some(*seam)), vertex);
                }
            }
        }
        Ok(())
    }

    /// Move the rails that a blend boundary terminates on, and any tangent
    /// termination seam, from the blended vertex to their contact vertices.
    fn retarget_terminated_rails(
        &mut self,
        rewrite: &BlendRewrite,
        patches: &[BlendPatch],
        junctions: &BTreeMap<VertexId, BlendJunction>,
    ) -> KernelResult<()> {
        let contact_vertices = &rewrite.contact_vertices;
        let collapsed_edges = &rewrite.collapsed_edges;
        let mut trimmed = BTreeSet::new();
        for patch in patches {
            for endpoint_index in 0..2 {
                let BlendPatchEnd::Boundary(endpoint) = &patch.ends[endpoint_index] else {
                    continue;
                };
                if let Some((seam, parent)) = endpoint.tangent_termination {
                    let vertex = contact_vertices
                        [&Self::contact_vertex_key(patch, endpoint_index, parent, junctions)];
                    self.retarget_edge_endpoint_allow_extension(
                        seam,
                        endpoint.old_vertex,
                        vertex,
                        endpoint.contact_points[parent],
                    )?;
                }
                if let (Some((seam, second_face)), Some((point, _))) =
                    (endpoint.crossing, &patch.crossings[endpoint_index])
                {
                    self.retarget_edge_endpoint(
                        seam,
                        endpoint.old_vertex,
                        contact_vertices[&(endpoint.old_vertex, second_face, Some(seam))],
                        *point,
                    )?;
                }
                for parent_index in 0..2 {
                    if endpoint.cap.is_some_and(|(free, _)| free == parent_index) {
                        continue;
                    }
                    let adjacent_edge = self
                        .working
                        .coedge(endpoint.adjacent_uses[parent_index])?
                        .edge;
                    if !trimmed.insert((adjacent_edge, endpoint.old_vertex)) {
                        continue;
                    }
                    let contact_vertex = contact_vertices
                        [&Self::contact_vertex_key(patch, endpoint_index, parent_index, junctions)];
                    if let Some(vertex) = collapsed_edges.get(&adjacent_edge) {
                        // Keep the loop-gap handles alive until the new boundary
                        // is inserted, then remove this consumed edge and its uses.
                        self.working.edges.get_mut(adjacent_edge)?.vertices = [*vertex; 2];
                        continue;
                    }
                    if endpoint.allow_adjacent_extension[parent_index] {
                        self.retarget_edge_endpoint_allow_extension(
                            adjacent_edge,
                            endpoint.old_vertex,
                            contact_vertex,
                            endpoint.contact_points[parent_index],
                        )?;
                    } else {
                        self.retarget_edge_endpoint(
                            adjacent_edge,
                            endpoint.old_vertex,
                            contact_vertex,
                            endpoint.contact_points[parent_index],
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Move the seams that pass through blended junction vertices onto the
    /// contact vertices of their faces.
    fn retarget_junction_seams(
        &mut self,
        rewrite: &BlendRewrite,
        patches: &[BlendPatch],
        junctions: &BTreeMap<VertexId, BlendJunction>,
    ) -> KernelResult<()> {
        let contact_vertices = &rewrite.contact_vertices;
        for (old_vertex, junction) in junctions {
            let (seam_edge, tangent_faces, second_seam) = match junction.kind {
                BlendJunctionKind::Smooth {
                    seam_edge,
                    tangent_faces,
                    second_seam,
                } => (seam_edge, tangent_faces, second_seam),
                BlendJunctionKind::Miter {
                    seam_edge,
                    wall_faces,
                } => (seam_edge, wall_faces, None),
                _ => continue,
            };
            let new_vertex = contact_vertices[&(*old_vertex, tangent_faces[0], None)];
            self.retarget_edge_endpoint(
                seam_edge,
                *old_vertex,
                new_vertex,
                junction.contact_points[&tangent_faces[0]],
            )?;
            if let Some((edge, pair)) = second_seam {
                self.retarget_edge_endpoint(
                    edge,
                    *old_vertex,
                    contact_vertices[&(*old_vertex, pair[0], None)],
                    junction.contact_points[&pair[0]],
                )?;
            }
        }
        let selected_edges = patches
            .iter()
            .map(|patch| patch.spine.edge_id)
            .collect::<BTreeSet<_>>();
        for (old_vertex, junction) in junctions {
            let BlendJunctionKind::TangentChain { tangent_faces } = junction.kind else {
                continue;
            };
            for seam_edge in self
                .working
                .vertex_edges(*old_vertex)?
                .into_iter()
                .filter(|edge| !selected_edges.contains(edge))
            {
                let seam_faces = self
                    .working
                    .edge_faces(seam_edge)?
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                if seam_faces.len() != 1 {
                    continue;
                }
                let seam_face = *seam_faces.first().ok_or_else(|| {
                    topology_error("tangent-chain seam lost its support face")
                })?;
                if !tangent_faces.contains(&seam_face) {
                    continue;
                }
                self.retarget_edge_endpoint(
                    seam_edge,
                    *old_vertex,
                    contact_vertices[&(*old_vertex, seam_face, None)],
                    junction.contact_points[&seam_face],
                )?;
            }
        }
        for (old_vertex, junction) in junctions {
            let BlendJunctionKind::RimCorner {
                carrier_face,
                seam_edge,
            } = junction.kind
            else {
                continue;
            };
            let singular_key = (*old_vertex, carrier_face, None);
            let singular_vertex = contact_vertices[&singular_key];
            let singular_point = self.working.vertex(singular_vertex)?.point;
            self.retarget_edge_endpoint(seam_edge, *old_vertex, singular_vertex, singular_point)?;
        }
        Ok(())
    }

    /// Create the face filling each trihedral or rim-corner junction.
    fn create_junction_faces(
        &mut self,
        rewrite: &mut BlendRewrite,
        shell: ShellId,
        junctions: &BTreeMap<VertexId, BlendJunction>,
        junction_surfaces: &BTreeMap<VertexId, Surface3>,
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        for (vertex, junction) in junctions {
            if !matches!(
                junction.kind,
                BlendJunctionKind::Trihedral | BlendJunctionKind::RimCorner { .. }
            ) {
                continue;
            }
            let surface = junction_surfaces[vertex].clone();
            let face_name = self.names.next_derived(kind.role(BlendRole::VertexFace))?;
            let loop_name = self.names.next_derived(kind.role(BlendRole::VertexLoop))?;
            let (face, loop_id) = self.make_face_loop(
                shell,
                face_name,
                loop_name,
                surface,
                FaceOrientation::Forward,
            )?;
            self.history.derived(
                face_name,
                junction
                    .faces
                    .iter()
                    .map(|face| self.working.face(*face).map(|value| value.name))
                    .collect::<KernelResult<Vec<_>>>()?,
                kind.role(BlendRole::VertexFace),
            );
            self.history
                .derived(loop_name, [face_name], kind.role(BlendRole::VertexLoop));
            rewrite.junction_faces.insert(*vertex, face);
            rewrite.junction_loops.insert(*vertex, loop_id);
            rewrite.junction_uses.insert(*vertex, Vec::with_capacity(3));
        }
        Ok(())
    }

    /// Create the planar face closing each capped blend end.
    fn create_cap_faces(
        &mut self,
        rewrite: &mut BlendRewrite,
        shell: ShellId,
        patches: &[BlendPatch],
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        for patch in patches {
            for index in 0..2 {
                if let BlendPatchEnd::Boundary(BlendEndpoint {
                    cap: Some((_, plane)),
                    ..
                }) = &patch.ends[index]
                {
                    let face_name = self.names.next_derived(kind.role(BlendRole::CapFace))?;
                    let loop_name = self.names.next_derived(kind.role(BlendRole::CapLoop))?;
                    let (_, loop_id) = self.make_face_loop(
                        shell,
                        face_name,
                        loop_name,
                        (*plane).into(),
                        FaceOrientation::Forward,
                    )?;
                    self.history.derived(
                        face_name,
                        [patch.spine.edge.name],
                        kind.role(BlendRole::CapFace),
                    );
                    self.history
                        .derived(loop_name, [face_name], kind.role(BlendRole::CapLoop));
                    rewrite.cap_loops.insert((patch.spine.edge_id, index), loop_id);
                    rewrite.cap_uses.insert((patch.spine.edge_id, index), Vec::new());
                }
            }
        }
        Ok(())
    }

    /// Create the rolling face of one blend patch with its contact edges and
    /// end boundaries, splicing the contacts into the parent face loops.
    fn create_blend_face(
        &mut self,
        rewrite: &mut BlendRewrite,
        shell: ShellId,
        patch: &BlendPatch,
        junctions: &BTreeMap<VertexId, BlendJunction>,
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        let tolerance = self.kernel.tolerance;
        let BlendRewrite {
            termination_gaps,
            crossing_gaps,
            contact_vertices,
            junction_loops,
            junction_uses,
            cap_loops,
            cap_uses,
            blend_cycles,
            smooth_edges,
            contact_parent_uses,
            ..
        } = rewrite;
        {
            let selected_name = patch.spine.edge.name;
            let blend_face_name = self.names.next_derived(kind.role(BlendRole::BlendFace))?;
            let blend_loop_name = self.names.next_derived(kind.role(BlendRole::BlendLoop))?;
            let (_blend_face, blend_loop) = self.make_face_loop(
                shell,
                blend_face_name,
                blend_loop_name,
                patch.blend_surface.clone(),
                FaceOrientation::Forward,
            )?;
            self.history.derived(
                blend_face_name,
                [
                    selected_name,
                    self.working.face(patch.spine.faces[0])?.name,
                    self.working.face(patch.spine.faces[1])?.name,
                ],
                kind.role(BlendRole::BlendFace),
            );
            self.history.inherit_construction_tags(blend_face_name, selected_name);
            self.history
                .derived(blend_loop_name, [blend_face_name], kind.role(BlendRole::BlendLoop));

            let mut blend_uses = Vec::with_capacity(4);
            for parent_index in 0..2 {
                let boundary = &patch.contact_boundaries[parent_index];
                let first_vertex =
                    contact_vertices[&Self::contact_vertex_key(patch, 0, parent_index, junctions)];
                let second_vertex =
                    contact_vertices[&Self::contact_vertex_key(patch, 1, parent_index, junctions)];
                let edge_name = self.names.next_derived(kind.role(BlendRole::ContactEdge))?;
                let parent_use_name = self.names.next_derived(kind.role(BlendRole::ContactParentUse))?;
                let blend_use_name = self.names.next_derived(kind.role(BlendRole::ContactBlendUse))?;
                let edge = self.make_edge(
                    edge_name,
                    [first_vertex, second_vertex],
                    boundary.curve.clone(),
                    boundary.range,
                )?;
                let old_use = patch.spine.uses[parent_index];
                let parent_forward = self.working.coedge(old_use)?.forward;
                let parent_loop = self.working.coedge(old_use)?.loop_id;
                let parent_use = self.make_coedge(
                    parent_use_name,
                    edge,
                    parent_loop,
                    parent_forward,
                    boundary.parent_pcurve.clone(),
                    if parent_forward {
                        boundary.range
                    } else {
                        boundary.range.reversed()
                    },
                )?;
                let blend_forward = !parent_forward;
                let blend_use = self.make_coedge(
                    blend_use_name,
                    edge,
                    blend_loop,
                    blend_forward,
                    boundary.blend_pcurve.clone(),
                    if blend_forward {
                        boundary.range
                    } else {
                        boundary.range.reversed()
                    },
                )?;
                self.replace_loop_coedge(old_use, parent_use)?;
                contact_parent_uses.insert((patch.spine.edge_id, parent_index), parent_use);
                blend_uses.push(blend_use);
                self.history.derived(
                    edge_name,
                    [
                        selected_name,
                        self.working.face(patch.spine.faces[parent_index])?.name,
                    ],
                    kind.role(BlendRole::ContactEdge),
                );
                self.history.inherit_construction_tags(edge_name, selected_name);
                self.history.derived(
                    parent_use_name,
                    [edge_name],
                    kind.role(BlendRole::ContactParentUse),
                );
                self.history
                    .derived(blend_use_name, [edge_name], kind.role(BlendRole::ContactBlendUse));
            }

            for endpoint_index in 0..2 {
                let boundary = &patch.end_boundaries[endpoint_index];
                if let BlendPatchEnd::Junction(vertex) = &patch.ends[endpoint_index] {
                    if matches!(
                        junctions[vertex].kind,
                        BlendJunctionKind::Smooth { .. }
                            | BlendJunctionKind::TangentChain { .. }
                            | BlendJunctionKind::Miter { .. }
                    ) {
                        let vertices = [
                            contact_vertices
                                [&Self::contact_vertex_key(patch, endpoint_index, 0, junctions)],
                            contact_vertices
                                [&Self::contact_vertex_key(patch, endpoint_index, 1, junctions)],
                        ];
                        let contact_vertices_on_use =
                            self.working.coedge_vertices(blend_uses[0])?;
                        let desired_current_forward = contact_vertices_on_use[0] != vertices[0];
                        let unit = if let Some((edge, _)) = smooth_edges.get(vertex) {
                            self.working.edge(*edge)?.range
                        } else {
                            boundary.range
                        };
                        let (edge, same_direction, pcurve) = if let Some((edge, mate_pcurve)) =
                            smooth_edges.get(vertex).cloned()
                        {
                            let retained = self.working.edge(edge)?;
                            let edge_vertices = retained.vertices;
                            let same_direction = if edge_vertices == vertices {
                                true
                            } else if edge_vertices == [vertices[1], vertices[0]] {
                                false
                            } else {
                                return Err(topology_error("smooth blend transition patches do not share contact vertices"));
                            };
                            let pcurve = if let Some(pcurve) = mate_pcurve {
                                pcurve
                            } else {
                                let first = patch.contact_parameters[endpoint_index][0];
                                let second = patch.contact_parameters[endpoint_index][1];
                                let guide = patch.blend_parameterization.oriented_end_pcurve(
                                    first,
                                    second,
                                    same_direction,
                                )?;
                                let start = guide.evaluate(0.0)?.point;
                                let direction = (guide.evaluate(1.0)?.point - start) / unit.span();
                                let guide =
                                    Line2::new(start - direction * unit.start, direction)?.into();
                                let projected: Curve2 = SurfaceProjectionPcurve2::new(
                                    retained.curve.clone(),
                                    patch.blend_surface.clone(),
                                    guide,
                                    unit,
                                    tolerance,
                                )?
                                .into();
                                super::super::features::exact_pcurve(
                                    &retained.curve,
                                    &patch.blend_surface,
                                    &projected,
                                    tolerance,
                                )?
                            };
                            (edge, same_direction, pcurve)
                        } else {
                            let edge_name = self.names.next_derived(kind.role(BlendRole::TransitionEdge))?;
                            let edge =
                                self.make_edge(edge_name, vertices, boundary.curve.clone(), unit)?;
                            self.history.derived(
                                edge_name,
                                [selected_name],
                                kind.role(BlendRole::TransitionEdge),
                            );
                            self.history.inherit_construction_tags(edge_name, self.working.vertex(*vertex)?.name);
                            let mate_pcurve = matches!(junctions[vertex].kind, BlendJunctionKind::Miter { .. })
                                .then(|| boundary.parent_pcurve.clone());
                            smooth_edges.insert(*vertex, (edge, mate_pcurve));
                            (edge, true, boundary.blend_pcurve.clone())
                        };
                        let blend_forward = if same_direction {
                            desired_current_forward
                        } else {
                            !desired_current_forward
                        };
                        let use_name = self.names.next_derived(kind.role(BlendRole::TransitionBlendUse))?;
                        let blend_use = self.make_coedge(
                            use_name,
                            edge,
                            blend_loop,
                            blend_forward,
                            pcurve,
                            if blend_forward { unit } else { unit.reversed() },
                        )?;
                        blend_uses.push(blend_use);
                        self.history.derived(
                            use_name,
                            [self.working.edge(edge)?.name],
                            kind.role(BlendRole::TransitionBlendUse),
                        );
                        continue;
                    }
                }
                let vertices = [
                    contact_vertices
                        [&Self::contact_vertex_key(patch, endpoint_index, 0, junctions)],
                    contact_vertices
                        [&Self::contact_vertex_key(patch, endpoint_index, 1, junctions)],
                ];
                if let (
                    BlendPatchEnd::Boundary(BlendEndpoint {
                        old_vertex,
                        crossing: Some((seam, second_face)),
                        ..
                    }),
                    Some((_, second)),
                ) = (&patch.ends[endpoint_index], &patch.crossings[endpoint_index])
                {
                    // Split the end boundary at the vertex on the crossed seam.
                    let crossing = contact_vertices[&(*old_vertex, *second_face, Some(*seam))];
                    let key = (selected_name, *old_vertex);
                    for (vertices, boundary, gap) in [
                        ([vertices[0], crossing], boundary, termination_gaps[&key]),
                        ([crossing, vertices[1]], second, crossing_gaps[&key]),
                    ] {
                        let other_loop = self.working.coedge(gap.0)?.loop_id;
                        let (other_use, blend_use) = self.make_end_edge(
                            kind, selected_name, vertices, boundary, other_loop, gap.2, blend_loop,
                        )?;
                        self.insert_loop_coedge(gap.0, gap.1, other_use)?;
                        blend_uses.push(blend_use);
                    }
                    continue;
                }
                let (other_loop, other_forward) = match &patch.ends[endpoint_index] {
                    BlendPatchEnd::Boundary(endpoint) => {
                        let gap = termination_gaps[&(selected_name, endpoint.old_vertex)];
                        (
                            if endpoint.cap.is_some() {
                                cap_loops[&(patch.spine.edge_id, endpoint_index)]
                            } else {
                                self.working.coedge(gap.0)?.loop_id
                            },
                            gap.2,
                        )
                    }
                    BlendPatchEnd::Junction(vertex) => (junction_loops[vertex], true),
                };
                let (other_use, blend_use) = self.make_end_edge(
                    kind, selected_name, vertices, boundary, other_loop, other_forward, blend_loop,
                )?;
                match &patch.ends[endpoint_index] {
                    BlendPatchEnd::Boundary(endpoint) => {
                        let gap = termination_gaps[&(selected_name, endpoint.old_vertex)];
                        if let Some((free, plane)) = endpoint.cap {
                            let cap_key = (patch.spine.edge_id, endpoint_index);
                            cap_uses.get_mut(&cap_key).unwrap().push(other_use);
                            for parent in 0..2 {
                                let contact_vertex = vertices[parent];
                                let original_vertex = endpoint.old_vertex;
                                let start = self.working.vertex(original_vertex)?.point;
                                let end = self.working.vertex(contact_vertex)?.point;
                                let target_loop = if parent == free {
                                    self.working
                                        .coedge(
                                            contact_parent_uses[&(patch.spine.edge_id, parent)],
                                        )?
                                        .loop_id
                                } else {
                                    self.working.coedge(gap.0)?.loop_id
                                };
                                let surface = self
                                    .working
                                    .face(self.working.loop_(target_loop)?.face)?
                                    .surface
                                    .clone();
                                let curve: Curve3 = super::super::Line3::new(start, end - start)?.into();
                                let range = ParameterRange::new(0.0, 1.0)?;
                                let pcurve = |support: &Surface3| -> KernelResult<Curve2> {
                                    let a = support.project(start, tolerance)?.parameters;
                                    let mut b = support.project(end, tolerance)?.parameters;
                                    b.0 = support.domains()[0].nearest(b.0, a.0);
                                    Line2::new(
                                        Point2::new(a.0, a.1),
                                        Vec2::new(b.0 - a.0, b.1 - a.1),
                                    )
                                    .map(Curve2::from)
                                };
                                let name = self.names.next_derived(kind.role(BlendRole::CapEdge))?;
                                let cap_edge = self.make_edge(
                                    name,
                                    [original_vertex, contact_vertex],
                                    curve,
                                    range,
                                )?;
                                let (previous, next) = if parent == free {
                                    let rail = contact_parent_uses[&(patch.spine.edge_id, parent)];
                                    let rail_vertices = self.working.coedge_vertices(rail)?;
                                    if rail_vertices[0] == contact_vertex {
                                        (self.working.coedge(rail)?.previous, rail)
                                    } else {
                                        (rail, self.working.coedge(rail)?.next)
                                    }
                                } else {
                                    (gap.0, gap.1)
                                };
                                let forward =
                                    self.working.coedge_vertices(previous)?[1] == original_vertex;
                                let use_name = self.names.next_derived(kind.role(BlendRole::CapParentUse))?;
                                let use_id = self.make_coedge(
                                    use_name,
                                    cap_edge,
                                    target_loop,
                                    forward,
                                    pcurve(&surface)?,
                                    if forward { range } else { range.reversed() },
                                )?;
                                self.insert_loop_coedge(previous, next, use_id)?;
                                let cap_name = self.names.next_derived(kind.role(BlendRole::CapUse))?;
                                let cap_use = self.make_coedge(
                                    cap_name,
                                    cap_edge,
                                    cap_loops[&cap_key],
                                    !forward,
                                    pcurve(&Surface3::from(plane))?,
                                    if forward { range.reversed() } else { range },
                                )?;
                                cap_uses.get_mut(&cap_key).unwrap().push(cap_use);
                                self.history
                                    .derived(name, [selected_name], kind.role(BlendRole::CapEdge));
                                self.history.derived(
                                    use_name,
                                    [name],
                                    kind.role(BlendRole::CapParentUse),
                                );
                                self.history
                                    .derived(cap_name, [name], kind.role(BlendRole::CapUse));
                            }
                        } else {
                            self.insert_loop_coedge(gap.0, gap.1, other_use)?;
                        }
                    }
                    BlendPatchEnd::Junction(vertex) => {
                        junction_uses.get_mut(vertex).unwrap().push(other_use);
                    }
                }
                blend_uses.push(blend_use);
            }
            // The first contact rail borders the patch at blend fraction zero;
            // its interior lies in the positive blend direction. Its directed
            // tangent therefore determines winding in the affine support chart,
            // independently of the curved boundaries trimming the two ends.
            let contact = self.working.coedge(blend_uses[0])?;
            let tangent = patch.blend_parameterization.center_direction * contact.range.span();
            let inward = patch.blend_parameterization.blend_direction;
            let orientation = if tangent.x * inward.y - tangent.y * inward.x >= 0.0 {
                FaceOrientation::Forward
            } else {
                FaceOrientation::Reversed
            };
            blend_cycles.push((blend_loop, blend_uses, orientation));
        }
        Ok(())
    }

    /// Create an end edge of a rolling patch with its uses on `other_loop`
    /// and on the patch's own loop, and return those two uses.
    #[allow(clippy::too_many_arguments)]
    fn make_end_edge(
        &mut self,
        kind: EdgeBlendKind,
        selected_name: PersistentId,
        vertices: [VertexId; 2],
        boundary: &BlendBoundary,
        other_loop: LoopId,
        other_forward: bool,
        blend_loop: LoopId,
    ) -> KernelResult<(CoedgeId, CoedgeId)> {
        let edge_name = self.names.next_derived(kind.role(BlendRole::EndEdge))?;
        let other_use_name = self.names.next_derived(kind.role(BlendRole::EndOtherUse))?;
        let blend_use_name = self.names.next_derived(kind.role(BlendRole::EndBlendUse))?;
        let edge = self.make_edge(edge_name, vertices, boundary.curve.clone(), boundary.range)?;
        let oriented = |forward: bool| {
            if forward {
                boundary.range
            } else {
                boundary.range.reversed()
            }
        };
        let other_use = self.make_coedge(
            other_use_name,
            edge,
            other_loop,
            other_forward,
            boundary.parent_pcurve.clone(),
            oriented(other_forward),
        )?;
        let blend_use = self.make_coedge(
            blend_use_name,
            edge,
            blend_loop,
            !other_forward,
            boundary.blend_pcurve.clone(),
            oriented(!other_forward),
        )?;
        self.history
            .derived(edge_name, [selected_name], kind.role(BlendRole::EndEdge));
        self.history
            .derived(other_use_name, [edge_name], kind.role(BlendRole::EndOtherUse));
        self.history
            .derived(blend_use_name, [edge_name], kind.role(BlendRole::EndBlendUse));
        Ok((other_use, blend_use))
    }

    /// Check that every smooth transition joins two rolling patches, and
    /// orient each junction boundary use against its rolling-patch mate.
    fn orient_junction_uses(&mut self, rewrite: &BlendRewrite) -> KernelResult<()> {
        let BlendRewrite {
            junction_uses,
            blend_cycles,
            smooth_edges,
            ..
        } = rewrite;
        for (vertex, (edge, _)) in smooth_edges {
            if self.working.edge(*edge)?.coedges.len() != 2 {
                return Err(KernelError::new(
                    DiagnosticKind::TopologyInvariant,
                    format!(
                        "smooth blend junction {:?} is not shared by exactly two rolling patches",
                        vertex
                    ),
                ));
            }
        }
        for uses in junction_uses.values() {
            for junction_use in uses {
                let edge = self.working.coedge(*junction_use)?.edge;
                let blend_use = self
                    .working
                    .edge(edge)?
                    .coedges
                    .iter()
                    .copied()
                    .find(|candidate| candidate != junction_use)
                    .ok_or_else(|| {
                        topology_error("blend junction boundary has no paired rolling-patch use")
                    })?;
                let blend_loop = self.working.coedge(blend_use)?.loop_id;
                let cycle = blend_cycles
                    .iter()
                    .find_map(|(loop_id, uses, _)| (*loop_id == blend_loop).then_some(uses))
                    .ok_or_else(|| {
                        topology_error("blend junction boundary lost its rolling-patch cycle")
                    })?;
                self.orient_closing_coedge(cycle, blend_use)?;
                let blend_forward = self.working.coedge(blend_use)?.forward;
                self.orient_coedge(*junction_use, !blend_forward)?;
            }
        }
        Ok(())
    }

    /// Close each inner-rim corner with a circular edge between the carrier
    /// face contacts of its two rolling patches.
    fn close_rim_corners(
        &mut self,
        rewrite: &mut BlendRewrite,
        patches: &[BlendPatch],
        junctions: &BTreeMap<VertexId, BlendJunction>,
        junction_surfaces: &BTreeMap<VertexId, Surface3>,
        kind: EdgeBlendKind,
    ) -> KernelResult<()> {
        let tolerance = self.kernel.tolerance;
        let BlendRewrite {
            junction_loops,
            junction_uses,
            contact_parent_uses,
            ..
        } = rewrite;
        for (vertex, junction) in junctions {
            let BlendJunctionKind::RimCorner { carrier_face, .. } = junction.kind else {
                continue;
            };
            let incidents = patches
                .iter()
                .flat_map(|patch| {
                    (0..2).filter_map(move |endpoint_index| {
                        (patch.spine.edge.vertices[endpoint_index] == *vertex)
                            .then_some((patch, endpoint_index))
                    })
                })
                .collect::<Vec<_>>();
            if incidents.len() != 2 {
                return Err(topology_error("an inner-rim corner must join exactly two rolling patches"));
            }
            let carrier_uses = incidents
                .iter()
                .map(|(patch, _)| {
                    let parent_index = patch
                        .spine
                        .faces
                        .iter()
                        .position(|face| *face == carrier_face)
                        .ok_or_else(|| {
                            topology_error("inner-rim patch lost its carrier face")
                        })?;
                    Ok(contact_parent_uses[&(patch.spine.edge_id, parent_index)])
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let (previous, next) = if self.working.coedge(carrier_uses[0])?.next == carrier_uses[1]
            {
                (carrier_uses[0], carrier_uses[1])
            } else if self.working.coedge(carrier_uses[1])?.next == carrier_uses[0] {
                (carrier_uses[1], carrier_uses[0])
            } else {
                return Err(topology_error("inner-rim carrier uses are not adjacent at their corner"));
            };
            let start_vertex = self.working.coedge_vertices(previous)?[1];
            let end_vertex = self.working.coedge_vertices(next)?[0];
            let start = self.working.vertex(start_vertex)?.point;
            let end = self.working.vertex(end_vertex)?.point;
            let center = self.working.vertex(*vertex)?.point;
            let axis = match &junction_surfaces[vertex] {
                Surface3::Revolved(surface) => surface.axis().direction().normalize(),
                _ => {
                    return Err(topology_error("inner-rim junction lost its revolved surface"))
                }
            };
            let x_radius = start - center;
            let radius = x_radius.magnitude();
            let y_radius = axis.cross(x_radius).normalize() * radius;
            let x_unit = x_radius / radius;
            let y_unit = y_radius / radius;
            let delta = (end - center).dot(y_unit).atan2((end - center).dot(x_unit));
            let range = ParameterRange::new(0.0, delta)?;
            let curve: Curve3 = Circle3::new(center, x_radius, y_radius, tolerance)?.into();
            let projection_guide = |surface: &Surface3| -> KernelResult<Curve2> {
                let first = surface.project(start, tolerance)?.parameters;
                let mut second = surface.project(end, tolerance)?.parameters;
                second.0 = surface.domains()[0].nearest(second.0, first.0);
                Line2::new(
                    Point2::new(first.0, first.1),
                    Vec2::new(second.0 - first.0, second.1 - first.1) / range.span(),
                )
                .map(Curve2::from)
            };
            let carrier_surface = self.working.face(carrier_face)?.surface.clone();
            let carrier_pcurve: Curve2 = SurfaceProjectionPcurve2::new(
                curve.clone(),
                carrier_surface.clone(),
                projection_guide(&carrier_surface)?,
                range,
                tolerance,
            )?
            .into();
            let junction_pcurve: Curve2 = SurfaceProjectionPcurve2::new(
                curve.clone(),
                junction_surfaces[vertex].clone(),
                projection_guide(&junction_surfaces[vertex])?,
                range,
                tolerance,
            )?
            .into();
            let edge_name = self.names.next_derived(kind.role(BlendRole::RimCornerContactEdge))?;
            let carrier_use_name = self.names.next_derived(kind.role(BlendRole::RimCornerCarrierUse))?;
            let junction_use_name = self.names.next_derived(kind.role(BlendRole::RimCornerJunctionUse))?;
            let edge = self.make_edge(edge_name, [start_vertex, end_vertex], curve, range)?;
            let carrier_use = self.make_coedge(
                carrier_use_name,
                edge,
                self.working.coedge(previous)?.loop_id,
                true,
                carrier_pcurve,
                range,
            )?;
            let junction_use = self.make_coedge(
                junction_use_name,
                edge,
                junction_loops[vertex],
                true,
                junction_pcurve,
                range,
            )?;
            self.insert_loop_coedge(previous, next, carrier_use)?;
            junction_uses.get_mut(vertex).unwrap().push(junction_use);
            self.orient_closing_coedge(&junction_uses[vertex], junction_use)?;
            self.history.derived(
                edge_name,
                [self.working.face(carrier_face)?.name],
                kind.role(BlendRole::RimCornerContactEdge),
            );
            self.history.derived(
                carrier_use_name,
                [edge_name],
                kind.role(BlendRole::RimCornerCarrierUse),
            );
            self.history.derived(
                junction_use_name,
                [edge_name],
                kind.role(BlendRole::RimCornerJunctionUse),
            );
        }
        Ok(())
    }

    /// Order the boundary uses of every new rolling, cap and junction face
    /// into closed loops. Rolling faces retain their construction winding;
    /// caps and junctions determine theirs from their complete boundaries.
    fn close_blend_loops(&mut self, rewrite: &mut BlendRewrite) -> KernelResult<()> {
        let tolerance = self.kernel.tolerance;
        let mut blend_cycles = std::mem::take(&mut rewrite.blend_cycles);
        for (key, uses) in std::mem::take(&mut rewrite.cap_uses) {
            let area = uses.iter().try_fold(0.0, |sum, use_id| {
                let use_value = self.working.coedge(*use_id)?;
                Ok::<_, KernelError>(
                    sum + use_value
                        .pcurve
                        .signed_area_contribution(use_value.range, tolerance.parameter)?,
                )
            })?;
            let orientation = if area >= 0.0 {
                FaceOrientation::Forward
            } else {
                FaceOrientation::Reversed
            };
            blend_cycles.push((rewrite.cap_loops[&key], uses, orientation));
        }
        for (loop_id, uses, orientation) in blend_cycles {
            self.close_loop_from_coedges(loop_id, &uses)
                .map_err(|error| {
                    topology_error(format!("rolling-patch boundary cycle failed: {error}"))
                })?;
            let face = self.working.loop_(loop_id)?.face;
            self.orient_face(face, orientation)?;
        }
        for (vertex, uses) in &rewrite.junction_uses {
            if uses.len() != 3 {
                return Err(topology_error("a regular blend junction must have three boundary patches"));
            }
            self.close_loop_from_coedges(rewrite.junction_loops[vertex], uses)
                .map_err(|error| {
                    topology_error(format!("blend junction boundary cycle failed: {error}"))
                })?;
            let area = self.loop_parameter_area(rewrite.junction_loops[vertex])?;
            self.orient_face(
                rewrite.junction_faces[vertex],
                if area >= 0.0 {
                    FaceOrientation::Forward
                } else {
                    FaceOrientation::Reversed
                },
            )?;
        }
        Ok(())
    }

    /// Delete the blended edges, the boundary edges consumed by contacts and
    /// the blended vertices. Returns the faces whose boundaries changed.
    fn remove_blended_topology(
        &mut self,
        rewrite: &BlendRewrite,
        patches: &[BlendPatch],
    ) -> KernelResult<BTreeSet<FaceId>> {
        let contact_vertices = &rewrite.contact_vertices;
        for edge in rewrite.collapsed_edges.keys().copied() {
            let uses = self.working.edge(edge)?.coedges.clone();
            for use_id in &uses {
                let coedge = self.working.coedge(*use_id)?.clone();
                self.working.coedges.get_mut(coedge.previous)?.next = coedge.next;
                self.working.coedges.get_mut(coedge.next)?.previous = coedge.previous;
                if self.working.loop_(coedge.loop_id)?.first == Some(*use_id) {
                    self.working.loops.get_mut(coedge.loop_id)?.first = Some(coedge.next);
                }
            }
            self.delete_edge_and_uses(edge, &uses)?;
        }

        let mut old_vertices = BTreeSet::new();
        let mut modified_faces = BTreeSet::new();
        for patch in patches {
            old_vertices.extend(patch.spine.edge.vertices);
            modified_faces.extend(patch.spine.faces);
            for end in &patch.ends {
                if let BlendPatchEnd::Boundary(endpoint) = end {
                    modified_faces.insert(endpoint.terminating_face);
                    let seam = endpoint.tangent_termination.map(|(seam, _)| seam);
                    if let Some(seam) = seam.or(endpoint.crossing.map(|(seam, _)| seam)) {
                        modified_faces.extend(self.working.edge_faces(seam)?);
                    }
                }
            }
            self.delete_edge_and_uses(patch.spine.edge_id, &patch.spine.uses)?;
        }
        for old_vertex in old_vertices {
            if patches.iter().any(|patch| patch.ends.iter().any(|end|
                matches!(end, BlendPatchEnd::Boundary(endpoint) if endpoint.old_vertex == old_vertex && endpoint.cap.is_some()))) {
                continue;
            }
            let outputs = contact_vertices
                .iter()
                .filter_map(|((vertex, _, _), replacement)| {
                    (*vertex == old_vertex).then_some(*replacement)
                })
                .map(|vertex| self.working.vertex(vertex).map(|value| value.name))
                .collect::<KernelResult<Vec<_>>>()?;
            self.delete_split_vertex(old_vertex, outputs)?;
        }
        Ok(modified_faces)
    }

    fn merge_transition_faces(
        &mut self,
        rewrite: &BlendRewrite,
        closed_seams: &BTreeSet<VertexId>,
    ) -> KernelResult<()> {
        // Closed periodic edges are split into arcs while the rolling network
        // is constructed so that its endpoint machinery always works on
        // ordinary vertices.  Those construction joins are not material
        // boundaries.  Collapse every join whose two patches use the exact
        // same support; the final join then remains as the single
        // same-face parameter seam required by the periodic surface.
        // Retain the source seam, including on analytical surfaces. Choosing
        // an arbitrary construction join would put the blend's seam at a
        // different vertex from the adjoining wall and split their shared rim.
        let transition_edges = rewrite
            .smooth_edges
            .iter()
            .filter(|(vertex, _)| !closed_seams.contains(vertex))
            .map(|(_, (edge, _))| *edge)
            .collect::<Vec<_>>();
        for edge in transition_edges {
            let faces = self.working.edge_faces(edge)?;
            if faces.len() != 2 || faces[0] == faces[1] {
                continue;
            }
            let first_surface = self.working.face(faces[0])?.surface.clone();
            let second_surface = self.working.face(faces[1])?.surface.clone();
            if first_surface == second_surface {
                self.kef(edge, faces[1])?;
            } else if let (Surface3::Cone(first), Surface3::Cone(second)) =
                (&first_surface, &second_surface)
            {
                // Independently constructed chamfer patches can describe the
                // same cone chart with slightly different rounded coefficients.
                // Compare the chart, not just its cone angle: existing p-curves
                // must remain valid on the surviving support without remapping.
                let tolerance = self.kernel.tolerance;
                let same_vector = |a: Vec3, b: Vec3| {
                    (a - b).magnitude()
                        <= tolerance.relative.max(f64::EPSILON * 64.0)
                            * a.magnitude().max(b.magnitude()).max(1.0)
                };
                if first.apex().distance(second.apex()) <= tolerance.linear
                    && same_vector(first.axis(), second.axis())
                    && same_vector(first.x_radius(), second.x_radius())
                    && same_vector(first.y_radius(), second.y_radius())
                    && self.working.face(faces[0])?.orientation
                        == self.working.face(faces[1])?.orientation
                {
                    // Keep the merge only if the combined boundary is valid on
                    // the survivor, i.e. the charts are interchangeable over
                    // this face's actual extent.
                    let _ = self.atomic(|transaction| {
                        let survivor = transaction.kef(edge, faces[1])?.surviving_face;
                        super::super::validation::validate_face(
                            &transaction.working,
                            transaction.kernel.tolerance,
                            survivor,
                        )
                    });
                }
            }
        }
        Ok(())
    }

    fn contact_vertex_key(
        patch: &BlendPatch,
        endpoint_index: usize,
        parent_index: usize,
        junctions: &BTreeMap<VertexId, BlendJunction>,
    ) -> (VertexId, FaceId, Option<EdgeId>) {
        let vertex = patch.spine.edge.vertices[endpoint_index];
        let face = patch.spine.faces[parent_index];
        match junctions.get(&vertex).map(|junction| &junction.kind) {
            Some(BlendJunctionKind::RimCorner { carrier_face, .. }) if face == *carrier_face => {
                (vertex, face, Some(patch.spine.edge_id))
            }
            Some(BlendJunctionKind::RimCorner { carrier_face, .. }) => {
                (vertex, *carrier_face, None)
            }
            Some(BlendJunctionKind::Smooth { tangent_faces, .. })
            | Some(BlendJunctionKind::Miter {
                wall_faces: tangent_faces,
                ..
            }) if tangent_faces.contains(&face) => (vertex, tangent_faces[0], None),
            Some(BlendJunctionKind::Smooth {
                second_seam: Some((_, pair)),
                ..
            }) if pair.contains(&face) => (vertex, pair[0], None),
            _ => (vertex, face, None),
        }
    }

    fn loop_parameter_area(&self, loop_id: super::super::LoopId) -> KernelResult<f64> {
        let uses = self.working.loop_coedges(loop_id)?;
        let mut area = 0.0;
        for (index, use_id) in uses.iter().enumerate() {
            let use_value = self.working.coedge(*use_id)?;
            area += use_value
                .pcurve
                .signed_area_contribution(use_value.range, self.kernel.tolerance.parameter)?;
            let next = self.working.coedge(uses[(index + 1) % uses.len()])?;
            let end = use_value.pcurve.evaluate(use_value.range.end)?.point;
            let start = next.pcurve.evaluate(next.range.start)?.point;
            area += 0.5 * (end.x * start.y - start.x * end.y);
        }
        Ok(area)
    }
}
