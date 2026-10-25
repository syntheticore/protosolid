//! Rational B-spline curves and surfaces, and their shared basis evaluation.
use super::*;

/// Validated non-uniform rational B-spline curve in homogeneous form.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve3 {
    pub(super) degree: usize,
    pub(super) control_points: Vec<Point3>,
    pub(super) weights: Vec<f64>,
    pub(super) knots: Vec<f64>,
}

impl NurbsCurve3 {
    pub fn new(
        degree: usize,
        control_points: Vec<Point3>,
        weights: Vec<f64>,
        knots: Vec<f64>,
    ) -> KernelResult<Self> {
        validate_nurbs(degree, control_points.len(), &weights, &knots)?;
        if control_points.iter().any(|point| !finite_point(*point)) {
            return Err(invalid("NURBS control points must be finite"));
        }
        Ok(Self {
            degree,
            control_points,
            weights,
            knots,
        })
    }

    pub fn degree(&self) -> usize {
        self.degree
    }
    pub fn control_points(&self) -> &[Point3] {
        &self.control_points
    }
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    pub fn domain(&self) -> ParameterDomain {
        ParameterDomain {
            bounds: DomainBounds::Bounded(ParameterRange {
                start: self.knots[self.degree],
                end: self.knots[self.control_points.len()],
            }),
            period: None,
        }
    }

    pub(super) fn evaluate(&self, parameter: f64) -> KernelResult<CurveEvaluation> {
        if !parameter.is_finite() {
            return Err(invalid("curve parameter must be finite"));
        }
        let domain_start = self.knots[self.degree];
        let domain_end = self.knots[self.control_points.len()];
        let endpoint_envelope =
            f64::EPSILON * domain_start.abs().max(domain_end.abs()).max(1.0) * 64.0;
        if parameter < domain_start - endpoint_envelope
            || parameter > domain_end + endpoint_envelope
        {
            return Err(invalid(format!(
                "3D NURBS parameter {parameter} lies outside its natural domain [{domain_start}, {domain_end}]",
            )));
        }
        let parameter = parameter.clamp(domain_start, domain_end);
        let [value, first, second] = homogeneous_curve_jet(
            self.degree,
            &self.knots,
            self.control_points.len(),
            parameter,
            |index| {
                let point = self.control_points[index];
                let weight = self.weights[index];
                Vec4::new(point.x * weight, point.y * weight, point.z * weight, weight)
            },
        );
        if ![value, first, second].into_iter().all(|v| {
            v.x.is_finite() && v.y.is_finite() && v.z.is_finite() && v.w.is_finite()
        }) {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "NURBS evaluation produced a non-finite value",
            ));
        }
        if value.w == 0.0 {
            return Err(geometry_error("NURBS has zero homogeneous weight"));
        }

        let point_vector = value.truncate() / value.w;
        let first_derivative = (first.truncate() - point_vector * first.w) / value.w;
        let second_derivative =
            (second.truncate() - first_derivative * (2.0 * first.w) - point_vector * second.w)
                / value.w;
        Ok(CurveEvaluation {
            point: Point3::from_vec(point_vector),
            first_derivative,
            second_derivative,
        })
    }

    pub(super) fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection1> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let start = self.knots[self.degree];
        let end = self.knots[self.control_points.len()];
        let mut candidates = vec![start, end];
        for span in self.knots.windows(2) {
            let low = span[0].max(start);
            let high = span[1].min(end);
            if high > low {
                for index in 1..=4 {
                    candidates.push(low + (high - low) * index as f64 / 5.0);
                }
            }
        }
        let mut parameter = candidates
            .into_iter()
            .map(|candidate| {
                self.evaluate(candidate)
                    .map(|evaluation| (candidate, evaluation.point.distance2(point)))
            })
            .collect::<KernelResult<Vec<_>>>()?
            .into_iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|candidate| candidate.0)
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "NURBS projection has no initial candidate",
                )
            })?;

        let mut converged = false;
        for _ in 0..64 {
            let evaluation = self.evaluate(parameter)?;
            let residual = evaluation.point - point;
            let function = residual.dot(evaluation.first_derivative);
            let mut derivative = evaluation.first_derivative.magnitude2()
                + residual.dot(evaluation.second_derivative);
            // Newton's stationary-point Hessian can be negative away from
            // the nearest point. Use a descent direction in that region.
            if derivative <= 0.0 {
                derivative = evaluation.first_derivative.magnitude2();
            }
            if derivative.abs() <= f64::EPSILON {
                break;
            }
            let previous_distance = residual.magnitude2();
            let mut step = -function / derivative;
            let mut next = (parameter + step).clamp(start, end);
            for _ in 0..12 {
                let next_distance = self.evaluate(next)?.point.distance2(point);
                if next_distance <= previous_distance {
                    break;
                }
                step *= 0.5;
                next = (parameter + step).clamp(start, end);
            }
            if (next - parameter).abs() <= tolerance.parameter {
                parameter = next;
                converged = true;
                break;
            }
            parameter = next;
        }
        if !converged {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "NURBS closest-point refinement did not converge",
            ));
        }
        let projected = self.evaluate(parameter)?.point;
        Ok(Projection1 {
            parameter,
            point: projected,
            distance: projected.distance(point),
        })
    }
}

pub(super) fn validate_nurbs(
    degree: usize,
    control_count: usize,
    weights: &[f64],
    knots: &[f64],
) -> KernelResult<()> {
    if degree == 0 || control_count < degree + 1 {
        return Err(invalid(
            "NURBS degree must be positive and smaller than its control-point count",
        ));
    }
    if weights.len() != control_count
        || weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
    {
        return Err(invalid(
            "NURBS requires one finite positive weight per control point",
        ));
    }
    if knots.len() != control_count + degree + 1
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Err(invalid(
            "NURBS knot vector has invalid length, values, or ordering",
        ));
    }
    let mut run = 1usize;
    for pair in knots.windows(2) {
        if pair[0] == pair[1] {
            run += 1;
            if run > degree + 1 {
                return Err(invalid(
                    "NURBS knot multiplicity may not exceed degree plus one",
                ));
            }
        } else {
            run = 1;
        }
    }
    if knots[degree] >= knots[control_count] {
        return Err(invalid(
            "NURBS active parameter domain must have positive length",
        ));
    }
    Ok(())
}

/// Split a Bézier control polygon at `parameter` by de Casteljau subdivision.
/// Both halves run in the original direction and share the split point.
pub(crate) fn de_casteljau_split<V>(controls: &[V], parameter: f64) -> (Vec<V>, Vec<V>)
where
    V: Copy + std::ops::Add<Output = V> + std::ops::Mul<f64, Output = V>,
{
    let lerp = |pair: &[V]| pair[0] * (1.0 - parameter) + pair[1] * parameter;
    let mut left = Vec::with_capacity(controls.len());
    let mut right = Vec::with_capacity(controls.len());
    left.push(controls[0]);
    right.push(controls[controls.len() - 1]);
    let mut row: Vec<V> = controls.windows(2).map(lerp).collect();
    while !row.is_empty() {
        left.push(row[0]);
        right.push(row[row.len() - 1]);
        row = row.windows(2).map(lerp).collect();
    }
    right.reverse();
    (left, right)
}

/// The nonzero B-spline basis values at `parameter`: the index of the first
/// contributing control and one value per control of the active span.
pub(crate) fn basis_values(
    degree: usize,
    knots: &[f64],
    control_count: usize,
    parameter: f64,
) -> (usize, Vec<f64>) {
    let jet = SplineBasisJet::<1>::new(degree, knots, control_count, parameter);
    (jet.first, jet.values().iter().map(|value| value[0]).collect())
}

pub(super) fn find_span(degree: usize, knots: &[f64], control_count: usize, parameter: f64) -> usize {
    let last = control_count - 1;
    if parameter >= knots[control_count] {
        return last;
    }
    let mut low = degree;
    let mut high = control_count;
    let mut middle = (low + high) / 2;
    while parameter < knots[middle] || parameter >= knots[middle + 1] {
        if parameter < knots[middle] {
            high = middle;
        } else {
            low = middle;
        }
        middle = (low + high) / 2;
    }
    middle
}

/// Scalar coefficients shared by every row of a surface's active control net.
pub(super) struct SplineBasisJet<const N: usize> {
    pub(super) first: usize,
    pub(super) count: usize,
    pub(super) stack: [[f64; N]; 16],
    pub(super) spill: Vec<[f64; N]>,
}

impl<const N: usize> SplineBasisJet<N> {
    pub(super) fn new(degree: usize, knots: &[f64], control_count: usize, parameter: f64) -> Self {
        let span = find_span(degree, knots, control_count, parameter);
        let count = degree + 1;
        let mut basis = Self {
            first: span - degree,
            count,
            stack: [[0.0; N]; 16],
            spill: Vec::new(),
        };
        let values = if count <= basis.stack.len() {
            &mut basis.stack[..count]
        } else {
            basis.spill.resize(count, [0.0; N]);
            basis.spill.as_mut_slice()
        };
        let knots = &knots[basis.first..basis.first + 2 * degree + 2];
        for order in 0..N.min(degree + 1) {
            let current_degree = degree - order;
            values[current_degree][order] = 1.0;
            // Transpose de Boor interpolation on the derivative control net.
            // This preserves its choice of side at repeated endpoint knots.
            for level in (1..=current_degree).rev() {
                for j in level..=current_degree {
                    let denominator = knots[j + degree + 1 - level] - knots[j + order];
                    let value = values[j][order];
                    if denominator == 0.0 {
                        values[j - 1][order] += value;
                        values[j][order] = 0.0;
                    } else {
                        let alpha = (parameter - knots[j + order]) / denominator;
                        values[j - 1][order] += value * (1.0 - alpha);
                        values[j][order] = value * alpha;
                    }
                }
            }
            // Map derivative-control coefficients back to the original controls.
            for derivative in (1..=order).rev() {
                for j in (0..=degree - derivative).rev() {
                    let denominator = knots[j + degree + 1] - knots[j + derivative];
                    let value = if denominator == 0.0 {
                        0.0
                    } else {
                        values[j][order] * ((degree - derivative + 1) as f64 / denominator)
                    };
                    values[j][order] = -value;
                    values[j + 1][order] += value;
                }
            }
        }
        basis
    }

    pub(super) fn values(&self) -> &[[f64; N]] {
        if self.count <= self.stack.len() {
            &self.stack[..self.count]
        } else {
            &self.spill
        }
    }
}

/// Evaluate a homogeneous curve and its derivatives on the active
/// knot span. Only degree + 1 controls can contribute at a parameter; building
/// whole derivative curves here makes every sample linear in model size.
/// The same local evaluator serves rational 2D trims and 3D geometry. Common
/// degrees use stack storage; arbitrary higher degrees remain supported.
pub(super) fn homogeneous_curve_jet<V: cgmath::VectorSpace<Scalar = f64>, const N: usize>(
    degree: usize,
    knots: &[f64],
    control_count: usize,
    parameter: f64,
    mut control: impl FnMut(usize) -> V,
) -> [V; N] {
    let span = find_span(degree, knots, control_count, parameter);
    let first = span - degree;
    let mut knots = &knots[first..first + 2 * degree + 2];
    let count = degree + 1;
    let mut stack = [V::zero(); 32];
    let mut spill = Vec::new();
    let storage = if count <= 16 {
        &mut stack[..count * 2]
    } else {
        spill.resize(count * 2, V::zero());
        spill.as_mut_slice()
    };
    let (controls, values) = storage.split_at_mut(count);
    for (index, value) in controls.iter_mut().enumerate() {
        *value = control(first + index);
    }
    let mut jet = [V::zero(); N];
    for (order, result) in jet.iter_mut().enumerate().take(degree + 1) {
        let current_degree = degree - order;
        values[..=current_degree].copy_from_slice(&controls[..=current_degree]);
        for level in 1..=current_degree {
            for j in (level..=current_degree).rev() {
                let denominator = knots[j + current_degree + 1 - level] - knots[j];
                values[j] = if denominator == 0.0 {
                    values[j - 1]
                } else {
                    let alpha = (parameter - knots[j]) / denominator;
                    values[j - 1] * (1.0 - alpha) + values[j] * alpha
                };
            }
        }
        *result = values[current_degree];
        if order + 1 < N && current_degree > 0 {
            for j in 0..current_degree {
                let denominator = knots[j + current_degree + 1] - knots[j + 1];
                controls[j] = if denominator == 0.0 {
                    V::zero()
                } else {
                    (controls[j + 1] - controls[j]) * (current_degree as f64 / denominator)
                };
            }
            knots = &knots[1..knots.len() - 1];
        }
    }
    jet
}

/// Validated tensor-product rational B-spline surface.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurface3 {
    pub(super) degrees: (usize, usize),
    pub(super) control_points: Vec<Vec<Point3>>,
    pub(super) weights: Vec<Vec<f64>>,
    pub(super) knots: (Vec<f64>, Vec<f64>),
}

#[derive(Debug, Clone)]
pub(crate) struct BezierSurfacePatch {
    pub(crate) u_range: ParameterRange,
    pub(crate) v_range: ParameterRange,
    pub(crate) controls: Vec<Vec<Vec4>>,
}

pub(super) fn bezier_controls_on_interval(controls: &[Vec4], a: f64, b: f64) -> Vec<Vec4> {
    let degree = controls.len() - 1;
    (0..=degree).map(|index| {
        let mut work = controls.to_vec();
        for step in 0..degree {
            let t = if step < degree - index { a } else { b };
            for j in 0..degree - step {
                work[j] = work[j] * (1.0 - t) + work[j + 1] * t;
            }
        }
        work[0]
    }).collect()
}

impl BezierSurfacePatch {
    pub(crate) fn controls_in_bounds(&self, bounds: [ParameterRange; 2]) -> Vec<Vec<Vec4>> {
        let a = (bounds[0].start - self.u_range.start) / self.u_range.span();
        let b = (bounds[0].end - self.u_range.start) / self.u_range.span();
        let mut net = self.controls.iter()
            .map(|row| bezier_controls_on_interval(row, a, b)).collect::<Vec<_>>();
        let a = (bounds[1].start - self.v_range.start) / self.v_range.span();
        let b = (bounds[1].end - self.v_range.start) / self.v_range.span();
        for column in 0..net[0].len() {
            let controls = net.iter().map(|row| row[column]).collect::<Vec<_>>();
            for (row, value) in bezier_controls_on_interval(&controls, a, b).into_iter().enumerate() {
                net[row][column] = value;
            }
        }
        net
    }

    pub(super) fn coincident_in_bounds(&self, other: &Self, bounds: [ParameterRange; 2], tolerance: ModelTolerance) -> bool {
        let first = self.controls_in_bounds(bounds);
        let second = other.controls_in_bounds(bounds);
        let degrees = |net: &[Vec<Vec4>]| (net[0].len() - 1, net.len() - 1);
        let (p, q) = degrees(&first);
        let (r, s) = degrees(&second);
        let binomial = |degree: usize, index: usize| {
            (0..index.min(degree - index)).fold(1.0, |value, i| value * (degree - i) as f64 / (i + 1) as f64)
        };
        let factors = |a: usize, b: usize| (0..=a).map(|i| (0..=b).map(|j| {
            binomial(a, i) * binomial(b, j) / binomial(a + b, i + j)
        }).collect::<Vec<_>>()).collect::<Vec<_>>();
        let u_factors = factors(p, r);
        let v_factors = factors(q, s);
        let mut residual = vec![vec![Vec3::new(0.0, 0.0, 0.0); p + r + 1]; q + s + 1];
        let mut denominator = vec![vec![0.0; p + r + 1]; q + s + 1];
        for (v, row) in first.iter().enumerate() {
            for (u, a) in row.iter().enumerate() {
                for (w, row) in second.iter().enumerate() {
                    for (t, b) in row.iter().enumerate() {
                        let factor = u_factors[u][t] * v_factors[v][w];
                        residual[v + w][u + t] += (a.truncate() * b.w - b.truncate() * a.w) * factor;
                        denominator[v + w][u + t] += a.w * b.w * factor;
                    }
                }
            }
        }
        let scale = first.iter().flatten().chain(second.iter().flatten())
            .map(|point| (point.truncate() / point.w).magnitude()).fold(1.0_f64, f64::max);
        let linear = tolerance.linear_at_scale(scale);
        // Bernstein weights form a positive partition of unity. Bounding each
        // coefficient of A.xyz * B.w - B.xyz * A.w by its denominator therefore
        // bounds the distance everywhere in this patch, without point sampling.
        residual.iter().flatten().zip(denominator.iter().flatten())
            .all(|(error, weight)| *weight > 0.0 && error.magnitude() <= linear * weight)
    }
}

impl NurbsSurface3 {
    pub fn new(
        degrees: (usize, usize),
        control_points: Vec<Vec<Point3>>,
        weights: Vec<Vec<f64>>,
        knots: (Vec<f64>, Vec<f64>),
    ) -> KernelResult<Self> {
        if control_points.is_empty() || control_points[0].is_empty() {
            return Err(invalid("NURBS surface control net may not be empty"));
        }
        let v_count = control_points.len();
        let u_count = control_points[0].len();
        if control_points.iter().any(|row| row.len() != u_count)
            || weights.len() != v_count
            || weights.iter().any(|row| row.len() != u_count)
        {
            return Err(invalid(
                "NURBS surface control points and weights must form matching rectangles",
            ));
        }
        let flat_weights: Vec<f64> = weights.iter().flatten().copied().collect();
        validate_nurbs(degrees.0, u_count, &flat_weights[..u_count], &knots.0)?;
        validate_nurbs(
            degrees.1,
            v_count,
            &weights.iter().map(|row| row[0]).collect::<Vec<_>>(),
            &knots.1,
        )?;
        if flat_weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
            || control_points
                .iter()
                .flatten()
                .any(|point| !finite_point(*point))
        {
            return Err(invalid(
                "NURBS surface controls and weights must be finite with positive weights",
            ));
        }
        Ok(Self {
            degrees,
            control_points,
            weights,
            knots,
        })
    }

    pub fn degrees(&self) -> (usize, usize) {
        self.degrees
    }
    pub fn control_points(&self) -> &[Vec<Point3>] {
        &self.control_points
    }

    /// The positive rational basis lies in the affine hull of its controls.
    /// Recognize planar supports from that hull, not from sampled positions.
    pub(crate) fn plane(&self, tolerance: ModelTolerance) -> KernelResult<Option<Plane3>> {
        let domains = self.domains();
        let midpoint = |domain: ParameterDomain| match domain.bounds {
            DomainBounds::Bounded(range) => (range.start + range.end) * 0.5,
            _ => unreachable!(),
        };
        let jet = self.evaluate(midpoint(domains[0]), midpoint(domains[1]))?;
        let plane = Plane3::from_origin_normal(jet.point, jet.du.cross(jet.dv))?;
        for point in self.control_points.iter().flatten() {
            if plane.project(*point)?.distance > tolerance.linear {
                return Ok(None);
            }
        }
        Ok(Some(plane))
    }
    pub fn weights(&self) -> &[Vec<f64>] {
        &self.weights
    }
    pub fn knots(&self) -> (&[f64], &[f64]) {
        (&self.knots.0[..], &self.knots.1[..])
    }

    pub fn domains(&self) -> [ParameterDomain; 2] {
        let mut domains = [
            ParameterDomain {
                bounds: DomainBounds::Bounded(ParameterRange {
                    start: self.knots.0[self.degrees.0],
                    end: self.knots.0[self.control_points[0].len()],
                }),
                period: None,
            },
            ParameterDomain {
                bounds: DomainBounds::Bounded(ParameterRange {
                    start: self.knots.1[self.degrees.1],
                    end: self.knots.1[self.control_points.len()],
                }),
                period: None,
            },
        ];
        let same = |a: Point3, b: Point3| {
            a.distance(b)
                <= f64::EPSILON
                    * 256.0
                    * a.to_vec().magnitude().max(b.to_vec().magnitude()).max(1.0)
        };
        let closed = [
            self.control_points
                .iter()
                .zip(&self.weights)
                .all(|(row, weights)| {
                    same(row[0], *row.last().unwrap()) && weights[0] == *weights.last().unwrap()
                }),
            self.control_points[0]
                .iter()
                .zip(self.control_points.last().unwrap())
                .all(|(a, b)| same(*a, *b))
                && self.weights[0] == *self.weights.last().unwrap(),
        ];
        for axis in 0..2 {
            if closed[axis] {
                if let DomainBounds::Bounded(range) = domains[axis].bounds {
                    domains[axis].period = Some(range.span());
                }
            }
        }
        domains
    }

    pub(super) fn homogeneous_rows(&self) -> Vec<Vec<Vec4>> {
        self.control_points
            .iter()
            .zip(&self.weights)
            .map(|(points, weights)| {
                points
                    .iter()
                    .zip(weights)
                    .map(|(point, weight)| {
                        Vec4::new(
                            point.x * weight,
                            point.y * weight,
                            point.z * weight,
                            *weight,
                        )
                    })
                    .collect()
            })
            .collect()
    }

    /// Compare the complete rational control nets in a common parameter box.
    /// Continuation changes knots and controls, but retains this polynomial
    /// identity. Point samples alone cannot certify coincident spline supports.
    pub(crate) fn coincident_support(&self, other: &Self, tolerance: ModelTolerance) -> KernelResult<bool> {
        let first = self.bezier_patches()?;
        let second = other.bezier_patches()?;
        let mut overlap = false;
        for a in &first {
            for b in &second {
                let start = [a.u_range.start.max(b.u_range.start), a.v_range.start.max(b.v_range.start)];
                let end = [a.u_range.end.min(b.u_range.end), a.v_range.end.min(b.v_range.end)];
                if (0..2).any(|axis| end[axis] - start[axis] <= tolerance.parameter) {
                    continue;
                }
                overlap = true;
                let bounds = [ParameterRange::new(start[0], end[0])?, ParameterRange::new(start[1], end[1])?];
                if !a.coincident_in_bounds(b, bounds, tolerance) {
                    return Ok(false);
                }
            }
        }
        Ok(overlap)
    }

    /// Continue a single rational Bezier patch without changing its parameter
    /// chart or its values on the original domain. Shell openings can require
    /// an intersection just beyond a spline's original endpoint.
    pub(crate) fn extended_bezier(&self, padding: [f64; 2]) -> KernelResult<Self> {
        let (p, q) = self.degrees;
        let ranges = [
            ParameterRange::new(self.knots.0[p], self.knots.0[self.control_points[0].len()])?,
            ParameterRange::new(self.knots.1[q], self.knots.1[self.control_points.len()])?,
        ];
        let extend_axis = [
            self.control_points[0].len() == p + 1,
            self.control_points.len() == q + 1,
        ];
        let padding = std::array::from_fn::<_, 2, _>(|axis| {
            if extend_axis[axis] && self.domains()[axis].period.is_none() {
                padding[axis]
            } else {
                0.0
            }
        });
        // Blossom evaluation gives the controls on any affine subinterval,
        // including an interval extending beyond [0, 1], in homogeneous space.
        let extend = |controls: &[Vec4], axis: usize| -> Vec<Vec4> {
            if padding[axis] == 0.0 {
                return controls.to_vec();
            }
            let a = -padding[axis] / ranges[axis].span();
            let b = 1.0 - a;
            bezier_controls_on_interval(controls, a, b)
        };
        let mut net = self
            .homogeneous_rows()
            .iter()
            .map(|row| extend(row, 0))
            .collect::<Vec<_>>();
        for column in 0..net[0].len() {
            let controls = net.iter().map(|row| row[column]).collect::<Vec<_>>();
            for (row, value) in extend(&controls, 1).into_iter().enumerate() {
                net[row][column] = value;
            }
        }
        let knots = |axis: usize, degree: usize| {
            if padding[axis] == 0.0 {
                return if axis == 0 {
                    self.knots.0.to_vec()
                } else {
                    self.knots.1.to_vec()
                };
            }
            let mut knots = vec![ranges[axis].start - padding[axis]; degree + 1];
            knots.extend(vec![ranges[axis].end + padding[axis]; degree + 1]);
            knots
        };
        Self::new(
            self.degrees,
            net.iter()
                .map(|row| {
                    row.iter()
                        .map(|v| Point3::new(v.x / v.w, v.y / v.w, v.z / v.w))
                        .collect()
                })
                .collect(),
            net.iter()
                .map(|row| row.iter().map(|v| v.w).collect())
                .collect(),
            (knots(0, p), knots(1, q)),
        )
    }

    /// Extract exact rational Bézier patches by knot insertion in both
    /// parameter directions. This changes representation only; it is used for
    /// convex-hull intersection isolation and never tessellates the surface.
    pub(crate) fn bezier_patches(&self) -> KernelResult<Vec<BezierSurfacePatch>> {
        let mut controls = self.homogeneous_rows();
        let mut u_knots = self.knots.0.to_vec();
        let mut v_knots = self.knots.1.to_vec();
        let (u_degree, v_degree) = self.degrees;

        let u_start = u_knots[u_degree];
        let u_end = u_knots[controls[0].len()];
        let u_internal = u_knots
            .iter()
            .copied()
            .filter(|knot| *knot > u_start && *knot < u_end)
            .fold(Vec::<f64>::new(), |mut unique, knot| {
                if unique.last().is_none_or(|last| *last != knot) {
                    unique.push(knot);
                }
                unique
            });
        for knot in u_internal {
            while u_knots
                .iter()
                .filter(|candidate| **candidate == knot)
                .count()
                < u_degree
            {
                let old_knots = u_knots.clone();
                let mut next_knots = None;
                let mut next_rows = Vec::with_capacity(controls.len());
                for row in controls {
                    let mut row_knots = old_knots.clone();
                    let mut row = row;
                    insert_homogeneous_knot4(u_degree, knot, &mut row_knots, &mut row)?;
                    next_knots.get_or_insert(row_knots);
                    next_rows.push(row);
                }
                controls = next_rows;
                u_knots = next_knots.expect("a NURBS surface has at least one row");
            }
        }

        let v_start = v_knots[v_degree];
        let v_end = v_knots[controls.len()];
        let v_internal = v_knots
            .iter()
            .copied()
            .filter(|knot| *knot > v_start && *knot < v_end)
            .fold(Vec::<f64>::new(), |mut unique, knot| {
                if unique.last().is_none_or(|last| *last != knot) {
                    unique.push(knot);
                }
                unique
            });
        for knot in v_internal {
            while v_knots
                .iter()
                .filter(|candidate| **candidate == knot)
                .count()
                < v_degree
            {
                let old_knots = v_knots.clone();
                let width = controls[0].len();
                let mut columns = Vec::with_capacity(width);
                let mut next_knots = None;
                for u in 0..width {
                    let mut column = controls.iter().map(|row| row[u]).collect::<Vec<_>>();
                    let mut column_knots = old_knots.clone();
                    insert_homogeneous_knot4(v_degree, knot, &mut column_knots, &mut column)?;
                    next_knots.get_or_insert(column_knots);
                    columns.push(column);
                }
                let height = columns[0].len();
                controls = (0..height)
                    .map(|v| columns.iter().map(|column| column[v]).collect())
                    .collect();
                v_knots = next_knots.expect("a NURBS surface has at least one column");
            }
        }

        let u_spans = (u_degree..controls[0].len())
            .filter(|index| u_knots[*index + 1] > u_knots[*index])
            .collect::<Vec<_>>();
        let v_spans = (v_degree..controls.len())
            .filter(|index| v_knots[*index + 1] > v_knots[*index])
            .collect::<Vec<_>>();
        let mut patches = Vec::new();
        for v in v_spans {
            for u in &u_spans {
                patches.push(BezierSurfacePatch {
                    u_range: ParameterRange::new(u_knots[*u], u_knots[*u + 1])?,
                    v_range: ParameterRange::new(v_knots[v], v_knots[v + 1])?,
                    controls: controls[v - v_degree..=v]
                        .iter()
                        .map(|row| row[*u - u_degree..=*u].to_vec())
                        .collect(),
                });
            }
        }
        Ok(patches)
    }

    pub(super) fn homogeneous_jet<const N: usize>(&self, u: f64, v: f64) -> [[Vec4; N]; N] {
        let u_basis = SplineBasisJet::<N>::new(
            self.degrees.0,
            &self.knots.0,
            self.control_points[0].len(),
            u,
        );
        let v_basis = SplineBasisJet::<N>::new(
            self.degrees.1,
            &self.knots.1,
            self.control_points.len(),
            v,
        );
        let mut jet = [[Vec4::zero(); N]; N];
        for (row, v_coefficients) in v_basis.values().iter().enumerate() {
            let row = v_basis.first + row;
            let mut along_u = [Vec4::zero(); N];
            for (column, u_coefficients) in u_basis.values().iter().enumerate() {
                let column = u_basis.first + column;
                let point = self.control_points[row][column];
                let weight = self.weights[row][column];
                let point = Vec4::new(point.x * weight, point.y * weight, point.z * weight, weight);
                for (value, coefficient) in along_u.iter_mut().zip(u_coefficients) {
                    *value += point * *coefficient;
                }
            }
            for (derivatives, value) in jet.iter_mut().zip(along_u) {
                for (derivative, coefficient) in derivatives.iter_mut().zip(v_coefficients) {
                    *derivative += value * *coefficient;
                }
            }
        }
        jet
    }
    pub(super) fn evaluation_jet<const N: usize>(&self, u: f64, v: f64) -> KernelResult<[[Vec4; N]; N]> {
        let domains = self.domains();
        let in_domain = |domain: ParameterDomain, value: f64| match domain.bounds {
            crate::DomainBounds::Bounded(range) => {
                if domain.period.is_some() {
                    return Some(domain.normalize(value));
                }
                let minimum = range.start.min(range.end);
                let maximum = range.start.max(range.end);
                let slack = f64::EPSILON
                    * minimum.abs().max(maximum.abs()).max(value.abs()).max(1.0)
                    * 256.0;
                (value >= minimum - slack && value <= maximum + slack)
                    .then_some(value.clamp(minimum, maximum))
            }
            _ => Some(value),
        };
        let Some(u) = u.is_finite().then(|| in_domain(domains[0], u)).flatten() else {
            return Err(invalid(
                "NURBS surface parameters lie outside its natural domain",
            ));
        };
        let Some(v) = v.is_finite().then(|| in_domain(domains[1], v)).flatten() else {
            return Err(invalid(
                "NURBS surface parameters lie outside its natural domain",
            ));
        };
        let jet = self.homogeneous_jet::<N>(u, v);
        if !jet.iter().flatten().all(|value| {
            value.x.is_finite() && value.y.is_finite() && value.z.is_finite() && value.w.is_finite()
        }) {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "NURBS surface evaluation produced a non-finite value",
            ));
        }
        if jet[0][0].w == 0.0 {
            return Err(geometry_error("NURBS surface has zero homogeneous weight"));
        }

        Ok(jet)
    }

    pub(super) fn first_order(&self, u: f64, v: f64) -> KernelResult<(Point3, Vec3, Vec3)> {
        let [[value, dv], [du, _]] = self.evaluation_jet::<2>(u, v)?;
        let point = value.truncate() / value.w;
        Ok((
            Point3::from_vec(point),
            (du.truncate() - point * du.w) / value.w,
            (dv.truncate() - point * dv.w) / value.w,
        ))
    }

    pub(super) fn evaluate(&self, u: f64, v: f64) -> KernelResult<SurfaceEvaluation> {
        let jet = self.evaluation_jet::<3>(u, v)?;
        let [value, dv, dvv] = jet[0];
        let [du, duv, _] = jet[1];
        let duu = jet[2][0];
        let point = value.truncate() / value.w;
        let rational_du = (du.truncate() - point * du.w) / value.w;
        let rational_dv = (dv.truncate() - point * dv.w) / value.w;
        let rational_duu = (duu.truncate() - rational_du * (2.0 * du.w) - point * duu.w) / value.w;
        let rational_duv =
            (duv.truncate() - rational_du * dv.w - rational_dv * du.w - point * duv.w) / value.w;
        let rational_dvv = (dvv.truncate() - rational_dv * (2.0 * dv.w) - point * dvv.w) / value.w;
        Ok(SurfaceEvaluation {
            point: Point3::from_vec(point),
            du: rational_du,
            dv: rational_dv,
            duu: rational_duu,
            duv: rational_duv,
            dvv: rational_dvv,
        })
    }

    pub(super) fn project(&self, point: Point3, tolerance: ModelTolerance) -> KernelResult<Projection2> {
        if !finite_point(point) {
            return Err(invalid("projection point must be finite"));
        }
        let domains = self.domains();
        let crate::DomainBounds::Bounded(u_range) = domains[0].bounds else {
            return Err(geometry_error("NURBS surface has an unbounded U domain"));
        };
        let crate::DomainBounds::Bounded(v_range) = domains[1].bounds else {
            return Err(geometry_error("NURBS surface has an unbounded V domain"));
        };
        let axis_candidates = |knots: &[f64], range: crate::ParameterRange| {
            let mut values = vec![range.start, range.end];
            for span in knots.windows(2) {
                let low = span[0].max(range.start);
                let high = span[1].min(range.end);
                if high > low {
                    values.push((low + high) * 0.5);
                }
            }
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            values.dedup_by(|a, b| (*a - *b).abs() <= tolerance.parameter);
            values
        };
        let u_candidates = axis_candidates(&self.knots.0, u_range);
        let v_candidates = axis_candidates(&self.knots.1, v_range);
        let mut best: Option<((f64, f64), f64)> = None;
        for u in &u_candidates {
            for v in &v_candidates {
                let distance = self.evaluate(*u, *v)?.point.distance2(point);
                if best.map(|candidate| distance < candidate.1).unwrap_or(true) {
                    best = Some(((*u, *v), distance));
                }
            }
        }
        let (mut u, mut v) = best
            .ok_or_else(|| {
                KernelError::new(
                    DiagnosticKind::NumericalFailure,
                    "NURBS surface projection has no initial candidate",
                )
            })?
            .0;
        let constrain = |axis: usize, value: f64| {
            if domains[axis].period.is_some() {
                value
            } else if let DomainBounds::Bounded(range) = domains[axis].bounds {
                value.clamp(range.start, range.end)
            } else {
                value
            }
        };

        let mut converged = false;
        for _ in 0..80 {
            let evaluation = self.evaluate(u, v)?;
            let residual = evaluation.point - point;
            let mut gradient = [residual.dot(evaluation.du), residual.dot(evaluation.dv)];
            let mut h00 = evaluation.du.magnitude2() + residual.dot(evaluation.duu);
            let mut h01 = evaluation.du.dot(evaluation.dv) + residual.dot(evaluation.duv);
            let mut h11 = evaluation.dv.magnitude2() + residual.dot(evaluation.dvv);
            if h00 <= 0.0 || h11 <= 0.0 || h00 * h11 <= h01 * h01 {
                h00 = evaluation.du.magnitude2();
                h01 = evaluation.du.dot(evaluation.dv);
                h11 = evaluation.dv.magnitude2();
            }
            for (axis, parameter) in [u, v].into_iter().enumerate() {
                if domains[axis].period.is_none() {
                    let DomainBounds::Bounded(range) = domains[axis].bounds else {
                        continue;
                    };
                    if (parameter <= range.start && gradient[axis] >= 0.0)
                        || (parameter >= range.end && gradient[axis] <= 0.0)
                    {
                        gradient[axis] = 0.0;
                        h01 = 0.0;
                        if axis == 0 {
                            h00 = 1.0
                        } else {
                            h11 = 1.0
                        }
                    }
                }
            }
            let determinant = h00 * h11 - h01 * h01;
            if determinant.abs() <= f64::EPSILON {
                break;
            }
            let mut du = (-gradient[0] * h11 + gradient[1] * h01) / determinant;
            let mut dv = (-gradient[1] * h00 + gradient[0] * h01) / determinant;
            let previous_distance = residual.magnitude2();
            let mut next_u = constrain(0, u + du);
            let mut next_v = constrain(1, v + dv);
            for _ in 0..12 {
                let next_distance = self.evaluate(next_u, next_v)?.point.distance2(point);
                if next_distance <= previous_distance {
                    break;
                }
                du *= 0.5;
                dv *= 0.5;
                next_u = constrain(0, u + du);
                next_v = constrain(1, v + dv);
            }
            if (next_u - u).abs() <= tolerance.parameter
                && (next_v - v).abs() <= tolerance.parameter
            {
                u = next_u;
                v = next_v;
                converged = true;
                break;
            }
            u = next_u;
            v = next_v;
        }
        if !converged {
            return Err(KernelError::new(
                DiagnosticKind::NumericalFailure,
                "NURBS surface closest-point refinement did not converge",
            ));
        }
        let projected = self.evaluate(u, v)?.point;
        Ok(Projection2 {
            parameters: (u, v),
            point: projected,
            distance: projected.distance(point),
        })
    }
}
