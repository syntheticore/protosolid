//! Closed primitive bodies built from Euler operator sequences. Tori are
//! published by full circular revolutions; the other primitives are fixtures
//! for certifying analytic bodies in tests.

use std::f64::consts::{PI, TAU};

use crate::{Point2, Point3, Vec2, Vec3};

use super::transaction::Transaction;
use super::{
    BodyId, Circle2, Circle3, Curve2, FaceOrientation, KernelResult, Line2, Line3, ParameterRange,
    Plane3, Torus3,
};

impl Transaction<'_> {
    /// Construct a regular torus using the genus-changing Euler operators.
    ///
    /// KEMR/KFMRH first turn a pair of coincident disks into two annular faces;
    /// MEKR then cuts each periodic half at a meridian seam.
    pub(crate) fn torus(
        &mut self,
        center: Point3,
        x_major: Vec3,
        y_major: Vec3,
        minor_radius: f64,
    ) -> KernelResult<BodyId> {
        let torus = Torus3::new(
            center,
            x_major,
            y_major,
            minor_radius,
            self.kernel.tolerance,
        )?;
        let major_radius = torus.major_radius();
        let x_unit = x_major / major_radius;
        let y_unit = y_major / major_radius;
        let normal = torus.normal();
        let outer_radius = major_radius + minor_radius;
        let inner_radius = major_radius - minor_radius;
        let outer_circle = Circle3::new(
            center,
            x_unit * outer_radius,
            y_unit * outer_radius,
            self.kernel.tolerance,
        )?;
        let inner_circle = Circle3::new(
            center,
            x_unit * inner_radius,
            y_unit * inner_radius,
            self.kernel.tolerance,
        )?;
        let plane = Plane3::new(center, x_major, y_major, self.kernel.tolerance)?;
        let outer_pcurve: Curve2 = Circle2::new(Point2::new(0.0, 0.0), outer_radius)?.into();
        let inner_pcurve: Curve2 = Circle2::new(Point2::new(0.0, 0.0), inner_radius)?.into();
        let first_half = ParameterRange::new(0.0, PI)?;
        let second_half_reversed = ParameterRange::new(TAU, PI)?;
        let first_outer = outer_circle.evaluate(0.0)?.point;
        let second_outer = outer_circle.evaluate(PI)?.point;
        let first_inner = inner_circle.evaluate(0.0)?.point;
        let second_inner = inner_circle.evaluate(PI)?.point;

        let seed = self.mvfs(first_outer, plane.into(), FaceOrientation::Forward)?;
        self.names.push_scope("torus")?;
        let outer0_name = self.names.named("vertex", "outer.seam0")?;
        self.rename_vertex(seed.vertex, outer0_name)?;
        let outer_half = self.mev(
            seed.loop_id,
            None,
            seed.vertex,
            second_outer,
            outer_circle.into(),
            first_half,
            outer_pcurve.clone(),
            outer_pcurve.clone(),
            first_half,
        )?;
        self.name_mev(outer_half, "outer.seam1", "outer.arc0")?;
        let lamina = self.mef(
            outer_half.forward,
            outer_half.reverse,
            outer_circle.into(),
            second_half_reversed,
            plane.into(),
            FaceOrientation::Reversed,
            outer_pcurve.clone(),
            outer_pcurve,
            second_half_reversed,
        )?;
        self.name_mef(lamina, "outer.arc1", Some("lower.annulus"))?;

        // Build a circular disk on a temporary bridge, then use KEMR and KFMRH
        // to transfer its boundary into paired annular holes.
        let outer_previous = self.working.coedge(outer_half.forward)?.previous;
        let bridge_pcurve: Curve2 = Line2::new(
            Point2::new(outer_radius, 0.0),
            Vec2::new(inner_radius - outer_radius, 0.0),
        )?
        .into();
        let bridge = self.mev(
            seed.loop_id,
            Some(outer_previous),
            seed.vertex,
            first_inner,
            Line3::through(first_outer, first_inner)?.into(),
            ParameterRange::new(0.0, 1.0)?,
            bridge_pcurve.clone(),
            bridge_pcurve,
            ParameterRange::new(0.0, 1.0)?,
        )?;
        self.name_mev(bridge, "inner.seam0", "temporary.bridge")?;
        let inner_half = self.mev(
            seed.loop_id,
            Some(bridge.forward),
            bridge.vertex,
            second_inner,
            inner_circle.into(),
            first_half,
            inner_pcurve.clone(),
            inner_pcurve.clone(),
            first_half,
        )?;
        self.name_mev(inner_half, "inner.seam1", "inner.arc0")?;
        let disk = self.mef(
            inner_half.reverse,
            inner_half.forward,
            inner_circle.into(),
            ParameterRange::new(PI, TAU)?,
            plane.into(),
            FaceOrientation::Forward,
            inner_pcurve.clone(),
            inner_pcurve,
            ParameterRange::new(PI, TAU)?,
        )?;
        self.name_mef(disk, "inner.arc1", Some("temporary.disk"))?;
        self.kemr(bridge.edge, bridge.forward)?;
        self.kfmrh(lamina.face, disk.face)?;

        for (face, chart) in [
            (seed.face, super::SurfaceChart::SectionPositive),
            (lamina.face, super::SurfaceChart::SectionNegative),
        ] {
            let value = self.working.faces.get_mut(face)?;
            value.surface = torus.into();
            value.orientation = FaceOrientation::Forward;
            let name = value.name;
            self.history.construction_roles.insert(
                name,
                vec![super::CreationRole::ProfileSide {
                    generators: vec![[0, 0, 0]],
                    chart: Some(chart),
                }],
            );
        }
        let horizontal = |v: f64| -> KernelResult<Curve2> {
            Ok(Line2::new(Point2::new(0.0, v), Vec2::new(1.0, 0.0))?.into())
        };
        for (coedges, v) in [
            ([outer_half.forward, lamina.reverse], 0.0),
            ([disk.reverse, inner_half.reverse], PI),
            ([outer_half.reverse, lamina.forward], TAU),
            ([inner_half.forward, disk.forward], PI),
        ] {
            for coedge in coedges {
                self.working.coedges.get_mut(coedge)?.pcurve = horizontal(v)?;
            }
        }

        let tube_circle = Circle3::new(
            center + x_major,
            x_unit * minor_radius,
            normal * minor_radius,
            self.kernel.tolerance,
        )?;
        let upper_seam = self.mekr(
            outer_half.forward,
            disk.reverse,
            tube_circle.into(),
            ParameterRange::new(0.0, PI)?,
            Line2::new(Point2::new(TAU, 0.0), Vec2::new(0.0, 1.0))?.into(),
            Line2::new(Point2::new(0.0, 0.0), Vec2::new(0.0, 1.0))?.into(),
            ParameterRange::new(0.0, PI)?,
        )?;
        let lower_seam = self.mekr(
            lamina.forward,
            inner_half.forward,
            tube_circle.into(),
            ParameterRange::new(TAU, PI)?,
            Line2::new(Point2::new(0.0, 0.0), Vec2::new(0.0, 1.0))?.into(),
            Line2::new(Point2::new(TAU, 0.0), Vec2::new(0.0, 1.0))?.into(),
            ParameterRange::new(TAU, PI)?,
        )?;
        for (seam, key) in [(upper_seam, "meridian.upper"), (lower_seam, "meridian.lower")] {
            let edge_name = self.names.named("edge", key)?;
            let forward_name = self.names.named("coedge.forward", key)?;
            let reverse_name = self.names.named("coedge.reverse", key)?;
            self.rename_edge(seam.edge, edge_name)?;
            self.rename_coedge(seam.forward, forward_name)?;
            self.rename_coedge(seam.reverse, reverse_name)?;
        }
        for (face, loop_id, key) in [
            (seed.face, upper_seam.loop_id, "upper"),
            (lamina.face, lower_seam.loop_id, "lower"),
        ] {
            let face_name = self.names.named("face", key)?;
            let loop_name = self.names.named("loop.outer", key)?;
            self.rename_face(face, face_name)?;
            self.rename_loop(loop_id, loop_name)?;
        }
        self.names.pop_scope()?;
        self.seal_shell(seed.shell)?;
        Ok(seed.body)
    }
}

#[cfg(test)]
mod fixtures {
    use std::collections::{BTreeMap, BTreeSet};
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    use cgmath::prelude::*;

    use crate::{Point2, Point3, Vec2, Vec3};

    use super::super::polygon::ordered;
    use super::super::polyhedron::PolyhedralTriangle;
    use super::super::transaction::Transaction;
    use super::super::{
        invalid, topology_error, unsupported, BodyId, Circle2, Circle3, Cone3, Curve2, Curve3,
        Cylinder3, FaceId, FaceOrientation, KernelResult, Line2, Line3, LoopId, ParameterRange,
        Plane3, PolyhedralMesh, Sphere3, VertexId,
    };

    impl Transaction<'_> {
        /// Build a closed triangular polyhedron through a shelling sequence of
        /// MEV and MEF operations.
        pub(crate) fn polyhedron(&mut self, mesh: &PolyhedralMesh) -> KernelResult<BodyId> {
            let vertices = mesh.vertices();
            let triangles = mesh.triangles();
            let seed_triangle = &triangles[0];
            let [a, b, c] = seed_triangle.vertices;
            let seed_plane = Plane3::new(
                vertices[a].point,
                vertices[b].point - vertices[a].point,
                vertices[c].point - vertices[a].point,
                self.kernel.tolerance,
            )?;
            let range = ParameterRange::new(0.0, 1.0)?;
            let placeholder: Curve2 =
                Line2::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))?.into();
            let seed = self.mvfs(vertices[a].point, seed_plane.into(), FaceOrientation::Forward)?;
            self.names.push_scope("polyhedron")?;
            let seed_vertex_name = self.names.named("vertex", &vertices[a].key)?;
            self.rename_vertex(seed.vertex, seed_vertex_name)?;

            let first_edge = self.mev(
                seed.loop_id,
                None,
                seed.vertex,
                vertices[b].point,
                Line3::through(vertices[a].point, vertices[b].point)?.into(),
                range,
                placeholder.clone(),
                placeholder.clone(),
                range,
            )?;
            self.name_mev(first_edge, &vertices[b].key, &polyhedral_edge_key(mesh, a, b))?;
            let second_edge = self.mev(
                seed.loop_id,
                Some(first_edge.forward),
                first_edge.vertex,
                vertices[c].point,
                Line3::through(vertices[b].point, vertices[c].point)?.into(),
                range,
                placeholder.clone(),
                placeholder.clone(),
                range,
            )?;
            self.name_mev(second_edge, &vertices[c].key, &polyhedral_edge_key(mesh, b, c))?;
            let lamina = self.mef(
                first_edge.forward,
                second_edge.reverse,
                Line3::through(vertices[a].point, vertices[c].point)?.into(),
                range,
                seed_plane.into(),
                FaceOrientation::Reversed,
                placeholder.clone(),
                placeholder.clone(),
                range,
            )?;
            self.name_mef(lamina, &polyhedral_edge_key(mesh, a, c), Some("construction"))?;
            self.assign_triangle_face(seed.face, seed_triangle, mesh)?;
            let seed_face_name = self.names.named("face", &seed_triangle.key)?;
            let seed_loop_name = self.names.named("loop.outer", &seed_triangle.key)?;
            self.rename_face(seed.face, seed_face_name)?;
            self.rename_loop(seed.loop_id, seed_loop_name)?;

            let mut topology_vertices = BTreeMap::new();
            topology_vertices.insert(a, seed.vertex);
            topology_vertices.insert(b, first_edge.vertex);
            topology_vertices.insert(c, second_edge.vertex);
            let mut input_vertices = BTreeMap::new();
            input_vertices.insert(seed.vertex, a);
            input_vertices.insert(first_edge.vertex, b);
            input_vertices.insert(second_edge.vertex, c);
            let input = |input_vertices: &BTreeMap<VertexId, usize>, vertex: VertexId| {
                input_vertices
                    .get(&vertex)
                    .copied()
                    .ok_or_else(|| topology_error("polyhedron frontier lost an input vertex"))
            };
            let mut built = BTreeSet::from([0usize]);
            let work_face = lamina.face;
            let work_loop = lamina.loop_id;

            while built.len() < triangles.len() {
                let boundary = self.working.loop_coedges(work_loop)?;
                let mut frontier = BTreeMap::new();
                for coedge_id in &boundary {
                    let endpoints = self.working.coedge_vertices(*coedge_id)?;
                    frontier.insert(
                        ordered(
                            input(&input_vertices, endpoints[0])?,
                            input(&input_vertices, endpoints[1])?,
                        ),
                        *coedge_id,
                    );
                }

                if boundary.len() == 3 {
                    let mut boundary_vertices = BTreeSet::new();
                    for coedge in &boundary {
                        for vertex in self.working.coedge_vertices(*coedge)? {
                            boundary_vertices.insert(input(&input_vertices, vertex)?);
                        }
                    }
                    if let Some((triangle_index, triangle)) =
                        triangles.iter().enumerate().find(|(index, triangle)| {
                            !built.contains(index)
                                && BTreeSet::from(triangle.vertices) == boundary_vertices
                        })
                    {
                        self.assign_triangle_face(work_face, triangle, mesh)?;
                        let face_name = self.names.named("face", &triangle.key)?;
                        let loop_name = self.names.named("loop.outer", &triangle.key)?;
                        self.rename_face(work_face, face_name)?;
                        self.rename_loop(work_loop, loop_name)?;
                        built.insert(triangle_index);
                        break;
                    }
                }

                let mut progress = false;
                for (triangle_index, triangle) in triangles.iter().enumerate() {
                    if built.contains(&triangle_index) {
                        continue;
                    }
                    let [ta, tb, tc] = triangle.vertices;
                    let triangle_edges = [ordered(ta, tb), ordered(tb, tc), ordered(tc, ta)];
                    let on_frontier = triangle_edges
                        .iter()
                        .filter(|edge| frontier.contains_key(edge))
                        .copied()
                        .collect::<Vec<_>>();
                    if let [edge] = on_frontier.as_slice() {
                        let third = triangle
                            .vertices
                            .iter()
                            .copied()
                            .find(|vertex| *vertex != edge.0 && *vertex != edge.1)
                            .ok_or_else(|| {
                                topology_error("polyhedron triangle has no third vertex")
                            })?;
                        if topology_vertices.contains_key(&third) {
                            continue;
                        }
                        let boundary_use = frontier[edge];
                        let [start, end] = self.working.coedge_vertices(boundary_use)?;
                        let end_input = input_vertices[&end];
                        let start_input = input_vertices[&start];
                        let spike = self.mev(
                            work_loop,
                            Some(boundary_use),
                            end,
                            vertices[third].point,
                            Line3::through(vertices[end_input].point, vertices[third].point)?
                                .into(),
                            range,
                            placeholder.clone(),
                            placeholder.clone(),
                            range,
                        )?;
                        self.name_mev(
                            spike,
                            &vertices[third].key,
                            &polyhedral_edge_key(mesh, end_input, third),
                        )?;
                        topology_vertices.insert(third, spike.vertex);
                        input_vertices.insert(spike.vertex, third);
                        let made = self.mef(
                            spike.reverse,
                            boundary_use,
                            Line3::through(vertices[third].point, vertices[start_input].point)?
                                .into(),
                            range,
                            self.triangle_plane(triangle, mesh)?.into(),
                            FaceOrientation::Forward,
                            placeholder.clone(),
                            placeholder.clone(),
                            range,
                        )?;
                        self.name_mef(
                            made,
                            &polyhedral_edge_key(mesh, third, start_input),
                            Some(&triangle.key),
                        )?;
                        self.assign_triangle_face(made.face, triangle, mesh)?;
                        built.insert(triangle_index);
                        progress = true;
                        break;
                    }
                    if on_frontier.len() == 2 {
                        let triangle_set = BTreeSet::from(triangle.vertices);
                        let mut chain = None;
                        for boundary_use in &boundary {
                            let next = self.working.coedge(*boundary_use)?.next;
                            let endpoints = [
                                self.working.coedge_vertices(*boundary_use)?[0],
                                self.working.coedge_vertices(*boundary_use)?[1],
                                self.working.coedge_vertices(next)?[1],
                            ];
                            let inputs: BTreeSet<_> = endpoints
                                .iter()
                                .filter_map(|vertex| input_vertices.get(vertex).copied())
                                .collect();
                            if inputs == triangle_set {
                                chain = Some((*boundary_use, next));
                                break;
                            }
                        }
                        let Some((chain_start, chain_second)) = chain else {
                            continue;
                        };
                        let after_chain = self.working.coedge(chain_second)?.next;
                        let [new_start, _] = self.working.coedge_vertices(after_chain)?;
                        let [new_end, _] = self.working.coedge_vertices(chain_start)?;
                        let start_input = input_vertices[&new_start];
                        let end_input = input_vertices[&new_end];
                        let made = self.mef(
                            after_chain,
                            chain_start,
                            Line3::through(vertices[start_input].point, vertices[end_input].point)?
                                .into(),
                            range,
                            self.triangle_plane(triangle, mesh)?.into(),
                            FaceOrientation::Forward,
                            placeholder.clone(),
                            placeholder.clone(),
                            range,
                        )?;
                        self.name_mef(
                            made,
                            &polyhedral_edge_key(mesh, start_input, end_input),
                            Some(&triangle.key),
                        )?;
                        self.assign_triangle_face(made.face, triangle, mesh)?;
                        built.insert(triangle_index);
                        progress = true;
                        break;
                    }
                }
                if !progress {
                    return Err(unsupported(
                        "triangular polyhedron has no Euler shelling step compatible with the current frontier",
                    ));
                }
            }
            if built.len() != triangles.len() {
                return Err(topology_error(
                    "polyhedron Euler shelling did not consume every triangle",
                ));
            }
            self.names.pop_scope()?;
            self.seal_shell(seed.shell)?;
            Ok(seed.body)
        }

        fn triangle_plane(
            &self,
            triangle: &PolyhedralTriangle,
            mesh: &PolyhedralMesh,
        ) -> KernelResult<Plane3> {
            let [a, b, c] = triangle.vertices;
            Plane3::new(
                mesh.vertices()[a].point,
                mesh.vertices()[b].point - mesh.vertices()[a].point,
                mesh.vertices()[c].point - mesh.vertices()[a].point,
                self.kernel.tolerance,
            )
        }

        fn assign_triangle_face(
            &mut self,
            face_id: FaceId,
            triangle: &PolyhedralTriangle,
            mesh: &PolyhedralMesh,
        ) -> KernelResult<()> {
            let plane = self.triangle_plane(triangle, mesh)?;
            let loop_ids = self.working.face(face_id)?.loops.clone();
            if loop_ids.len() != 1 {
                return Err(topology_error(
                    "polyhedron triangle face must have exactly one loop",
                ));
            }
            self.working.faces.get_mut(face_id)?.surface = plane.into();
            self.working.faces.get_mut(face_id)?.orientation = FaceOrientation::Forward;
            for coedge_id in self.working.loop_coedges(loop_ids[0])? {
                let edge = self.working.edge(self.working.coedge(coedge_id)?.edge)?;
                let start = plane.coordinates(self.working.vertex(edge.vertices[0])?.point)?;
                let end = plane.coordinates(self.working.vertex(edge.vertices[1])?.point)?;
                self.working.coedges.get_mut(coedge_id)?.pcurve =
                    Line2::new(start, end - start)?.into();
            }
            Ok(())
        }

        /// Construct a closed right circular cylinder from Euler operators.
        pub(crate) fn cylinder(
            &mut self,
            center: Point3,
            x_radius: Vec3,
            y_radius: Vec3,
            axis: Vec3,
        ) -> KernelResult<BodyId> {
            let circle = Circle3::new(center, x_radius, y_radius, self.kernel.tolerance)?;
            let cylinder =
                Cylinder3::new(center, axis, x_radius, y_radius, self.kernel.tolerance)?;
            let plane = Plane3::new(center, x_radius, y_radius, self.kernel.tolerance)?;
            let circle2: Curve2 = Circle2::new(Point2::new(0.0, 0.0), 1.0)?.into();
            let first_half = ParameterRange::new(0.0, PI)?;
            let second_half_reversed = ParameterRange::new(TAU, PI)?;
            let first = circle.evaluate(0.0)?.point;
            let second = circle.evaluate(PI)?.point;
            let seed = self.mvfs(first, plane.into(), FaceOrientation::Forward)?;
            self.names.push_scope("circle_profile")?;
            let seed_vertex_name = self.names.named("vertex", "seam0")?;
            self.rename_vertex(seed.vertex, seed_vertex_name)?;
            let half = self.mev(
                seed.loop_id,
                None,
                seed.vertex,
                second,
                circle.into(),
                first_half,
                circle2.clone(),
                circle2.clone(),
                first_half,
            )?;
            self.name_mev(half, "seam1", "arc0")?;
            let lamina = self.mef(
                half.forward,
                half.reverse,
                circle.into(),
                second_half_reversed,
                plane.into(),
                FaceOrientation::Reversed,
                circle2.clone(),
                circle2,
                second_half_reversed,
            )?;
            self.name_mef(lamina, "arc1", Some("opposite_cap"))?;
            self.names.pop_scope()?;

            let height = axis.dot(plane.normal());
            let (sweep_face, sweep_loop, opposite_face) = if height > 0.0 {
                (seed.face, seed.loop_id, lamina.face)
            } else {
                (lamina.face, lamina.loop_id, seed.face)
            };
            let translated_plane =
                Plane3::new(center + axis, x_radius, y_radius, self.kernel.tolerance)?;
            self.working.faces.get_mut(sweep_face)?.surface = translated_plane.into();
            self.names.push_scope("cylinder_sweep")?;
            self.sweep_circle_loop(
                sweep_loop,
                axis,
                cylinder,
                if height > 0.0 {
                    FaceOrientation::Forward
                } else {
                    FaceOrientation::Reversed
                },
                circle,
            )?;
            self.names.pop_scope()?;

            self.names.push_scope("caps")?;
            for (face, key) in [(opposite_face, "start"), (sweep_face, "end")] {
                let face_name = self.names.named("face", key)?;
                let loop_name = self.names.named("loop.outer", key)?;
                self.rename_face(face, face_name)?;
                let loop_id = self.working.face(face)?.loops[0];
                self.rename_loop(loop_id, loop_name)?;
            }
            self.names.pop_scope()?;
            self.seal_shell(seed.shell)?;
            Ok(seed.body)
        }

        /// Construct a closed sphere as four spherical lunes joined by four
        /// meridian edges. The two poles are ordinary topological vertices;
        /// only the surface parameterization is singular there.
        pub(crate) fn sphere(
            &mut self,
            center: Point3,
            x_radius: Vec3,
            y_radius: Vec3,
            z_radius: Vec3,
        ) -> KernelResult<BodyId> {
            let sphere =
                Sphere3::new(center, x_radius, y_radius, z_radius, self.kernel.tolerance)?;
            let north = center + z_radius;
            let south = center - z_radius;
            let meridian_range = ParameterRange::new(FRAC_PI_2, -FRAC_PI_2)?;
            let tolerance = self.kernel.tolerance;
            let meridian = |u: f64| -> KernelResult<(Curve3, Curve2)> {
                let radial = x_radius * u.cos() + y_radius * u.sin();
                let curve: Curve3 = Circle3::new(center, radial, z_radius, tolerance)?.into();
                let pcurve: Curve2 = Line2::new(Point2::new(u, 0.0), Vec2::new(0.0, 1.0))?.into();
                Ok((curve, pcurve))
            };

            let seed = self.mvfs(north, sphere.into(), FaceOrientation::Forward)?;
            self.names.push_scope("sphere")?;
            let north_name = self.names.named("vertex", "north")?;
            self.rename_vertex(seed.vertex, north_name)?;

            let (curve0, pcurve0) = meridian(0.0)?;
            let (_, pcurve2pi) = meridian(TAU)?;
            let first = self.mev(
                seed.loop_id,
                None,
                seed.vertex,
                south,
                curve0,
                meridian_range,
                pcurve0,
                pcurve2pi,
                meridian_range,
            )?;
            self.name_mev(first, "south", "meridian.0")?;

            let (curve_pi, pcurve_pi) = meridian(PI)?;
            let opposite = self.mef(
                first.forward,
                first.reverse,
                curve_pi,
                meridian_range,
                sphere.into(),
                FaceOrientation::Forward,
                pcurve_pi.clone(),
                pcurve_pi,
                meridian_range,
            )?;
            self.name_mef(opposite, "meridian.2", Some("lune.2"))?;

            let (curve_quarter, pcurve_quarter) = meridian(FRAC_PI_2)?;
            let quarter = self.mef(
                first.forward,
                opposite.reverse,
                curve_quarter,
                meridian_range,
                sphere.into(),
                FaceOrientation::Forward,
                pcurve_quarter.clone(),
                pcurve_quarter,
                meridian_range,
            )?;
            self.name_mef(quarter, "meridian.1", Some("lune.1"))?;

            let (curve_three_quarters, pcurve_three_quarters) = meridian(3.0 * FRAC_PI_2)?;
            let three_quarters = self.mef(
                opposite.forward,
                first.reverse,
                curve_three_quarters,
                meridian_range,
                sphere.into(),
                FaceOrientation::Forward,
                pcurve_three_quarters.clone(),
                pcurve_three_quarters,
                meridian_range,
            )?;
            self.name_mef(three_quarters, "meridian.3", Some("lune.3"))?;
            let first_face_name = self.names.named("face", "lune.0")?;
            let first_loop_name = self.names.named("loop.outer", "lune.0")?;
            self.rename_face(seed.face, first_face_name)?;
            self.rename_loop(seed.loop_id, first_loop_name)?;
            self.names.pop_scope()?;
            self.seal_shell(seed.shell)?;
            Ok(seed.body)
        }

        /// Construct a closed right circular cone from one circular lamina and
        /// one apex spike. A final MEF divides the construction face into two
        /// conical patches while retaining a single topological apex vertex.
        pub(crate) fn cone(
            &mut self,
            apex: Point3,
            axis: Vec3,
            x_radius: Vec3,
            y_radius: Vec3,
        ) -> KernelResult<BodyId> {
            if !axis.x.is_finite() || !axis.y.is_finite() || !axis.z.is_finite() {
                return Err(invalid("cone axis must be finite"));
            }
            let frame_normal = x_radius.cross(y_radius);
            if frame_normal.magnitude2() == 0.0 {
                return Err(invalid("cone radial frame must be nondegenerate"));
            }
            // Canonicalize the analytic parameterization so its forward normal
            // is outward, independent of the handedness supplied by the caller.
            let y_radius = if axis.dot(frame_normal) >= 0.0 {
                y_radius
            } else {
                -y_radius
            };
            let base_center = apex + axis;
            let cone = Cone3::new(apex, axis, x_radius, y_radius, self.kernel.tolerance)?;
            let circle = Circle3::new(base_center, x_radius, y_radius, self.kernel.tolerance)?;
            let plane = Plane3::new(base_center, x_radius, y_radius, self.kernel.tolerance)?;
            let circle2: Curve2 = Circle2::new(Point2::new(0.0, 0.0), 1.0)?.into();
            let first_half = ParameterRange::new(0.0, PI)?;
            let second_half_reversed = ParameterRange::new(TAU, PI)?;
            let unit_range = ParameterRange::new(0.0, 1.0)?;
            let first_point = circle.evaluate(0.0)?.point;
            let second_point = circle.evaluate(PI)?.point;

            let seed = self.mvfs(first_point, plane.into(), FaceOrientation::Forward)?;
            self.names.push_scope("cone")?;
            let first_vertex_name = self.names.named("vertex", "base.seam0")?;
            self.rename_vertex(seed.vertex, first_vertex_name)?;
            let half = self.mev(
                seed.loop_id,
                None,
                seed.vertex,
                second_point,
                circle.into(),
                first_half,
                circle2.clone(),
                circle2.clone(),
                first_half,
            )?;
            self.name_mev(half, "base.seam1", "base.arc0")?;
            let lamina = self.mef(
                half.forward,
                half.reverse,
                circle.into(),
                second_half_reversed,
                plane.into(),
                FaceOrientation::Reversed,
                circle2.clone(),
                circle2,
                second_half_reversed,
            )?;
            self.name_mef(lamina, "base.arc1", Some("construction"))?;

            let work_loop = lamina.loop_id;
            self.working.faces.get_mut(lamina.face)?.surface = cone.into();
            self.working.faces.get_mut(lamina.face)?.orientation = FaceOrientation::Forward;
            let boundary = self.working.loop_coedges(work_loop)?;
            if boundary.len() != 2 {
                return Err(topology_error(
                    "cone lamina must contain two semicircular edges",
                ));
            }
            let first_base_vertex = self.working.coedge_vertices(boundary[0])?[1];
            let generator0_forward: Curve2 =
                Line2::new(Point2::new(0.0, 1.0), Vec2::new(0.0, -1.0))?.into();
            let generator0_reverse: Curve2 =
                Line2::new(Point2::new(TAU, 1.0), Vec2::new(0.0, -1.0))?.into();
            let apex_spike = self.mev(
                work_loop,
                Some(boundary[0]),
                first_base_vertex,
                apex,
                Line3::through(first_point, apex)?.into(),
                unit_range,
                generator0_forward,
                generator0_reverse,
                unit_range,
            )?;
            self.name_mev(apex_spike, "apex", "generator.0")?;

            let side_boundary: Curve2 =
                Line2::new(Point2::new(0.0, 1.0), Vec2::new(1.0, 0.0))?.into();
            self.working.coedges.get_mut(boundary[0])?.pcurve = side_boundary.clone();
            self.working.coedges.get_mut(boundary[1])?.pcurve = side_boundary;
            let generator1_pcurve: Curve2 =
                Line2::new(Point2::new(PI, 1.0), Vec2::new(0.0, -1.0))?.into();
            let split = self.mef(
                boundary[0],
                apex_spike.reverse,
                Line3::through(second_point, apex)?.into(),
                unit_range,
                cone.into(),
                FaceOrientation::Forward,
                generator1_pcurve.clone(),
                generator1_pcurve,
                unit_range,
            )?;
            self.name_mef(split, "generator.1", Some("side.1"))?;

            let cap_name = self.names.named("face", "base")?;
            let cap_loop_name = self.names.named("loop.outer", "base")?;
            let side_name = self.names.named("face", "side.0")?;
            let side_loop_name = self.names.named("loop.outer", "side.0")?;
            self.rename_face(seed.face, cap_name)?;
            self.rename_loop(seed.loop_id, cap_loop_name)?;
            self.rename_face(lamina.face, side_name)?;
            self.rename_loop(lamina.loop_id, side_loop_name)?;
            self.names.pop_scope()?;
            self.seal_shell(seed.shell)?;
            Ok(seed.body)
        }

        fn sweep_circle_loop(
            &mut self,
            sweep_loop: LoopId,
            axis: Vec3,
            cylinder: Cylinder3,
            side_orientation: FaceOrientation,
            circle: Circle3,
        ) -> KernelResult<()> {
            let unit_range = ParameterRange::new(0.0, 1.0)?;
            let boundary = self.working.loop_coedges(sweep_loop)?;
            if boundary.len() != 2 {
                return Err(topology_error(
                    "circular lamina must contain two semicircular edges",
                ));
            }
            let mut vertices = Vec::new();
            let mut angular_ranges = Vec::new();
            let mut boundary_keys = Vec::new();
            for coedge_id in &boundary {
                let coedge = self.working.coedge(*coedge_id)?;
                vertices.push(self.working.coedge_vertices(*coedge_id)?[0]);
                angular_ranges.push(coedge.range);
                boundary_keys.push(format!(
                    "{:016x}",
                    self.working.edge(coedge.edge)?.name.local
                ));
            }
            let placeholder: Curve2 =
                Line2::new(Point2::new(0.0, 0.0), Vec2::new(0.0, 1.0))?.into();
            let mut up = Vec::new();
            let mut down = Vec::new();
            for index in 0..2 {
                let vertex = vertices[index];
                let bottom = self.working.vertex(vertex)?.point;
                let source_key = format!("{:016x}", self.working.vertex(vertex)?.name.local);
                let made = self.mev(
                    sweep_loop,
                    Some(boundary[(index + 1) % 2]),
                    vertex,
                    bottom + axis,
                    Line3::new(bottom, axis)?.into(),
                    unit_range,
                    placeholder.clone(),
                    placeholder.clone(),
                    unit_range,
                )?;
                self.name_mev(
                    made,
                    &format!("top-{source_key}"),
                    &format!("vertical-{source_key}"),
                )?;
                up.push(made.forward);
                down.push(made.reverse);
            }
            let mut anchor = None;
            for index in 0..2 {
                let next = (index + 1) % 2;
                let first = if index == 1 {
                    anchor
                        .ok_or_else(|| topology_error("cylinder sweep lost its first top arc"))?
                } else {
                    down[next]
                };
                let second = down[index];
                let angular = angular_ranges[index];
                let top_range = ParameterRange::new(angular.end, angular.start)?;
                let side_top: Curve2 =
                    Line2::new(Point2::new(0.0, 1.0), Vec2::new(1.0, 0.0))?.into();
                let side = self.mef(
                    first,
                    second,
                    Circle3::new(
                        circle.center() + axis,
                        circle.x_radius(),
                        circle.y_radius(),
                        self.kernel.tolerance,
                    )?
                    .into(),
                    top_range,
                    cylinder.into(),
                    side_orientation,
                    side_top,
                    Circle2::new(Point2::new(0.0, 0.0), 1.0)?.into(),
                    top_range,
                )?;
                self.name_mef(
                    side,
                    &format!("top-{}", boundary_keys[index]),
                    Some(&format!("side-{}", boundary_keys[index])),
                )?;
                self.working.coedges.get_mut(boundary[index])?.pcurve =
                    Line2::new(Point2::new(0.0, 0.0), Vec2::new(1.0, 0.0))?.into();
                self.working.coedges.get_mut(down[index])?.pcurve =
                    Line2::new(Point2::new(angular.start, 0.0), Vec2::new(0.0, 1.0))?.into();
                self.working.coedges.get_mut(up[next])?.pcurve =
                    Line2::new(Point2::new(angular.end, 0.0), Vec2::new(0.0, 1.0))?.into();
                if index == 0 {
                    anchor = Some(side.reverse);
                }
            }
            Ok(())
        }
    }

    fn polyhedral_edge_key(mesh: &PolyhedralMesh, first: usize, second: usize) -> String {
        let first = &mesh.vertices()[first].key;
        let second = &mesh.vertices()[second].key;
        if first < second {
            format!("{first}<->{second}")
        } else {
            format!("{second}<->{first}")
        }
    }
}
