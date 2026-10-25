//! Small dense linear solves: Newton corrections and B-spline interpolation.

use std::ops::{Div, Mul, Sub};

use crate::geometry::basis_values;
use crate::{invalid, KernelResult};

/// Solve `matrix · x = right` by Gauss-Jordan elimination with partial
/// pivoting. Returns `None` for a numerically singular system.
pub(crate) fn solve_linear<const N: usize>(
    mut matrix: [[f64; N]; N],
    mut right: [f64; N],
) -> Option<[f64; N]> {
    for column in 0..N {
        let pivot = (column..N).max_by(|first, second| {
            matrix[*first][column]
                .abs()
                .total_cmp(&matrix[*second][column].abs())
        })?;
        if matrix[pivot][column].abs() <= f64::EPSILON * 256.0 {
            return None;
        }
        matrix.swap(column, pivot);
        right.swap(column, pivot);
        let divisor = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= divisor;
        }
        right[column] /= divisor;
        for row in 0..N {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            for index in column..N {
                matrix[row][index] -= factor * matrix[column][index];
            }
            right[row] -= factor * right[column];
        }
    }
    Some(right)
}

/// A clamped interpolation basis for strictly increasing sample parameters,
/// factored once so every row of a control net solves against it.
pub(crate) struct Interpolation {
    pub(crate) degree: usize,
    pub(crate) knots: Vec<f64>,
    factors: Vec<Vec<f64>>,
    pivots: Vec<usize>,
}

impl Interpolation {
    pub(crate) fn new(parameters: &[f64]) -> KernelResult<Self> {
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
            let (first, values) = basis_values(degree, &knots, count, *parameter);
            factors[row][first..first + values.len()].copy_from_slice(&values);
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

    /// Solve for the controls interpolating `values` at the factored
    /// parameters; `T` is a point coordinate vector or a homogeneous one.
    pub(crate) fn solve<T>(&self, values: &[T]) -> Vec<T>
    where
        T: Copy + Sub<Output = T> + Mul<f64, Output = T> + Div<f64, Output = T>,
    {
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
