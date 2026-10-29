// Shared linear solvers for the voxel simulations: preconditioned conjugate
// gradients with a multigrid V-cycle as preconditioner.
// A level provides size, diagonal and apply(input, output); coarsen(level)
// returns the next coarser level and gives the finer one restrict(fine, coarse)
// and prolong(coarse, fine), which adds the coarse correction.

const coarseSize = 300


export function makeConjugateGradient(apply, precondition, size) {
  const residual = new Float64Array(size)
  const preconditioned = new Float64Array(size)
  const direction = new Float64Array(size)
  const applied = new Float64Array(size)
  // Refines solution in place until the residual falls below tolerance relative to right
  return (right, solution, tolerance, maxIterations, onProgress=() => {}) => {
    apply(solution, applied)
    let residualSquared = 0
    for(let i = 0; i < size; i++) {
      residual[i] = right[i] - applied[i]
      residualSquared += residual[i] * residual[i]
    }
    const initial = residualSquared
    const target = dot(right, right) * tolerance * tolerance
    precondition(residual, preconditioned)
    direction.set(preconditioned)
    let product = dot(residual, preconditioned)
    let iteration = 0
    for(; iteration < maxIterations && residualSquared > target; iteration++) {
      onProgress(Math.log(initial / residualSquared) / Math.log(initial / target))
      apply(direction, applied)
      const curvature = dot(direction, applied)
      if(!(curvature > 0)) break
      const alpha = product / curvature
      residualSquared = 0
      for(let i = 0; i < size; i++) {
        solution[i] += alpha * direction[i]
        residual[i] -= alpha * applied[i]
        residualSquared += residual[i] * residual[i]
      }
      precondition(residual, preconditioned)
      const next = dot(residual, preconditioned)
      const beta = next / product
      for(let i = 0; i < size; i++) direction[i] = preconditioned[i] + beta * direction[i]
      product = next
    }
    return iteration
  }
}


// Symmetric V-cycle with damped Jacobi smoothing and a dense coarsest solve,
// so it stays a valid preconditioner for conjugate gradients.
export function makeMultigrid(fine, coarsen, sweeps=2) {
  const levels = [fine]
  while(levels.at(-1).size > coarseSize) levels.push(coarsen(levels.at(-1)))
  const last = levels.length - 1
  const work = levels.map(level => ({
    right: new Float64Array(level.size),
    solution: new Float64Array(level.size),
    residual: new Float64Array(level.size),
    weights: smoothingWeights(level),
  }))
  const coarse = denseSolver(levels[last].dense(), levels[last].size)

  const cycle = (index, right, solution) => {
    if(index == last) return coarse(right, solution)
    const level = levels[index]
    const { residual, weights } = work[index]
    for(let i = 0; i < level.size; i++) solution[i] = weights[i] * right[i]
    smooth(level, right, solution, residual, weights, sweeps - 1)
    level.apply(solution, residual)
    for(let i = 0; i < level.size; i++) residual[i] = right[i] - residual[i]
    const next = work[index + 1]
    level.restrict(residual, next.right)
    cycle(index + 1, next.right, next.solution)
    level.prolong(next.solution, solution)
    smooth(level, right, solution, residual, weights, sweeps)
  }
  return (right, solution) => cycle(0, right, solution)
}


function smooth(level, right, solution, scratch, weights, sweeps) {
  for(let sweep = 0; sweep < sweeps; sweep++) {
    level.apply(solution, scratch)
    for(let i = 0; i < level.size; i++) solution[i] += weights[i] * (right[i] - scratch[i])
  }
}


// Jacobi damping of 4 / (3 λ) for the largest eigenvalue λ of D⁻¹A,
// estimated by power iteration and padded against underestimation.
function smoothingWeights(level) {
  const { size, diagonal } = level
  let vector = Float64Array.from({ length: size }, (_, i) => Math.sin(i + 1))
  let applied = new Float64Array(size)
  let eigenvalue = 0
  for(let iteration = 0; iteration < 12; iteration++) {
    const norm = Math.sqrt(dot(vector, vector))
    level.apply(vector, applied)
    let scaled = 0
    for(let i = 0; i < size; i++) {
      applied[i] /= diagonal[i]
      scaled += applied[i] * applied[i]
    }
    eigenvalue = Math.sqrt(scaled) / norm
    ;[vector, applied] = [applied, vector]
  }
  return diagonal.map(value => 4 / (3 * 1.1 * eigenvalue * value))
}


// Cholesky factorization of a dense symmetric matrix. Null pivots belong to
// directions the coarse grid cannot represent; dropping them keeps the
// coarse correction a pseudo-inverse instead of dividing by zero.
function denseSolver(matrix, size) {
  for(let j = 0; j < size; j++) {
    const row = j * size
    let pivot = matrix[row + j]
    for(let k = 0; k < j; k++) pivot -= matrix[row + k] * matrix[row + k]
    const scale = pivot > 1e-10 * matrix[row + j] ? Math.sqrt(pivot) : 0
    matrix[row + j] = scale
    for(let i = j + 1; i < size; i++) {
      const other = i * size
      let value = matrix[other + j]
      for(let k = 0; k < j; k++) value -= matrix[other + k] * matrix[row + k]
      matrix[other + j] = scale ? value / scale : 0
    }
  }
  return (right, solution) => {
    for(let i = 0; i < size; i++) {
      let value = right[i]
      for(let k = 0; k < i; k++) value -= matrix[i * size + k] * solution[k]
      const pivot = matrix[i * size + i]
      solution[i] = pivot ? value / pivot : 0
    }
    for(let i = size - 1; i >= 0; i--) {
      let value = solution[i]
      for(let k = i + 1; k < size; k++) value -= matrix[k * size + i] * solution[k]
      const pivot = matrix[i * size + i]
      solution[i] = pivot ? value / pivot : 0
    }
  }
}


// Symmetric matrix over voxel cells: diagonal plus off-diagonal rows in CSR form.
// Coordinates hold each cell's grid position for coarsening.
export function cellLevel({ diagonal, offsets, columns, values, coordinates }) {
  const size = diagonal.length
  return {
    size, diagonal, offsets, columns, values, coordinates,
    apply(input, output) {
      for(let row = 0; row < size; row++) {
        let value = diagonal[row] * input[row]
        for(let slot = offsets[row]; slot < offsets[row + 1]; slot++) value += values[slot] * input[columns[slot]]
        output[row] = value
      }
    },
    dense() {
      const matrix = new Float64Array(size * size)
      for(let row = 0; row < size; row++) {
        matrix[row * size + row] += diagonal[row]
        for(let slot = offsets[row]; slot < offsets[row + 1]; slot++) matrix[row * size + columns[slot]] += values[slot]
      }
      return matrix
    },
  }
}


// Merges each 2×2×2 block of cells into one coarse cell (piecewise constant
// prolongation) and forms the Galerkin product PᵀAP.
export function coarsenCells(level) {
  const { size, diagonal, offsets, columns, values } = level
  const coordinates = level.coordinates.map(value => value >> 1)
  const extent = [0, 1, 2].map(axis => {
    let maximum = 0
    for(let cell = 0; cell < size; cell++) maximum = Math.max(maximum, coordinates[cell * 3 + axis])
    return maximum + 1
  })
  const blocks = new Int32Array(extent[0] * extent[1] * extent[2]).fill(-1)
  const aggregates = new Int32Array(size)
  const coarseCoordinates = []
  for(let cell = 0; cell < size; cell++) {
    const block = coordinates[cell * 3] + extent[0] * (coordinates[cell * 3 + 1] + extent[1] * coordinates[cell * 3 + 2])
    if(blocks[block] < 0) {
      blocks[block] = coarseCoordinates.length / 3
      coarseCoordinates.push(...coordinates.subarray(cell * 3, cell * 3 + 3))
    }
    aggregates[cell] = blocks[block]
  }

  const coarseCount = coarseCoordinates.length / 3
  const memberOffsets = new Int32Array(coarseCount + 1)
  aggregates.forEach(aggregate => memberOffsets[aggregate + 1]++)
  for(let aggregate = 0; aggregate < coarseCount; aggregate++) memberOffsets[aggregate + 1] += memberOffsets[aggregate]
  const members = new Int32Array(size)
  const filled = memberOffsets.slice(0, coarseCount)
  aggregates.forEach((aggregate, cell) => members[filled[aggregate]++] = cell)

  const coarseDiagonal = new Float64Array(coarseCount)
  const coarseOffsets = new Int32Array(coarseCount + 1)
  const coarseColumns = []
  const coarseValues = []
  const slots = new Int32Array(coarseCount).fill(-1)
  for(let aggregate = 0; aggregate < coarseCount; aggregate++) {
    const start = coarseColumns.length
    coarseOffsets[aggregate] = start
    for(let member = memberOffsets[aggregate]; member < memberOffsets[aggregate + 1]; member++) {
      const cell = members[member]
      coarseDiagonal[aggregate] += diagonal[cell]
      for(let slot = offsets[cell]; slot < offsets[cell + 1]; slot++) {
        const other = aggregates[columns[slot]]
        if(other == aggregate) {
          coarseDiagonal[aggregate] += values[slot]
          continue
        }
        if(slots[other] < start) {
          slots[other] = coarseColumns.length
          coarseColumns.push(other)
          coarseValues.push(0)
        }
        coarseValues[slots[other]] += values[slot]
      }
    }
  }
  coarseOffsets[coarseCount] = coarseColumns.length

  level.restrict = (fine, coarse) => {
    coarse.fill(0)
    for(let cell = 0; cell < size; cell++) coarse[aggregates[cell]] += fine[cell]
  }
  level.prolong = (coarse, fine) => {
    for(let cell = 0; cell < size; cell++) fine[cell] += coarse[aggregates[cell]]
  }
  return cellLevel({
    diagonal: coarseDiagonal,
    offsets: coarseOffsets,
    columns: Int32Array.from(coarseColumns),
    values: Float64Array.from(coarseValues),
    coordinates: Int32Array.from(coarseCoordinates),
  })
}


export function dot(a, b) {
  let value = 0
  for(let i = 0; i < a.length; i++) value += a[i] * b[i]
  return value
}
