//! Sweeps along regular planar sketch rails, retaining the exact moving section.
use super::sew::{BrepPatch, BrepPatchLoop, BrepPatchSegment};
use super::*;
use crate::{Point2, Point3, Vec2, Vec3};
use cgmath::prelude::*;

/// Exact planar-rail sweep. The section rotates with the rail tangent about
/// its fixed plane normal, remaining continuous through spline inflections.
#[derive(Debug, Clone, PartialEq)]
pub struct SweptSurface3 {
    profile_plane: Plane3,
    profile: TrimmedCurve2,
    rail_plane: Plane3,
    rail: TrimmedCurve2,
    tolerance: ModelTolerance,
    closed_rail: bool,
    rail_origin: Point3,
    rail_tangent: Vec3,
}
impl SweptSurface3 {
    pub fn new(
        profile_plane: Plane3,
        profile: TrimmedCurve2,
        rail_plane: Plane3,
        rail: TrimmedCurve2,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        let start = rail.curve().evaluate(rail.range().start)?;
        let rail_origin = rail_plane.point_at(start.point);
        let rail_tangent = rail_plane.vector_at(start.first_derivative);
        if rail_tangent.magnitude2() <= tolerance.linear.powi(2) {
            return Err(invalid("sweep rail has a singular tangent"));
        }
        let rail_tangent = rail_tangent.normalize();
        let closed_rail = start.point.distance(rail.end()?) <= tolerance.linear;
        if closed_rail {
            let end = rail_plane
                .vector_at(rail.curve().evaluate(rail.range().end)?.first_derivative);
            if end.magnitude2() <= tolerance.linear.powi(2)
                || rail_tangent.dot(end.normalize()) < 1.0 - tolerance.angular * 16.0
            {
                return Err(invalid(
                    "closed sweep rail must have a continuous tangent at its seam",
                ));
            }
        }
        let value = Self {
            profile_plane,
            profile,
            rail_plane,
            rail,
            tolerance,
            closed_rail,
            rail_origin,
            rail_tangent,
        };
        for i in 0..=32 {
            value.frame(value.rail.range().start + value.rail.range().span() * i as f64 / 32.0)?;
        }
        Ok(value)
    }
    pub fn profile_plane(&self) -> Plane3 {
        self.profile_plane
    }
    pub fn profile(&self) -> &TrimmedCurve2 {
        &self.profile
    }
    pub fn rail_plane(&self) -> Plane3 {
        self.rail_plane
    }
    pub fn rail(&self) -> &TrimmedCurve2 {
        &self.rail
    }
    pub(crate) fn extended(&self, padding: [f64; 2]) -> KernelResult<Self> {
        let mut result = self.clone();
        let domains = self.domains();
        for (axis, curve) in [&mut result.profile, &mut result.rail].into_iter().enumerate() {
            if domains[axis].period.is_none() {
                let bounds = curve.range();
                let range = ParameterRange::new(
                    bounds.start.min(bounds.end) - padding[axis],
                    bounds.start.max(bounds.end) + padding[axis],
                )?;
                *curve = TrimmedCurve2::new(curve.curve().extended(range)?, range)?;
            }
        }
        // Continuation retains the original section frame and rail anchor.
        Ok(result)
    }
    pub(crate) fn domains(&self) -> [ParameterDomain; 2] {
        let mut domains = [&self.profile, &self.rail].map(|curve| ParameterDomain {
            bounds: DomainBounds::Bounded(curve.range()),
            period: curve
                .curve()
                .domain()
                .period
                .filter(|p| curve.range().span().abs() + self.tolerance.parameter >= *p),
        });
        if self.closed_rail {
            domains[1].period = Some(self.rail.range().span().abs());
        }
        domains
    }
    fn rail_parameter(&self, v: f64) -> f64 {
        if self.closed_rail {
            self.domains()[1].normalize(v)
        } else {
            v
        }
    }
    fn frame(&self, v: f64) -> KernelResult<(Point3, Vec3, Vec3, f64, f64, f64)> {
        let v = self.rail_parameter(v);
        let rail = self.rail.curve().evaluate(v)?;
        let tangent = self.rail_plane.vector_at(rail.first_derivative);
        let acceleration = self.rail_plane.vector_at(rail.second_derivative);
        if tangent.magnitude2() <= self.tolerance.linear.powi(2) {
            return Err(invalid("sweep rail has a singular tangent"));
        }
        let normal = self.rail_plane.normal();
        let cosine = self.rail_tangent.dot(tangent.normalize());
        let sine = normal.dot(self.rail_tangent.cross(tangent.normalize()));
        let omega = normal.dot(tangent.cross(acceleration)) / tangent.magnitude2();
        Ok((
            self.rail_plane.point_at(rail.point),
            tangent,
            acceleration,
            cosine,
            sine,
            omega,
        ))
    }
    fn rotate(&self, v: Vec3, cosine: f64, sine: f64) -> Vec3 {
        let normal = self.rail_plane.normal();
        v * cosine + normal.cross(v) * sine + normal * normal.dot(v) * (1.0 - cosine)
    }
    pub(crate) fn section_plane(&self, v: f64) -> KernelResult<Plane3> {
        let (rail, _, _, c, s, _) = self.frame(v)?;
        let start = self.rail_origin;
        Plane3::new(
            rail + self.rotate(self.profile_plane.origin() - start, c, s),
            self.rotate(self.profile_plane.u(), c, s),
            self.rotate(self.profile_plane.v(), c, s),
            self.tolerance,
        )
    }
    pub(crate) fn first_order(&self, u: f64, v: f64) -> KernelResult<(Point3, Vec3, Vec3)> {
        let profile = self.profile.curve().evaluate(u)?;
        let (rail, tangent, _, c, s, omega) = self.frame(v)?;
        let start = self.rail_origin;
        let delta = self.rotate(self.profile_plane.point_at(profile.point) - start, c, s);
        Ok((
            rail + delta,
            self.rotate(self.profile_plane.vector_at(profile.first_derivative), c, s),
            tangent + self.rail_plane.normal().cross(delta) * omega,
        ))
    }
    pub(crate) fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        let profile = self.profile.curve().evaluate(u)?;
        let (rail, tangent, acceleration, c, s, omega) = self.frame(v)?;
        let jerk = self
            .rail_plane
            .vector_at(self.rail.curve().third_derivative(self.rail_parameter(v))?);
        let normal = self.rail_plane.normal();
        let omega_derivative = normal.dot(tangent.cross(jerk)) / tangent.magnitude2()
            - omega * 2.0 * tangent.dot(acceleration) / tangent.magnitude2();
        let start = self.rail_origin;
        let delta = self.rotate(self.profile_plane.point_at(profile.point) - start, c, s);
        let du = self.rotate(self.profile_plane.vector_at(profile.first_derivative), c, s);
        Ok(SurfaceEvaluation {
            point: rail + delta,
            du,
            dv: tangent + normal.cross(delta) * omega,
            duu: self.rotate(self.profile_plane.vector_at(profile.second_derivative), c, s),
            duv: normal.cross(du) * omega,
            dvv: acceleration
                + normal.cross(normal.cross(delta)) * omega.powi(2)
                + normal.cross(delta) * omega_derivative,
        })
    }
    pub(crate) fn project(&self, target: Point3) -> KernelResult<Projection2> {
        let ranges = [self.profile.range(), self.rail.range()];
        let domains = self.domains();
        let mut best = None;
        for i in 0..=8 {
            for j in 0..=32 {
                let u = ranges[0].start + ranges[0].span() * i as f64 / 8.0;
                let v = ranges[1].start + ranges[1].span() * j as f64 / 32.0;
                let p = self.first_order(u, v)?.0;
                let distance = p.distance(target);
                if best
                    .as_ref()
                    .is_none_or(|p: &Projection2| distance < p.distance)
                {
                    best = Some(Projection2 {
                        parameters: (u, v),
                        point: p,
                        distance,
                    });
                }
            }
        }
        let mut best = best.ok_or_else(|| invalid("sweep projection has no seed"))?;
        for _ in 0..48 {
            let (p, du, dv) = self.first_order(best.parameters.0, best.parameters.1)?;
            let residual = p - target;
            let a = du.magnitude2();
            let b = du.dot(dv);
            let c = dv.magnitude2();
            let det = a * c - b * b;
            if det <= f64::EPSILON * a * c {
                break;
            }
            let step = [
                (c * du.dot(residual) - b * dv.dot(residual)) / det,
                (a * dv.dot(residual) - b * du.dot(residual)) / det,
            ];
            let mut factor = 1.0;
            let mut next = best;
            for _ in 0..24 {
                let uv = [best.parameters.0, best.parameters.1];
                let uv = std::array::from_fn::<_, 2, _>(|k| {
                    let parameter = uv[k] - step[k] * factor;
                    if domains[k].period.is_some() {
                        domains[k].normalize(parameter)
                    } else {
                        parameter.clamp(
                            ranges[k].start.min(ranges[k].end),
                            ranges[k].start.max(ranges[k].end),
                        )
                    }
                });
                let p = self.first_order(uv[0], uv[1])?.0;
                let distance = p.distance(target);
                if distance < best.distance {
                    next = Projection2 {
                        parameters: (uv[0], uv[1]),
                        point: p,
                        distance,
                    };
                    break;
                }
                factor *= 0.5;
            }
            let change = (next.parameters.0 - best.parameters.0)
                .abs()
                .max((next.parameters.1 - best.parameters.1).abs());
            best = next;
            if change <= self.tolerance.parameter {
                break;
            }
        }
        Ok(best)
    }
}

impl Profile {
    /// Sweep along one complete planar sketch rail. The profile retains its
    /// initial placement and turns with the rail's continuous tangent frame.
    pub fn sweep(
        &self,
        plane: Plane3,
        rail_plane: Plane3,
        rail: &TrimmedCurve2,
    ) -> KernelResult<Compound> {
        let tolerance = self.tolerance();
        if !self.holes().is_empty() {
            return Err(invalid("sweeps with holes are not supported"));
        }
        let start = rail_plane.point_at(rail.start()?);
        let end = rail_plane.point_at(rail.end()?);
        if matches!(rail.curve(), Curve2::Line(_)) {
            return self.extrude(plane, end - start);
        }
        if let Curve2::Circle(circle) = rail.curve() {
            return self.revolve(
                plane,
                Axis3::new(rail_plane.point_at(circle.center()), rail_plane.normal())?,
                rail.range().span(),
            );
        }
        if !matches!(rail.curve(), Curve2::Nurbs(_)) {
            return Err(invalid("unsupported sweep rail"));
        }
        let frame = SweptSurface3::new(
            plane,
            self.outer().segments()[0].clone(),
            rail_plane,
            rail.clone(),
            tolerance,
        )?;
        let tangent = rail_plane
            .vector_at(rail.curve().evaluate(rail.range().start)?.first_derivative)
            * rail.range().span().signum();
        let alignment = tangent.normalize().dot(plane.normal());
        if alignment.abs() <= tolerance.angular {
            return Err(invalid(
                "sweep rail must leave the profile plane at its start",
            ));
        }
        let forward = alignment > 0.0;
        let closed = start.distance(end) <= tolerance.linear;
        let mut patches = Vec::new();
        let mut roles = Vec::new();
        if !closed {
            patches.push(super::sweeps::cap(self, plane, forward)?);
            patches.push(super::sweeps::cap(
                self,
                frame.section_plane(rail.range().end)?,
                !forward,
            )?);
            roles.extend([super::CreationRole::ProfileStart, super::CreationRole::ProfileEnd]);
        }
        let mut parameters = rail.curve().integration_breakpoints(rail.range());
        parameters.extend([rail.range().start, rail.range().end]);
        if closed {
            parameters.push((rail.range().start + rail.range().end) * 0.5);
        }
        parameters.sort_by(f64::total_cmp);
        parameters.dedup();
        if rail.range().span() < 0.0 {
            parameters.reverse();
        }
        // Keep each source section edge intact. Periodic charts and display
        // subdivision must not introduce artificial CAD edges into the solid.
        for (source, section) in self.outer().segments().iter().enumerate() {
            let surface = Surface3::Swept(SweptSurface3::new(
                plane,
                section.clone(),
                rail_plane,
                rail.clone(),
                tolerance,
            )?);
            for span in parameters.windows(2) {
                let corners = [
                    Point2::new(section.range().start, span[0]),
                    Point2::new(section.range().end, span[0]),
                    Point2::new(section.range().end, span[1]),
                    Point2::new(section.range().start, span[1]),
                ];
                let mut segments = Vec::new();
                for i in 0..4 {
                    let a = corners[i];
                    let b = corners[(i + 1) % 4];
                    let pcurve: Curve2 = Line2::new(a, b - a)?.into();
                    let range = ParameterRange::new(0.0, 1.0)?;
                    let curve = if i == 0 || i == 2 {
                        // Exact section curves share their native parameter with
                        // caps; longitudinal edges retain the swept support.
                        let plane = frame.section_plane(a.y)?;
                        let curve = plane.lift_curve(section.curve(), tolerance)?;
                        let pcurve = Line2::new(Point2::new(0.0, a.y), Vec2::unit_x())?.into();
                        segments.push(BrepPatchSegment {
                            provenance: Default::default(),
                            curve,
                            pcurve,
                            range: ParameterRange::new(a.x, b.x)?,
                        });
                        continue;
                    } else {
                        SurfaceCurve3::new(surface.clone(), pcurve.clone(), range, tolerance)?
                            .into()
                    };
                    segments.push(BrepPatchSegment {
                        provenance: Default::default(),
                        curve,
                        pcurve,
                        range,
                    });
                }
                let orientation = if section.range().span() * (span[1] - span[0]) > 0.0 {
                    FaceOrientation::Forward
                } else {
                    FaceOrientation::Reversed
                };
                let mut patch = BrepPatch {
                    preserved: Vec::new(),
                    sources: Vec::new(),
                    surface: surface.clone(),
                    orientation,
                    loops: vec![BrepPatchLoop {
                        kind: LoopKind::Outer,
                        segments,
                    }],
                };
                if !forward {
                    patch.orientation = patch.orientation.reversed();
                    patch.loops[0].segments.reverse();
                    for segment in &mut patch.loops[0].segments {
                        segment.range = segment.range.reversed();
                        segment.provenance = segment.provenance.reversed();
                    }
                }
                patches.push(patch);
                roles.push(super::CreationRole::ProfileSide { generators: vec![[0, 0, self.generator(0, source)]], chart: None });
            }
        }
        super::sweeps::publish_with_roles(patches, roles, tolerance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn circle() -> Profile {
        let tolerance = ModelTolerance::default();
        Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(Point2::new(0.0, 0.0), 1.0).unwrap()],
                tolerance,
            )
            .unwrap(),
            [],
            tolerance,
        )
        .unwrap()
    }
    fn verify(body: &Compound, expected: f64) {
        assert!(
            (body.volume() - expected).abs() < expected * 1e-4,
            "volume {} != {expected}",
            body.volume()
        );
        assert_eq!(body.solids().len(), 1);
        assert!(!body
            .tessellate(TessellationOptions::new(0.05, 0.15, 24).unwrap())
            .unwrap()
            .triangles
            .is_empty());
    }
    #[test]
    fn sweep_lines_arcs_and_circles() {
        let p = circle();
        let t = p.tolerance();
        let plane = Plane3::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::unit_y(),
            Vec3::unit_z(),
            t,
        )
        .unwrap();
        let line = TrimmedCurve2::line(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)).unwrap();
        verify(
            &p.sweep(plane, Plane3::xy(), &line).unwrap(),
            p.area() * 10.0,
        );
        let plane = Plane3::new(
            Point3::new(10.0, 0.0, 0.0),
            Vec3::unit_x(),
            Vec3::unit_z(),
            t,
        )
        .unwrap();
        for angle in [
            std::f64::consts::PI,
            -std::f64::consts::PI,
            std::f64::consts::TAU,
        ] {
            let rail = TrimmedCurve2::new(
                Circle2::new(Point2::new(0.0, 0.0), 10.0).unwrap(),
                ParameterRange::new(0.0, angle).unwrap(),
            )
            .unwrap();
            verify(
                &p.sweep(plane, Plane3::xy(), &rail).unwrap(),
                p.area() * 10.0 * angle.abs(),
            );
        }
    }
    #[test]
    fn sweep_spline_preserves_section_and_reverses() {
        let p = circle();
        let t = p.tolerance();
        let rail = TrimmedCurve2::new(
            NurbsCurve2::new(
                3,
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 0.0),
                    Point2::new(10.0, 10.0),
                    Point2::new(20.0, 10.0),
                ],
                vec![1.0; 4],
                vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            ParameterRange::new(0.0, 1.0).unwrap(),
        )
        .unwrap();
        let length = (0..4096)
            .map(|i| {
                rail.curve()
                    .evaluate((i as f64 + 0.5) / 4096.0)
                    .unwrap()
                    .first_derivative
                    .magnitude()
                    / 4096.0
            })
            .sum::<f64>();
        for reverse in [false, true] {
            let rail = if reverse {
                rail.reversed()
            } else {
                rail.clone()
            };
            let origin = Plane3::xy().point_at(rail.start().unwrap());
            let plane = Plane3::new(origin, Vec3::unit_y(), Vec3::unit_z(), t).unwrap();
            let result = p.sweep(plane, Plane3::xy(), &rail).unwrap();
            verify(&result, p.area() * length);
            let transform = RigidTransform3::new(
                crate::Matrix3::from_angle_y(cgmath::Deg(35.0)),
                Vec3::new(3.0, 5.0, 7.0),
                t,
            )
            .unwrap();
            verify(&result.transformed(transform).unwrap(), p.area() * length);
        }
    }
    fn rail() -> TrimmedCurve2 {
        TrimmedCurve2::new(
            NurbsCurve2::new(
                3,
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(5.0, 0.0),
                    Point2::new(5.0, 5.0),
                    Point2::new(10.0, 5.0),
                    Point2::new(15.0, 5.0),
                    Point2::new(15.0, 10.0),
                    Point2::new(20.0, 10.0),
                ],
                vec![1.0, 0.9, 1.1, 1.0, 1.1, 0.9, 1.0],
                vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            ParameterRange::new(0.0, 1.0).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn sweep_rational_spline_derivatives_and_boolean() {
        let t = ModelTolerance::default();
        let rail = rail();
        let plane = Plane3::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::unit_y(),
            Vec3::unit_z(),
            t,
        )
        .unwrap();
        let section = TrimmedCurve2::circle(Point2::new(0.0, 0.0), 0.5).unwrap();
        let surface =
            SweptSurface3::new(plane, section.clone(), Plane3::xy(), rail.clone(), t).unwrap();
        for v in [0.1, 0.4, 0.7, 0.9] {
            let value = surface.evaluate(0.4, v).unwrap();
            let step = 1e-5;
            let before = surface.first_order(0.4, v - step).unwrap();
            let after = surface.first_order(0.4, v + step).unwrap();
            assert!(((after.0 - before.0) / (2.0 * step) - value.dv).magnitude() < 1e-4);
            assert!(((after.2 - before.2) / (2.0 * step) - value.dvv).magnitude() < 1e-3);
        }
        let p = Profile::new(Wire2::new([section], t).unwrap(), [], t).unwrap();
        let body = p.sweep(plane, Plane3::xy(), &rail).unwrap();
        let block = Profile::new(
            Wire2::polygon(
                [
                    Point2::new(-2.0, -2.0),
                    Point2::new(2.0, -2.0),
                    Point2::new(2.0, 2.0),
                    Point2::new(-2.0, 2.0),
                ],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap()
        .extrude(
            Plane3::new(
                Point3::new(-2.0, 0.0, 0.0),
                Vec3::unit_y(),
                Vec3::unit_z(),
                t,
            )
            .unwrap(),
            Vec3::new(4.0, 0.0, 0.0),
        )
        .unwrap();
        let joined = body.union(&block).unwrap();
        assert!(
            joined.volume() > block.volume() && joined.volume() < body.volume() + block.volume()
        );
        assert_eq!(joined.solids().len(), 1);
        let cut = body.difference(&block).unwrap();
        let common = body.intersection(&block).unwrap();
        assert_eq!(cut.solids().len(), 1);
        assert_eq!(common.solids().len(), 1);
        assert!(cut.volume() > 0.0 && common.volume() > 0.0);
        // Mass-property quadrature uses the whole solid's spatial scale;
        // this long, thin sweep has a much smaller volume than that scale cubed.
        let volume_tolerance = body.volume() * 1e-3;
        assert!((cut.volume() + common.volume() - body.volume()).abs() < volume_tolerance);
        assert!(
            (joined.volume() + common.volume() - body.volume() - block.volume()).abs()
                < volume_tolerance
        );
    }
    #[test]
    fn sweep_closed_rational_rail() {
        let t = ModelTolerance::default();
        let w = 0.5_f64.sqrt();
        let points = [
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (-10.0, 10.0),
            (-10.0, 0.0),
            (-10.0, -10.0),
            (0.0, -10.0),
            (10.0, -10.0),
            (10.0, 0.0),
        ];
        let rail = TrimmedCurve2::new(
            NurbsCurve2::new(
                2,
                points.map(|(x, y)| Point2::new(x, y)).to_vec(),
                vec![1.0, w, 1.0, w, 1.0, w, 1.0, w, 1.0],
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0],
            )
            .unwrap(),
            ParameterRange::new(0.0, 4.0).unwrap(),
        )
        .unwrap();
        let p = Profile::new(
            Wire2::polygon(
                [
                    Point2::new(-0.5, -0.5),
                    Point2::new(0.5, -0.5),
                    Point2::new(0.5, 0.5),
                    Point2::new(-0.5, 0.5),
                ],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap();
        let plane = Plane3::new(
            Point3::new(10.0, 0.0, 0.0),
            Vec3::unit_x(),
            Vec3::unit_z(),
            t,
        )
        .unwrap();
        let body = p.sweep(plane, Plane3::xy(), &rail).unwrap();
        assert!((body.volume() - std::f64::consts::TAU * 10.0).abs() < 1e-4);
        assert!(!body
            .tessellate(TessellationOptions::new(0.05, 0.15, 24).unwrap())
            .unwrap()
            .triangles
            .is_empty());
    }
    fn document_65_sweep() -> Compound {
        // untitled(65).cad.cad: circle on the application's YZ workplane
        // (local X points along -Z), swept along the XY cubic sketch spline.
        let t = ModelTolerance::default();
        let profile = Profile::new(
            Wire2::new(
                [
                    TrimmedCurve2::circle(Point2::new(0.0, 8.442693762145785), 31.213640464146206)
                        .unwrap(),
                ],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap();
        let rail = TrimmedCurve2::new(
            NurbsCurve2::new(
                3,
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(89.2170942259588, 11.288693555121299),
                    Point2::new(163.50398084675714, -89.94539703596661),
                    Point2::new(274.2060079679468, 31.681172235340455),
                ],
                vec![1.0; 4],
                vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            ParameterRange::new(0.0, 1.0).unwrap(),
        )
        .unwrap();
        let plane = Plane3::new(
            Point3::new(0.0, 0.0, 0.0),
            -Vec3::unit_z(),
            Vec3::unit_y(),
            t,
        )
        .unwrap();
        profile.sweep(plane, Plane3::xy(), &rail).unwrap()
    }
    #[test]
    fn swept_circle_end_fillets_from_document_65() {
        let body = document_65_sweep();
        assert_eq!(body.faces().count(), 3);
        let rims = body
            .edges()
            .filter(|edge| matches!(edge.curve(), Curve3::Circle(_)))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(rims.len(), 2);
        for edge in &rims {
            assert!(edge.endpoints().unwrap()[0].distance(edge.endpoints().unwrap()[1]) < 1e-7);
        }
        for radius in [1.0, 5.0] {
            for selection in [&rims[..1], &rims[1..], &rims[..]] {
                let result = body.fillet(selection, radius).unwrap_or_else(|error| {
                    panic!("{} rims, radius {radius}: {error}", selection.len())
                });
                assert!(result.volume() < body.volume() && result.volume() > body.volume() * 0.99);
                assert!(!result
                    .tessellate(TessellationOptions::new(0.1, 0.15, 24).unwrap())
                    .unwrap()
                    .triangles
                    .is_empty());
            }
        }
    }
    #[test]
    fn swept_circle_fillet_cut_from_document_65() {
        let start = std::time::Instant::now();
        let body = document_65_sweep();
        eprintln!("sweep {:?}", start.elapsed());
        let rim = body
            .edges()
            .find(|edge| matches!(edge.curve(), Curve3::Circle(c) if c.center().x > 200.0))
            .unwrap()
            .clone();
        let start = std::time::Instant::now();
        let rounded = body.fillet(&[rim], 3.0).unwrap();
        eprintln!("fillet {:?}", start.elapsed());
        let t = body.tolerance();
        let plane = body
            .faces()
            .find_map(|face| match face.surface() {
                Surface3::Plane(plane) if plane.origin().x > 200.0 => Some(*plane),
                _ => None,
            })
            .unwrap();
        let tool = Profile::new(
            Wire2::new(
                [TrimmedCurve2::circle(
                    Point2::new(-0.3730080029745935, 7.445467850896364),
                    15.095354657612303,
                )
                .unwrap()],
                t,
            )
            .unwrap(),
            [],
            t,
        )
        .unwrap()
        .extrude(plane, -plane.normal() * 3.0)
        .unwrap();
        for (label, source) in [("plain", &body), ("rounded", &rounded)] {
            let start = std::time::Instant::now();
            let result = source.difference(&tool).unwrap();
            eprintln!("{label} cut {:?}", start.elapsed());
            assert_eq!(result.solids().len(), 1);
            assert!(
                (source.volume() - result.volume() - tool.volume()).abs() < tool.volume() * 1e-3
            );
        }
    }
}
