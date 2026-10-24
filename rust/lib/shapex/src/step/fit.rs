//! Cubic interpolation of procedural geometry.
//!
//! Exchange formats carry analytic and B-spline entities only. Procedural
//! supports and implicit curves are sampled in their own parameterization and
//! interpolated until the midpoints between samples agree within tolerance.
//! Samples are added only to intervals that miss it: blend supports follow
//! their spines' knots, across which uniform refinement converges slowly.

use std::collections::HashMap;

use cgmath::prelude::*;

use crate::{
    invalid, Curve3, KernelResult, NurbsCurve3, NurbsSurface3, ParameterRange, Point3, Surface3,
    Vec3,
};

const INITIAL_SAMPLES: usize = 9;
const MAXIMUM_SAMPLES: usize = 257;

/// A clamped interpolation basis for strictly increasing sample parameters,
/// factored once so every row of a control net solves against it.
struct Interpolation {
    degree: usize,
    knots: Vec<f64>,
    factors: Vec<Vec<f64>>,
    pivots: Vec<usize>,
}

impl Interpolation {
    fn new(parameters: &[f64]) -> KernelResult<Self> {
        let count = parameters.len();
        let degree = 3.min(count - 1);
        // Knot averaging keeps the collocation matrix totally positive.
        let mut knots = vec![parameters[0]; degree + 1];
        for index in 1..count - degree {
            knots.push(parameters[index..index + degree].iter().sum::<f64>() / degree as f64);
        }
        knots.extend(vec![parameters[count - 1]; degree + 1]);

        let mut factors = vec![vec![0.0; count]; count];
        for (row, parameter) in parameters.iter().enumerate() {
            let span = find_span(&knots, degree, count, *parameter);
            for (offset, value) in basis(&knots, degree, span, *parameter).into_iter().enumerate() {
                factors[row][span - degree + offset] = value;
            }
        }
        let mut pivots = (0..count).collect::<Vec<_>>();
        for column in 0..count {
            let pivot = (column..count)
                .max_by(|a, b| factors[*a][column].abs().total_cmp(&factors[*b][column].abs()))
                .unwrap();
            if factors[pivot][column].abs() <= 1.0e-14 {
                return Err(invalid("interpolation samples have a singular basis"));
            }
            factors.swap(column, pivot);
            pivots.swap(column, pivot);
            for row in column + 1..count {
                let factor = factors[row][column] / factors[column][column];
                factors[row][column] = factor;
                for index in column + 1..count {
                    factors[row][index] -= factor * factors[column][index];
                }
            }
        }
        Ok(Self { degree, knots, factors, pivots })
    }

    fn solve(&self, values: &[Vec3]) -> Vec<Vec3> {
        let count = values.len();
        let mut result = self.pivots.iter().map(|index| values[*index]).collect::<Vec<_>>();
        for row in 0..count {
            for index in 0..row {
                result[row] = result[row] - result[index] * self.factors[row][index];
            }
        }
        for row in (0..count).rev() {
            for index in row + 1..count {
                result[row] = result[row] - result[index] * self.factors[row][index];
            }
            result[row] = result[row] / self.factors[row][row];
        }
        result
    }
}

fn find_span(knots: &[f64], degree: usize, count: usize, parameter: f64) -> usize {
    if parameter >= knots[count] {
        return count - 1;
    }
    let (mut low, mut high) = (degree, count);
    while high - low > 1 {
        let middle = (low + high) / 2;
        if parameter < knots[middle] {
            high = middle;
        } else {
            low = middle;
        }
    }
    low
}

fn basis(knots: &[f64], degree: usize, span: usize, parameter: f64) -> Vec<f64> {
    let mut values = vec![0.0; degree + 1];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    values[0] = 1.0;
    for j in 1..=degree {
        left[j] = parameter - knots[span + 1 - j];
        right[j] = knots[span + j] - parameter;
        let mut saved = 0.0;
        for r in 0..j {
            let temp = values[r] / (right[r + 1] + left[j - r]);
            values[r] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        values[j] = saved;
    }
    values
}

fn samples(range: ParameterRange, count: usize) -> Vec<f64> {
    (0..count)
        .map(|index| range.start + range.span() * index as f64 / (count - 1) as f64)
        .collect()
}

/// Insert the midpoint of every interval whose error exceeds `tolerance`
/// while the sample budget lasts. Returns whether any sample was added.
fn refine(parameters: &mut Vec<f64>, errors: &[f64], tolerance: f64) -> bool {
    let mut budget = MAXIMUM_SAMPLES.saturating_sub(parameters.len());
    let mut refined = Vec::with_capacity(parameters.len() * 2);
    for (index, pair) in parameters.windows(2).enumerate() {
        refined.push(pair[0]);
        if errors[index] > tolerance && budget > 0 {
            refined.push((pair[0] + pair[1]) * 0.5);
            budget -= 1;
        }
    }
    refined.push(*parameters.last().unwrap());
    let added = refined.len() > parameters.len();
    *parameters = refined;
    added
}

/// Exact evaluations, reused across refinement passes.
struct Samples<F> {
    evaluate: F,
    values: HashMap<(u64, u64), Vec3>,
}

impl<F: Fn(f64, f64) -> KernelResult<Point3>> Samples<F> {
    fn at(&mut self, u: f64, v: f64) -> KernelResult<Vec3> {
        let key = (u.to_bits(), v.to_bits());
        if let Some(value) = self.values.get(&key) {
            return Ok(*value);
        }
        let value = (self.evaluate)(u, v)?.to_vec();
        self.values.insert(key, value);
        Ok(value)
    }
}

/// The curve parameter at each fraction of the arc length of `curve` over
/// `range`. Tangential intersections reach their ends at unbounded parameter
/// speed, where interpolation in the native parameter converges poorly.
struct ArcLength {
    parameters: Vec<f64>,
    lengths: Vec<f64>,
}

impl ArcLength {
    fn new(curve: &Curve3, range: ParameterRange) -> KernelResult<Self> {
        let point = |parameter: f64| Ok(curve.evaluate(parameter)?.point);
        let mut samples = samples(range, 65)
            .into_iter()
            .map(|parameter| Ok((parameter, point(parameter)?)))
            .collect::<KernelResult<Vec<_>>>()?;
        let total = |samples: &[(f64, Point3)]| samples.windows(2).map(|pair| pair[0].1.distance(pair[1].1)).sum::<f64>();
        for _ in 0..12 {
            let limit = total(&samples) / 1024.0;
            let mut refined = vec![samples[0]];
            for pair in samples.windows(2) {
                if pair[0].1.distance(pair[1].1) > limit {
                    let middle = (pair[0].0 + pair[1].0) * 0.5;
                    refined.push((middle, point(middle)?));
                }
                refined.push(pair[1]);
            }
            if refined.len() == samples.len() {
                break;
            }
            samples = refined;
        }
        let mut lengths = vec![0.0];
        for pair in samples.windows(2) {
            lengths.push(lengths.last().unwrap() + pair[0].1.distance(pair[1].1));
        }
        let total = *lengths.last().unwrap();
        Ok(Self {
            parameters: samples.iter().map(|(parameter, _)| *parameter).collect(),
            lengths: lengths.into_iter().map(|length| length / total).collect(),
        })
    }

    fn parameter(&self, fraction: f64) -> f64 {
        let index = self.lengths.partition_point(|length| *length < fraction).clamp(1, self.lengths.len() - 1);
        let (a, b) = (self.lengths[index - 1], self.lengths[index]);
        let weight = if b > a { (fraction - a) / (b - a) } else { 0.0 };
        self.parameters[index - 1] + (self.parameters[index] - self.parameters[index - 1]) * weight
    }
}

/// A cubic B-spline through `curve` over `range`, directed from its start to
/// its end and parameterized by arc length over `[0, 1]`.
pub(super) fn curve(curve: &Curve3, range: ParameterRange, tolerance: f64) -> KernelResult<NurbsCurve3> {
    let arc = ArcLength::new(curve, range)?;
    let mut exact = Samples {
        evaluate: |fraction: f64, _| Ok(curve.evaluate(arc.parameter(fraction))?.point),
        values: HashMap::new(),
    };
    let mut parameters = samples(ParameterRange { start: 0.0, end: 1.0 }, INITIAL_SAMPLES);
    loop {
        let points = parameters.iter().map(|t| exact.at(*t, 0.0)).collect::<KernelResult<Vec<_>>>()?;
        let interpolation = Interpolation::new(&parameters)?;
        let fitted = NurbsCurve3::new(
            interpolation.degree,
            interpolation.solve(&points).into_iter().map(Point3::from_vec).collect(),
            vec![1.0; parameters.len()],
            interpolation.knots.clone(),
        )?;
        let evaluated = Curve3::Nurbs(fitted.clone());
        let errors = parameters
            .windows(2)
            .map(|pair| {
                let middle = (pair[0] + pair[1]) * 0.5;
                Ok(evaluated.evaluate(middle)?.point.to_vec().distance(exact.at(middle, 0.0)?))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        if !refine(&mut parameters, &errors, tolerance) {
            return Ok(fitted);
        }
    }
}

/// A bicubic B-spline through `surface` over `bounds`, retaining its
/// parameterization and therefore its natural normal.
pub(super) fn surface(
    surface: &Surface3,
    bounds: [ParameterRange; 2],
    tolerance: f64,
) -> KernelResult<NurbsSurface3> {
    // A face around (nearly) a whole period gets a support of exactly one
    // period whose seam columns coincide, so importers recover it as closed.
    let mut bounds = bounds;
    let mut closed = [false; 2];
    for (axis, domain) in surface.domains().into_iter().enumerate() {
        if let Some(period) = domain.period {
            if bounds[axis].span().abs() >= period * 0.99 {
                let start = bounds[axis].start.min(bounds[axis].end);
                bounds[axis] = ParameterRange::new(start, start + period)?;
                closed[axis] = true;
            }
        }
    }
    let mut us = samples(bounds[0], INITIAL_SAMPLES);
    let mut vs = samples(bounds[1], INITIAL_SAMPLES);
    let ends = [us[us.len() - 1], vs[vs.len() - 1]];
    let seam = |value: f64, axis: usize| if closed[axis] && value == ends[axis] { bounds[axis].start } else { value };
    let mut exact = Samples {
        evaluate: |u: f64, v: f64| Ok(surface.evaluate(seam(u, 0), seam(v, 1))?.point),
        values: HashMap::new(),
    };
    loop {
        let along_u = Interpolation::new(&us)?;
        let along_v = Interpolation::new(&vs)?;
        let solved_rows = vs
            .iter()
            .map(|v| {
                let row = us.iter().map(|u| exact.at(*u, *v)).collect::<KernelResult<Vec<_>>>()?;
                Ok(along_u.solve(&row))
            })
            .collect::<KernelResult<Vec<_>>>()?;
        let mut controls = vec![vec![Point3::origin(); us.len()]; vs.len()];
        for column in 0..us.len() {
            let values = solved_rows.iter().map(|row| row[column]).collect::<Vec<_>>();
            for (row, value) in along_v.solve(&values).into_iter().enumerate() {
                controls[row][column] = Point3::from_vec(value);
            }
        }
        let fitted = NurbsSurface3::new(
            (along_u.degree, along_v.degree),
            controls,
            vec![vec![1.0; us.len()]; vs.len()],
            (along_u.knots.clone(), along_v.knots.clone()),
        )?;
        let evaluated = Surface3::Nurbs(fitted.clone());
        let mut error = |u: f64, v: f64| -> KernelResult<f64> {
            Ok(evaluated.evaluate(u, v)?.point.to_vec().distance(exact.at(u, v)?))
        };
        // Each interval's error is the worst midpoint on its grid lines. A
        // cell center refines both of its intervals only when neither line
        // already accounts for its error.
        let middles = |parameters: &[f64]| parameters.windows(2).map(|pair| (pair[0] + pair[1]) * 0.5).collect::<Vec<_>>();
        let (middle_us, middle_vs) = (middles(&us), middles(&vs));
        let mut u_errors = vec![0.0f64; middle_us.len()];
        let mut v_errors = vec![0.0f64; middle_vs.len()];
        for v in &vs {
            for (column, u) in middle_us.iter().enumerate() {
                u_errors[column] = u_errors[column].max(error(*u, *v)?);
            }
        }
        for u in &us {
            for (row, v) in middle_vs.iter().enumerate() {
                v_errors[row] = v_errors[row].max(error(*u, *v)?);
            }
        }
        let (line_u, line_v) = (u_errors.clone(), v_errors.clone());
        for (row, v) in middle_vs.iter().enumerate() {
            for (column, u) in middle_us.iter().enumerate() {
                if line_u[column] <= tolerance && line_v[row] <= tolerance {
                    let center = error(*u, *v)?;
                    u_errors[column] = u_errors[column].max(center);
                    v_errors[row] = v_errors[row].max(center);
                }
            }
        }
        let refined_u = refine(&mut us, &u_errors, tolerance);
        let refined_v = refine(&mut vs, &v_errors, tolerance);
        if !refined_u && !refined_v {
            return Ok(fitted);
        }
    }
}
