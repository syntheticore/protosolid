//! Sweeps along regular planar or spatial rails, retaining the exact moving section.
use super::sew::{BrepPatch, BrepPatchLoop, BrepPatchSegment};
use super::*;
use crate::{Matrix3, Point2, Point3, Quaternion, Vec2, Vec3};
use cgmath::prelude::*;
use std::sync::Arc;

/// Largest rotation error accepted for one integration step of a spatial
/// rail frame, well below every model tolerance.
const FRAME_STEP_ERROR: f64 = 1.0e-13;

/// How the section turns along the rail. The planar and integrated variants
/// follow the rail's rotation-minimizing frame, which never twists the section
/// about the rail.
#[derive(Debug, Clone, PartialEq)]
enum Orientation {
    /// A planar rail turns the section about the plane's fixed normal by the
    /// angle its tangent has turned, in closed form.
    Planar(Vec3),
    /// A spatial rail integrates the frame. Checkpoints hold verified rotations
    /// in travel order from the rail start. `twist` adds a constant turn about
    /// the tangent per unit length, closing the frame on closed rails.
    Integrated {
        checkpoints: Arc<[Checkpoint]>,
        twist: f64,
    },
    /// A straight rail screws the section about itself, turning by `rate`
    /// radians per unit rail parameter from `start`. This is the exact
    /// helicoidal sweep.
    Helical { axis: Vec3, rate: f64, start: f64 },
}

/// A verified rotation of a spatial rail frame, with its rate of change.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Checkpoint {
    parameter: f64,
    rotation: Quaternion,
    // One-sided rates before and after this parameter, in travel order.
    incoming_rate: Quaternion,
    rate: Quaternion,
}
impl Checkpoint {
    fn new(parameter: f64, rotation: Quaternion, omega: Vec3) -> Self {
        Self {
            parameter,
            rotation,
            incoming_rate: spin(rotation, omega),
            rate: spin(rotation, omega),
        }
    }
}

/// Rate of change of a rotation turning with angular velocity `omega`.
fn spin(rotation: Quaternion, omega: Vec3) -> Quaternion {
    Quaternion::from_sv(0.0, omega) * rotation * 0.5
}

/// One classical Runge-Kutta step of a rotation, from the angular velocities
/// at the start, middle and end of the step.
fn runge_kutta(rotation: Quaternion, omega: [Vec3; 3], step: f64) -> Quaternion {
    let k1 = spin(rotation, omega[0]);
    let k2 = spin(rotation + k1 * (step * 0.5), omega[1]);
    let k3 = spin(rotation + k2 * (step * 0.5), omega[1]);
    let k4 = spin(rotation + k3 * step, omega[2]);
    (rotation + (k1 + k2 * 2.0 + k3 * 2.0 + k4) * (step / 6.0)).normalize()
}

/// Cubic Hermite interpolation between neighbouring checkpoints. It matches
/// the integration's order, so evaluation needs no further rail samples.
fn hermite(start: Checkpoint, end: Checkpoint, parameter: f64) -> Quaternion {
    let span = end.parameter - start.parameter;
    let s = (parameter - start.parameter) / span;
    let (s2, s3) = (s * s, s * s * s);
    (start.rotation * (2.0 * s3 - 3.0 * s2 + 1.0)
        + start.rate * (span * (s3 - 2.0 * s2 + s))
        + end.rotation * (3.0 * s2 - 2.0 * s3)
        + end.incoming_rate * (span * (s3 - s2)))
        .normalize()
}

/// The rail and the section rotation at one rail parameter.
struct RailFrame {
    point: Point3,
    tangent: Vec3,
    acceleration: Vec3,
    rotation: Matrix3,
    /// Angular velocity of the section with respect to the rail parameter
    omega: Vec3,
}

/// Exact sweep of a section along a rail. The section turns with the rail's
/// rotation-minimizing frame, remaining continuous through inflections.
#[derive(Debug, Clone, PartialEq)]
pub struct SweptSurface3 {
    profile_plane: Plane3,
    profile: TrimmedCurve2,
    rail: Box<Curve3>,
    rail_range: ParameterRange,
    tolerance: ModelTolerance,
    closed_rail: bool,
    rail_origin: Point3,
    rail_tangent: Vec3,
    orientation: Orientation,
}
impl SweptSurface3 {
    pub fn new(
        profile_plane: Plane3,
        profile: TrimmedCurve2,
        rail: Curve3,
        rail_range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        let start = rail.evaluate(rail_range.start)?;
        if start.first_derivative.magnitude2() <= tolerance.linear.powi(2) {
            return Err(invalid("sweep rail has a singular tangent"));
        }
        let rail_tangent = start.first_derivative.normalize();
        let end = rail.evaluate(rail_range.end)?;
        let closed_rail = start.point.distance(end.point) <= tolerance.linear;
        if closed_rail
            && (end.first_derivative.magnitude2() <= tolerance.linear.powi(2)
                || rail_tangent.dot(end.first_derivative.normalize())
                    < 1.0 - tolerance.angular * 16.0)
        {
            return Err(invalid(
                "closed sweep rail must have a continuous tangent at its seam",
            ));
        }
        let orientation = match planar_normal(&rail, rail_range, tolerance)? {
            Some(normal) => Orientation::Planar(normal),
            None => Orientation::Integrated {
                checkpoints: Arc::from([]),
                twist: 0.0,
            },
        };
        let mut value = Self {
            profile_plane,
            profile,
            rail: Box::new(rail),
            rail_range,
            tolerance,
            closed_rail,
            rail_origin: start.point,
            rail_tangent,
            orientation,
        };
        if let Orientation::Integrated { .. } = value.orientation {
            value.orientation = value.integrate()?;
        }
        for i in 0..=32 {
            value.frame(rail_range.start + rail_range.span() * i as f64 / 32.0)?;
        }
        Ok(value)
    }
    pub fn profile_plane(&self) -> Plane3 {
        self.profile_plane
    }
    pub fn profile(&self) -> &TrimmedCurve2 {
        &self.profile
    }
    pub fn rail(&self) -> &Curve3 {
        &self.rail
    }
    pub fn rail_range(&self) -> ParameterRange {
        self.rail_range
    }
    /// Helicoidal sweep of a section screwing about a straight rail, turning
    /// by `rate` radians per unit rail parameter from the rail start.
    pub fn helical(
        profile_plane: Plane3,
        profile: TrimmedCurve2,
        rail: Line3,
        rail_range: ParameterRange,
        rate: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !rate.is_finite() || rate == 0.0 {
            return Err(invalid("helical sweep rate must be finite and nonzero"));
        }
        let axis = rail.direction().normalize();
        Ok(Self {
            profile_plane,
            profile,
            rail_origin: rail.evaluate(rail_range.start)?.point,
            rail: Box::new(rail.into()),
            rail_range,
            tolerance,
            closed_rail: false,
            rail_tangent: axis,
            orientation: Orientation::Helical { axis, rate, start: rail_range.start },
        })
    }
    /// The turn per unit rail parameter of a helicoidal sweep.
    pub fn helix_rate(&self) -> Option<f64> {
        match self.orientation {
            Orientation::Helical { rate, .. } => Some(rate),
            _ => None,
        }
    }
    pub(crate) fn extended(&self, padding: [f64; 2]) -> KernelResult<Self> {
        let mut result = self.clone();
        let domains = self.domains();
        let padded = |range: ParameterRange, padding: f64| {
            ParameterRange::new(
                range.start.min(range.end) - padding,
                range.start.max(range.end) + padding,
            )
        };
        if domains[0].period.is_none() {
            let range = padded(self.profile.range(), padding[0])?;
            result.profile = TrimmedCurve2::new(self.profile.curve().extended(range)?, range)?;
        }
        if domains[1].period.is_none() {
            let range = padded(self.rail_range, padding[1])?;
            let normal = match self.orientation {
                Orientation::Planar(normal) => Some(normal),
                // A straight rail extends itself; the screw continues from its start.
                Orientation::Helical { .. } => None,
                Orientation::Integrated { .. } => {
                    return Err(unsupported("extending a sweep along a spatial rail"))
                }
            };
            if let (
                Some(normal),
                Curve3::Nurbs(_) | Curve3::SurfaceCurve(_) | Curve3::SurfaceIntersection(_),
            ) = (normal, &*self.rail)
            {
                let frame = Frame3::from_z(self.rail_origin, normal)?;
                let plane = Plane3::new(frame.origin(), frame.x(), frame.y(), self.tolerance)?;
                let flat = plane
                    .flatten_curve(&self.rail, self.tolerance)?
                    .ok_or_else(|| unsupported("extending a sweep along an implicit rail"))?;
                result.rail = Box::new(plane.lift_curve(&flat.extended(range)?, self.tolerance)?);
            }
            result.rail_range = range;
        }
        // Continuation retains the original section frame and rail anchor.
        Ok(result)
    }
    pub(crate) fn domains(&self) -> [ParameterDomain; 2] {
        let profile = ParameterDomain {
            bounds: DomainBounds::Bounded(self.profile.range()),
            period: self
                .profile
                .curve()
                .domain()
                .period
                .filter(|p| self.profile.range().span().abs() + self.tolerance.parameter >= *p),
        };
        let mut rail = ParameterDomain {
            bounds: DomainBounds::Bounded(self.rail_range),
            period: self
                .rail
                .domain()
                .period
                .filter(|p| self.rail_range.span().abs() + self.tolerance.parameter >= *p),
        };
        if self.closed_rail {
            rail.period = Some(self.rail_range.span().abs());
        }
        [profile, rail]
    }
    fn rail_parameter(&self, v: f64) -> f64 {
        if self.closed_rail {
            self.domains()[1].normalize(v)
        } else {
            v
        }
    }
    fn angular_velocity(&self, tangent: Vec3, acceleration: Vec3) -> Vec3 {
        let omega = tangent.cross(acceleration) / tangent.magnitude2();
        match &self.orientation {
            Orientation::Planar(normal) => normal * normal.dot(omega),
            Orientation::Integrated { twist, .. } => omega + tangent * *twist,
            Orientation::Helical { axis, rate, .. } => axis * *rate,
        }
    }
    fn angular_acceleration(&self, tangent: Vec3, acceleration: Vec3, jerk: Vec3) -> Vec3 {
        let speed = tangent.magnitude2();
        let omega = tangent.cross(acceleration) / speed;
        let change = tangent.cross(jerk) / speed - omega * (2.0 * tangent.dot(acceleration) / speed);
        match &self.orientation {
            Orientation::Planar(normal) => normal * normal.dot(change),
            Orientation::Integrated { twist, .. } => change + acceleration * *twist,
            Orientation::Helical { .. } => Vec3::new(0.0, 0.0, 0.0),
        }
    }
    /// The frame's angular velocity and the rail speed at a rail parameter.
    fn transport_rate(&self, twist: f64, v: f64) -> KernelResult<(Vec3, f64)> {
        let rail = self.rail.evaluate(v)?;
        let tangent = rail.first_derivative;
        let omega = tangent.cross(rail.second_derivative) / tangent.magnitude2() + tangent * twist;
        Ok((omega, tangent.magnitude()))
    }
    /// Integrate the rotation-minimizing frame over the rail with step
    /// doubling, returning verified checkpoints and the signed rail length.
    fn transport(&self, twist: f64) -> KernelResult<(Vec<Checkpoint>, f64)> {
        let range = self.rail_range;
        let direction = range.span().signum();
        let mut ends = self.rail.integration_breakpoints(range);
        if direction < 0.0 {
            ends.reverse();
        }
        ends.push(range.end);
        let mut v = range.start;
        let mut rotation = Quaternion::one();
        let mut checkpoints = vec![Checkpoint::new(v, rotation, self.transport_rate(twist, v)?.0)];
        let mut length = 0.0;
        let mut step = range.span() / 16.0;
        for end in ends {
            // Each span uses its own one-sided rates at knot boundaries.
            let low = v.min(end).next_up();
            let high = v.max(end).next_down();
            if low > high {
                // Too narrow to hold a sample; the next span covers it
                continue;
            }
            let rate = |parameter: f64| self.transport_rate(twist, parameter.clamp(low, high));
            let (mut omega, mut speed) = rate(v)?;
            checkpoints.last_mut().unwrap().rate = spin(rotation, omega);
            while (end - v) * direction > 0.0 {
                let mut h = if (end - v).abs() < step.abs() { end - v } else { step };
                loop {
                    let [quarter, middle, three_quarters, last] =
                        [0.25, 0.5, 0.75, 1.0].map(|fraction| rate(v + h * fraction));
                    let (quarter, middle, three_quarters, last) =
                        (quarter?, middle?, three_quarters?, last?);
                    let full = runge_kutta(rotation, [omega, middle.0, last.0], h);
                    let half = runge_kutta(rotation, [omega, quarter.0, middle.0], h * 0.5);
                    let double = runge_kutta(half, [middle.0, three_quarters.0, last.0], h * 0.5);
                    // Local error is fifth order in the step; aim just below the bound
                    let error = (full - double).magnitude();
                    let scale = (0.9 * (FRAME_STEP_ERROR / error.max(f64::MIN_POSITIVE)).powf(0.2))
                        .clamp(0.2, 2.0);
                    if error <= FRAME_STEP_ERROR {
                        v = if h == end - v { end } else { v + h };
                        rotation = double;
                        omega = last.0;
                        length += (speed + quarter.1 * 4.0 + middle.1 * 2.0 + three_quarters.1 * 4.0 + last.1)
                            * h
                            / 12.0;
                        speed = last.1;
                        checkpoints.push(Checkpoint::new(v, rotation, omega));
                        step = h * scale;
                        break;
                    }
                    h *= scale;
                    if h.abs() <= self.tolerance.parameter {
                        return Err(invalid("sweep rail frame could not be integrated"));
                    }
                }
            }
        }
        Ok((checkpoints, length))
    }
    fn integrate(&self) -> KernelResult<Orientation> {
        let (checkpoints, length) = self.transport(0.0)?;
        if !self.closed_rail {
            return Ok(Orientation::Integrated {
                checkpoints: checkpoints.into(),
                twist: 0.0,
            });
        }
        // Around a closed spatial rail the transported section returns turned
        // about the seam tangent. Spread the opposite turn along the rail.
        let end = checkpoints[checkpoints.len() - 1].rotation;
        let end = if end.s < 0.0 { -end } else { end };
        let twist = -2.0 * end.v.dot(self.rail_tangent).atan2(end.s) / length;
        let (checkpoints, _) = self.transport(twist)?;
        let end = checkpoints[checkpoints.len() - 1].rotation;
        if 2.0 * end.v.magnitude().atan2(end.s.abs()) > self.tolerance.angular * 16.0 {
            return Err(invalid("closed sweep rail frame does not close at its seam"));
        }
        Ok(Orientation::Integrated {
            checkpoints: checkpoints.into(),
            twist,
        })
    }
    fn frame(&self, v: f64) -> KernelResult<RailFrame> {
        let v = self.rail_parameter(v);
        let rail = self.rail.evaluate(v)?;
        let tangent = rail.first_derivative;
        if tangent.magnitude2() <= self.tolerance.linear.powi(2) {
            return Err(invalid("sweep rail has a singular tangent"));
        }
        let rotation = match &self.orientation {
            Orientation::Planar(normal) => {
                let direction = tangent.normalize();
                let angle = normal
                    .dot(self.rail_tangent.cross(direction))
                    .atan2(self.rail_tangent.dot(direction));
                Matrix3::from_axis_angle(*normal, cgmath::Rad(angle))
            }
            Orientation::Helical { axis, rate, start } => {
                Matrix3::from_axis_angle(*axis, cgmath::Rad(rate * (v - start)))
            }
            Orientation::Integrated { checkpoints, twist } => {
                let direction = self.rail_range.span().signum();
                let index = checkpoints.partition_point(|checkpoint| (v - checkpoint.parameter) * direction >= 0.0);
                Matrix3::from(if index > 0 && index < checkpoints.len() {
                    hermite(checkpoints[index - 1], checkpoints[index], v)
                } else {
                    // At or beyond a rail end, continue from that end
                    let end = checkpoints[index.min(checkpoints.len() - 1)];
                    if v == end.parameter {
                        end.rotation
                    } else {
                        let step = v - end.parameter;
                        let [start, middle, last] = [0.0, 0.5, 1.0]
                            .map(|fraction| self.transport_rate(*twist, end.parameter + step * fraction));
                        runge_kutta(end.rotation, [start?.0, middle?.0, last?.0], step)
                    }
                })
            }
        };
        Ok(RailFrame {
            point: rail.point,
            tangent,
            acceleration: rail.second_derivative,
            rotation,
            omega: self.angular_velocity(tangent, rail.second_derivative),
        })
    }
    pub(crate) fn section_plane(&self, v: f64) -> KernelResult<Plane3> {
        let frame = self.frame(v)?;
        Plane3::new(
            frame.point + frame.rotation * (self.profile_plane.origin() - self.rail_origin),
            frame.rotation * self.profile_plane.u(),
            frame.rotation * self.profile_plane.v(),
            self.tolerance,
        )
    }
    pub(crate) fn first_order(&self, u: f64, v: f64) -> KernelResult<(Point3, Vec3, Vec3)> {
        let profile = self.profile.curve().evaluate(u)?;
        let frame = self.frame(v)?;
        let delta = frame.rotation * (self.profile_plane.point_at(profile.point) - self.rail_origin);
        Ok((
            frame.point + delta,
            frame.rotation * self.profile_plane.vector_at(profile.first_derivative),
            frame.tangent + frame.omega.cross(delta),
        ))
    }
    pub(crate) fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        let profile = self.profile.curve().evaluate(u)?;
        let frame = self.frame(v)?;
        let jerk = self.rail.third_derivative(self.rail_parameter(v))?;
        let omega = frame.omega;
        let omega_derivative = self.angular_acceleration(frame.tangent, frame.acceleration, jerk);
        let delta = frame.rotation * (self.profile_plane.point_at(profile.point) - self.rail_origin);
        let du = frame.rotation * self.profile_plane.vector_at(profile.first_derivative);
        Ok(SurfaceEvaluation {
            point: frame.point + delta,
            du,
            dv: frame.tangent + omega.cross(delta),
            duu: frame.rotation * self.profile_plane.vector_at(profile.second_derivative),
            duv: omega.cross(du),
            dvv: frame.acceleration
                + omega.cross(omega.cross(delta))
                + omega_derivative.cross(delta),
        })
    }
    pub(crate) fn project(&self, target: Point3) -> KernelResult<Projection2> {
        let ranges = [self.profile.range(), self.rail_range];
        let domains = self.domains();
        // Seed every turn of a helicoidal sweep alike; its turns lie side by side.
        let rail_samples = match self.orientation {
            Orientation::Helical { rate, .. } => {
                32 * ((ranges[1].span() * rate).abs() / std::f64::consts::TAU).ceil().max(1.0) as usize
            }
            _ => 32,
        };
        let mut best = None;
        for i in 0..=8 {
            for j in 0..=rail_samples {
                let u = ranges[0].start + ranges[0].span() * i as f64 / 8.0;
                let v = ranges[1].start + ranges[1].span() * j as f64 / rail_samples as f64;
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

/// The normal of the plane holding a rail, if it has one. A NURBS curve is
/// planar exactly when its control polygon is; implicit curves are sampled.
fn planar_normal(
    rail: &Curve3,
    range: ParameterRange,
    tolerance: ModelTolerance,
) -> KernelResult<Option<Vec3>> {
    let points = match rail {
        Curve3::Circle(circle) => return Ok(Some(circle.normal())),
        Curve3::Line(line) => {
            return Ok(Some(Frame3::from_z(line.origin(), line.direction())?.x()))
        }
        Curve3::Nurbs(curve) => curve.control_points().to_vec(),
        _ => (0..=64)
            .map(|i| Ok(rail.evaluate(range.start + range.span() * i as f64 / 64.0)?.point))
            .collect::<KernelResult<Vec<_>>>()?,
    };
    let origin = points[0];
    // Span the plane by the point farthest from the origin and the one
    // farthest from the line through both.
    let farthest = |measure: &dyn Fn(Vec3) -> f64| {
        points
            .iter()
            .map(|point| point - origin)
            .fold(Vec3::new(0.0, 0.0, 0.0), |best, offset| {
                if measure(offset) > measure(best) { offset } else { best }
            })
    };
    let axis = farthest(&|offset| offset.magnitude());
    let extent = axis.magnitude();
    let normal = axis.cross(farthest(&|offset| axis.cross(offset).magnitude()));
    if normal.magnitude() <= tolerance.linear_at_scale(extent) * extent {
        // Collinear points: any plane through their line holds the rail
        return Ok(Some(Frame3::from_z(origin, axis)?.x()));
    }
    let normal = normal.normalize();
    Ok(points
        .iter()
        .all(|point| normal.dot(point - origin).abs() <= tolerance.linear_at_scale(extent))
        .then_some(normal))
}

impl Profile {
    /// Sweep along a rail wire, planar or spatial, whose segments join
    /// tangentially. The profile retains its initial placement and turns with
    /// the rail's rotation-minimizing frame, leaving each segment in the frame
    /// it reached. An open rail starts at whichever end lies closer to the
    /// profile plane; a closed one where it passes the profile. Holes share
    /// the outer boundary's frame and remain hollow along the whole rail.
    pub fn sweep(&self, plane: Plane3, rail: &Wire3) -> KernelResult<Compound> {
        let tolerance = self.tolerance();
        if !rail.is_tangent_continuous()? {
            return Err(invalid("sweep rail segments must join tangentially"));
        }
        let offset = |point: Point3| plane.normal().dot(point - plane.origin()).abs();
        let rail = if rail.segments().len() > 1 && rail.is_closed() {
            let center = super::sweep_limits::profile_center(self, plane)?;
            rail.opened_at(rail.closest_point(center)?.0)?
        } else if offset(rail.end()?.0) < offset(rail.start()?.0) {
            rail.reversed()
        } else {
            rail.clone()
        };
        let (start, tangent) = rail.start()?;
        let end = rail.end()?.0;
        if let [segment] = rail.segments() {
            if let Curve3::Line(_) = segment.curve() {
                return self.extrude(plane, end - start);
            }
            if let Curve3::Circle(circle) = segment.curve() {
                return self.revolve(
                    plane,
                    Axis3::new(circle.center(), circle.normal())?,
                    segment.range().span(),
                );
            }
        }
        let mut frames = Vec::new();
        let mut section_plane = plane;
        for segment in rail.segments() {
            let frame = SweptSurface3::new(
                section_plane,
                self.outer().segments()[0].clone(),
                segment.curve().clone(),
                segment.range(),
                tolerance,
            )?;
            section_plane = frame.section_plane(segment.range().end)?;
            frames.push(frame);
        }
        let alignment = tangent.dot(plane.normal());
        if alignment.abs() <= tolerance.angular {
            return Err(invalid(
                "sweep rail must leave the profile plane at its start",
            ));
        }
        let forward = alignment > 0.0;
        let closed = rail.is_closed();
        let single = frames.len() == 1;
        // A single closed rail closes its own frame; a chain must return the
        // section to where it started.
        if closed
            && !single
            && [(section_plane.u(), plane.u()), (section_plane.v(), plane.v())]
                .iter()
                .any(|(end, start)| (end - start).magnitude() > start.magnitude() * tolerance.angular * 16.0)
        {
            return Err(invalid("closed sweep rail frame does not close at its seam"));
        }
        let mut patches = Vec::new();
        let mut roles = Vec::new();
        if !closed {
            patches.push(super::sew::cap(self, plane, forward)?);
            patches.push(super::sew::cap(self, section_plane, !forward)?);
            roles.extend([super::CreationRole::ProfileStart, super::CreationRole::ProfileEnd]);
        }
        for (segment, frame) in rail.segments().iter().zip(&frames) {
            let range = segment.range();
            let mut parameters = segment.curve().integration_breakpoints(range);
            parameters.extend([range.start, range.end]);
            if closed && single {
                parameters.push((range.start + range.end) * 0.5);
            }
            parameters.sort_by(f64::total_cmp);
            parameters.dedup();
            if range.span() < 0.0 {
                parameters.reverse();
            }
            self.sweep_sides(frame, &parameters, forward, &mut patches, &mut roles)?;
        }
        // Around a closed rail each hole bounds a sealed cavity.
        if closed && !self.holes().is_empty() {
            return super::sew::publish_shells_with_roles(patches, roles, tolerance);
        }
        super::sew::publish_with_roles(patches, roles, tolerance)
    }

    /// Coil about an axis in the profile plane, turning by `angle` radians
    /// while advancing `pitch` along the axis per full turn. The profile screws
    /// about the axis, so its sides are exact helicoidal sweeps.
    pub fn coil(&self, plane: Plane3, axis: Axis3, angle: f64, pitch: f64) -> KernelResult<Compound> {
        let tolerance = self.tolerance();
        if !angle.is_finite() || angle.abs() <= tolerance.parameter {
            return Err(invalid("coil angle must be finite and nonzero"));
        }
        if !pitch.is_finite() || pitch.abs() <= tolerance.linear {
            return Err(invalid("coil pitch must be finite and nonzero"));
        }
        let (profile, frame) = super::revolve_loft::radial_profile(self, plane, axis)?;
        // Radial and axial reach of the profile in its meridian coordinates
        let mut radial = f64::INFINITY;
        let mut axial = [f64::INFINITY, f64::NEG_INFINITY];
        for segment in profile.outer().segments() {
            let mut reach = |point: Point2| {
                radial = radial.min(point.x);
                axial = [axial[0].min(point.y), axial[1].max(point.y)];
            };
            match segment.curve() {
                Curve2::Line(_) => [segment.start()?, segment.end()?].into_iter().for_each(&mut reach),
                Curve2::Circle(_) => {
                    [segment.start()?, segment.end()?].into_iter().for_each(&mut reach);
                    // Arcs reach their extremes at quarter turns
                    for k in -8..=8 {
                        let extreme = k as f64 * std::f64::consts::FRAC_PI_2;
                        if segment.range().contains(extreme, tolerance) {
                            reach(segment.curve().evaluate(extreme)?.point);
                        }
                    }
                }
                curve => {
                    let bounds = super::bounds::curve2(curve, segment.range())?;
                    reach(Point2::new(bounds.0.x, bounds.0.y));
                    reach(Point2::new(bounds.1.x, bounds.1.y));
                }
            }
        }
        if radial <= tolerance.linear {
            return Err(invalid("coil profile may not touch its axis"));
        }
        if angle.abs() >= std::f64::consts::TAU - tolerance.parameter
            && axial[1] - axial[0] >= pitch.abs() - tolerance.linear
        {
            return Err(invalid("coil pitch must exceed the profile's extent along the axis"));
        }
        let span = angle.abs();
        let meridian = Plane3::new(frame.origin(), frame.x(), frame.z(), tolerance)?;
        let helix = SweptSurface3::helical(
            meridian,
            profile.outer().segments()[0].clone(),
            Line3::new(frame.origin(), frame.z() * (pitch / std::f64::consts::TAU))?,
            ParameterRange::new(0.0, span)?,
            // The screw turns about the rail's direction, which follows the pitch.
            angle.signum() * pitch.signum(),
            tolerance,
        )?;
        // The meridian normal points against a positive turn.
        let forward = angle < 0.0;
        let mut patches = vec![
            super::sew::cap(&profile, meridian, forward)?,
            super::sew::cap(&profile, helix.section_plane(span)?, !forward)?,
        ];
        let mut roles = vec![super::CreationRole::ProfileStart, super::CreationRole::ProfileEnd];
        profile.sweep_sides(&helix, &[0.0, span], forward, &mut patches, &mut roles)?;
        super::sew::publish_with_roles(patches, roles, tolerance)
    }

    /// Side patches of one rail segment between consecutive rail parameters.
    fn sweep_sides(
        &self,
        frame: &SweptSurface3,
        parameters: &[f64],
        forward: bool,
        patches: &mut Vec<BrepPatch>,
        roles: &mut Vec<super::CreationRole>,
    ) -> KernelResult<()> {
        let tolerance = self.tolerance();
        // Keep each source section edge intact. Periodic charts and display
        // subdivision must not introduce artificial CAD edges into the solid.
        let rings = std::iter::once(self.outer()).chain(self.holes()).enumerate();
        for (ring, source, section) in rings.flat_map(|(ring, wire)| {
            wire.segments().iter().enumerate().map(move |(source, section)| (ring, source, section))
        }) {
            // Sections share the rail frame, which is integrated only once
            let surface = Surface3::Swept(SweptSurface3 {
                profile: section.clone(),
                ..frame.clone()
            });
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
                    patch.reverse();
                }
                patches.push(patch);
                roles.push(super::CreationRole::ProfileSide { generators: vec![[0, ring, self.generator(ring, source)]], chart: None });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/sweep.rs"]
mod tests;
