// Gaussian elimination with partial pivoting.
export function solveLinear(matrix, rhs) {
  const n = rhs.length
  const rows = matrix.map((row, i) => [...row, rhs[i]])
  for(let column = 0; column < n; column++) {
    const pivot = rows.slice(column).reduce((best, row, i) =>
      Math.abs(row[column]) > Math.abs(rows[best][column]) ? column + i : best, column)
    ;[rows[column], rows[pivot]] = [rows[pivot], rows[column]]
    for(let row = column + 1; row < n; row++) {
      const factor = rows[row][column] / rows[column][column]
      for(let i = column; i <= n; i++) rows[row][i] -= factor * rows[column][i]
    }
  }
  const solution = Array(n).fill(0)
  for(let row = n - 1; row >= 0; row--) {
    solution[row] = (rows[row][n] - rows[row].slice(row + 1, n).reduce((sum, value, i) =>
      sum + value * solution[row + 1 + i], 0)) / rows[row][row]
  }
  return solution
}
