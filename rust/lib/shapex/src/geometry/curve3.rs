//! Analytic spatial curves.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line3 {
    pub(super) origin: Point3,
    pub(super) direction: Vec3,
}

impl Line3 {
    pub fn new(origin: Point3, direction: Vec3) -> KernelResult<Self> {
        if !finite_point(origin) || !finite_vector(direction) || direction.magnitude2() == 0.0 {
            return Err(invalid(
                "a line requires a finite origin and nonzero finite direction",
            ));
        }
        Ok(Self { origin, direction })
    }

    pub fn through(start: Point3, end: Point3) -> KernelResult<Self> {
        Self::new(start, end - start)
    }

    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn direction(self) -> Vec3 {
        self.direction
    }

    pub(crate) fn evaluate(self, parameter: f64) -> KernelResult<CurveEvaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        Ok(CurveEvaluation {
            point: self.origin + self.direction * parameter,
            first_derivative: self.direction,
            second_derivative: Vec3::new(0.0, 0.0, 0.0),
        })
    }

    pub(crate) fn project(self, point: Point3) -> KernelResult<Projection1> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let parameter = (point - self.origin).dot(self.direction) / self.direction.magnitude2();
        let projected = self.origin + self.direction * parameter;
        Ok(Projection1 {
            parameter,
            point: projected,
            distance: projected.distance(point),
        })
    }
}

/// An exact circle stored as two radius vectors. This retains its parameter frame
/// and avoids silently choosing a new seam during regeneration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circle3 {
    pub(super) center: Point3,
    pub(super) x_radius: Vec3,
    pub(super) y_radius: Vec3,
}

impl Circle3 {
    pub fn new(
        center: Point3,
        x_radius: Vec3,
        y_radius: Vec3,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !finite_point(center) || !finite_vector(x_radius) || !finite_vector(y_radius) {
            return Err(invalid("circle frame must be finite"));
        }
        let x_length = x_radius.magnitude();
        let y_length = y_radius.magnitude();
        let scale = x_length.max(y_length);
        if x_length <= tolerance.linear_at_scale(scale)
            || y_length <= tolerance.linear_at_scale(scale)
        {
            return Err(invalid("circle radius must exceed model tolerance"));
        }
        if (x_length - y_length).abs() > tolerance.linear_at_scale(scale)
            || (x_radius.angle(y_radius).0 - std::f64::consts::FRAC_PI_2).abs() > tolerance.angular
        {
            return Err(invalid(
                "circle radius vectors must be perpendicular and equally long",
            ));
        }
        Ok(Self {
            center,
            x_radius,
            y_radius,
        })
    }

    pub fn from_axis(axis: Axis3, radius: f64, tolerance: ModelTolerance) -> KernelResult<Self> {
        if !radius.is_finite() || radius <= tolerance.linear_at_scale(radius) {
            return Err(invalid("circle radius must exceed model tolerance"));
        }
        let frame = Frame3::from_z(axis.origin(), axis.direction())?;
        Self::new(
            axis.origin(),
            frame.x() * radius,
            frame.y() * radius,
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
    pub fn normal(self) -> Vec3 {
        self.x_radius.cross(self.y_radius).normalize()
    }

    pub(crate) fn evaluate(self, parameter: f64) -> KernelResult<CurveEvaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        let (sin, cos) = parameter.sin_cos();
        let radial = self.x_radius * cos + self.y_radius * sin;
        let tangent = -self.x_radius * sin + self.y_radius * cos;
        Ok(CurveEvaluation {
            point: self.center + radial,
            first_derivative: tangent,
            second_derivative: -radial,
        })
    }

    pub(crate) fn project(self, point: Point3) -> KernelResult<Projection1> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let delta = point - self.center;
        let x = delta.dot(self.x_radius) / self.x_radius.magnitude2();
        let y = delta.dot(self.y_radius) / self.y_radius.magnitude2();
        if x == 0.0 && y == 0.0 {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "circle projection is not unique on its axis",
            ));
        }
        let parameter = y.atan2(x).rem_euclid(std::f64::consts::TAU);
        let projected = self.evaluate(parameter)?.point;
        Ok(Projection1 {
            parameter,
            point: projected,
            distance: projected.distance(point),
        })
    }
}
