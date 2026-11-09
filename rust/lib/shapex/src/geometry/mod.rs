use cgmath::prelude::*;

use crate::{Point2, Point3, Vec2, Vec3, Vec4};

use super::linalg::solve_linear;
use super::{
    invalid, DiagnosticKind, DomainBounds, KernelError, KernelResult, ModelTolerance,
    ParameterDomain, ParameterRange,
};
use crate::geometry_error;

mod blend_surface;
mod curve2;
mod curve3;
mod intersection_curve;
mod nurbs;
mod surfaces;

pub use blend_surface::*;
pub use curve2::*;
pub use curve3::*;
pub use intersection_curve::*;
pub use nurbs::*;
pub use surfaces::*;

#[cfg(test)]
#[path = "../geometry_tests.rs"]
mod tests;

pub(crate) fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn finite_vector(vector: Vec3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

/// A finite directed axis with a unit direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis3 {
    origin: Point3,
    direction: Vec3,
}

impl Axis3 {
    pub fn new(origin: Point3, direction: Vec3) -> KernelResult<Self> {
        if !finite_point(origin) || !finite_vector(direction) || direction.magnitude2() == 0.0 {
            return Err(invalid(
                "an axis requires a finite origin and nonzero direction",
            ));
        }
        Ok(Self {
            origin,
            direction: direction.normalize(),
        })
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

    pub fn point_at(self, distance: f64) -> KernelResult<Point3> {
        if !distance.is_finite() {
            return Err(invalid("axis distance must be finite"));
        }
        Ok(self.origin + self.direction * distance)
    }
}

/// An orthonormal coordinate frame, right-handed unless reflected.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame3 {
    origin: Point3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
}

impl Frame3 {
    pub fn new(origin: Point3, x: Vec3, y: Vec3, tolerance: ModelTolerance) -> KernelResult<Self> {
        if !finite_point(origin) || !finite_vector(x) || !finite_vector(y) {
            return Err(invalid("coordinate frame must be finite"));
        }
        let x_length = x.magnitude();
        let y_length = y.magnitude();
        if x_length == 0.0
            || y_length == 0.0
            || x.dot(y).abs() > tolerance.angular * x_length * y_length
        {
            return Err(invalid(
                "coordinate frame axes must be nonzero and perpendicular",
            ));
        }
        let x = x / x_length;
        let y = y / y_length;
        Ok(Self {
            origin,
            x,
            y,
            z: x.cross(y).normalize(),
        })
    }

    /// Build a deterministic frame around a desired Z direction.
    pub fn from_z(origin: Point3, z: Vec3) -> KernelResult<Self> {
        let axis = Axis3::new(origin, z)?;
        let z = axis.direction();
        let absolute = [z.x.abs(), z.y.abs(), z.z.abs()];
        let reference = if absolute[0] <= absolute[1] && absolute[0] <= absolute[2] {
            Vec3::unit_x()
        } else if absolute[1] <= absolute[2] {
            Vec3::unit_y()
        } else {
            Vec3::unit_z()
        };
        let x = reference.cross(z).normalize();
        let y = z.cross(x).normalize();
        Ok(Self { origin, x, y, z })
    }

    pub fn origin(self) -> Point3 {
        self.origin
    }
    pub fn x(self) -> Vec3 {
        self.x
    }
    pub fn y(self) -> Vec3 {
        self.y
    }
    pub fn z(self) -> Vec3 {
        self.z
    }
    pub fn axis(self) -> Axis3 {
        Axis3 {
            origin: self.origin,
            direction: self.z,
        }
    }

    /// The left-handed frame produced by reflecting a right-handed one.
    pub(crate) fn with_reversed_z(mut self) -> Self {
        self.z = -self.z;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve2Evaluation {
    pub point: Point2,
    pub first_derivative: Vec2,
    pub second_derivative: Vec2,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Curve2 {
    Line(Line2),
    Circle(Circle2),
    Nurbs(NurbsCurve2),
    Offset(super::OffsetCurve2),
    Projected(super::ProjectedCurve2),
    SurfaceIntersection(SurfaceIntersectionPcurve2),
    SurfaceProjection(SurfaceProjectionPcurve2),
}

impl Curve2 {
    /// Curves defined pointwise from another curve and bounded by their trim.
    pub(crate) fn is_derived(&self) -> bool {
        matches!(self, Self::Offset(_) | Self::Projected(_))
    }

    /// Curves without a closed form. Queries on them use the generic
    /// parametric algorithms rather than analytic or Bézier ones.
    pub(crate) fn is_general(&self) -> bool {
        self.is_derived()
            || matches!(self, Self::SurfaceIntersection(_) | Self::SurfaceProjection(_))
    }

    /// A short human-readable name of the curve type, for diagnostics.
    pub(crate) fn kind_name(&self) -> &'static str {
        match self {
            Self::Line(_) => "line",
            Self::Circle(_) => "circle",
            Self::Nurbs(_) => "NURBS",
            Self::Offset(_) => "offset",
            Self::Projected(_) => "projected",
            Self::SurfaceIntersection(_) => "surface intersection",
            Self::SurfaceProjection(_) => "surface projection",
        }
    }

    pub(crate) fn extended(&self, range: ParameterRange) -> KernelResult<Self> {
        if self.domain().contains_range(range, 0.0) {
            return Ok(self.clone());
        }
        let range = match self.domain().bounds {
            DomainBounds::Bounded(previous) => ParameterRange::new(
                range.start.min(range.end).min(previous.start.min(previous.end)),
                range.start.max(range.end).max(previous.start.max(previous.end)),
            )?,
            _ => range,
        };
        match self {
            Self::Nurbs(curve) => Ok(curve.extended(range)?.into()),
            Self::Projected(curve) => curve.extended(range).map(Self::Projected),
            Self::SurfaceIntersection(pcurve) => {
                Ok(Self::SurfaceIntersection(SurfaceIntersectionPcurve2 {
                    curve: std::sync::Arc::new(pcurve.curve.extended(range)?),
                    first: pcurve.first,
                }))
            }
            Self::SurfaceProjection(pcurve) => {
                // A trimmed projection may end at a periodic chart seam even
                // though its exact source circle or line continues beyond it.
                if !matches!(pcurve.curve.as_ref(), Curve3::SurfaceIntersection(_))
                    && pcurve.curve.domain().contains_range(range, 0.0)
                {
                    let mut points = (0..=32)
                        .map(|index| {
                            let t = range.start + range.span() * index as f64 / 32.0;
                            let point = pcurve.curve.evaluate(t)?.point;
                            let uv = pcurve.surface.project(point, pcurve.tolerance)?.parameters;
                            Ok(Point2::new(uv.0, uv.1))
                        })
                        .collect::<KernelResult<Vec<_>>>()?;
                    super::intersection::unwrap_periodic_points(
                        &mut points,
                        pcurve.surface.domains(),
                    );
                    let guide = super::intersection::smooth_nurbs_guide(&points, range)?;
                    let anchor = (pcurve.range.start + pcurve.range.end) * 0.5;
                    let previous = self.evaluate(anchor)?.point;
                    let current = guide.evaluate(anchor)?.point;
                    let shift =
                        super::whole_periods(pcurve.surface.domains(), previous - current);
                    for point in &mut points {
                        *point += shift;
                    }
                    return Ok(SurfaceProjectionPcurve2::new(
                        (*pcurve.curve).clone(),
                        (*pcurve.surface).clone(),
                        super::intersection::smooth_nurbs_guide(&points, range)?.into(),
                        range,
                        pcurve.tolerance,
                    )?
                    .into());
                }
                let Curve3::SurfaceIntersection(curve) = pcurve.curve.as_ref() else {
                    return Err(invalid(
                        "projection source cannot cover its extension",
                    ));
                };
                let range = ParameterRange::new(
                    range.start.min(pcurve.range.start),
                    range.end.max(pcurve.range.end),
                )?;
                let extended = curve.extended(range)?;
                // Boolean trimming can wrap an intersection's own exact
                // p-curve in a projection. Reuse that retained chart when
                // extending: extrapolating the fitted projection guide can
                // wander by a full period and introduce a seam jump.
                if pcurve.surface.as_ref() == curve.first_surface() {
                    return Ok(extended.exact_pcurves()[0].clone());
                }
                if pcurve.surface.as_ref() == curve.second_surface() {
                    return Ok(extended.exact_pcurves()[1].clone());
                }
                Ok(SurfaceProjectionPcurve2::new(
                    extended.into(),
                    (*pcurve.surface).clone(),
                    pcurve.guide.extended(range)?,
                    range,
                    pcurve.tolerance,
                )?
                .into())
            }
            _ => Err(invalid("curve cannot cover its extension")),
        }
    }

    /// Split quadrature at guide knots: the exact intersection is smooth in
    /// space, but its guide can give it a piecewise-smooth parameter speed.
    pub(crate) fn integration_breakpoints(&self, range: ParameterRange) -> Vec<f64> {
        let mut parameters = Vec::new();
        self.collect_parameter_breaks(&mut parameters);
        parameters.retain(|value| {
            *value > range.start.min(range.end) && *value < range.start.max(range.end)
        });
        parameters.sort_by(f64::total_cmp);
        parameters.dedup();
        parameters
    }

    fn collect_parameter_breaks(&self, parameters: &mut Vec<f64>) {
        match self {
            Self::Nurbs(curve) => parameters.extend_from_slice(curve.knots()),
            Self::Offset(curve) => curve.base().collect_parameter_breaks(parameters),
            Self::Projected(curve) => curve.curve().collect_parameter_breaks(parameters),
            Self::SurfaceIntersection(curve) => {
                curve
                    .curve
                    .first_pcurve()
                    .collect_parameter_breaks(parameters);
                curve
                    .curve
                    .second_pcurve()
                    .collect_parameter_breaks(parameters);
            }
            Self::SurfaceProjection(curve) => {
                curve.guide.collect_parameter_breaks(parameters);
                curve.curve.collect_parameter_breaks(parameters);
            }
            Self::Line(_) | Self::Circle(_) => {}
        }
    }

    // A trim can cross knots in a supporting blend's spine even when its own
    // guide is smooth. Map those knots into the boundary parameter for quadrature.
    pub(crate) fn support_integration_breakpoints(&self, range: ParameterRange) -> KernelResult<Vec<f64>> {
        let Self::SurfaceIntersection(pcurve) = self else { return Ok(Vec::new()); };
        let curve = &pcurve.curve;
        let mut result = Vec::new();
        for (side, surface) in [curve.first_surface(), curve.second_surface()].into_iter().enumerate() {
            for axis in 0..2 {
                let knots = surface.integration_breakpoints(axis);
                if knots.is_empty() { continue; }
                let samples = (0..=64).map(|index| {
                    let parameter = range.start + range.span() * index as f64 / 64.0;
                    Ok((parameter, curve.corrected_parameters(parameter)?[side * 2 + axis]))
                }).collect::<KernelResult<Vec<_>>>()?;
                for knot in knots {
                    for pair in samples.windows(2) {
                        if (pair[0].1 - knot) * (pair[1].1 - knot) >= 0.0 { continue; }
                        let mut low = pair[0];
                        let mut high = pair[1];
                        for _ in 0..48 {
                            let parameter = (low.0 + high.0) * 0.5;
                            let value = curve.corrected_parameters(parameter)?[side * 2 + axis];
                            if (low.1 - knot) * (value - knot) > 0.0 {
                                low = (parameter, value);
                            } else { high = (parameter, value); }
                        }
                        result.push((low.0 + high.0) * 0.5);
                    }
                }
            }
        }
        Ok(result)
    }

    pub fn domain(&self) -> ParameterDomain {
        match self {
            Self::Line(_) => ParameterDomain::unbounded(),
            Self::Circle(_) => ParameterDomain::ANGLE,
            Self::Nurbs(curve) => curve.domain(),
            Self::Offset(curve) => curve.base().domain(),
            Self::Projected(curve) => ParameterDomain { bounds: DomainBounds::Bounded(curve.range()),period:None },
            Self::SurfaceIntersection(curve) => ParameterDomain {
                bounds: DomainBounds::Bounded(curve.curve.range()),
                period: None,
            },
            Self::SurfaceProjection(curve) => curve.domain(),
        }
    }

    pub fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        match self {
            Self::Line(line) => line.evaluate(parameter),
            Self::Circle(circle) => circle.evaluate(parameter),
            Self::Nurbs(curve) => curve.evaluate(parameter),
            Self::Offset(curve) => curve.evaluate(parameter),
            Self::Projected(curve) => curve.evaluate(parameter),
            Self::SurfaceIntersection(curve) => curve.evaluate(parameter),
            Self::SurfaceProjection(curve) => curve.evaluate(parameter),
        }
    }

    pub(crate) fn first_order(&self, parameter: f64) -> KernelResult<(Point2, Vec2)> {
        match self {
            Self::Offset(curve) => curve.first_order(parameter),
            Self::SurfaceProjection(curve) => curve.first_order(parameter),
            _ => self
                .evaluate(parameter)
                .map(|value| (value.point, value.first_derivative)),
        }
    }

    /// Contribution to signed UV area using Green's theorem.
    pub fn signed_area_contribution(
        &self,
        range: ParameterRange,
        tolerance: f64,
    ) -> KernelResult<f64> {
        self.area_contribution_estimate(range, tolerance, false).map(|value| value.0)
    }

    pub(crate) fn precise_area_contribution(&self, range: ParameterRange, tolerance: f64) -> KernelResult<f64> {
        self.area_contribution_estimate(range, tolerance, true).map(|value| value.0)
    }

    /// Area and the numerical budget used to compute it. Validation can accept
    /// a cheap winding only when the whole loop clears the accumulated budget.
    pub(super) fn area_contribution_estimate(&self, range: ParameterRange, tolerance: f64, precise: bool) -> KernelResult<(f64, f64)> {
        let roundoff = if precise { f64::EPSILON } else { f64::EPSILON.sqrt() };
        match self {
            Self::Line(line) => {
                let cross = line.origin.x * line.direction.y - line.origin.y * line.direction.x;
                let error = f64::EPSILON * 8.0 * range.span().abs()
                    * (line.origin.x.abs() * line.direction.y.abs() + line.origin.y.abs() * line.direction.x.abs());
                Ok((0.5 * cross * range.span(), error))
            }
            Self::Circle(circle) => {
                let start = range.start;
                let end = range.end;
                let integral = circle.center.x * circle.radius * (end.sin() - start.sin())
                    + circle.center.y * circle.radius * (start.cos() - end.cos())
                    + circle.radius * circle.radius * range.span();
                let error = f64::EPSILON * 8.0 * (
                    circle.center.x.abs() * circle.radius * (end.sin().abs() + start.sin().abs())
                    + circle.center.y.abs() * circle.radius * (start.cos().abs() + end.cos().abs())
                    + circle.radius * circle.radius * range.span().abs());
                Ok((0.5 * integral, error))
            }
            Self::Nurbs(curve) => {
                let integrand = |parameter: f64| -> KernelResult<f64> {
                    let (point, derivative) = self.first_order(parameter)?;
                    Ok(0.5 * (point.x * derivative.y - point.y * derivative.x))
                };
                let coordinate_scale = curve
                    .control_points()
                    .iter()
                    .map(|point| point.x.abs().max(point.y.abs()))
                    .fold(1.0_f64, f64::max);
                let absolute_tolerance =
                    tolerance.max(roundoff * coordinate_scale.powi(2) * 64.0);
                let mut breaks = curve
                    .knots()
                    .iter()
                    .copied()
                    .filter(|value| {
                        *value >= range.start.min(range.end) && *value <= range.start.max(range.end)
                    })
                    .collect::<Vec<_>>();
                breaks.extend([range.start, range.end]);
                breaks.sort_by(f64::total_cmp);
                breaks.dedup_by(|first, second| (*first - *second).abs() <= f64::EPSILON * 64.0);
                if range.span() < 0.0 {
                    breaks.reverse();
                }
                let interval_tolerance = absolute_tolerance / breaks.len().max(2) as f64;
                let area = breaks.windows(2).try_fold(0.0, |sum, interval| {
                    if (interval[1] - interval[0]).abs() <= f64::EPSILON * 64.0 {
                        Ok::<_, KernelError>(sum)
                    } else {
                        Ok(sum
                            + super::integration::adaptive_gauss_kronrod(
                                interval[0],
                                interval[1],
                                |parameter| Ok([integrand(parameter)?]),
                                [interval_tolerance],
                                24,
                                "adaptive p-curve area integration did not converge",
                            )?[0])
                    }
                })?;
                Ok((area, absolute_tolerance))
            }
            Self::Offset(_) | Self::Projected(_) => {
                Ok((super::integration::adaptive_gauss_kronrod_partitioned(range.start, range.end,
                    |u| { let (p,d) = self.first_order(u)?; Ok([0.5*(p.x*d.y-p.y*d.x)]) },
                    [tolerance],24,"offset area integration did not converge",&self.integration_breakpoints(range))?[0], tolerance))
            }
            Self::SurfaceIntersection(_) | Self::SurfaceProjection(_) => {
                let integrand = |parameter: f64| -> KernelResult<f64> {
                    let (point, derivative) = self.first_order(parameter)?;
                    Ok(0.5 * (point.x * derivative.y - point.y * derivative.x))
                };
                let coordinate_scale = [0.25, 0.5, 0.75]
                    .into_iter()
                    .map(|fraction| range.start + range.span() * fraction)
                    .map(|parameter| self.first_order(parameter))
                    .collect::<KernelResult<Vec<_>>>()?
                    .into_iter()
                    .map(|value| value.0.x.abs().max(value.0.y.abs()))
                    .fold(1.0_f64, f64::max);
                let absolute_tolerance =
                    tolerance.max(roundoff * coordinate_scale.powi(2) * 64.0);
                let integrate = |breaks: &[f64]| super::integration::adaptive_gauss_kronrod_partitioned(
                    range.start,
                    range.end,
                    |parameter| Ok([integrand(parameter)?]),
                    [absolute_tolerance],
                    16,
                    "adaptive exact p-curve area integration did not converge",
                    breaks,
                ).map(|value| value[0]);
                let exact = if precise { integrate(&self.integration_breakpoints(range)) } else {
                    super::integration::adaptive_gauss_kronrod(range.start, range.end,
                        |parameter| Ok([integrand(parameter)?]), [absolute_tolerance], 16,
                        "adaptive exact p-curve area integration did not converge").map(|value| value[0])
                };
                exact.map(|area| (area, absolute_tolerance)).or_else(|error| {
                    // Signed area is used only to establish loop winding. At a
                    // tangential trim endpoint the exact inverse derivative can
                    // be singular even though the retained branch guide has an
                    // unambiguous winding; integrate that guide geometrically
                    // without substituting it for the modeled p-curve.
                    let guide = match self {
                        Self::SurfaceIntersection(pcurve) => pcurve.guide(),
                        Self::SurfaceProjection(pcurve) => pcurve.guide(),
                        _ => unreachable!(),
                    };
                    let mut previous = guide
                        .evaluate(range.start)
                        .map_err(|_| error.clone())?
                        .point;
                    let mut area = 0.0;
                    for index in 1..=64 {
                        let parameter = range.start + range.span() * index as f64 / 64.0;
                        let next = guide.evaluate(parameter).map_err(|_| error.clone())?.point;
                        area += 0.5 * (previous.x * next.y - previous.y * next.x);
                        previous = next;
                    }
                    // The guide fallback has no quadrature error estimate.
                    // It cannot certify a cheap winding; retry precisely.
                    Ok((area, f64::INFINITY))
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveEvaluation {
    pub point: Point3,
    pub first_derivative: Vec3,
    pub second_derivative: Vec3,
}

impl CurveEvaluation {
    pub fn tangent(self) -> KernelResult<Vec3> {
        if self.first_derivative.magnitude2() == 0.0 {
            return Err(geometry_error("curve tangent is singular at the requested parameter"));
        }
        Ok(self.first_derivative.normalize())
    }

    pub fn curvature(self) -> KernelResult<f64> {
        let speed = self.first_derivative.magnitude();
        if speed == 0.0 {
            return Err(geometry_error("curve curvature is undefined at a singular parameter"));
        }
        Ok(self
            .first_derivative
            .cross(self.second_derivative)
            .magnitude()
            / speed.powi(3))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceEvaluation {
    pub point: Point3,
    pub du: Vec3,
    pub dv: Vec3,
    pub duu: Vec3,
    pub duv: Vec3,
    pub dvv: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceCurvature {
    pub gaussian: f64,
    pub mean: f64,
    pub principal: [f64; 2],
}

impl SurfaceEvaluation {
    pub fn normal(self) -> KernelResult<Vec3> {
        let normal = self.du.cross(self.dv);
        if normal.magnitude2() == 0.0 || !finite_vector(normal) {
            return Err(geometry_error("surface derivatives are singular at the requested parameter"));
        }
        Ok(normal.normalize())
    }

    pub fn curvature(self) -> KernelResult<SurfaceCurvature> {
        let normal = self.normal()?;
        let first = [
            self.du.dot(self.du),
            self.du.dot(self.dv),
            self.dv.dot(self.dv),
        ];
        let determinant = first[0] * first[2] - first[1] * first[1];
        if determinant <= 0.0 {
            return Err(geometry_error("surface first fundamental form is singular"));
        }
        let second = [
            normal.dot(self.duu),
            normal.dot(self.duv),
            normal.dot(self.dvv),
        ];
        let gaussian = (second[0] * second[2] - second[1] * second[1]) / determinant;
        let mean = (first[0] * second[2] - 2.0 * first[1] * second[1] + first[2] * second[0])
            / (2.0 * determinant);
        let discriminant = (mean * mean - gaussian).max(0.0).sqrt();
        Ok(SurfaceCurvature {
            gaussian,
            mean,
            principal: [mean - discriminant, mean + discriminant],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection1 {
    pub parameter: f64,
    pub point: Point3,
    pub distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection2 {
    pub parameters: (f64, f64),
    pub point: Point3,
    pub distance: f64,
}

// Damped Gauss-Newton refinement shared by globally seeded offset projections
// and projections that must stay near a caller's UV seed. Keep periodic lifts;
// only nonperiodic bounded coordinates are clamped.
fn refine_surface_projection(
    point: Point3,
    seed: Point2,
    domains: [ParameterDomain; 2],
    first_order: impl Fn(f64, f64) -> KernelResult<(Point3, Vec3, Vec3)>,
    determinant_tolerance: impl Fn(f64, f64) -> f64,
    converged: impl Fn(Vec2) -> bool,
) -> KernelResult<Projection2> {
    let constrain = |uv: Point2| Point2::from(std::array::from_fn::<_, 2, _>(|axis| {
        if domains[axis].period.is_none() {
            if let DomainBounds::Bounded(range) = domains[axis].bounds {
                return uv[axis].clamp(range.start.min(range.end), range.start.max(range.end));
            }
        }
        uv[axis]
    }));
    let mut uv = constrain(seed);
    for _ in 0..48 {
        let (projected, du, dv) = first_order(uv.x, uv.y)?;
        let residual = projected - point;
        let a = du.magnitude2();
        let b = du.dot(dv);
        let c = dv.magnitude2();
        let determinant = a * c - b * b;
        if determinant.abs() <= determinant_tolerance(a, c) { break; }
        let first = -du.dot(residual);
        let second = -dv.dot(residual);
        let step = Vec2::new((first * c - second * b) / determinant, (second * a - first * b) / determinant);
        let mut damping = 1.0;
        let mut next = uv;
        for _ in 0..24 {
            let trial = constrain(uv + step * damping);
            if first_order(trial.x, trial.y)?.0.distance2(point) < residual.magnitude2() {
                next = trial;
                break;
            }
            damping *= 0.5;
        }
        let movement = next - uv;
        uv = next;
        if converged(movement) { break; }
    }
    let projected = first_order(uv.x, uv.y)?.0;
    Ok(Projection2 { parameters: (uv.x, uv.y), point: projected, distance: projected.distance(point) })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Curve3 {
    Line(Line3),
    Circle(Circle3),
    Nurbs(NurbsCurve3),
    SurfaceIntersection(SurfaceIntersectionCurve),
    SurfaceCurve(SurfaceCurve3),
}

impl Curve3 {
    fn collect_parameter_breaks(&self, parameters: &mut Vec<f64>) {
        match self {
            Self::Nurbs(curve) => parameters.extend_from_slice(curve.knots()),
            Self::SurfaceIntersection(curve) => {
                curve.first_pcurve().collect_parameter_breaks(parameters);
                curve.second_pcurve().collect_parameter_breaks(parameters);
            }
            Self::SurfaceCurve(curve) => curve.pcurve().collect_parameter_breaks(parameters),
            Self::Line(_) | Self::Circle(_) => {}
        }
    }

    pub fn domain(&self) -> ParameterDomain {
        match self {
            Self::Line(_) => ParameterDomain::unbounded(),
            Self::Circle(_) => ParameterDomain::ANGLE,
            Self::Nurbs(curve) => curve.domain(),
            Self::SurfaceIntersection(curve) => ParameterDomain {
                bounds: DomainBounds::Bounded(curve.range()),
                period: None,
            },
            Self::SurfaceCurve(curve) => ParameterDomain {
                bounds: DomainBounds::Bounded(curve.range()),
                period: match curve.pcurve() {
                    Curve2::Line(line) => curve
                        .surface()
                        .parameter_periods((line.origin().x, line.origin().y))
                        .ok()
                        .and_then(|periods| {
                            (0..2).find_map(|axis| {
                                (line.direction()[1 - axis] == 0.0 && line.direction()[axis] != 0.0)
                                    .then(|| {
                                        periods[axis]
                                            .map(|period| period / line.direction()[axis].abs())
                                    })
                                    .flatten()
                            })
                        }),
                    _ => None,
                },
            },
        }
    }

    pub fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        match self {
            Self::Line(line) => line.evaluate(parameter),
            Self::Circle(circle) => circle.evaluate(parameter),
            Self::Nurbs(curve) => curve.evaluate(parameter),
            Self::SurfaceIntersection(curve) => curve.evaluate(parameter),
            Self::SurfaceCurve(curve) => curve.evaluate(parameter),
        }
    }

    pub(crate) fn third_derivative(&self, parameter: f64) -> KernelResult<Vec3> {
        match self {
            Self::Line(_) => Ok(Vec3::new(0.0, 0.0, 0.0)),
            Self::Circle(_) => Ok(-self.evaluate(parameter)?.first_derivative),
            Self::Nurbs(curve) => {
                let value = curve.evaluate(parameter)?;
                let parameter = parameter.clamp(curve.knots[curve.degree], curve.knots[curve.control_points.len()]);
                let [weight, first, second, third] = homogeneous_curve_jet(
                    curve.degree, &curve.knots, curve.control_points.len(), parameter,
                    |index| {
                        let point = curve.control_points[index];
                        let weight = curve.weights[index];
                        Vec4::new(point.x * weight, point.y * weight, point.z * weight, weight)
                    },
                );
                Ok((third.truncate()
                    - value.point.to_vec() * third.w
                    - value.first_derivative * (3.0 * second.w)
                    - value.second_derivative * (3.0 * first.w)) / weight.w)
            }
            // Implicit curves have no closed-form jerk; difference their
            // curvature within the certified domain
            _ => {
                let step = 1.0e-5 * parameter.abs().max(1.0);
                let (mut before, mut after) = (parameter - step, parameter + step);
                if let DomainBounds::Bounded(range) = self.domain().bounds {
                    before = before.max(range.start.min(range.end));
                    after = after.min(range.start.max(range.end));
                }
                let ahead = self.evaluate(after)?.second_derivative;
                let behind = self.evaluate(before)?.second_derivative;
                Ok((ahead - behind) / (after - before))
            }
        }
    }

    /// Parameters inside `range` where the curve's smoothness may drop.
    pub(crate) fn integration_breakpoints(&self, range: ParameterRange) -> Vec<f64> {
        let mut parameters = Vec::new();
        self.collect_parameter_breaks(&mut parameters);
        parameters.retain(|value| {
            *value > range.start.min(range.end) && *value < range.start.max(range.end)
        });
        parameters.sort_by(f64::total_cmp);
        parameters.dedup();
        parameters
    }

    pub(crate) fn first_order(&self, parameter: f64) -> KernelResult<(Point3, Vec3)> {
        match self {
            Self::SurfaceIntersection(curve) => curve.first_order(parameter),
            Self::SurfaceCurve(curve) => curve.first_order(parameter),
            _ => self
                .evaluate(parameter)
                .map(|value| (value.point, value.first_derivative)),
        }
    }

    /// Closest point on a retained edge interval, including its endpoints.
    /// Searching the whole basis curve can select another branch or periodic
    /// copy and miss a nearby boundary of a thin Boolean region.
    pub(crate) fn project_on_trim(
        &self,
        point: Point3,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Projection1> {
        let low = range.start.min(range.end);
        let high = range.start.max(range.end);
        let mut seeds = vec![low, high];
        if matches!(self, Self::Line(_) | Self::Circle(_)) {
            // On a circle's axis every retained parameter minimizes distance.
            // A trim still has a well-defined closest point at either endpoint.
            let on_axis = matches!(self, Self::Circle(circle) if {
                let delta = point - circle.center();
                delta.dot(circle.x_radius()) == 0.0 && delta.dot(circle.y_radius()) == 0.0
            });
            let parameter = if on_axis { low } else { self.project(point, tolerance)?.parameter };
            let parameter = self.domain().nearest(parameter, (low + high) * 0.5);
            seeds.push(parameter.clamp(low, high));
        } else {
            seeds.extend((1..32).map(|i| low + (high - low) * i as f64 / 32.0));
            self.collect_parameter_breaks(&mut seeds);
        }
        seeds.retain(|parameter| *parameter >= low && *parameter <= high);
        // Nested intersection guides share knots. Each distinct parameter
        // needs one exact evaluation, irrespective of how many guides use it.
        seeds.sort_by(f64::total_cmp);
        seeds.dedup();
        let mut best = None;
        let mut previous = None;
        let mut minima = Vec::new();
        for parameter in seeds {
            let (projected, tangent) = self.first_order(parameter)?;
            let gradient = (projected - point).dot(tangent);
            if let Some((last_parameter, last_gradient)) = previous {
                if last_gradient < 0.0 && gradient > 0.0 { minima.push((last_parameter, parameter)); }
            }
            previous = Some((parameter, gradient));
            let distance = projected.distance(point);
            if best
                .as_ref()
                .is_none_or(|value: &Projection1| distance < value.distance)
            {
                best = Some(Projection1 {
                    parameter,
                    point: projected,
                    distance,
                });
            }
        }
        let best = best.ok_or_else(|| invalid("curve trim has no projection seed"))?;
        if matches!(self, Self::Line(_) | Self::Circle(_)) {
            return Ok(best);
        }
        let mut best = refine_curve_projection(point, range, tolerance, best, |parameter| {
            self.first_order(parameter)
        })?;
        // A focal configuration makes distance-only descent arbitrarily slow.
        // Bracket the stationary minima to retain the closest interior foot.
        for (mut low, mut high) in minima {
            for _ in 0..48 {
                let middle = (low + high) * 0.5;
                let (value, tangent) = self.first_order(middle)?;
                if (value - point).dot(tangent) < 0.0 { low = middle; }
                else { high = middle; }
            }
            let parameter = (low + high) * 0.5;
            let projected = self.first_order(parameter)?.0;
            let distance = projected.distance(point);
            if distance < best.distance {
                best = Projection1 { parameter, point: projected, distance };
            }
        }
        Ok(best)
    }

    pub fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection1> {
        match self {
            Self::Line(line) => line.project(point),
            Self::Circle(circle) => circle.project(point),
            Self::Nurbs(curve) => curve.project(point, tolerance),
            Self::SurfaceIntersection(curve) => curve.project(point),
            Self::SurfaceCurve(curve) => curve.project(point),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Surface3 {
    Plane(Plane3),
    Cylinder(Cylinder3),
    Cone(Cone3),
    Sphere(Sphere3),
    Torus(Torus3),
    Revolved(RevolvedSurface3),
    Swept(super::SweptSurface3),
    Nurbs(NurbsSurface3),
    Offset(OffsetSurface3),
    RollingBall(RollingBallSurface3),
    Blend(EdgeBlendSurface3),
    Ruled(EdgeBlendSurface3),
}

impl Surface3 {
    /// A short human-readable name of the support type, for diagnostics.
    pub(crate) fn kind_name(&self) -> &'static str {
        match self {
            Self::Plane(_) => "plane",
            Self::Cylinder(_) => "cylinder",
            Self::Cone(_) => "cone",
            Self::Sphere(_) => "sphere",
            Self::Torus(_) => "torus",
            Self::Revolved(_) => "revolved",
            Self::Swept(_) => "swept",
            Self::Nurbs(_) => "NURBS",
            Self::Offset(_) => "offset",
            Self::RollingBall(_) => "rolling-ball",
            Self::Blend(_) => "edge blend",
            Self::Ruled(_) => "ruled",
        }
    }

    pub(crate) fn integration_breakpoints(&self, axis: usize) -> Vec<f64> {
        match self {
            Self::Nurbs(surface) => {
                let (u, v) = surface.knots();
                if axis == 0 { u.to_vec() } else { v.to_vec() }
            }
            Self::RollingBall(surface) | Self::Blend(surface) if axis == 0 => {
                let mut knots = Vec::new();
                surface.center_curve.collect_parameter_breaks(&mut knots);
                surface.first_pcurve.collect_parameter_breaks(&mut knots);
                surface.second_pcurve.collect_parameter_breaks(&mut knots);
                knots.sort_by(f64::total_cmp);
                knots.dedup();
                knots
            }
            Self::Offset(surface) => surface.base().integration_breakpoints(axis),
            _ => Vec::new(),
        }
    }
    fn parameter_periods(&self, uv: (f64, f64)) -> KernelResult<[Option<f64>; 2]> {
        let mut periods = self.domains().map(|domain| domain.period);
        match self {
            Self::RollingBall(surface) | Self::Blend(surface) => periods[1] = surface.blend_period(uv.0)?,
            Self::Offset(surface) => return surface.base.parameter_periods(uv),
            _ => {}
        }
        Ok(periods)
    }

    /// Construct a spatial boundary in the p-curve's parameterization. Use
    /// analytic geometry only when it follows from the defining surface and
    /// p-curve; no sampled recognition or geometric fitting is performed.
    pub(crate) fn boundary_curve(
        &self,
        pcurve: Curve2,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Curve3> {
        if let Curve2::SurfaceIntersection(intersection) = &pcurve {
            if intersection.surface() == self {
                // Keep the defining support pair visible to overlap detection.
                // Wrapping this exact image would hide coincident intersections.
                return Ok(intersection.spatial_curve());
            }
        }
        // A projected latitude circle is still an exact circle after a torus
        // offset. Preserve its periodic support so a seam endpoint can move past
        // its old trim when the neighboring face is offset.
        if let (Surface3::Torus(torus), Curve2::SurfaceProjection(projection)) = (self, &pcurve) {
            if let (Surface3::Torus(parent), Curve3::Circle(circle)) =
                (projection.surface(), projection.curve())
            {
                let axis = parent.normal();
                let delta = circle.center() - parent.center();
                if torus.center() == parent.center()
                    && torus.x_major() == parent.x_major()
                    && torus.y_major() == parent.y_major()
                    && (delta - axis * delta.dot(axis)).magnitude() <= tolerance.linear
                    && circle.normal().cross(axis).magnitude() <= tolerance.angular
                {
                    let v = pcurve.evaluate((range.start + range.end) * 0.5)?.point.y;
                    let radius = torus.major_radius() + torus.minor_radius() * v.cos();
                    return super::Circle3::new(
                        torus.center() + axis * (torus.minor_radius() * v.sin()),
                        circle.x_radius().normalize() * radius,
                        circle.y_radius().normalize() * radius,
                        tolerance,
                    )
                    .map(Into::into);
                }
            }
        }
        if let (Self::Plane(plane), Curve2::Circle(circle)) = (self, &pcurve) {
            let u = plane.u();
            let v = plane.v();
            if u.dot(v).abs() <= tolerance.angular * u.magnitude() * v.magnitude()
                && (u.magnitude() - v.magnitude()).abs() <= tolerance.linear {
                return Circle3::new(
                    plane.origin() + u * circle.center().x + v * circle.center().y,
                    u * circle.radius(), v * circle.radius(), tolerance,
                ).map(Curve3::from);
            }
        }
        if let (Self::Plane(plane), Curve2::Line(_) | Curve2::Nurbs(_)) = (self, &pcurve) {
            return plane.lift_curve(&pcurve, tolerance);
        }
        if let Curve2::SurfaceProjection(projected) = &pcurve {
            if projected.surface() == self {
                return Ok(projected.curve().clone());
            }
            // Parallel offset planes retain their UV basis. Their boundary is
            // the source analytic curve translated between the two origins.
            if let (Self::Plane(from), Self::Plane(to)) = (projected.surface(), self) {
                if from.u() == to.u() && from.v() == to.v() {
                    let translation = to.origin() - from.origin();
                    match projected.curve() {
                        Curve3::Line(line) => {
                            return Line3::new(line.origin() + translation, line.direction())
                                .map(Curve3::from);
                        }
                        Curve3::Circle(circle) => {
                            return Circle3::new(
                                circle.center() + translation,
                                circle.x_radius(),
                                circle.y_radius(),
                                tolerance,
                            )
                            .map(Curve3::from);
                        }
                        Curve3::Nurbs(curve) => {
                            return NurbsCurve3::new(curve.degree(),
                                curve.control_points().iter().map(|point| *point + translation).collect(),
                                curve.weights().to_vec(), curve.knots().to_vec()).map(Curve3::from);
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Curve2::Line(line) = &pcurve {
            let origin = line.origin();
            let direction = line.direction();
            match self {
                Self::Blend(surface) => {
                    if let Some(section) = surface.quintic_section(line, range)? {
                        return Ok(section.into());
                    }
                }
                Self::Nurbs(surface) if direction.x == 0.0 || direction.y == 0.0 => {
                    let (axis, constant, origin, rate) = if direction.x == 0.0 {
                        (0, origin.x, origin.y, direction.y)
                    } else { (1, origin.y, origin.x, direction.x) };
                    return surface.isoparametric_curve(axis, constant)?
                        .reparameterized(origin, rate).map(Curve3::from);
                }
                Self::Cylinder(_) | Self::Cone(_) if direction.x == 0.0 => {
                    let value = self.evaluate(origin.x, origin.y)?;
                    return Line3::new(value.point, value.dv * direction.y).map(Curve3::from);
                }
                _ => {}
            }
            // Circle3 uses radians. Unit angular rates preserve the edge and
            // coedge parameters, including reversed traversal and seam crossings.
            let circular_frame = if direction.y == 0.0 && direction.x.abs() == 1.0 {
                match self {
                    Self::Cylinder(cylinder) => Some((
                        cylinder.origin() + cylinder.axis() * origin.y,
                        cylinder.x_radius(),
                        cylinder.y_radius(),
                    )),
                    Self::Cone(cone) if origin.y != 0.0 => Some((
                        cone.apex() + cone.axis() * origin.y,
                        cone.x_radius() * origin.y,
                        cone.y_radius() * origin.y,
                    )),
                    Self::Sphere(sphere) => Some((
                        sphere.center() + sphere.z_radius() * origin.y.sin(),
                        sphere.x_radius() * origin.y.cos(),
                        sphere.y_radius() * origin.y.cos(),
                    )),
                    Self::Revolved(surface) => {
                        let profile = surface.profile().evaluate(origin.y)?.point;
                        let frame = surface.frame();
                        Some((
                            frame.origin() + frame.z() * profile.y,
                            frame.x() * profile.x,
                            frame.y() * profile.x,
                        ))
                    }
                    Self::Torus(torus) => {
                        let scale =
                            1.0 + torus.minor_radius() * origin.y.cos() / torus.major_radius();
                        Some((
                            torus.center() + torus.normal() * (torus.minor_radius() * origin.y.sin()),
                            torus.x_major() * scale,
                            torus.y_major() * scale,
                        ))
                    }
                    _ => None,
                }
                .map(|(center, x, y)| (center, x, y, origin.x, direction.x))
            } else if direction.x == 0.0 && direction.y.abs() == 1.0 {
                match self {
                    Self::Sphere(sphere) => Some((
                        sphere.center(),
                        sphere.x_radius() * origin.x.cos() + sphere.y_radius() * origin.x.sin(),
                        sphere.z_radius(),
                    )),
                    Self::Torus(torus) => {
                        let radial =
                            torus.x_major() * origin.x.cos() + torus.y_major() * origin.x.sin();
                        Some((
                            torus.center() + radial,
                            radial * (torus.minor_radius() / torus.major_radius()),
                            torus.normal() * torus.minor_radius(),
                        ))
                    }
                    _ => None,
                }
                .map(|(center, x, y)| (center, x, y, origin.y, direction.y))
            } else {
                None
            };
            if let Some((center, x, y, angle, sense)) = circular_frame {
                return Circle3::new(
                    center,
                    x * angle.cos() + y * angle.sin(),
                    (-x * angle.sin() + y * angle.cos()) * sense,
                    tolerance,
                )
                .map(Curve3::from);
            }
        }
        SurfaceCurve3::new(self.clone(), pcurve, range, tolerance).map(Curve3::from)
    }

    pub(crate) fn first_order(&self, u: f64, v: f64) -> KernelResult<(Point3, Vec3, Vec3)> {
        match self {
            Self::Nurbs(surface) => surface.first_order(u, v),
            Self::Swept(surface) => surface.first_order(u, v),
            Self::Offset(surface) => surface.first_order(u, v),
            Self::RollingBall(surface) | Self::Blend(surface) | Self::Ruled(surface) => surface.first_order(u, v),
            _ => self
                .evaluate(u, v)
                .map(|value| (value.point, value.du, value.dv)),
        }
    }

    pub fn domains(&self) -> [ParameterDomain; 2] {
        match self {
            Self::Plane(_) => [ParameterDomain::unbounded(), ParameterDomain::unbounded()],
            Self::Cylinder(_) | Self::Cone(_) => {
                [ParameterDomain::ANGLE, ParameterDomain::unbounded()]
            }
            Self::Sphere(_) => [
                ParameterDomain::ANGLE,
                ParameterDomain {
                    bounds: DomainBounds::Bounded(ParameterRange {
                        start: -std::f64::consts::FRAC_PI_2,
                        end: std::f64::consts::FRAC_PI_2,
                    }),
                    period: None,
                },
            ],
            Self::Torus(_) => [ParameterDomain::ANGLE, ParameterDomain::ANGLE],
            Self::Revolved(surface) => [ParameterDomain::ANGLE, surface.profile_domain()],
            Self::Swept(surface) => surface.domains(),
            Self::Nurbs(surface) => surface.domains(),
            Self::Offset(surface) => surface.base.domains(),
            Self::RollingBall(surface) | Self::Blend(surface) | Self::Ruled(surface) => [
                ParameterDomain {
                    bounds: DomainBounds::Bounded(surface.center_range()),
                    period: surface.center_period,
                },
                ParameterDomain::unbounded(),
            ],
        }
    }

    pub fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        match self {
            Self::Plane(plane) => plane.evaluate(u, v),
            Self::Cylinder(cylinder) => cylinder.evaluate(u, v),
            Self::Cone(cone) => cone.evaluate(u, v),
            Self::Sphere(sphere) => sphere.evaluate(u, v),
            Self::Torus(torus) => torus.evaluate(u, v),
            Self::Revolved(surface) => surface.evaluate(u, v),
            Self::Swept(surface) => surface.evaluate(u, v),
            Self::Nurbs(surface) => surface.evaluate(u, v),
            Self::Offset(surface) => surface.evaluate(u, v),
            Self::RollingBall(surface) | Self::Blend(surface) | Self::Ruled(surface) => surface.evaluate(u, v),
        }
    }

    pub fn normal(&self, u: f64, v: f64) -> KernelResult<Vec3> {
        match self {
            Self::RollingBall(blend) => blend.normal(u, v),
            Self::Sphere(sphere) => {
                let radial = sphere.evaluate(u, v)?.point - sphere.center();
                if radial.magnitude2() == 0.0 {
                    return Err(geometry_error("sphere normal is singular at its center"));
                }
                Ok(radial.normalize())
            }
            _ => {
                let (_, du, dv) = self.first_order(u, v)?;
                let normal = du.cross(dv);
                if normal.magnitude2() == 0.0 || !finite_vector(normal) {
                    return Err(geometry_error("surface derivatives are singular at the requested parameter"));
                }
                Ok(normal.normalize())
            }
        }
    }

    pub fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection2> {
        match self {
            Self::Plane(plane) => plane.project(point),
            Self::Cylinder(cylinder) => cylinder.project(point),
            Self::Cone(cone) => cone.project(point),
            Self::Sphere(sphere) => sphere.project(point),
            Self::Torus(torus) => torus.project(point),
            Self::Revolved(surface) => surface.project(point),
            Self::Swept(surface) => surface.project(point),
            Self::Nurbs(surface) => surface.project(point, tolerance),
            Self::Offset(surface) => surface.project(point),
            Self::RollingBall(surface) | Self::Blend(surface) | Self::Ruled(surface) => surface.project(point),
        }
    }

    pub(crate) fn project_near(
        &self, point: Point3, seed: Point2, tolerance: ModelTolerance,
    ) -> KernelResult<Projection2> {
        refine_surface_projection(
            point, seed, self.domains(),
            |u, v| self.first_order(u, v),
            |a, c| f64::EPSILON * a.max(c).powi(2) * 256.0,
            |step| step.magnitude() <= tolerance.parameter,
        )
    }

    pub fn offset(&self, distance: f64, tolerance: ModelTolerance) -> KernelResult<Self> {
        match self {
            Self::Plane(plane) => Ok(Plane3::new(
                plane.origin() + plane.normal() * distance,
                plane.u(),
                plane.v(),
                tolerance,
            )?
            .into()),
            Self::Cylinder(cylinder) => {
                // `Cylinder3` permits its axial direction to be parallel or
                // antiparallel to the radial frame normal.  The parameter-space
                // surface normal (`du × dv`) points radially outward in the
                // first case and inward in the second, so an offset along that
                // normal changes the geometric radius with the same sign.
                let normal_sign = if cylinder
                    .axis()
                    .dot(cylinder.x_radius().cross(cylinder.y_radius()))
                    >= 0.0
                {
                    1.0
                } else {
                    -1.0
                };
                let radius = cylinder.radius() + distance * normal_sign;
                if radius <= tolerance.linear_at_scale(cylinder.radius()) {
                    return Err(invalid("cylinder offset collapses its radius"));
                }
                let scale = radius / cylinder.radius();
                Ok(Cylinder3::new(
                    cylinder.origin(),
                    cylinder.axis(),
                    cylinder.x_radius() * scale,
                    cylinder.y_radius() * scale,
                    tolerance,
                )?
                .into())
            }
            Self::Sphere(sphere) => {
                let source_radius = sphere.radius();
                let radius = source_radius + distance;
                if radius <= tolerance.linear_at_scale(source_radius) {
                    return Err(invalid("sphere offset collapses its radius"));
                }
                let scale = radius / source_radius;
                Ok(Sphere3::new(
                    sphere.center(),
                    sphere.x_radius() * scale,
                    sphere.y_radius() * scale,
                    sphere.z_radius() * scale,
                    tolerance,
                )?
                .into())
            }
            Self::Torus(torus) => {
                let minor_radius = torus.minor_radius() + distance;
                Ok(Torus3::new(
                    torus.center(),
                    torus.x_major(),
                    torus.y_major(),
                    minor_radius,
                    tolerance,
                )?
                .into())
            }
            _ => OffsetSurface3::new(self.clone(), distance, tolerance).map(Self::Offset),
        }
    }
}

impl From<RollingBallSurface3> for Surface3 {
    fn from(value: RollingBallSurface3) -> Self {
        if value.is_ruled() {
            Self::Ruled(value)
        } else if !value.is_circular() {
            Self::Blend(value)
        } else {
            Self::RollingBall(value)
        }
    }
}

macro_rules! variant_from {
    ($target:ident { $($variant:ident($source:ty)),* $(,)? }) => {$(
        impl From<$source> for $target {
            fn from(value: $source) -> Self {
                Self::$variant(value)
            }
        }
    )*};
}
variant_from!(Curve2 { Line(Line2), Circle(Circle2), Nurbs(NurbsCurve2), SurfaceProjection(SurfaceProjectionPcurve2) });
variant_from!(Curve3 { Line(Line3), Circle(Circle3), Nurbs(NurbsCurve3), SurfaceIntersection(SurfaceIntersectionCurve), SurfaceCurve(SurfaceCurve3) });
variant_from!(Surface3 { Plane(Plane3), Cylinder(Cylinder3), Cone(Cone3), Sphere(Sphere3), Torus(Torus3), Revolved(RevolvedSurface3), Nurbs(NurbsSurface3), Offset(OffsetSurface3) });
