//! Small dense linear solves used by Newton corrections.

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
