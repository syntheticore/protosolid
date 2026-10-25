//! Exact surface intersection curves, curves on surfaces, and their p-curves.
use super::*;

/// Guide data fixing the parameterization of an intersection curve.
pub(super) struct IntersectionGauge {
    pub(super) guide: [f64; 4],
    pub(super) derivative: [f64; 4],
    pub(super) second: [f64; 4],
    pub(super) tangent: [f64; 4],
    pub(super) tangent_derivative: [f64; 4],
}

pub(super) fn dot4(a: [f64; 4], b: [f64; 4]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

pub(super) fn sub4(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    std::array::from_fn(|index| a[index] - b[index])
}

/// A general regular intersection of two parametric surfaces.
///
/// The dual parameter-space guides provide a stable global parameterization;
/// evaluation corrects their four parameters against the two retained parent
/// surfaces. Consequently the modeling curve is the implicit common zero set,
/// rather than a polyline or a sampled 3D replacement.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceIntersectionCurve {
    pub(super) first_surface: Box<Surface3>,
    pub(super) second_surface: Box<Surface3>,
    pub(super) first_pcurve: Curve2,
    pub(super) second_pcurve: Curve2,
    pub(super) range: ParameterRange,
    pub(super) tolerance: ModelTolerance,
    pub(super) achieved_error: f64,
    pub(super) evaluations: ParameterEvaluationCache<IntersectionEvaluation>,
}

/// Evaluations belong to immutable geometry and can be shared by its edge,
/// p-curves and blend supports. Keys are exact parameter bits, never rounded.
/// Keep the cache bounded and exclude its incidental contents from identity.
pub(super) type IntersectionEvaluation = ([f64; 4], Option<[f64; 4]>, Option<(Point3, Vec3)>);

#[derive(Clone)]
pub(super) struct ParameterEvaluationCache<T>(
    pub(super) std::sync::Arc<std::sync::Mutex<ParameterCacheEntries<T>>>,
);

pub(super) const PARAMETER_CACHE_SETS: usize = 128;
pub(super) const PARAMETER_CACHE_WAYS: usize = 4;

/// Small set-associative cache: constant lookup cost, no per-entry allocation,
/// and local eviction instead of periodically discarding every reusable value.
/// Storage is allocated only on the first successful evaluation.
pub(super) struct ParameterCacheEntries<T> {
    pub(super) sets: Vec<Option<Box<ParameterCacheSet<T>>>>,
}

pub(super) struct ParameterCacheSet<T> {
    pub(super) slots: [Option<(u64, T)>; PARAMETER_CACHE_WAYS],
    pub(super) next: usize,
}

impl<T> Default for ParameterCacheEntries<T> {
    fn default() -> Self {
        Self { sets: Vec::new() }
    }
}

pub(super) fn parameter_cache_set(mut bits: u64) -> usize {
    // Avalanche both the exponent and mantissa. Neighboring floating-point
    // samples often differ only in their low bits, or share many trailing zeros.
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((bits ^ (bits >> 31)) as usize) & (PARAMETER_CACHE_SETS - 1)
}

impl<T> Default for ParameterEvaluationCache<T> {
    fn default() -> Self {
        Self(std::sync::Arc::default())
    }
}
impl<T> std::fmt::Debug for ParameterEvaluationCache<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ParameterEvaluationCache")
    }
}
impl<T> PartialEq for ParameterEvaluationCache<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl<T: Copy> ParameterEvaluationCache<T> {
    pub(super) fn get(&self, parameter: f64) -> Option<T> {
        let entries = self.0.lock().ok()?;
        let bits = parameter.to_bits();
        entries.sets.get(parameter_cache_set(bits))?.as_ref()?
            .slots
            .iter()
            .flatten()
            .find(|(key, _)| *key == bits)
            .map(|(_, value)| *value)
    }
    pub(super) fn insert(&self, parameter: f64, value: T) {
        if let Ok(mut entries) = self.0.lock() {
            if entries.sets.is_empty() {
                entries.sets.resize_with(PARAMETER_CACHE_SETS, || None);
            }
            let bits = parameter.to_bits();
            // Most newly created supports are sampled at only a few parameters.
            // Allocate four entries at a time instead of reserving 512 large
            // differential records for every short-lived geometry object.
            let set = entries.sets[parameter_cache_set(bits)].get_or_insert_with(|| {
                Box::new(ParameterCacheSet {
                    slots: [None; PARAMETER_CACHE_WAYS],
                    next: 0,
                })
            });
            if let Some(slot) = set.slots.iter_mut()
                .find(|slot| slot.is_some_and(|(key, _)| key == bits))
            {
                *slot = Some((bits, value));
                return;
            }
            let way = set.next;
            set.slots[way] = Some((bits, value));
            set.next = (way + 1) % PARAMETER_CACHE_WAYS;
        }
    }
}

/// Exact curve obtained by evaluating a parameter-space curve on a retained
/// surface support.  This is the natural representation for a fillet contact
/// where the parent and blend surfaces are tangent rather than transversely
/// intersecting.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceCurve3 {
    pub(super) surface: Box<Surface3>,
    pub(super) pcurve: Curve2,
    pub(super) range: ParameterRange,
    pub(super) tolerance: ModelTolerance,
}

impl SurfaceCurve3 {
    pub fn new(
        surface: Surface3,
        pcurve: Curve2,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        if !pcurve.domain().contains_range(range, tolerance.parameter) {
            return Err(invalid(
                "surface curve p-curve does not cover its trim range",
            ));
        }
        let curve = Self {
            surface: Box::new(surface),
            pcurve,
            range,
            tolerance,
        };
        for parameter in [range.start, (range.start + range.end) * 0.5, range.end] {
            curve.evaluate(parameter)?;
        }
        Ok(curve)
    }

    pub fn surface(&self) -> &Surface3 {
        &self.surface
    }

    pub fn pcurve(&self) -> &Curve2 {
        &self.pcurve
    }

    pub fn range(&self) -> ParameterRange {
        self.range
    }

    pub(super) fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        let uv = self.pcurve.evaluate(parameter)?;
        let surface = self.surface.evaluate(uv.point.x, uv.point.y)?;
        Ok(CurveEvaluation {
            point: surface.point,
            first_derivative: surface.du * uv.first_derivative.x
                + surface.dv * uv.first_derivative.y,
            second_derivative: surface.duu * uv.first_derivative.x.powi(2)
                + surface.duv * (2.0 * uv.first_derivative.x * uv.first_derivative.y)
                + surface.dvv * uv.first_derivative.y.powi(2)
                + surface.du * uv.second_derivative.x
                + surface.dv * uv.second_derivative.y,
        })
    }

    pub(super) fn first_order(&self, parameter: f64) -> KernelResult<(Point3, Vec3)> {
        let (uv, uv_tangent) = self.pcurve.first_order(parameter)?;
        let (point, du, dv) = self.surface.first_order(uv.x, uv.y)?;
        Ok((point, du * uv_tangent.x + dv * uv_tangent.y))
    }

    pub(super) fn project(&self, point: Point3) -> KernelResult<Projection1> {
        let mut parameter = (0..=48)
            .map(|index| self.range.start + self.range.span() * index as f64 / 48.0)
            .map(|parameter| {
                self.evaluate(parameter)
                    .map(|value| (parameter, value.point.distance2(point)))
            })
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|candidate| candidate.0)
            .ok_or_else(|| invalid("surface curve projection has no seed"))?;
        for _ in 0..32 {
            let value = self.evaluate(parameter)?;
            let residual = value.point - point;
            let derivative =
                value.first_derivative.magnitude2() + residual.dot(value.second_derivative);
            if derivative.abs() <= f64::EPSILON * 256.0 {
                break;
            }
            let next = (parameter - residual.dot(value.first_derivative) / derivative).clamp(
                self.range.start.min(self.range.end),
                self.range.start.max(self.range.end),
            );
            if (next - parameter).abs() <= self.tolerance.parameter {
                parameter = next;
                break;
            }
            parameter = next;
        }
        let projected = self.evaluate(parameter)?.point;
        Ok(Projection1 {
            parameter,
            point: projected,
            distance: projected.distance(point),
        })
    }
}

/// Exact parameter-space representation of one retained side of a regular
/// surface/surface intersection.  It shares the same Newton-corrected implicit
/// curve as the 3D edge, so a BREP coedge never stores a fitted UV surrogate.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceIntersectionPcurve2 {
    pub(super) curve: Box<SurfaceIntersectionCurve>,
    pub(super) first: bool,
}

impl SurfaceIntersectionPcurve2 {
    pub(crate) fn surface(&self) -> &Surface3 {
        if self.first {
            self.curve.first_surface()
        } else {
            self.curve.second_surface()
        }
    }

    pub(crate) fn guide(&self) -> &Curve2 {
        if self.first {
            self.curve.first_pcurve()
        } else {
            self.curve.second_pcurve()
        }
    }

    pub(crate) fn spatial_curve(&self) -> Curve3 {
        Curve3::SurfaceIntersection((*self.curve).clone())
    }

    pub(crate) fn lies_on(&self, surface: &Surface3) -> bool {
        self.curve.first_surface() == surface || self.curve.second_surface() == surface
    }

    pub(super) fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        let parameters = self.curve.corrected_parameters(parameter)?;
        let derivative = self.curve.parameter_derivative(parameter, parameters)?;
        let (point, first_derivative) = if self.first {
            (
                Point2::new(parameters[0], parameters[1]),
                Vec2::new(derivative[0], derivative[1]),
            )
        } else {
            (
                Point2::new(parameters[2], parameters[3]),
                Vec2::new(derivative[2], derivative[3]),
            )
        };
        Ok(Curve2Evaluation {
            point,
            first_derivative,
            second_derivative: Vec2::new(0.0, 0.0),
        })
    }
}

/// Exact parameter-space image of a retained 3D curve on a surface.
///
/// The guide is used only to select a local inverse branch. Every evaluation
/// corrects the parameters against the exact curve and surface and obtains the
/// derivatives by differentiating that identity. This covers tangential shared
/// boundaries, where a regular surface/surface intersection is singular.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceProjectionPcurve2 {
    pub(super) curve: Box<Curve3>,
    pub(super) surface: Box<Surface3>,
    pub(super) guide: Box<Curve2>,
    pub(super) range: ParameterRange,
    pub(super) tolerance: ModelTolerance,
    pub(super) evaluations: ParameterEvaluationCache<(Point2, Vec2)>,
}

impl SurfaceProjectionPcurve2 {
    pub fn new(
        curve: Curve3,
        surface: Surface3,
        guide: Curve2,
        range: ParameterRange,
        tolerance: ModelTolerance,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !curve.domain().contains_range(range, tolerance.parameter)
            || !guide.domain().contains_range(range, tolerance.parameter)
        {
            return Err(invalid(
                "surface-projection curve and guide must cover the trim range",
            ));
        }
        let value = Self {
            curve: Box::new(curve),
            surface: Box::new(surface),
            guide: Box::new(guide),
            range,
            tolerance,
            evaluations: ParameterEvaluationCache::default(),
        };
        for parameter in [range.start, (range.start + range.end) * 0.5, range.end] {
            value.evaluate(parameter)?;
        }
        Ok(value)
    }

    pub fn curve(&self) -> &Curve3 {
        &self.curve
    }

    pub fn surface(&self) -> &Surface3 {
        &self.surface
    }

    pub fn guide(&self) -> &Curve2 {
        &self.guide
    }

    pub fn range(&self) -> ParameterRange {
        self.range
    }

    pub(super) fn continuation_period(&self) -> Option<f64> {
        self.curve.domain().period
            .filter(|period| self.range.span().abs() + self.tolerance.parameter >= *period)
    }

    pub(super) fn domain(&self) -> ParameterDomain {
        // A closed spatial curve need not close in the surface's UV chart:
        // a circle on a cylinder advances u by one full turn. Its projection
        // continues by a UV translation rather than periodic evaluation.
        ParameterDomain {
            bounds: if self.continuation_period().is_some() {
                DomainBounds::Unbounded
            } else {
                DomainBounds::Bounded(self.range)
            },
            period: None,
        }
    }

    pub(super) fn corrected_parameters(&self, parameter: f64) -> KernelResult<(f64, f64)> {
        let mut parameter = parameter;
        let mut lift = Vec2::new(0.0, 0.0);
        if !self.range.contains(parameter, self.tolerance) {
            if let Some(period) = self.continuation_period() {
                let center = (self.range.start + self.range.end) * 0.5;
                let turns = ((parameter - center) / period).round();
                parameter -= turns * period;
                let start = self.range.start.min(self.range.end);
                let first = self.guide.first_order(start)?.0;
                let last = self.guide.first_order(start + period)?.0;
                for (axis, domain) in self.surface.domains().iter().enumerate() {
                    if let Some(surface_period) = domain.period {
                        lift[axis] = ((last[axis] - first[axis]) / surface_period).round()
                            * surface_period * turns;
                    }
                }
            }
        }
        if !self.range.contains(parameter, self.tolerance) {
            return Err(invalid(
                "surface-projection parameter lies outside its certified range",
            ));
        }
        parameter = parameter.clamp(
            self.range.start.min(self.range.end),
            self.range.start.max(self.range.end),
        );
        let target = self.curve.first_order(parameter)?.0;
        let guide = self.guide.first_order(parameter)?.0;
        let domains = self.surface.domains();
        let constrain = |domain: ParameterDomain, value: f64| {
            if domain.period.is_some() {
                value
            } else if let DomainBounds::Bounded(range) = domain.bounds {
                value.clamp(range.start.min(range.end), range.start.max(range.end))
            } else {
                value
            }
        };
        let mut uv = (guide.x, guide.y);
        for _ in 0..48 {
            uv.0 = constrain(domains[0], uv.0);
            uv.1 = constrain(domains[1], uv.1);
            let (point, du, dv) = self.surface.first_order(uv.0, uv.1)?;
            let residual = point - target;
            let scale = point
                .to_vec()
                .magnitude()
                .max(target.to_vec().magnitude())
                .max(1.0);
            // Model tolerance permits a small normal offset, but is too coarse
            // for the tangential position: adjacent uses can acquire different
            // endpoints when their guides stop at different residuals.
            if residual.magnitude() <= f64::EPSILON * scale * 2048.0 {
                return Ok((uv.0 + lift.x, uv.1 + lift.y));
            }
            let a00 = du.magnitude2();
            let a01 = du.dot(dv);
            let a11 = dv.magnitude2();
            let determinant = a00 * a11 - a01 * a01;
            let b0 = -du.dot(residual);
            let b1 = -dv.dot(residual);
            let delta = if determinant.abs() <= f64::EPSILON * a00.max(a11).powi(2) * 256.0 {
                if a00 + a11 == 0.0 { break; }
                (b0 / (a00 + a11), b1 / (a00 + a11))
            } else { (
                (b0 * a11 - b1 * a01) / determinant,
                (b1 * a00 - b0 * a01) / determinant,
            ) };
            let previous = residual.magnitude2();
            let mut damping = 1.0;
            let mut accepted = false;
            for _ in 0..12 {
                let candidate = (
                    constrain(domains[0], uv.0 + delta.0 * damping),
                    constrain(domains[1], uv.1 + delta.1 * damping),
                );
                if (self.surface.first_order(candidate.0, candidate.1)?.0 - target).magnitude2()
                    < previous
                {
                    uv = candidate;
                    accepted = true;
                    break;
                }
                damping *= 0.5;
            }
            if !accepted {
                break;
            }
        }
        let point = self.surface.first_order(uv.0, uv.1)?.0;
        let scale = point
            .to_vec()
            .magnitude()
            .max(target.to_vec().magnitude())
            .max(1.0);
        // Once refinement stops improving, retain the existing certification
        // allowance for a curve that lies only within tolerance of its support.
        if point.distance(target) <= self.tolerance.linear_at_scale(scale) {
            return Ok((uv.0 + lift.x, uv.1 + lift.y));
        }
        Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "exact curve-to-surface parameter correction did not converge",
        ))
    }

    pub(super) fn solve_tangent(du: Vec3, dv: Vec3, vector: Vec3, guide: impl FnOnce() -> KernelResult<Vec2>) -> KernelResult<Vec2> {
        let a00 = du.magnitude2();
        let a01 = du.dot(dv);
        let a11 = dv.magnitude2();
        let determinant = a00 * a11 - a01 * a01;
        if determinant.abs() <= f64::EPSILON * a00.max(a11).powi(2) * 256.0 {
            // At a collapsed rail the inverse has one free chart direction.
            // Retain the guide's component in that null space and solve the
            // remaining spatial derivative in the rank-one tangent plane.
            let trace = a00 + a11;
            if trace > 0.0 {
                let guide = guide()?;
                let residual = vector - du * guide.x - dv * guide.y;
                return Ok(guide + Vec2::new(du.dot(residual), dv.dot(residual)) / trace);
            }
            return Err(geometry_error("surface-projection p-curve reached a singular parameterization"));
        }
        let b0 = du.dot(vector);
        let b1 = dv.dot(vector);
        Ok(Vec2::new(
            (b0 * a11 - b1 * a01) / determinant,
            (b1 * a00 - b0 * a01) / determinant,
        ))
    }

    pub(super) fn evaluate(&self, parameter: f64) -> KernelResult<Curve2Evaluation> {
        let uv = self.corrected_parameters(parameter)?;
        let curve = self.curve.evaluate(parameter)?;
        let surface = self.surface.evaluate(uv.0, uv.1)?;
        let first = Self::solve_tangent(surface.du, surface.dv, curve.first_derivative,
            || Ok(self.guide.evaluate(parameter)?.first_derivative))?;
        let curvature = surface.duu * first.x.powi(2)
            + surface.duv * (2.0 * first.x * first.y)
            + surface.dvv * first.y.powi(2);
        let second =
            Self::solve_tangent(surface.du, surface.dv, curve.second_derivative - curvature,
                || Ok(self.guide.evaluate(parameter)?.second_derivative))?;
        Ok(Curve2Evaluation {
            point: Point2::new(uv.0, uv.1),
            first_derivative: first,
            second_derivative: second,
        })
    }

    pub(super) fn first_order(&self, parameter: f64) -> KernelResult<(Point2, Vec2)> {
        if let Some(value) = self.evaluations.get(parameter) {
            return Ok(value);
        }
        let uv = self.corrected_parameters(parameter)?;
        let (_, curve_tangent) = self.curve.first_order(parameter)?;
        let (_, du, dv) = self.surface.first_order(uv.0, uv.1)?;
        let value = (
            Point2::new(uv.0, uv.1),
            Self::solve_tangent(du, dv, curve_tangent, || Ok(self.guide.first_order(parameter)?.1))?,
        );
        self.evaluations.insert(parameter, value);
        Ok(value)
    }
}

// Splice rational guide spans without fitting a new approximation or extending
// either certified branch beyond its trim. Full knot multiplicity retains each
// guide's own gauge at the junction; the corrected spatial curve is continuous.
pub(super) fn join_intersection_guides(
    first: &Curve2, first_range: ParameterRange,
    second: &Curve2, second_range: ParameterRange,
    join: f64, offset: Vec2,
) -> KernelResult<Option<(NurbsCurve2, usize)>> {
    let spans = |curve: &Curve2, trim: ParameterRange| -> KernelResult<Option<(usize, Vec<(ParameterRange, Vec<Vec3>)>)>> {
        Ok(match curve {
            Curve2::Nurbs(curve) => Some((curve.degree, curve.bezier_spans()?)),
            Curve2::Line(line) => {
                let range = ParameterRange::new(trim.start.min(trim.end), trim.start.max(trim.end))?;
                let a = line.evaluate(range.start)?.point;
                let b = line.evaluate(range.end)?.point;
                Some((1, vec![(range, vec![Vec3::new(a.x, a.y, 1.0), Vec3::new(b.x, b.y, 1.0)])]))
            }
            _ => None,
        })
    };
    let Some((first_degree, first_spans)) = spans(first, first_range)? else { return Ok(None); };
    let Some((second_degree, second_spans)) = spans(second, second_range)? else { return Ok(None); };
    let degree = first_degree.max(second_degree);
    let mut pieces = Vec::new();
    let mut junction = 0;
    for (side, (source_degree, spans, source_range, target, shift)) in [
        (first_degree, first_spans, first_range, ParameterRange::new(0.0, join)?, Vec2::new(0.0, 0.0)),
        (second_degree, second_spans, second_range, ParameterRange::new(join, 1.0)?, offset),
    ].into_iter().enumerate() {
        let mut source_pieces = Vec::new();
        for (span, controls) in spans {
            let start = span.start.max(source_range.start.min(source_range.end));
            let end = span.end.min(source_range.start.max(source_range.end));
            if end <= start { continue; }
            let (start, end) = if source_range.span() > 0.0 { (start, end) } else { (end, start) };
            let a = (start - span.start) / span.span();
            let b = (end - span.start) / span.span();
            let mut controls = (0..=source_degree).map(|index| {
                let mut work = controls.clone();
                for step in 0..source_degree {
                    let t = if step < source_degree - index { a } else { b };
                    for j in 0..source_degree - step {
                        work[j] = work[j] * (1.0 - t) + work[j + 1] * t;
                    }
                }
                work[0]
            }).collect::<Vec<_>>();
            // Degree elevation in homogeneous coordinates is exact.
            while controls.len() <= degree {
                let n = controls.len();
                controls = (0..=n).map(|i| {
                    if i == 0 { controls[0] }
                    else if i == n { controls[n - 1] }
                    else { controls[i - 1] * (i as f64 / n as f64) + controls[i] * (1.0 - i as f64 / n as f64) }
                }).collect();
            }
            for point in &mut controls {
                point.x += shift.x * point.z;
                point.y += shift.y * point.z;
            }
            let map = |t| target.start + (t - source_range.start) / source_range.span() * target.span();
            let start = map(start);
            let end = map(end);
            if end > start { source_pieces.push((start, end, controls)); }
        }
        source_pieces.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (Some(first), Some(last)) = (source_pieces.first(), source_pieces.last()) else {
            return Ok(None);
        };
        // Both source trims must still reach their endpoints after mapping.
        // Otherwise there is no complete branch to anchor at the splice.
        if first.0 != target.start || last.1 != target.end { return Ok(None); }
        if side == 1 { junction = pieces.len() * (degree + 1); }
        pieces.extend(source_pieces);
    }
    let mut controls = Vec::new();
    let mut knots = Vec::new();
    for (start, _, points) in &pieces {
        knots.extend(vec![*start; degree + 1]);
        controls.extend_from_slice(points);
    }
    knots.extend(vec![1.0; degree + 1]);
    let guide = NurbsCurve2::new(degree,
        controls.iter().map(|p| Point2::new(p.x / p.z, p.y / p.z)).collect(),
        controls.iter().map(|p| p.z).collect(), knots,
    )?;
    Ok(Some((guide, junction)))
}

impl SurfaceIntersectionCurve {
    /// Join adjacent trims of the same support pair. Only the branch guides
    /// are reparameterized; evaluation still solves the exact two surfaces.
    pub(crate) fn joined(
        &self,
        range: ParameterRange,
        other: &Self,
        other_range: ParameterRange,
    ) -> KernelResult<Option<Self>> {
        let swapped = self.first_surface == other.second_surface
            && self.second_surface == other.first_surface;
        if !swapped && (self.first_surface != other.first_surface
            || self.second_surface != other.second_surface) {
            return Ok(None);
        }
        let end = self.evaluate(range.end)?;
        let start = other.evaluate(other_range.start)?;
        let a = end.first_derivative * range.span().signum();
        let b = start.first_derivative * other_range.span().signum();
        if end.point.distance(start.point) > self.tolerance.linear_at_scale(end.point.to_vec().magnitude()) * 8.0
            || a.normalize().dot(b.normalize()) <= 0.0
            || a.normalize().cross(b.normalize()).magnitude() > self.tolerance.angular * 8.0 {
            return Ok(None);
        }
        // Endpoint speed is ill-conditioned at a tangential support contact.
        // Use spatial traversal lengths to allocate the joined parameter range
        // so an ordinary arc cannot be compressed into a microscopic interval.
        let length = |curve: &Self, range: ParameterRange| -> KernelResult<f64> {
            let points = (0..=16).map(|index| {
                curve.first_order(range.start + range.span() * index as f64 / 16.0)
                    .map(|value| value.0)
            }).collect::<KernelResult<Vec<_>>>()?;
            Ok(points.windows(2).map(|pair| pair[0].distance(pair[1])).sum())
        };
        let other_length = length(other, other_range)?;
        let length = length(self, range)?;
        if length.min(other_length) <= self.tolerance.linear_at_scale(end.point.to_vec().magnitude()) {
            return Ok(None);
        }
        let join = length / (length + other_length);
        let parameters = self.corrected_parameters(range.end)?;
        let other_parameters = other.corrected_parameters(other_range.start)?;
        let first_parameters = self.corrected_parameters(range.start)?;
        let last_parameters = other.corrected_parameters(other_range.end)?;
        let guides = [&self.first_pcurve, &self.second_pcurve];
        let other_guides = if swapped { [&other.second_pcurve, &other.first_pcurve] }
            else { [&other.first_pcurve, &other.second_pcurve] };
        let surfaces = [self.first_surface.as_ref(), self.second_surface.as_ref()];
        let mut joined = Vec::<Curve2>::new();
        for side in 0..2 {
            let other_side = if swapped { 1 - side } else { side };
            let offset = crate::whole_periods(
                surfaces[side].domains(),
                Vec2::new(
                    parameters[side * 2] - other_parameters[other_side * 2],
                    parameters[side * 2 + 1] - other_parameters[other_side * 2 + 1],
                ),
            );
            let Some((mut guide, junction)) = join_intersection_guides(
                guides[side], range, other_guides[side], other_range, join, offset,
            )? else { return Ok(None); };
            // Retain the corrected endpoints as exact guide anchors. Near a
            // tangency, roundoff from restricting the rational spans can move
            // a freshly corrected endpoint by much more than model tolerance.
            let point = |values: [f64; 4], side: usize| Point2::new(values[side * 2], values[side * 2 + 1]);
            let last = guide.control_points.len() - 1;
            let mut anchor = |index: usize, neighbor: usize, point: Point2| {
                let delta = point - guide.control_points[index];
                guide.control_points[index] = point;
                if guide.degree > 1 { guide.control_points[neighbor] += delta; }
            };
            anchor(0, 1, point(first_parameters, side));
            anchor(last, last - 1, point(last_parameters, other_side) + offset);
            anchor(junction - 1, junction - 2, point(parameters, side));
            anchor(junction, junction + 1, point(parameters, side));
            joined.push(guide.into());
        }
        for parameter in [0.0, 1.0] {
            let first = joined[0].evaluate(parameter)?.first_derivative;
            let second = joined[1].evaluate(parameter)?.first_derivative;
            if first.magnitude2() + second.magnitude2() <= self.tolerance.parameter.powi(2) {
                return Ok(None);
            }
        }
        Ok(Some(Self::new(
            (*self.first_surface).clone(), (*self.second_surface).clone(),
            joined.remove(0), joined.remove(0), ParameterRange::new(0.0, 1.0)?,
            self.tolerance, self.achieved_error.max(other.achieved_error),
        )?))
    }

    pub(crate) fn extended(&self, range: ParameterRange) -> KernelResult<Self> {
        let range = ParameterRange::new(
            range.start.min(self.range.start),
            range.end.max(self.range.end),
        )?;
        let extend = |guide: &Curve2| match guide {
            Curve2::Nurbs(curve) => curve.extended_guide(range).map(Curve2::Nurbs),
            _ => guide.extended(range),
        };
        Self::new(
            (*self.first_surface).clone(),
            (*self.second_surface).clone(),
            extend(&self.first_pcurve)?,
            extend(&self.second_pcurve)?,
            range,
            self.tolerance,
            self.achieved_error,
        )
    }

    pub fn new(
        first_surface: Surface3,
        second_surface: Surface3,
        first_pcurve: Curve2,
        second_pcurve: Curve2,
        range: ParameterRange,
        tolerance: ModelTolerance,
        achieved_error: f64,
    ) -> KernelResult<Self> {
        tolerance.validate()?;
        if !achieved_error.is_finite() || achieved_error < 0.0 {
            return Err(invalid(
                "surface-intersection certification error must be finite and nonnegative",
            ));
        }
        if !first_pcurve
            .domain()
            .contains_range(range, tolerance.parameter)
            || !second_pcurve
                .domain()
                .contains_range(range, tolerance.parameter)
        {
            return Err(invalid(
                "surface-intersection p-curves must cover the certified curve range",
            ));
        }
        let curve = Self {
            first_surface: Box::new(first_surface),
            second_surface: Box::new(second_surface),
            first_pcurve,
            second_pcurve,
            range,
            tolerance,
            achieved_error,
            evaluations: ParameterEvaluationCache::default(),
        };
        for parameter in [range.start, (range.start + range.end) * 0.5, range.end] {
            curve.corrected_parameters(parameter)?;
        }
        Ok(curve)
    }

    pub fn first_surface(&self) -> &Surface3 {
        &self.first_surface
    }

    pub fn second_surface(&self) -> &Surface3 {
        &self.second_surface
    }

    pub fn first_pcurve(&self) -> &Curve2 {
        &self.first_pcurve
    }

    pub fn second_pcurve(&self) -> &Curve2 {
        &self.second_pcurve
    }

    pub fn range(&self) -> ParameterRange {
        self.range
    }

    pub fn achieved_error(&self) -> f64 {
        self.achieved_error
    }

    /// Convert support residual to transverse positional uncertainty. Near
    /// tangency, a small surface residual does not establish a small distance
    /// between intersection branches or between an intersection and a trim.
    pub(crate) fn positional_uncertainty(&self, parameter: f64) -> KernelResult<f64> {
        let parameters = self.corrected_parameters(parameter)?;
        let (point, first_u, first_v) = self.first_surface.first_order(parameters[0], parameters[1])?;
        let (_, second_u, second_v) = self.second_surface.first_order(parameters[2], parameters[3])?;
        let sine = first_u.cross(first_v).normalize()
            .cross(second_u.cross(second_v).normalize()).magnitude();
        if sine <= self.tolerance.angular { return Ok(f64::INFINITY); }
        Ok(self.achieved_error.max(self.tolerance.linear_at_scale(point.to_vec().magnitude().max(1.0))) / sine)
    }

    pub fn pcurve_for_surface(&self, surface: &Surface3) -> Option<&Curve2> {
        if self.first_surface.as_ref() == surface {
            Some(&self.first_pcurve)
        } else if self.second_surface.as_ref() == surface {
            Some(&self.second_pcurve)
        } else {
            None
        }
    }

    pub fn exact_pcurves(&self) -> [Curve2; 2] {
        [
            Curve2::SurfaceIntersection(SurfaceIntersectionPcurve2 {
                curve: Box::new(self.clone()),
                first: true,
            }),
            Curve2::SurfaceIntersection(SurfaceIntersectionPcurve2 {
                curve: Box::new(self.clone()),
                first: false,
            }),
        ]
    }

    /// The parameter gauge at a curve parameter: the guides' combined value
    /// and derivatives, with the guide's unit tangent and its derivative.
    pub(super) fn gauge(&self, parameter: f64) -> KernelResult<IntersectionGauge> {
        let first_guide = self.first_pcurve.evaluate(parameter)?;
        let second_guide = self.second_pcurve.evaluate(parameter)?;
        let guide = [
            first_guide.point.x,
            first_guide.point.y,
            second_guide.point.x,
            second_guide.point.y,
        ];
        let derivative = [
            first_guide.first_derivative.x,
            first_guide.first_derivative.y,
            second_guide.first_derivative.x,
            second_guide.first_derivative.y,
        ];
        let second = [
            first_guide.second_derivative.x,
            first_guide.second_derivative.y,
            second_guide.second_derivative.x,
            second_guide.second_derivative.y,
        ];
        let speed = derivative
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if speed <= self.tolerance.parameter {
            return Err(geometry_error("surface-intersection p-curve guide has a singular tangent"));
        }
        let tangent = derivative.map(|value| value / speed);
        let speed_derivative = derivative
            .iter()
            .zip(second)
            .map(|(first, second)| first * second)
            .sum::<f64>()
            / speed;
        let tangent_derivative = std::array::from_fn(|index| {
            second[index] / speed - derivative[index] * speed_derivative / speed.powi(2)
        });
        Ok(IntersectionGauge {
            guide,
            derivative,
            second,
            tangent,
            tangent_derivative,
        })
    }

    // Reuse the converged parameters for both spatial and parameter-space
    // tangents, without cloning the intersection or running Newton again.
    pub(super) fn parameter_derivative(&self, parameter: f64, parameters: [f64; 4]) -> KernelResult<[f64; 4]> {
        let parameter = parameter.clamp(
            self.range.start.min(self.range.end),
            self.range.start.max(self.range.end),
        );
        if let Some((_, Some(derivative), _)) = self.evaluations.get(parameter) {
            return Ok(derivative);
        }
        let gauge = self.gauge(parameter)?;
        let (_, first_du, first_dv) = self
            .first_surface
            .first_order(parameters[0], parameters[1])?;
        let (_, second_du, second_dv) = self
            .second_surface
            .first_order(parameters[2], parameters[3])?;
        let gauge_rhs = dot4(gauge.tangent, gauge.derivative)
            - dot4(gauge.tangent_derivative, sub4(parameters, gauge.guide));
        let matrix = [
            [first_du.x, first_dv.x, -second_du.x, -second_dv.x],
            [first_du.y, first_dv.y, -second_du.y, -second_dv.y],
            [first_du.z, first_dv.z, -second_du.z, -second_dv.z],
            gauge.tangent,
        ];
        let derivative =
            solve_affine_least_squares_4(matrix, [0.0, 0.0, 0.0, gauge_rhs], gauge.derivative)?;
        self.evaluations
            .insert(parameter, (parameters, Some(derivative), None));
        Ok(derivative)
    }

    /// Second parameter derivatives, from differentiating the coincidence of
    /// both surfaces and the guide gauge once more. `bend` is the difference
    /// of the surfaces' second-order terms along the tangent.
    pub(super) fn parameter_second_derivative(
        &self,
        parameter: f64,
        parameters: [f64; 4],
        derivative: [f64; 4],
        tangents: [Vec3; 4],
        bend: Vec3,
    ) -> KernelResult<[f64; 4]> {
        let low = self.range.start.min(self.range.end);
        let high = self.range.start.max(self.range.end);
        let parameter = parameter.clamp(low, high);
        let gauge = self.gauge(parameter)?;
        // The gauge tangent's curvature only weighs the small deviation of the
        // converged parameters from the guide, so a difference suffices.
        let step = 1.0e-6 * (high - low);
        let before = (parameter - step).max(low);
        let after = (parameter + step).min(high);
        let early = self.gauge(before)?.tangent_derivative;
        let late = self.gauge(after)?.tangent_derivative;
        let tangent_second: [f64; 4] =
            std::array::from_fn(|index| (late[index] - early[index]) / (after - before));
        let gauge_rhs = dot4(gauge.tangent, gauge.second)
            - 2.0 * dot4(gauge.tangent_derivative, sub4(derivative, gauge.derivative))
            - dot4(tangent_second, sub4(parameters, gauge.guide));
        let [first_du, first_dv, second_du, second_dv] = tangents;
        let matrix = [
            [first_du.x, first_dv.x, -second_du.x, -second_dv.x],
            [first_du.y, first_dv.y, -second_du.y, -second_dv.y],
            [first_du.z, first_dv.z, -second_du.z, -second_dv.z],
            gauge.tangent,
        ];
        // At a tangential contact both surfaces share a normal along which their
        // bending cannot agree; the least-squares value is the best estimate.
        affine_least_squares_4(matrix, [bend.x, bend.y, bend.z, gauge_rhs], gauge.second)
    }

    pub(super) fn support_residual(&self, parameters: [f64; 4], first: Point3, second: Point3) -> Vec3 {
        let residual = first - second;
        let (plane, torus, u, v, sign) = match (self.first_surface.as_ref(), self.second_surface.as_ref()) {
            (Surface3::Plane(plane), Surface3::Torus(torus)) => (*plane, *torus, parameters[2], parameters[3], -1.0),
            (Surface3::Torus(torus), Surface3::Plane(plane)) => (*plane, *torus, parameters[0], parameters[1], 1.0),
            _ => return residual,
        };
        let normal = plane.normal();
        let q = normal.dot(torus.x_major().normalize() * u.cos() + torus.y_major().normalize() * u.sin());
        let axial = normal.dot(torus.normal()) * torus.minor_radius();
        let offset = normal.dot(torus.center() - plane.origin());
        // Evaluate sin(v) near either extremum without subtracting nearly
        // equal world coordinates. The small normal residual controls the
        // much larger transverse displacement of a nearly tangent section.
        let distance = if v.sin() >= 0.0 {
            let delta = v - std::f64::consts::FRAC_PI_2;
            (offset + axial) + torus.major_radius() * q
                - 2.0 * axial * (delta * 0.5).sin().powi(2)
                - torus.minor_radius() * q * delta.sin()
        } else {
            let delta = v + std::f64::consts::FRAC_PI_2;
            (offset - axial) + torus.major_radius() * q
                + 2.0 * axial * (delta * 0.5).sin().powi(2)
                + torus.minor_radius() * q * delta.sin()
        };
        residual + normal * (sign * distance - residual.dot(normal))
    }

    pub(super) fn corrected_parameters(&self, parameter: f64) -> KernelResult<[f64; 4]> {
        if !self.range.contains(parameter, self.tolerance) {
            return Err(invalid(
                "surface-intersection parameter lies outside its certified range",
            ));
        }
        let parameter = parameter.clamp(
            self.range.start.min(self.range.end),
            self.range.start.max(self.range.end),
        );
        if let Some((parameters, _, _)) = self.evaluations.get(parameter) {
            return Ok(parameters);
        }
        let first_guide = self.first_pcurve.evaluate(parameter)?;
        let second_guide = self.second_pcurve.evaluate(parameter)?;
        let guide = [
            first_guide.point.x,
            first_guide.point.y,
            second_guide.point.x,
            second_guide.point.y,
        ];
        let tangent = [
            first_guide.first_derivative.x,
            first_guide.first_derivative.y,
            second_guide.first_derivative.x,
            second_guide.first_derivative.y,
        ];
        let tangent_norm = tangent
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        if tangent_norm <= self.tolerance.parameter {
            return Err(geometry_error("surface-intersection guide has a singular tangent"));
        }
        let tangent = tangent.map(|value| value / tangent_norm);
        let mut parameters = guide;
        let plane_normal = match (self.first_surface.as_ref(), self.second_surface.as_ref()) {
            (Surface3::Plane(plane), Surface3::Torus(_)) | (Surface3::Torus(_), Surface3::Plane(plane)) => Some(plane.normal()),
            _ => None,
        };
        let mut first_evaluation = self.first_surface.first_order(parameters[0], parameters[1])?;
        let mut second_evaluation = self.second_surface.first_order(parameters[2], parameters[3])?;
        // Newton needs no second derivatives; evaluating them on rolling
        // surfaces would repeatedly finite-difference the same contact curves.
        // An accepted trial already supplies both differentials for the next step.
        for _ in 0..24 {
            let (first_point, first_du, first_dv) = first_evaluation;
            let (second_point, second_du, second_dv) = second_evaluation;
            let residual = self.support_residual(parameters, first_point, second_point);
            let gauge = parameters
                .iter()
                .zip(guide)
                .zip(tangent)
                .map(|((value, guide), tangent)| (value - guide) * tangent)
                .sum::<f64>();
            // The model tolerance certifies that the retained guide selects the
            // right intersection branch; it must not define curve evaluation.
            // Stopping at that comparatively coarse threshold switches between
            // corrected and uncorrected parameterizations along one edge and
            // creates a false derivative kink. Converge the implicit identity
            // to floating-point accuracy so position and derivatives describe
            // one continuous curve.
            let coordinate_scale = first_point
                .to_vec()
                .magnitude()
                .max(second_point.to_vec().magnitude())
                .max(1.0);
            let parameter_scale = parameters
                .iter()
                .chain(guide.iter())
                .map(|value| value.abs())
                .fold(1.0_f64, f64::max);
            let linear_target = f64::EPSILON * coordinate_scale * 2048.0;
            let parameter_target = f64::EPSILON * parameter_scale * 2048.0;
            let sine = first_du.cross(first_dv).normalize()
                .cross(second_du.cross(second_dv).normalize()).magnitude().max(self.tolerance.angular);
            let normal_target = (self.tolerance.linear_at_scale(coordinate_scale) * sine / 32.0)
                .max(f64::EPSILON * self.tolerance.linear);
            if residual.magnitude() <= linear_target && gauge.abs() <= parameter_target
                && plane_normal.is_none_or(|normal| residual.dot(normal).abs() <= normal_target) {
                self.evaluations.insert(parameter, (parameters, None, None));
                return Ok(parameters);
            }
            let matrix = [
                [first_du.x, first_dv.x, -second_du.x, -second_dv.x],
                [first_du.y, first_dv.y, -second_du.y, -second_dv.y],
                [first_du.z, first_dv.z, -second_du.z, -second_dv.z],
                tangent,
            ];
            let Some(delta) =
                solve_linear(matrix, [-residual.x, -residual.y, -residual.z, -gauge])
            else {
                break;
            };
            let mut accepted = false;
            let merit = |residual: Vec3, gauge: f64| residual.magnitude2() + gauge * gauge
                + plane_normal.map_or(0.0, |normal| (residual.dot(normal) / sine).powi(2));
            let previous = merit(residual, gauge);
            let mut damping = 1.0;
            for _ in 0..12 {
                let mut candidate =
                    std::array::from_fn(|index| parameters[index] + delta[index] * damping);
                if parameters_in_surface_domains(
                    &self.first_surface,
                    [candidate[0], candidate[1]],
                    self.tolerance,
                ) && parameters_in_surface_domains(
                    &self.second_surface,
                    [candidate[2], candidate[3]],
                    self.tolerance,
                ) {
                    // Domain acceptance includes the parameter tolerance;
                    // evaluators require coordinates inside the natural box.
                    for (axis, domain) in self
                        .first_surface
                        .domains()
                        .into_iter()
                        .chain(self.second_surface.domains())
                        .enumerate()
                    {
                        if domain.period.is_none() {
                            if let DomainBounds::Bounded(range) = domain.bounds {
                                candidate[axis] = candidate[axis]
                                    .clamp(range.start.min(range.end), range.start.max(range.end));
                            }
                        }
                    }
                    let candidate_first =
                        self.first_surface.first_order(candidate[0], candidate[1])?;
                    let candidate_second = self
                        .second_surface
                        .first_order(candidate[2], candidate[3])?;
                    let residual = self.support_residual(candidate, candidate_first.0, candidate_second.0);
                    let gauge = candidate
                        .iter()
                        .zip(guide)
                        .zip(tangent)
                        .map(|((value, guide), tangent)| (value - guide) * tangent)
                        .sum::<f64>();
                    if merit(residual, gauge) < previous {
                        parameters = candidate;
                        first_evaluation = candidate_first;
                        second_evaluation = candidate_second;
                        accepted = true;
                        break;
                    }
                }
                damping *= 0.5;
            }
            if !accepted {
                break;
            }
        }
        let (first_point, _, _) = first_evaluation;
        let (second_point, _, _) = second_evaluation;
        let scale = first_point
            .to_vec()
            .magnitude()
            .max(second_point.to_vec().magnitude())
            .max(1.0);
        if first_point.distance(second_point)
            <= self
                .achieved_error
                .max(self.tolerance.linear_at_scale(scale))
        {
            return Ok(parameters);
        }
        Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            format!(
                "surface-intersection corrector did not converge on {} / {} at parameter {parameter} (guide residual {}, corrected residual {})",
                self.first_surface.kind_name(), self.second_surface.kind_name(),
                self.first_surface
                    .evaluate(guide[0], guide[1])?
                    .point
                    .distance(self.second_surface.evaluate(guide[2], guide[3])?.point),
                first_point.distance(second_point),
            ),
        ))
    }

    pub(super) fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        let parameters = self.corrected_parameters(parameter)?;
        let first = self.first_surface.evaluate(parameters[0], parameters[1])?;
        let second = self.second_surface.evaluate(parameters[2], parameters[3])?;
        let derivative = self.parameter_derivative(parameter, parameters)?;
        let first_derivative = first.du * derivative[0] + first.dv * derivative[1];
        // Both surfaces bend the curve; the parameters' own acceleration makes
        // up the difference between them.
        let bend = |surface: &SurfaceEvaluation, du: f64, dv: f64| {
            surface.duu * du.powi(2) + surface.duv * (2.0 * du * dv) + surface.dvv * dv.powi(2)
        };
        let first_bend = bend(&first, derivative[0], derivative[1]);
        let second_bend = bend(&second, derivative[2], derivative[3]);
        let acceleration = self.parameter_second_derivative(
            parameter,
            parameters,
            derivative,
            [first.du, first.dv, second.du, second.dv],
            second_bend - first_bend,
        )?;
        let second_derivative = first_bend + first.du * acceleration[0] + first.dv * acceleration[1];
        Ok(CurveEvaluation {
            point: Point3::from_vec((first.point.to_vec() + second.point.to_vec()) * 0.5),
            first_derivative,
            second_derivative,
        })
    }

    pub(super) fn first_order(&self, parameter: f64) -> KernelResult<(Point3, Vec3)> {
        // Trim projections and boundary checks revisit the same parameters.
        // Cache the exact spatial differential with the converged chart data.
        if let Some((_, _, Some(value))) = self.evaluations.get(parameter) {
            return Ok(value);
        }
        let parameters = self.corrected_parameters(parameter)?;
        let derivative = self.parameter_derivative(parameter, parameters)?;
        let value = self.first_order_at(parameters, derivative)?;
        self.evaluations.insert(parameter, (parameters, Some(derivative), Some(value)));
        Ok(value)
    }

    pub(super) fn first_order_at(
        &self,
        parameters: [f64; 4],
        derivative: [f64; 4],
    ) -> KernelResult<(Point3, Vec3)> {
        let (first_point, first_du, first_dv) = self
            .first_surface
            .first_order(parameters[0], parameters[1])?;
        let (second_point, _, _) = self
            .second_surface
            .first_order(parameters[2], parameters[3])?;
        Ok((
            Point3::from_vec((first_point.to_vec() + second_point.to_vec()) * 0.5),
            first_du * derivative[0] + first_dv * derivative[1],
        ))
    }

    pub(super) fn project(&self, point: Point3) -> KernelResult<Projection1> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let mut seeds = (0..=32)
            .map(|index| self.range.start + self.range.span() * index as f64 / 32.0)
            .collect::<Vec<_>>();
        self.first_pcurve.collect_parameter_breaks(&mut seeds);
        self.second_pcurve.collect_parameter_breaks(&mut seeds);
        seeds.retain(|parameter| self.range.contains(*parameter, self.tolerance));
        seeds.sort_by(f64::total_cmp);
        seeds.dedup();
        let parameter = seeds.into_iter()
            .filter_map(|parameter| {
                self.evaluate(parameter)
                    .ok()
                    .map(|value| (parameter, value.point.distance2(point)))
            })
            .min_by(|first, second| first.1.total_cmp(&second.1))
            .map(|value| value.0)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "surface-intersection projection has no seed",
                )
            })?;
        let projected = self.evaluate(parameter)?.point;
        refine_curve_projection(
            point,
            self.range,
            self.tolerance,
            Projection1 {
                parameter,
                point: projected,
                distance: projected.distance(point),
            },
            |parameter| self.first_order(parameter),
        )
    }
}

// Refine a seeded projection without leaving its retained parameter interval.
// Shared by complete implicit curves and trimmed Boolean arrangement edges.
pub(super) fn refine_curve_projection(
    point: Point3,
    range: ParameterRange,
    tolerance: ModelTolerance,
    mut best: Projection1,
    first_order: impl Fn(f64) -> KernelResult<(Point3, Vec3)>,
) -> KernelResult<Projection1> {
    let low = range.start.min(range.end);
    let high = range.start.max(range.end);
    // Corrected implicit intersections provide a reliable first derivative,
    // but not full curvature. Damping keeps the distance decreasing without
    // depending on an incomplete second derivative or leaving the trim.
    for _ in 0..64 {
        let (projected, tangent) = first_order(best.parameter)?;
        let residual = projected - point;
        let speed2 = tangent.magnitude2();
        if speed2 <= f64::EPSILON {
            break;
        }
        let mut step = residual.dot(tangent) / speed2;
        let mut next = best;
        for _ in 0..24 {
            let parameter = (best.parameter - step).clamp(low, high);
            let Ok((projected, _)) = first_order(parameter) else {
                step *= 0.5;
                continue;
            };
            let distance = projected.distance(point);
            if distance < best.distance {
                next = Projection1 {
                    parameter,
                    point: projected,
                    distance,
                };
                break;
            }
            step *= 0.5;
        }
        let converged = (next.parameter - best.parameter).abs() <= tolerance.parameter;
        best = next;
        if converged {
            break;
        }
    }
    Ok(best)
}

pub(super) fn parameters_in_surface_domains(
    surface: &Surface3,
    parameters: [f64; 2],
    tolerance: ModelTolerance,
) -> bool {
    surface.domains()[0].contains(parameters[0], tolerance.parameter)
        && surface.domains()[1].contains(parameters[1], tolerance.parameter)
}


/// Best least-squares solution of `matrix * value = right`, choosing the one
/// closest to `reference` when the system loses rank at a tangential
/// intersection. The system need not be consistent.
///
/// The symmetric Jacobi decomposition computes the Moore-Penrose inverse of
/// the normal matrix.  Unlike substituting the guide derivative when Gaussian
/// elimination becomes singular, this preserves every independent exact
/// surface and gauge constraint and remains well-defined at rank transitions.
pub(super) fn affine_least_squares_4(
    matrix: [[f64; 4]; 4],
    right: [f64; 4],
    reference: [f64; 4],
) -> KernelResult<[f64; 4]> {
    // A regular surface/surface intersection has four independent tangent
    // constraints. Solve that system directly: forming A^T A would square its
    // condition number and can manufacture a discontinuous rank transition
    // near an otherwise regular endpoint. Only use the affine pseudoinverse
    // below when the original system is genuinely rank deficient.
    if let Some(solution) = solve_linear(matrix, right) {
        let achieved = (0..4)
            .map(|row| {
                ((0..4)
                    .map(|column| matrix[row][column] * solution[column])
                    .sum::<f64>()
                    - right[row])
                    .powi(2)
            })
            .sum::<f64>()
            .sqrt();
        let scale = right
            .iter()
            .chain(solution.iter())
            .map(|value| value.abs())
            .fold(1.0_f64, f64::max);
        if achieved.is_finite() && achieved <= f64::EPSILON.sqrt() * scale * 64.0 {
            return Ok(solution);
        }
    }
    let residual = std::array::from_fn::<_, 4, _>(|row| {
        right[row]
            - (0..4)
                .map(|column| matrix[row][column] * reference[column])
                .sum::<f64>()
    });
    let mut normal: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            (0..4)
                .map(|index| matrix[index][row] * matrix[index][column])
                .sum::<f64>()
        })
    });
    let projected = std::array::from_fn::<_, 4, _>(|column| {
        (0..4)
            .map(|row| matrix[row][column] * residual[row])
            .sum::<f64>()
    });
    let mut eigenvectors: [[f64; 4]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| if row == column { 1.0 } else { 0.0 })
    });
    for _ in 0..96 {
        let mut pivot = (0, 1);
        let mut largest = normal[0][1].abs();
        for row in 0..4 {
            for column in row + 1..4 {
                if normal[row][column].abs() > largest {
                    largest = normal[row][column].abs();
                    pivot = (row, column);
                }
            }
        }
        let diagonal_scale = (0..4)
            .map(|index| normal[index][index].abs())
            .fold(1.0_f64, f64::max);
        if largest <= f64::EPSILON * diagonal_scale * 64.0 {
            break;
        }
        let (first, second) = pivot;
        let off_diagonal = normal[first][second];
        let tau = (normal[second][second] - normal[first][first]) / (2.0 * off_diagonal);
        let tangent = tau.signum() / (tau.abs() + (1.0 + tau * tau).sqrt());
        let cosine = 1.0 / (1.0 + tangent * tangent).sqrt();
        let sine = tangent * cosine;
        let first_diagonal = normal[first][first];
        let second_diagonal = normal[second][second];
        normal[first][first] = cosine * cosine * first_diagonal
            - 2.0 * sine * cosine * off_diagonal
            + sine * sine * second_diagonal;
        normal[second][second] = sine * sine * first_diagonal
            + 2.0 * sine * cosine * off_diagonal
            + cosine * cosine * second_diagonal;
        normal[first][second] = 0.0;
        normal[second][first] = 0.0;
        for index in 0..4 {
            if index == first || index == second {
                continue;
            }
            let first_value = normal[index][first];
            let second_value = normal[index][second];
            normal[index][first] = cosine * first_value - sine * second_value;
            normal[first][index] = normal[index][first];
            normal[index][second] = sine * first_value + cosine * second_value;
            normal[second][index] = normal[index][second];
        }
        for row in &mut eigenvectors {
            let first_value = row[first];
            let second_value = row[second];
            row[first] = cosine * first_value - sine * second_value;
            row[second] = sine * first_value + cosine * second_value;
        }
    }
    let maximum_eigenvalue = (0..4)
        .map(|index| normal[index][index].abs())
        .fold(0.0_f64, f64::max);
    if !maximum_eigenvalue.is_finite() || maximum_eigenvalue <= f64::MIN_POSITIVE {
        return Err(KernelError::new(
            DiagnosticKind::NumericalFailure,
            "surface-intersection tangent constraints have zero rank",
        ));
    }
    let eigenvalue_tolerance = maximum_eigenvalue * f64::EPSILON * 4096.0;
    let mut correction = [0.0; 4];
    for eigenvector in 0..4 {
        let eigenvalue = normal[eigenvector][eigenvector];
        if eigenvalue <= eigenvalue_tolerance {
            continue;
        }
        let coefficient = (0..4)
            .map(|row| eigenvectors[row][eigenvector] * projected[row])
            .sum::<f64>()
            / eigenvalue;
        for row in 0..4 {
            correction[row] += eigenvectors[row][eigenvector] * coefficient;
        }
    }
    Ok(std::array::from_fn(|index| reference[index] + correction[index]))
}

/// Solve `matrix * value = right` like `affine_least_squares_4`, but reject
/// systems whose constraints cannot all hold.
pub(super) fn solve_affine_least_squares_4(
    matrix: [[f64; 4]; 4],
    right: [f64; 4],
    reference: [f64; 4],
) -> KernelResult<[f64; 4]> {
    let solution = affine_least_squares_4(matrix, right, reference)?;
    let achieved = (0..4)
        .map(|row| {
            ((0..4)
                .map(|column| matrix[row][column] * solution[column])
                .sum::<f64>()
                - right[row])
                .powi(2)
        })
        .sum::<f64>()
        .sqrt();
    let scale = right
        .iter()
        .chain(solution.iter())
        .map(|value| value.abs())
        .fold(1.0_f64, f64::max);
    let allowed = f64::EPSILON.sqrt() * scale * 256.0;
    if !achieved.is_finite() || achieved > allowed {
        return Err(KernelError::from_diagnostic(
            crate::Diagnostic::new(
                DiagnosticKind::NumericalFailure,
                "surface-intersection tangent constraints are inconsistent",
            )
            .with_accuracy(allowed, achieved),
        ));
    }
    Ok(solution)
}
