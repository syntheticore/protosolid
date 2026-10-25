//! Planes, analytic surfaces of revolution, and offset surfaces.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane3 {
    pub(super) origin: Point3,
    pub(super) u: Vec3,
    pub(super) v: Vec3,
}

impl Plane3 {
    pub fn xy() -> Self {
        Self {
            origin: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
        }
    }

    pub fn xz() -> Self {
        Self {
            origin: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 0.0, 1.0),
        }
    }

    pub fn yz() -> Self {
        Self {
            origin: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(0.0, 1.0, 0.0),
            v: Vec3::new(0.0, 0.0, 1.0),
        }
    }

    pub fn new(origin: Point3, u: Vec3, v: Vec3, tolerance: ModelTolerance) -> KernelResult<Self> {
        if !finite_point(origin) || !finite_vector(u) || !finite_vector(v) {
            return Err(invalid("plane frame must be finite"));
        }
        let normal = u.cross(v);
        let scale = u.magnitude().max(v.magnitude());
        if normal.magnitude() <= tolerance.linear_at_scale(scale).powi(2) {
            return Err(invalid("plane parameter axes must be linearly independent"));
        }
        Ok(Self { origin, u, v })
    }

    pub fn from_frame(frame: Frame3) -> Self {
        Self {
            origin: frame.origin(),
            u: frame.x(),
            v: frame.y(),
        }
    }

    pub fn from_origin_normal(origin: Point3, normal: Vec3) -> KernelResult<Self> {
        Ok(Self::from_frame(Frame3::from_z(origin, normal)?))
    }

    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn u(self) -> Vec3 {
        self.u
    }
    pub fn v(self) -> Vec3 {
        self.v
    }
    pub fn normal(self) -> Vec3 {
        self.u.cross(self.v).normalize()
    }

    /// The spatial point at plane coordinates `uv`.
    pub fn point_at(self, uv: Point2) -> Point3 {
        self.origin + self.vector_at(uv.to_vec())
    }

    /// The spatial direction of the plane-coordinate vector `uv`.
    pub fn vector_at(self, uv: Vec2) -> Vec3 {
        self.u * uv.x + self.v * uv.y
    }

    /// Plane coordinates of the orthogonal projection of `point`.
    pub fn coordinates(self, point: Point3) -> KernelResult<Point2> {
        let (u, v) = self.project(point)?.parameters;
        Ok(Point2::new(u, v))
    }

    /// Lift an exact curve given in plane coordinates into space.
    pub fn lift_curve(self, curve: &Curve2, tolerance: ModelTolerance) -> KernelResult<Curve3> {
        Ok(match curve {
            Curve2::Line(line) => {
                Line3::new(self.point_at(line.origin()), self.vector_at(line.direction()))?.into()
            }
            Curve2::Circle(circle) => Circle3::new(
                self.point_at(circle.center()),
                self.u * circle.radius(),
                self.v * circle.radius(),
                tolerance,
            )?
            .into(),
            Curve2::Nurbs(curve) => NurbsCurve3::new(
                curve.degree(),
                curve.control_points().iter().map(|point| self.point_at(*point)).collect(),
                curve.weights().to_vec(),
                curve.knots().to_vec(),
            )?
            .into(),
            Curve2::SurfaceIntersection(curve) if curve.lies_on(&Surface3::Plane(self)) => {
                curve.spatial_curve()
            }
            Curve2::SurfaceProjection(curve) if curve.surface() == &Surface3::Plane(self) => {
                curve.curve().clone()
            }
            Curve2::SurfaceIntersection(_) | Curve2::SurfaceProjection(_) => {
                return Err(crate::unsupported(
                    "an implicit p-curve can only be lifted onto its retained plane",
                ))
            }
            Curve2::Offset(_) | Curve2::Projected(_) => SurfaceCurve3::new(
                Surface3::Plane(self), curve.clone(),
                match curve.domain().bounds {
                    DomainBounds::Bounded(range) => range,
                    _ => return Err(crate::unsupported("lifting an unbounded offset requires a trim")),
                }, tolerance,
            )?.into(),
        })
    }

    /// The exact plane-coordinate image of a line, circle or NURBS curve lying
    /// in this plane. Other curve types have no closed form here.
    pub(crate) fn flatten_curve(
        self,
        curve: &Curve3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Option<Curve2>> {
        Ok(match curve {
            Curve3::Line(line) => {
                let origin = self.coordinates(line.origin())?;
                let end = self.coordinates(line.origin() + line.direction())?;
                Some(Line2::new(origin, end - origin)?.into())
            }
            Curve3::Nurbs(curve) => Some(
                NurbsCurve2::new(
                    curve.degree(),
                    curve
                        .control_points()
                        .iter()
                        .map(|point| self.coordinates(*point))
                        .collect::<KernelResult<_>>()?,
                    curve.weights().to_vec(),
                    curve.knots().to_vec(),
                )?
                .into(),
            ),
            Curve3::Circle(circle) => {
                let center = self.coordinates(circle.center())?;
                let x = self.coordinates(circle.center() + circle.x_radius())? - center;
                let y = self.coordinates(circle.center() + circle.y_radius())? - center;
                let scale = x.magnitude().max(y.magnitude()).max(1.0);
                let allowed = tolerance.parameter * scale * 64.0;
                (x.x > 0.0
                    && y.y > 0.0
                    && x.y.abs() <= allowed
                    && y.x.abs() <= allowed
                    && (x.x - y.y).abs() <= allowed)
                    .then(|| Circle2::new(center, (x.x + y.y) * 0.5).map(Into::into))
                    .transpose()?
            }
            Curve3::SurfaceIntersection(_) | Curve3::SurfaceCurve(_) => None,
        })
    }

    pub(crate) fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let zero = Vec3::new(0.0, 0.0, 0.0);
        Ok(SurfaceEvaluation {
            point: self.origin + self.u * u + self.v * v,
            du: self.u,
            dv: self.v,
            duu: zero,
            duv: zero,
            dvv: zero,
        })
    }

    pub(crate) fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.origin;
        let uu = self.u.dot(self.u);
        let uv = self.u.dot(self.v);
        let vv = self.v.dot(self.v);
        let determinant = uu * vv - uv * uv;
        if determinant == 0.0 {
            return Err(geometry_error("plane frame is singular"));
        }
        let du = delta.dot(self.u);
        let dv = delta.dot(self.v);
        let u = (du * vv - dv * uv) / determinant;
        let v = (dv * uu - du * uv) / determinant;
        let projected = self.origin + self.u * u + self.v * v;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cylinder3 {
    pub(super) origin: Point3,
    pub(super) axis: Vec3,
    pub(super) x_radius: Vec3,
    pub(super) y_radius: Vec3,
}

/// Circular cone parameterized from its apex (`v = 0`) through a reference
/// circle (`v = 1`). Trims select the finite sheet used by topology.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cone3 {
    pub(super) apex: Point3,
    pub(super) axis: Vec3,
    pub(super) x_radius: Vec3,
    pub(super) y_radius: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere3 {
    pub(super) center: Point3,
    pub(super) x_radius: Vec3,
    pub(super) y_radius: Vec3,
    pub(super) z_radius: Vec3,
}

/// A regular ring torus. The minor radius is strictly smaller than the major
/// radius so the natural surface cannot self-intersect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Torus3 {
    pub(super) center: Point3,
    pub(super) x_major: Vec3,
    pub(super) y_major: Vec3,
    pub(super) minor_radius: f64,
    // Unit axis, opposite to x × y in a mirrored torus
    pub(super) normal: Vec3,
}

/// An exact surface of revolution whose meridian is retained as a 2D curve.
///
/// The profile uses `(radius, axial_distance)` coordinates in `frame`; the
/// first surface parameter is the revolution angle and the second is the
/// profile-curve parameter. Canonical cylinders, cones, spheres, and tori are
/// represented by their dedicated types instead, while this type preserves
/// rotational structure for more general analytical or rational profiles.
#[derive(Debug, Clone, PartialEq)]
pub struct RevolvedSurface3 {
    pub(super) frame: Frame3,
    pub(super) profile: Curve2,
    pub(super) profile_range: ParameterRange,
    pub(super) tolerance: ModelTolerance,
}

impl Cylinder3 {
    pub fn new(
        origin: Point3,
        axis: Vec3,
        x_radius: Vec3,
        y_radius: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let circle = Circle3::new(origin, x_radius, y_radius, tolerance)?;
        if !finite_vector(axis) || axis.magnitude() <= tolerance.linear {
            return Err(invalid("cylinder axis must be finite and nonzero"));
        }
        if axis
            .angle(circle.normal())
            .0
            .min(axis.angle(-circle.normal()).0)
            > tolerance.angular
        {
            return Err(invalid(
                "cylinder axis must be perpendicular to its radial frame",
            ));
        }
        Ok(Self {
            origin,
            axis,
            x_radius,
            y_radius,
        })
    }

    pub fn from_axis(
        axis: Axis3,
        axial_scale: f64,
        radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !axial_scale.is_finite()
            || axial_scale.abs() <= tolerance.linear_at_scale(axial_scale)
            || !radius.is_finite()
            || radius <= tolerance.linear_at_scale(radius)
        {
            return Err(invalid("cylinder axial scale must exceed model tolerance"));
        }
        let frame = Frame3::from_z(axis.origin(), axis.direction())?;
        Self::new(
            axis.origin(),
            axis.direction() * axial_scale,
            frame.x() * radius,
            frame.y() * radius,
            tolerance,
        )
    }

    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn axis(self) -> Vec3 {
        self.axis
    }
    pub fn radius(self) -> f64 {
        self.x_radius.magnitude()
    }
    pub fn x_radius(self) -> Vec3 {
        self.x_radius
    }
    pub fn y_radius(self) -> Vec3 {
        self.y_radius
    }

    pub(crate) fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let (sin, cos) = u.sin_cos();
        let radial = self.x_radius * cos + self.y_radius * sin;
        let tangent = -self.x_radius * sin + self.y_radius * cos;
        let zero = Vec3::new(0.0, 0.0, 0.0);
        Ok(SurfaceEvaluation {
            point: self.origin + radial + self.axis * v,
            du: tangent,
            dv: self.axis,
            duu: -radial,
            duv: zero,
            dvv: zero,
        })
    }

    pub(super) fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.origin;
        let v = delta.dot(self.axis) / self.axis.magnitude2();
        let radial = delta - self.axis * v;
        let x = radial.dot(self.x_radius) / self.x_radius.magnitude2();
        let y = radial.dot(self.y_radius) / self.y_radius.magnitude2();
        if x == 0.0 && y == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "cylinder projection is not unique on its axis",
            ));
        }
        let u = y.atan2(x).rem_euclid(std::f64::consts::TAU);
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

impl Cone3 {
    pub fn new(
        apex: Point3,
        axis: Vec3,
        x_radius: Vec3,
        y_radius: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let reference = Circle3::new(apex + axis, x_radius, y_radius, tolerance)?;
        if !finite_vector(axis)
            || axis.magnitude() <= tolerance.linear
            || axis
                .angle(reference.normal())
                .0
                .min(axis.angle(-reference.normal()).0)
                > tolerance.angular
        {
            return Err(invalid(
                "cone axis must be finite, nonzero, and perpendicular to its reference circle",
            ));
        }
        Ok(Self {
            apex,
            axis,
            x_radius,
            y_radius,
        })
    }

    pub fn from_axis(
        axis: Axis3,
        height: f64,
        reference_radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !height.is_finite()
            || height.abs() <= tolerance.linear_at_scale(height)
            || !reference_radius.is_finite()
            || reference_radius <= tolerance.linear_at_scale(reference_radius)
        {
            return Err(invalid("cone height must exceed model tolerance"));
        }
        let frame = Frame3::from_z(axis.origin(), axis.direction())?;
        Self::new(
            axis.origin(),
            axis.direction() * height,
            frame.x() * reference_radius,
            frame.y() * reference_radius,
            tolerance,
        )
    }

    pub fn apex(self) -> Point3 {
        self.apex
    }
    pub fn axis(self) -> Vec3 {
        self.axis
    }
    pub fn x_radius(self) -> Vec3 {
        self.x_radius
    }
    pub fn y_radius(self) -> Vec3 {
        self.y_radius
    }
    pub fn reference_radius(self) -> f64 {
        self.x_radius.magnitude()
    }

    pub(super) fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let (sin, cos) = u.sin_cos();
        let radial = self.x_radius * cos + self.y_radius * sin;
        let tangent = -self.x_radius * sin + self.y_radius * cos;
        let zero = Vec3::new(0.0, 0.0, 0.0);
        Ok(SurfaceEvaluation {
            point: self.apex + (self.axis + radial) * v,
            du: tangent * v,
            dv: self.axis + radial,
            duu: -radial * v,
            duv: tangent,
            dvv: zero,
        })
    }

    pub(super) fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.apex;
        let x = delta.dot(self.x_radius) / self.x_radius.magnitude2();
        let y = delta.dot(self.y_radius) / self.y_radius.magnitude2();
        let u = if x == 0.0 && y == 0.0 {
            0.0
        } else {
            y.atan2(x).rem_euclid(std::f64::consts::TAU)
        };
        let (sin, cos) = u.sin_cos();
        let generator = self.axis + self.x_radius * cos + self.y_radius * sin;
        let v = delta.dot(generator) / generator.magnitude2();
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

impl Sphere3 {
    pub fn new(
        center: Point3,
        x_radius: Vec3,
        y_radius: Vec3,
        z_radius: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let equator = Circle3::new(center, x_radius, y_radius, tolerance)?;
        let radius = equator.radius();
        if !finite_vector(z_radius)
            || (z_radius.magnitude() - radius).abs() > tolerance.linear_at_scale(radius)
            || z_radius
                .angle(equator.normal())
                .0
                .min(z_radius.angle(-equator.normal()).0)
                > tolerance.angular
        {
            return Err(invalid(
                "sphere frame requires three perpendicular radius vectors of equal length",
            ));
        }
        Ok(Self {
            center,
            x_radius,
            y_radius,
            z_radius,
        })
    }

    pub fn from_radius(
        center: Point3,
        radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !radius.is_finite() || radius <= tolerance.linear_at_scale(radius) {
            return Err(invalid("sphere radius must exceed model tolerance"));
        }
        Self::new(
            center,
            Vec3::unit_x() * radius,
            Vec3::unit_y() * radius,
            Vec3::unit_z() * radius,
            tolerance,
        )
    }

    pub fn center(self) -> Point3 {
        self.center
    }
    pub fn radius(self) -> f64 {
        self.x_radius.magnitude()
    }
    pub fn x_radius(self) -> Vec3 {
        self.x_radius
    }
    pub fn y_radius(self) -> Vec3 {
        self.y_radius
    }
    pub fn z_radius(self) -> Vec3 {
        self.z_radius
    }

    pub(super) fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let (u_sin, u_cos) = u.sin_cos();
        let (v_sin, v_cos) = v.sin_cos();
        let radial = self.x_radius * u_cos + self.y_radius * u_sin;
        let tangent = -self.x_radius * u_sin + self.y_radius * u_cos;
        Ok(SurfaceEvaluation {
            point: self.center + radial * v_cos + self.z_radius * v_sin,
            du: tangent * v_cos,
            dv: -radial * v_sin + self.z_radius * v_cos,
            duu: -radial * v_cos,
            duv: -tangent * v_sin,
            dvv: -radial * v_cos - self.z_radius * v_sin,
        })
    }

    pub(super) fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.center;
        if delta.magnitude2() == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "sphere projection is not unique at its center",
            ));
        }
        let radius = self.radius();
        let x = delta.dot(self.x_radius) / radius.powi(2);
        let y = delta.dot(self.y_radius) / radius.powi(2);
        let z = delta.dot(self.z_radius) / radius.powi(2);
        let u = y.atan2(x).rem_euclid(std::f64::consts::TAU);
        let v = z.atan2((x * x + y * y).sqrt());
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

impl Torus3 {
    pub fn new(
        center: Point3,
        x_major: Vec3,
        y_major: Vec3,
        minor_radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        let major = Circle3::new(center, x_major, y_major, tolerance)?;
        if !minor_radius.is_finite()
            || minor_radius <= tolerance.linear
            || minor_radius >= major.radius() - tolerance.linear_at_scale(major.radius())
        {
            return Err(invalid(
                "a regular torus needs a positive minor radius strictly smaller than its major radius",
            ));
        }
        Ok(Self {
            center,
            x_major,
            y_major,
            minor_radius,
            normal: x_major.cross(y_major).normalize(),
        })
    }

    /// The same torus with its tube parameter running the other way around.
    /// Reflections keep the parameterization of a torus with this axis.
    pub(crate) fn with_reversed_normal(mut self) -> Self {
        self.normal = -self.normal;
        self
    }

    pub fn from_axis(
        axis: Axis3,
        major_radius: f64,
        minor_radius: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !major_radius.is_finite() || major_radius <= tolerance.linear_at_scale(major_radius) {
            return Err(invalid("torus major radius must exceed model tolerance"));
        }
        let frame = Frame3::from_z(axis.origin(), axis.direction())?;
        Self::new(
            axis.origin(),
            frame.x() * major_radius,
            frame.y() * major_radius,
            minor_radius,
            tolerance,
        )
    }

    pub fn center(self) -> Point3 {
        self.center
    }
    pub fn major_radius(self) -> f64 {
        self.x_major.magnitude()
    }
    pub fn minor_radius(self) -> f64 {
        self.minor_radius
    }
    pub fn x_major(self) -> Vec3 {
        self.x_major
    }
    pub fn y_major(self) -> Vec3 {
        self.y_major
    }
    pub fn normal(self) -> Vec3 {
        self.normal
    }

    pub(super) fn evaluate(self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        if !u.is_finite() || !v.is_finite() {
            return Err(invalid("surface parameters must be finite"));
        }
        let major = self.major_radius();
        let (u_sin, u_cos) = u.sin_cos();
        let (v_sin, v_cos) = v.sin_cos();
        let radial_unit = (self.x_major * u_cos + self.y_major * u_sin) / major;
        let tangent_unit = (-self.x_major * u_sin + self.y_major * u_cos) / major;
        let normal = self.normal();
        let radial_distance = major + self.minor_radius * v_cos;
        Ok(SurfaceEvaluation {
            point: self.center
                + radial_unit * radial_distance
                + normal * (self.minor_radius * v_sin),
            du: tangent_unit * radial_distance,
            dv: radial_unit * (-self.minor_radius * v_sin) + normal * (self.minor_radius * v_cos),
            duu: -radial_unit * radial_distance,
            duv: tangent_unit * (-self.minor_radius * v_sin),
            dvv: radial_unit * (-self.minor_radius * v_cos) - normal * (self.minor_radius * v_sin),
        })
    }

    pub(super) fn project(self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.center;
        let major = self.major_radius();
        let x = delta.dot(self.x_major) / major.powi(2);
        let y = delta.dot(self.y_major) / major.powi(2);
        if x == 0.0 && y == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "torus projection is not unique on its axis",
            ));
        }
        let u = y.atan2(x).rem_euclid(std::f64::consts::TAU);
        let (sin, cos) = u.sin_cos();
        let radial = (self.x_major * cos + self.y_major * sin) / major;
        let tube_center = self.center + radial * major;
        let tube_delta = point - tube_center;
        let v = tube_delta
            .dot(self.normal())
            .atan2(tube_delta.dot(radial))
            .rem_euclid(std::f64::consts::TAU);
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

impl RevolvedSurface3 {
    pub fn new(
        frame: Frame3,
        profile: Curve2,
        profile_range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !profile
            .domain()
            .contains_range(profile_range, tolerance.parameter)
        {
            return Err(invalid(
                "revolved-surface profile does not cover its trim range",
            ));
        }
        let surface = Self {
            frame,
            profile,
            profile_range,
            tolerance,
        };
        for index in 0..=8 {
            let parameter = profile_range.start + profile_range.span() * index as f64 / 8.0;
            let profile = surface.profile.evaluate(parameter)?;
            let scale = profile.point.x.abs().max(profile.point.y.abs()).max(1.0);
            let linear = tolerance.linear_at_scale(scale);
            let boundary_pole = (index == 0 || index == 8)
                && profile.point.x.abs() <= linear
                && profile.first_derivative.magnitude() > linear;
            if profile.point.x < -linear
                || (profile.point.x <= linear && !boundary_pole)
                || profile.first_derivative.magnitude() <= linear
            {
                return Err(invalid(
                    "revolved-surface profile must remain regular and may meet its axis only at an isolated boundary pole",
                ));
            }
        }
        Ok(surface)
    }

    pub fn frame(&self) -> Frame3 {
        self.frame
    }

    pub fn axis(&self) -> Axis3 {
        self.frame.axis()
    }

    pub fn profile(&self) -> &Curve2 {
        &self.profile
    }

    pub fn profile_range(&self) -> ParameterRange {
        self.profile_range
    }

    pub(super) fn profile_domain(&self) -> ParameterDomain {
        ParameterDomain {
            bounds: DomainBounds::Bounded(self.profile_range),
            period: self.profile.domain().period.filter(|period| {
                self.profile_range.span().abs() + self.tolerance.parameter >= *period
            }),
        }
    }

    pub(super) fn evaluate(&self, angle: f64, parameter: f64) -> KernelResult<SurfaceEvaluation> {
        let parameter = self.profile_domain().normalize(parameter);
        if !angle.is_finite() || !self.profile_range.contains(parameter, self.tolerance) {
            return Err(invalid(
                "revolved-surface parameter lies outside its domain",
            ));
        }
        let parameter = parameter.clamp(
            self.profile_range.start.min(self.profile_range.end),
            self.profile_range.start.max(self.profile_range.end),
        );
        let profile = self.profile.evaluate(parameter)?;
        let (sin, cos) = angle.sin_cos();
        let radial = self.frame.x() * cos + self.frame.y() * sin;
        let tangent = -self.frame.x() * sin + self.frame.y() * cos;
        Ok(SurfaceEvaluation {
            point: self.frame.origin()
                + radial * profile.point.x
                + self.frame.z() * profile.point.y,
            du: tangent * profile.point.x,
            dv: radial * profile.first_derivative.x + self.frame.z() * profile.first_derivative.y,
            duu: -radial * profile.point.x,
            duv: tangent * profile.first_derivative.x,
            dvv: radial * profile.second_derivative.x
                + self.frame.z() * profile.second_derivative.y,
        })
    }

    pub(super) fn project(&self, point: Point3) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.frame.origin();
        let x = delta.dot(self.frame.x());
        let y = delta.dot(self.frame.y());
        let axial = delta.dot(self.frame.z());
        let angle = if x == 0.0 && y == 0.0 {
            0.0
        } else {
            y.atan2(x).rem_euclid(std::f64::consts::TAU)
        };
        let target = Point2::new((x * x + y * y).sqrt(), axial);
        let mut parameter = (0..=32)
            .map(|index| self.profile_range.start + self.profile_range.span() * index as f64 / 32.0)
            .map(|parameter| {
                self.profile
                    .evaluate(parameter)
                    .map(|value| (parameter, value.point.distance2(target)))
            })
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|candidate| candidate.0)
            .ok_or_else(|| invalid("revolved-surface projection has no seed"))?;
        for _ in 0..32 {
            let value = self.profile.evaluate(parameter)?;
            let residual = value.point - target;
            let derivative =
                value.first_derivative.magnitude2() + residual.dot(value.second_derivative);
            if derivative.abs() <= f64::EPSILON * 256.0 {
                break;
            }
            let next = (parameter - residual.dot(value.first_derivative) / derivative).clamp(
                self.profile_range.start.min(self.profile_range.end),
                self.profile_range.start.max(self.profile_range.end),
            );
            if (next - parameter).abs() <= self.tolerance.parameter {
                parameter = next;
                break;
            }
            parameter = next;
        }
        let projected = self.evaluate(angle, parameter)?.point;
        Ok(Projection2 {
            parameters: (angle, parameter),
            point: projected,
            distance: projected.distance(point),
        })
    }
}

/// Exact normal-offset definition for a regular parent surface. General NURBS
/// offsets are not NURBS in closed form, so retaining the parent and signed
/// distance is the non-approximating representation.
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetSurface3 {
    pub(super) base: Box<Surface3>,
    pub(super) distance: f64,
    pub(super) v_slope: f64,
    pub(super) tolerance: ModelTolerance,
}

impl OffsetSurface3 {
    pub fn new(base: Surface3, distance: f64, tolerance: ModelTolerance) -> KernelResult<Self> {
        tolerance.validate()?;
        if !distance.is_finite() || distance.abs() <= tolerance.linear {
            return Err(invalid(
                "surface offset distance must be finite and exceed model tolerance",
            ));
        }
        Ok(Self {
            base: Box::new(base),
            distance,
            v_slope: 0.0,
            tolerance,
        })
    }

    pub fn base(&self) -> &Surface3 {
        &self.base
    }

    pub fn distance(&self) -> f64 {
        self.distance
    }

    pub fn v_slope(&self) -> f64 {
        self.v_slope
    }

    /// An affine offset law in the parent's second parameter. A vertical
    /// extrusion with this law is an exact constant-angle draft surface.
    pub fn with_linear_distance(
        base: Surface3,
        distance: f64,
        v_slope: f64,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !distance.is_finite() || !v_slope.is_finite()
            || !matches!(base.domains()[1].bounds, DomainBounds::Bounded(_))
        {
            return Err(invalid("a linear offset law needs finite coefficients and a bounded second parameter"));
        }
        Ok(Self { base: Box::new(base), distance, v_slope, tolerance })
    }

    pub(crate) fn maximum_distance(&self) -> f64 {
        if self.v_slope == 0.0 {
            return self.distance.abs();
        }
        let DomainBounds::Bounded(range) = self.base.domains()[1].bounds else {
            unreachable!()
        };
        (self.distance + self.v_slope * range.start).abs()
            .max((self.distance + self.v_slope * range.end).abs())
    }

    pub(super) fn first_order(&self, u: f64, v: f64) -> KernelResult<(Point3, Vec3, Vec3)> {
        if let Surface3::RollingBall(blend) = self.base.as_ref() {
            // The ball's radial normal and its derivatives are exact. Using
            // finite-differenced surface curvature here magnifies spine knots.
            let (point, du, dv) = blend.first_order(u, v)?;
            let (center, tangent) = blend.center_curve.first_order(blend.center_parameter(u))?;
            let radial = (point - center) / blend.radius;
            let sign = du.cross(dv).dot(radial).signum();
            let distance = (self.distance + self.v_slope * v) * sign;
            return Ok((point + radial * distance,
                du + (du - tangent) * (distance / blend.radius),
                dv * (1.0 + distance / blend.radius) + radial * self.v_slope * sign));
        }
        let value = self.base.evaluate(u, v)?;
        let cross = value.du.cross(value.dv);
        let magnitude = cross.magnitude();
        if magnitude
            <= self
                .tolerance
                .linear_at_scale(value.du.magnitude().max(value.dv.magnitude()).max(1.0))
        {
            return Err(geometry_error("offset surface parent has a singular parameterization"));
        }
        let normal = cross / magnitude;
        let project_tangent = |derivative: Vec3| derivative - normal * normal.dot(derivative);
        let cross_u = value.duu.cross(value.dv) + value.du.cross(value.duv);
        let cross_v = value.duv.cross(value.dv) + value.du.cross(value.dvv);
        let normal_u = project_tangent(cross_u) / magnitude;
        let normal_v = project_tangent(cross_v) / magnitude;
        let distance = self.distance + self.v_slope * v;
        Ok((
            value.point + normal * distance,
            value.du + normal_u * distance,
            value.dv + normal_v * distance + normal * self.v_slope,
        ))
    }

    pub(super) fn shifted_parameter(&self, direction: usize, parameter: f64, delta: f64) -> f64 {
        let domain = self.base.domains()[direction];
        let shifted = parameter + delta;
        if domain.period.is_some() {
            shifted
        } else if let DomainBounds::Bounded(range) = domain.bounds {
            shifted.clamp(range.start.min(range.end), range.start.max(range.end))
        } else {
            shifted
        }
    }

    pub(super) fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        let (point, du, dv) = self.first_order(u, v)?;
        let domains = self.base.domains();
        let parameter_scale = domains
            .iter()
            .filter_map(|domain| match domain.bounds {
                DomainBounds::Bounded(range) => Some(range.span().abs()),
                _ => None,
            })
            .fold(1.0_f64, f64::max);
        let step = (self.tolerance.parameter.sqrt() * parameter_scale)
            .max(f64::EPSILON.sqrt() * parameter_scale);
        let u_minus = self.shifted_parameter(0, u, -step);
        let u_plus = self.shifted_parameter(0, u, step);
        let v_minus = self.shifted_parameter(1, v, -step);
        let v_plus = self.shifted_parameter(1, v, step);
        let (_, du_minus, dv_u_minus) = self.first_order(u_minus, v)?;
        let (_, du_plus, dv_u_plus) = self.first_order(u_plus, v)?;
        let (_, du_v_minus, dv_minus) = self.first_order(u, v_minus)?;
        let (_, du_v_plus, dv_plus) = self.first_order(u, v_plus)?;
        let u_span = u_plus - u_minus;
        let v_span = v_plus - v_minus;
        if u_span.abs() <= self.tolerance.parameter || v_span.abs() <= self.tolerance.parameter {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "offset surface derivative stencil collapsed at a parameter boundary",
            ));
        }
        let duu = (du_plus - du_minus) / u_span;
        let dvv = (dv_plus - dv_minus) / v_span;
        let duv = ((dv_u_plus - dv_u_minus) / u_span + (du_v_plus - du_v_minus) / v_span) * 0.5;
        Ok(SurfaceEvaluation {
            point,
            du,
            dv,
            duu,
            duv,
            dvv,
        })
    }

    pub(super) fn project(&self, point: Point3) -> KernelResult<Projection2> {
        let domains = self.base.domains();
        let bounded = match (domains[0].bounds, domains[1].bounds) {
            (DomainBounds::Bounded(u), DomainBounds::Bounded(v)) => Some([u, v]),
            _ => None,
        };
        // An unbounded blend has no finite grid to search. Its parent provides
        // a local seed in the same UV chart; bounded parents keep the grid path.
        let parent_seed = if bounded.is_none() {
            self.base
                .project(point, self.tolerance)
                .ok()
                .map(|projection| projection.parameters)
        } else {
            None
        };
        let parameters = bounded
            .into_iter()
            .flat_map(|[u_range, v_range]| {
                (0..=8).flat_map(move |u_index| {
                    (0..=8).map(move |v_index| {
                        (
                            u_range.start + u_range.span() * u_index as f64 / 8.0,
                            v_range.start + v_range.span() * v_index as f64 / 8.0,
                        )
                    })
                })
            })
            .chain(parent_seed)
            .filter_map(|parameters| {
                self.evaluate(parameters.0, parameters.1)
                    .ok()
                    .map(|value| (parameters, value.point.distance2(point)))
            })
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|candidate| candidate.0)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "offset surface projection has no seed",
                )
            })?;
        refine_surface_projection(
            point, Point2::new(parameters.0, parameters.1), domains,
            |u, v| self.first_order(u, v),
            |_, _| f64::EPSILON * 256.0,
            |step| step.x.abs().max(step.y.abs()) <= self.tolerance.parameter,
        )
    }
}
