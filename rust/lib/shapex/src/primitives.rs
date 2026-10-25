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
        self.rename(seed.vertex, outer0_name)?;
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
            self.rename(seam.edge, edge_name)?;
            self.rename(seam.forward, forward_name)?;
            self.rename(seam.reverse, reverse_name)?;
        }
        for (face, loop_id, key) in [
            (seed.face, upper_seam.loop_id, "upper"),
            (lamina.face, lower_seam.loop_id, "lower"),
        ] {
            let face_name = self.names.named("face", key)?;
            let loop_name = self.names.named("loop.outer", key)?;
            self.rename(face, face_name)?;
            self.rename(loop_id, loop_name)?;
        }
        self.names.pop_scope()?;
        self.seal_shell(seed.shell)?;
        Ok(seed.body)
    }
}

#[cfg(test)]
#[path = "tests/primitives_fixtures.rs"]
mod fixtures;
