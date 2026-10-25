//! Straight extrusion of a planar profile, built from Euler operators.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

use cgmath::prelude::*;

use crate::{Point2, Vec2, Vec3};

use super::history::ChangeKind;
use super::topology::Edge;
use super::transaction::Transaction;
use super::{
    geometry_error, invalid, topology_error, unsupported, BodyId, Curve2, Curve3, Cylinder3,
    EdgeId, FaceId, FaceOrientation, KernelError, KernelResult, Line2, Line3, LoopId, LoopKind,
    ModelTolerance, NurbsSurface3, ParameterDomain, ParameterRange, Plane3, Profile,
    RigidTransform3, Surface3,
    TrimmedCurve2, VertexId, Wire2,
};

impl Transaction<'_> {
    /// Extrude a profile bounded by exact line, circular-arc, or NURBS curve
    /// uses along `vector`.
    pub(crate) fn extrude_curve_profile(
        &mut self,
        plane: Plane3,
        profile: &Profile,
        vector: Vec3,
    ) -> KernelResult<BodyId> {
        if !vector.x.is_finite() || !vector.y.is_finite() || !vector.z.is_finite() {
            return Err(invalid("extrusion vector must be finite"));
        }
        let height = vector.dot(plane.normal());
        if height.abs() <= self.kernel.tolerance.linear_at_scale(vector.magnitude()) {
            return Err(invalid(
                "extrusion vector must have a nonzero component normal to its profile plane",
            ));
        }
        let outer = prepared_wire_segments(profile.outer())?;
        let seed = self.mvfs(
            plane.point_at(outer[0].start()?),
            plane.into(),
            FaceOrientation::Forward,
        )?;
        self.names.push_scope("profile")?;
        self.names.push_scope("outer")?;
        let seed_name = self.names.named("vertex", "segment0:start")?;
        self.rename(seed.vertex, seed_name)?;
        let mut vertices = vec![seed.vertex];
        let mut forwards = Vec::new();
        let mut reverses = Vec::new();
        for (index, segment) in outer.iter().take(outer.len() - 1).enumerate() {
            let made = self.mev(
                seed.loop_id,
                (index != 0).then(|| forwards[index - 1]),
                vertices[index],
                plane.point_at(segment.end()?),
                plane.lift_trimmed_curve(segment, self.kernel.tolerance)?,
                segment.range(),
                segment.curve().clone(),
                segment.curve().clone(),
                segment.range(),
            )?;
            self.name_mev(
                made,
                &format!("segment{index}:end"),
                &format!("segment{index}"),
            )?;
            vertices.push(made.vertex);
            forwards.push(made.forward);
            reverses.push(made.reverse);
        }
        let closing = outer.last().expect("a prepared wire has at least two segments");
        let lamina = self.mef(
            forwards[0],
            reverses[reverses.len() - 1],
            plane.lift_trimmed_curve(closing, self.kernel.tolerance)?,
            closing.range().reversed(),
            plane.into(),
            FaceOrientation::Reversed,
            closing.curve().clone(),
            closing.curve().clone(),
            closing.range().reversed(),
        )?;
        self.name_mef(
            lamina,
            &format!("segment{}", outer.len() - 1),
            Some("opposite_cap"),
        )?;
        self.names.pop_scope()?;
        self.names.pop_scope()?;

        let (sweep_face, sweep_loop, opposite_face) = if height > 0.0 {
            (seed.face, seed.loop_id, lamina.face)
        } else {
            (lamina.face, lamina.loop_id, seed.face)
        };
        let sweep_orientation = self.working.face(sweep_face)?.orientation;
        for (hole_index, hole) in profile.holes().iter().enumerate() {
            self.names.push_scope("profile")?;
            self.names.push_scope(format!("hole{hole_index}"))?;
            self.add_curve_lamina_hole(
                sweep_face,
                opposite_face,
                sweep_loop,
                plane,
                hole,
                sweep_orientation,
            )?;
            self.names.pop_scope()?;
            self.names.pop_scope()?;
        }
        let loops_to_sweep = self.working.face(sweep_face)?.loops.clone();
        let translated_plane = Plane3::new(
            plane.origin() + vector,
            plane.u(),
            plane.v(),
            self.kernel.tolerance,
        )?;
        self.working.faces.get_mut(sweep_face)?.surface = translated_plane.into();
        for (index, loop_id) in loops_to_sweep.into_iter().enumerate() {
            let (boundary, source_wire) = if index == 0 {
                (super::ProfileBoundary::Outer, profile.outer())
            } else {
                (
                    super::ProfileBoundary::Hole(index - 1),
                    &profile.holes()[index - 1],
                )
            };
            self.names.push_scope(if index == 0 {
                "outer_sweep".to_owned()
            } else {
                format!("hole{}_sweep", index - 1)
            })?;
            let side_faces = self.sweep_curve_loop(loop_id, vector)?;
            for (face, (segment, part)) in side_faces
                .iter()
                .copied()
                .zip(prepared_wire_segment_sources(source_wire))
            {
                let name = self.working.face(face)?.name;
                self.history.created_as(
                    name,
                    super::CreationRole::ExtrusionSide {
                        boundary,
                        segment: profile.generator(index, segment),
                        part,
                    },
                );
            }
            if is_periodic_circle_wire(source_wire, self.kernel.tolerance) {
                self.merge_periodic_cylinder_sides(&side_faces)?;
            }
            self.names.pop_scope()?;
        }
        self.names.push_scope("caps")?;
        for (face, key, role) in [
            (opposite_face, "start", super::CreationRole::ExtrusionStart),
            (sweep_face, "end", super::CreationRole::ExtrusionEnd),
        ] {
            let face_name = self.names.named("face", key)?;
            self.rename(face, face_name)?;
            self.history.created_as(face_name, role);
            let loops = self.working.face(face)?.loops.clone();
            for (index, loop_id) in loops.into_iter().enumerate() {
                let name = self.names.named(
                    if index == 0 {
                        "loop.outer"
                    } else {
                        "loop.inner"
                    },
                    &format!("{key}{index}"),
                )?;
                self.rename(loop_id, name)?;
            }
        }
        self.names.pop_scope()?;
        self.seal_shell(seed.shell)?;
        Ok(seed.body)
    }

    /// A full circular profile is temporarily partitioned into arcs so the
    /// elementary Euler sweep can seed distinct vertices. Remove the internal
    /// joins afterwards, leaving one periodic cylinder face and one remaining
    /// join as its topological seam.
    fn merge_periodic_cylinder_sides(&mut self, side_faces: &[FaceId]) -> KernelResult<()> {
        if side_faces.len() < 2 {
            return Err(topology_error(
                "a periodic circular sweep must initially have multiple side faces",
            ));
        }
        let first = side_faces[0];
        let Surface3::Cylinder(first_surface) = self.working.face(first)?.surface.clone() else {
            return Err(topology_error(
                "a periodic circular sweep did not produce cylindrical sides",
            ));
        };
        let first_axis = first_surface.axis().normalize();
        let mut survivor = first;
        for face in side_faces.iter().copied().skip(1) {
            let Surface3::Cylinder(surface) = self.working.face(face)?.surface.clone() else {
                return Err(topology_error(
                    "a periodic circular sweep did not produce cylindrical sides",
                ));
            };
            if first_axis.cross(surface.axis().normalize()).magnitude()
                > self.kernel.tolerance.angular
                || (surface.origin() - first_surface.origin())
                    .cross(first_axis)
                    .magnitude()
                    > self.kernel.tolerance.linear
                || (first_surface.radius() - surface.radius()).abs() > self.kernel.tolerance.linear
            {
                return Err(geometry_error(
                    "periodic circular sweep sides do not share one cylinder",
                ));
            }
            let survivor_edges = BTreeSet::from_iter(self.working.face_edges(survivor)?);
            let join = self
                .working
                .face_edges(face)?
                .into_iter()
                .find(|edge| {
                    survivor_edges.contains(edge)
                        && self
                            .working
                            .edge(*edge)
                            .is_ok_and(|value| matches!(value.curve(), Curve3::Line(_)))
                })
                .ok_or_else(|| {
                    topology_error("periodic cylinder patches have no shared sweep edge")
                })?;
            survivor = self.kef(join, face)?.surviving_face;
            self.working.faces.get_mut(survivor)?.surface = Surface3::Cylinder(first_surface);
        }
        let mut circle_groups = Vec::<Vec<EdgeId>>::new();
        for edge_id in self.working.face_edges(survivor)? {
            let Curve3::Circle(circle) = self.working.edge(edge_id)?.curve.clone() else {
                continue;
            };
            if let Some(group) = circle_groups.iter_mut().find(|group| {
                matches!(
                    self.working.edge(group[0]).map(Edge::curve),
                    Ok(Curve3::Circle(candidate)) if *candidate == circle
                )
            }) {
                group.push(edge_id);
            } else {
                circle_groups.push(vec![edge_id]);
            }
        }
        for group in circle_groups {
            if group.len() > 1 {
                self.merge_periodic_circle_edge_cycle(&group)?;
            }
        }
        self.reparameterize_cylindrical_face(survivor)
    }

    /// Collapse the temporary arc cycle of a full circle into one periodic
    /// edge. One edge and one vertex are removed per join while both incident
    /// face loops retain their exact p-curves.
    fn merge_periodic_circle_edge_cycle(&mut self, edges: &[EdgeId]) -> KernelResult<EdgeId> {
        let edge_set = edges.iter().copied().collect::<BTreeSet<_>>();
        let reference = self.working.edge(edges[0])?.clone();
        let Curve3::Circle(reference_circle) = reference.curve else {
            return Err(invalid("periodic edge consolidation requires circular arcs"));
        };
        let mut incidence = BTreeMap::<VertexId, Vec<EdgeId>>::new();
        for edge_id in edges {
            let edge = self.working.edge(*edge_id)?;
            if !matches!(&edge.curve, Curve3::Circle(circle) if *circle == reference_circle)
                || edge.coedges.len() != 2
            {
                return Err(topology_error(
                    "periodic edge cycle does not share one exact circle and two face uses",
                ));
            }
            for vertex in edge.vertices {
                incidence.entry(vertex).or_default().push(*edge_id);
            }
        }
        if incidence.values().any(|incident| incident.len() != 2) {
            return Err(topology_error(
                "periodic circular arcs do not form a closed degree-two cycle",
            ));
        }
        let seam = incidence
            .keys()
            .copied()
            .find(|vertex| {
                self.working
                    .vertex_edges(*vertex)
                    .is_ok_and(|incident| incident.iter().any(|edge| !edge_set.contains(edge)))
            })
            .unwrap_or_else(|| *incidence.keys().next().expect("the cycle has vertices"));

        let mut ordered = Vec::<(EdgeId, bool)>::with_capacity(edges.len());
        let mut used = BTreeSet::new();
        let mut vertex = seam;
        let mut edge_id = incidence[&seam][0];
        loop {
            let edge = self.working.edge(edge_id)?;
            let forward = edge.vertices[0] == vertex;
            vertex = edge.vertices[usize::from(forward)];
            ordered.push((edge_id, forward));
            used.insert(edge_id);
            if vertex == seam {
                break;
            }
            edge_id = incidence[&vertex]
                .iter()
                .copied()
                .find(|candidate| !used.contains(candidate))
                .ok_or_else(|| {
                    topology_error("periodic circular arc cycle terminates before its seam")
                })?;
        }
        if ordered.len() != edges.len() {
            return Err(topology_error(
                "periodic circular arc cycle has disconnected components",
            ));
        }

        let survivor = ordered[0].0;
        let first = self.working.edge(survivor)?.clone();
        let start = if ordered[0].1 {
            first.range.start
        } else {
            first.range.end
        };
        let span = ordered.iter().try_fold(0.0, |span, (edge_id, forward)| {
            let range = self.working.edge(*edge_id)?.range;
            Ok::<_, KernelError>(span + if *forward { range.span() } else { -range.span() })
        })?;
        if (span.abs() - TAU).abs() > self.kernel.tolerance.parameter * 32.0 {
            return Err(geometry_error(
                "periodic circular arc cycle does not cover one full turn",
            ));
        }
        let range = ParameterRange::new(start, start + span)?;
        let mut removed_uses = Vec::new();
        for survivor_use in &first.coedges {
            let loop_id = self.working.coedge(*survivor_use)?.loop_id;
            let loop_uses = edges
                .iter()
                .map(|edge_id| {
                    self.working
                        .edge(*edge_id)?
                        .coedges
                        .iter()
                        .copied()
                        .find(|coedge| {
                            self.working
                                .coedge(*coedge)
                                .is_ok_and(|value| value.loop_id == loop_id)
                        })
                        .ok_or_else(|| {
                            topology_error("periodic circular arcs do not share both incident loops")
                        })
                })
                .collect::<KernelResult<Vec<_>>>()?;
            let original_range = self.working.coedge(*survivor_use)?.range;
            for use_id in loop_uses
                .into_iter()
                .filter(|use_id| use_id != survivor_use)
            {
                let use_value = self.working.coedge(use_id)?.clone();
                self.working.coedges.get_mut(use_value.previous)?.next = use_value.next;
                self.working.coedges.get_mut(use_value.next)?.previous = use_value.previous;
                if self.working.loop_(loop_id)?.first == Some(use_id) {
                    self.working.loops.get_mut(loop_id)?.first = Some(use_value.next);
                }
                removed_uses.push((use_id, use_value.name));
            }
            let forward = original_range.span().signum() == span.signum();
            let value = self.working.coedges.get_mut(*survivor_use)?;
            value.forward = forward;
            value.range = if forward { range } else { range.reversed() };
        }

        {
            let value = self.working.edges.get_mut(survivor)?;
            value.vertices = [seam, seam];
            value.curve = reference_circle.into();
            value.range = range;
        }
        for (use_id, name) in removed_uses {
            self.working.coedges.remove(use_id)?;
            self.history.changed(name, ChangeKind::Deleted, []);
        }
        for edge_id in edges.iter().copied().filter(|edge_id| *edge_id != survivor) {
            let name = self.working.edge(edge_id)?.name;
            self.working.edges.remove(edge_id)?;
            self.history.changed(name, ChangeKind::Deleted, []);
        }
        for vertex_id in incidence.keys().copied().filter(|vertex| *vertex != seam) {
            if !self.working.vertex_edges(vertex_id)?.is_empty() {
                return Err(topology_error(
                    "periodic arc split vertex remains incident after edge consolidation",
                ));
            }
            let name = self.working.vertex(vertex_id)?.name;
            self.working.vertices.remove(vertex_id)?;
            self.history.changed(name, ChangeKind::Deleted, []);
        }
        let survivor_name = self.working.edge(survivor)?.name;
        self.history
            .changed(survivor_name, ChangeKind::Modified, [survivor_name]);
        Ok(survivor)
    }

    /// Re-express every boundary of a merged cylindrical side in one
    /// continuous lift of the cylinder's periodic chart.
    fn reparameterize_cylindrical_face(&mut self, face_id: FaceId) -> KernelResult<()> {
        let surface = self.working.face(face_id)?.surface.clone();
        let loops = self.working.face(face_id)?.loops.clone();
        for loop_id in loops {
            let coedges = self.working.loop_coedges(loop_id)?;
            for coedge_id in &coedges {
                let edge = self.working.edge(self.working.coedge(*coedge_id)?.edge)?.clone();
                let pcurve = match &edge.curve {
                    Curve3::Circle(_) => {
                        // The discarded edge may use a different angular
                        // frame even when both describe the same arc.
                        let parameter = edge.range.start;
                        let curve = edge.curve.evaluate(parameter)?;
                        let (u, v) = surface.project(curve.point, self.kernel.tolerance)?.parameters;
                        let du = surface.evaluate(u, v)?.du;
                        let direction = if curve.first_derivative.dot(du) >= 0.0 {
                            1.0
                        } else {
                            -1.0
                        };
                        Line2::new(
                            Point2::new(u - direction * parameter, v),
                            Vec2::new(direction, 0.0),
                        )?
                        .into()
                    }
                    Curve3::Line(_) => {
                        let start = edge.curve.evaluate(edge.range.start)?.point;
                        let end = edge.curve.evaluate(edge.range.end)?.point;
                        let start_uv = surface.project(start, self.kernel.tolerance)?.parameters;
                        let mut end_uv = surface.project(end, self.kernel.tolerance)?.parameters;
                        end_uv.0 = ParameterDomain::ANGLE.nearest(end_uv.0, start_uv.0);
                        let direction = Vec2::new(end_uv.0 - start_uv.0, end_uv.1 - start_uv.1)
                            / edge.range.span();
                        let origin =
                            Point2::new(start_uv.0, start_uv.1) - direction * edge.range.start;
                        Line2::new(origin, direction)?.into()
                    }
                    Curve3::Nurbs(_) => {
                        return Err(unsupported(
                            "a cylindrical face currently requires line or circle boundary edges",
                        ));
                    }
                    Curve3::SurfaceIntersection(curve) => {
                        curve.pcurve_for_surface(&surface).cloned().ok_or_else(|| {
                            geometry_error(
                                "implicit intersection edge does not retain the cylindrical face",
                            )
                        })?
                    }
                    Curve3::SurfaceCurve(curve) => {
                        if curve.surface() != &surface {
                            return Err(geometry_error(
                                "surface curve does not retain the cylindrical face",
                            ));
                        }
                        curve.pcurve().clone()
                    }
                };
                let coedge = self.working.coedges.get_mut(*coedge_id)?;
                coedge.pcurve = pcurve;
                coedge.range = if coedge.forward {
                    edge.range
                } else {
                    edge.range.reversed()
                };
            }

            // Cylinder u is periodic. Choose one continuous lift of the loop
            // into parameter space so area and winding are evaluated on the
            // intended strip instead of cancelling across the 2π seam.
            let mut previous_end = None;
            for coedge_id in coedges {
                let coedge = self.working.coedge(coedge_id)?;
                let start = coedge.pcurve.evaluate(coedge.range.start)?.point;
                let end = coedge.pcurve.evaluate(coedge.range.end)?.point;
                let shift = previous_end.map_or(0.0, |previous: Point2| {
                    ParameterDomain::ANGLE.whole_periods(previous.x - start.x)
                });
                if shift != 0.0 {
                    let Curve2::Line(line) = coedge.pcurve else {
                        return Err(unsupported(
                            "cylindrical boundary reparameterization requires linear p-curves",
                        ));
                    };
                    self.working.coedges.get_mut(coedge_id)?.pcurve =
                        Line2::new(line.origin() + Vec2::new(shift, 0.0), line.direction())?.into();
                }
                previous_end = Some(end + Vec2::new(shift, 0.0));
            }
        }
        Ok(())
    }

    /// Add a hole to both caps of a profile lamina: build the hole as a
    /// temporary disk bridged to the outer loop, then KEMR and KFMRH turn its
    /// boundary into an inner loop on each cap.
    fn add_curve_lamina_hole(
        &mut self,
        sweep_face: FaceId,
        opposite_face: FaceId,
        outer_loop: LoopId,
        plane: Plane3,
        wire: &Wire2,
        orientation: FaceOrientation,
    ) -> KernelResult<()> {
        // `Profile` stores holes clockwise, while the temporary disk must
        // follow the owning face's positive parameter-space orientation.
        let normalized = if orientation == FaceOrientation::Forward {
            wire.reversed()
        } else {
            wire.clone()
        };
        let segments = prepared_wire_segments(&normalized)?;
        let first_outer = self
            .working
            .loop_(outer_loop)?
            .first
            .ok_or_else(|| topology_error("cannot attach a hole to an empty outer loop"))?;
        let after = self.working.coedge(first_outer)?.previous;
        let outer_vertex = self.working.coedge_vertices(first_outer)?[0];
        let outer_point = self.working.vertex(outer_vertex)?.point;
        let outer_coedge = self.working.coedge(first_outer)?;
        let outer_uv = outer_coedge.pcurve.evaluate(outer_coedge.range.start)?.point;
        let first_uv = segments[0].start()?;
        let first_point = plane.point_at(first_uv);
        let unit = ParameterRange::new(0.0, 1.0)?;
        let bridge_pcurve: Curve2 = Line2::new(outer_uv, first_uv - outer_uv)?.into();
        let bridge = self.mev(
            outer_loop,
            Some(after),
            outer_vertex,
            first_point,
            Line3::through(outer_point, first_point)?.into(),
            unit,
            bridge_pcurve.clone(),
            bridge_pcurve,
            unit,
        )?;
        self.name_mev(bridge, "segment0:start", "bridge->segment0")?;
        let mut vertices = vec![bridge.vertex];
        let mut forwards = Vec::new();
        let mut reverses = Vec::new();
        for (index, segment) in segments.iter().take(segments.len() - 1).enumerate() {
            let made = self.mev(
                outer_loop,
                Some(if index == 0 {
                    bridge.forward
                } else {
                    forwards[index - 1]
                }),
                vertices[index],
                plane.point_at(segment.end()?),
                plane.lift_trimmed_curve(segment, self.kernel.tolerance)?,
                segment.range(),
                segment.curve().clone(),
                segment.curve().clone(),
                segment.range(),
            )?;
            self.name_mev(
                made,
                &format!("segment{index}:end"),
                &format!("segment{index}"),
            )?;
            vertices.push(made.vertex);
            forwards.push(made.forward);
            reverses.push(made.reverse);
        }
        let closing = segments.last().expect("a prepared wire has at least two segments");
        let disk = self.mef(
            reverses[reverses.len() - 1],
            forwards[0],
            plane.lift_trimmed_curve(closing, self.kernel.tolerance)?,
            closing.range(),
            plane.into(),
            orientation,
            closing.curve().clone(),
            closing.curve().clone(),
            closing.range(),
        )?;
        self.name_mef(
            disk,
            &format!("segment{}", segments.len() - 1),
            Some("temporary_disk"),
        )?;
        let hole_result = self.kemr(bridge.edge, bridge.forward)?;
        let sweep_loop_name = self.names.named("loop.inner", "sweep_cap")?;
        self.rename(hole_result.inner_loop, sweep_loop_name)?;
        let opposite_result = self.kfmrh(opposite_face, disk.face)?;
        let opposite_loop_name = self.names.named("loop.inner", "opposite_cap")?;
        self.rename(opposite_result.inner_loop, opposite_loop_name)?;
        if self.working.face(sweep_face)?.loops.len() < 2
            || self.working.face(opposite_face)?.loops.len() < 2
        {
            return Err(topology_error(
                "hole construction did not produce paired cap loops",
            ));
        }
        Ok(())
    }

    /// Sweep one cap loop along `vector`: a vertical edge from every boundary
    /// vertex, then one side face per boundary edge. Returns the side faces in
    /// boundary order.
    fn sweep_curve_loop(&mut self, sweep_loop: LoopId, vector: Vec3) -> KernelResult<Vec<FaceId>> {
        let unit = ParameterRange::new(0.0, 1.0)?;
        let boundary = self.working.loop_coedges(sweep_loop)?;
        let loop_kind = self.working.loop_(sweep_loop)?.kind;
        let side_orientation = match loop_kind {
            LoopKind::Outer => FaceOrientation::Forward,
            LoopKind::Inner => FaceOrientation::Reversed,
        };
        if boundary.len() < 2 {
            return Err(topology_error("a swept curve loop needs at least two edges"));
        }
        let mut boundary_vertices = Vec::new();
        let mut boundary_curves = Vec::new();
        let mut boundary_ranges = Vec::new();
        let mut boundary_keys = Vec::new();
        let mut vertex_keys = Vec::new();
        for coedge_id in &boundary {
            let coedge = self.working.coedge(*coedge_id)?;
            let vertex = self.working.coedge_vertices(*coedge_id)?[0];
            let edge = self.working.edge(coedge.edge)?;
            boundary_vertices.push(vertex);
            boundary_curves.push(edge.curve.clone());
            boundary_ranges.push(if coedge.forward {
                edge.range
            } else {
                edge.range.reversed()
            });
            boundary_keys.push(format!("{:016x}", edge.name.local));
            vertex_keys.push(format!("{:016x}", self.working.vertex(vertex)?.name.local));
        }
        let placeholder: Curve2 = Line2::new(Point2::new(0.0, 0.0), Vec2::new(0.0, 1.0))?.into();
        let mut up = Vec::new();
        let mut down = Vec::new();
        for index in 0..boundary.len() {
            let vertex = boundary_vertices[index];
            let bottom = self.working.vertex(vertex)?.point;
            let made = self.mev(
                sweep_loop,
                Some(boundary[(index + boundary.len() - 1) % boundary.len()]),
                vertex,
                bottom + vector,
                Line3::new(bottom, vector)?.into(),
                unit,
                placeholder.clone(),
                placeholder.clone(),
                unit,
            )?;
            self.name_mev(
                made,
                &format!("top-{}", vertex_keys[index]),
                &format!("vertical-{}", vertex_keys[index]),
            )?;
            up.push(made.forward);
            down.push(made.reverse);
        }
        let translation = RigidTransform3::from_translation(vector);
        let mut anchor = None;
        let mut side_faces = Vec::with_capacity(boundary.len());
        for index in 0..boundary.len() {
            let next = (index + 1) % boundary.len();
            let first = if index + 1 == boundary.len() {
                anchor.ok_or_else(|| topology_error("extrusion did not retain its first top edge"))?
            } else {
                down[next]
            };
            let second = down[index];
            let curve = &boundary_curves[index];
            let range = boundary_ranges[index];
            let top_curve =
                super::transform::transform_curve(curve.clone(), translation, self.kernel.tolerance)?;
            let surface = extrusion_surface(curve, vector, self.kernel.tolerance)?;
            let face_orientation = match curve {
                Curve3::Circle(_) if loop_kind == LoopKind::Outer && range.span() < 0.0 => {
                    side_orientation.reversed()
                }
                Curve3::Circle(_) => side_orientation,
                _ if range.span() < 0.0 => FaceOrientation::Reversed,
                _ => FaceOrientation::Forward,
            };
            let side_top: Curve2 = Line2::new(Point2::new(0.0, 1.0), Vec2::new(1.0, 0.0))?.into();
            let cap_pcurve = self.working.coedge(boundary[index])?.pcurve.clone();
            let side = self.mef(
                first,
                second,
                top_curve,
                range.reversed(),
                surface,
                face_orientation,
                side_top,
                cap_pcurve,
                range.reversed(),
            )?;
            self.name_mef(
                side,
                &format!("top-{}", boundary_keys[index]),
                Some(&format!("side-{}", boundary_keys[index])),
            )?;
            side_faces.push(side.face);
            let bottom = self.working.coedges.get_mut(boundary[index])?;
            bottom.pcurve = Line2::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))?.into();
            bottom.range = range;
            self.working.coedges.get_mut(down[index])?.pcurve =
                Line2::new(Point2::new(range.start, 0.0), Vec2::new(0.0, 1.0))?.into();
            self.working.coedges.get_mut(up[next])?.pcurve =
                Line2::new(Point2::new(range.end, 0.0), Vec2::new(0.0, 1.0))?.into();
            if index == 0 {
                anchor = Some(side.reverse);
            }
        }
        Ok(side_faces)
    }
}

/// Closed single-segment wires are split so a sweep can seed distinct
/// vertices: a full circle into quarters, any other closed curve into halves.
fn prepared_part_count(wire: &Wire2) -> usize {
    match wire.segments() {
        [segment]
            if matches!(segment.curve(), Curve2::Circle(_))
                && (segment.range().span().abs() - TAU).abs() <= f64::EPSILON * TAU * 128.0 =>
        {
            4
        }
        [_] => 2,
        _ => 1,
    }
}

fn prepared_wire_segments(wire: &Wire2) -> KernelResult<Vec<TrimmedCurve2>> {
    let [segment] = wire.segments() else {
        return Ok(wire.segments().to_vec());
    };
    let parts = prepared_part_count(wire);
    (0..parts)
        .map(|part| {
            let range = segment.range();
            let start = range.start + range.span() * part as f64 / parts as f64;
            let end = range.start + range.span() * (part + 1) as f64 / parts as f64;
            TrimmedCurve2::new(segment.curve().clone(), ParameterRange::new(start, end)?)
        })
        .collect()
}

/// The source `(segment, part)` of every prepared wire segment.
fn prepared_wire_segment_sources(wire: &Wire2) -> Vec<(usize, usize)> {
    match prepared_part_count(wire) {
        1 => (0..wire.segments().len()).map(|segment| (segment, 0)).collect(),
        parts => (0..parts).map(|part| (0, part)).collect(),
    }
}

fn is_periodic_circle_wire(wire: &Wire2, tolerance: ModelTolerance) -> bool {
    let Some(Curve2::Circle(reference)) = wire.segments().first().map(TrimmedCurve2::curve) else {
        return false;
    };
    let total_turn = wire
        .segments()
        .iter()
        .map(|segment| segment.range().span())
        .sum::<f64>();
    wire.segments().iter().all(|segment| {
        matches!(segment.curve(), Curve2::Circle(circle)
            if circle.center().distance(reference.center()) <= tolerance.linear
                && (circle.radius() - reference.radius()).abs() <= tolerance.linear
                && segment.range().span().signum() == total_turn.signum())
    }) && (total_turn.abs() - TAU).abs() <= tolerance.parameter
}

fn extrusion_surface(
    curve: &Curve3,
    vector: Vec3,
    tolerance: ModelTolerance,
) -> KernelResult<Surface3> {
    Ok(match curve {
        Curve3::Line(line) => Plane3::new(line.origin(), line.direction(), vector, tolerance)?.into(),
        Curve3::Circle(circle) => Cylinder3::new(
            circle.center(),
            vector,
            circle.x_radius(),
            circle.y_radius(),
            tolerance,
        )?
        .into(),
        Curve3::Nurbs(curve) => {
            let bottom = curve.control_points().to_vec();
            let top = bottom.iter().map(|point| *point + vector).collect();
            NurbsSurface3::new(
                (curve.degree(), 1),
                vec![bottom, top],
                vec![curve.weights().to_vec(), curve.weights().to_vec()],
                (curve.knots().to_vec(), vec![0.0, 0.0, 1.0, 1.0]),
            )?
            .into()
        }
        Curve3::SurfaceCurve(curve) => {
            let Surface3::Plane(plane)=curve.surface() else {
                return Err(unsupported("a translational profile must retain its plane"));
            };
            Surface3::Swept(super::SweptSurface3::new(
                *plane,
                TrimmedCurve2::new(curve.pcurve().clone(),curve.range())?,
                Line3::new(plane.origin(),vector)?.into(),
                ParameterRange::new(0.0,1.0)?,tolerance,
            )?)
        }
        Curve3::SurfaceIntersection(_) => {
            return Err(unsupported(
                "an implicit surface-intersection curve has no exact translational sweep representation",
            ));
        }
    })
}
