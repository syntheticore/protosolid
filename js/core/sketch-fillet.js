import * as THREE from 'three'
import { Line, Arc, Spline } from './geom2d.js'
import { EPSILON, cross2d } from './utils.js'
import { modelTolerance, getSketchTessellationTolerance } from './shapex.js'
import { isModelingFailure } from './errors.js'

const vector = point => new THREE.Vector3(point.x, point.y, 0)

export const supportsFillet = curve => curve instanceof Line || curve instanceof Arc || curve instanceof Spline

// Return the tangent points and fillet arc without changing either curve.
// Both curves are traversed away from the shared corner; offsetting each
// toward the inside of the corner by the radius meets at the arc center.
export function curveFillet(left, right, radius) {
  if(!Number.isFinite(radius) || radius <= EPSILON) throw new Error('Enter a positive fillet radius')
  const ends = [0, 1].flatMap(i => [0, 1].flatMap(j =>
    left.endpoints()[i].almost(right.endpoints()[j]) ? [[i, j]] : []))[0]
  if(!ends) throw new Error('Select two curves with a shared endpoint')
  const curves = [left, right].map((elem, i) => ends[i] ? elem.geom().reversed() : elem.geom())
  const corner = vector(curves[0].start())
  const tangents = curves.map(curve => {
    const range = curve.range()
    return vector(curve.evaluate(range.start).firstDerivative).multiplyScalar(Math.sign(range.span())).normalize()
  })
  const turn = cross2d(...tangents)
  if(Math.abs(turn) <= EPSILON) {
    throw new Error(tangents[0].dot(tangents[1]) < 0 ? 'Tangent curves cannot form a fillet' : 'Curves meeting in a cusp cannot form a fillet')
  }
  // Positive offsets lie left of the traversal direction.
  const side = Math.sign(turn)
  let hits = []
  try {
    const offsets = curves.map((curve, i) => curve.offset((i ? -side : side) * radius))
    hits = offsets[0].intersections(offsets[1], modelTolerance).filter(hit => hit.kind == 'point')
  } catch(error) {
    if(!isModelingFailure(error)) throw error
  }
  const candidates = hits.map(hit => {
    const parameters = [hit.firstParameter, hit.secondParameter]
    const points = curves.map((curve, i) => vector(curve.evaluate(parameters[i]).point))
    return { parameters, points, center: vector(hit.point) }
  }).filter(({ points }) => points.every((point, i) =>
    !point.almost(corner) && !point.almost(vector(curves[i].end()))))
  if(!candidates.length) throw new Error('The fillet radius is too large for this corner')
  const { parameters, points, center } = candidates.minMaxBy(Math.min, ({ points }) =>
    points[0].distanceTo(corner) + points[1].distanceTo(corner))

  // The arc replaces the corner, so it turns through the corner's side.
  const angle = point => Math.atan2(point.y - center.y, point.x - center.x)
  const start = angle(points[0])
  const span = THREE.MathUtils.euclideanModulo(angle(points[1]) - start, Math.PI * 2)
  const sweep = THREE.MathUtils.euclideanModulo(angle(corner) - start, Math.PI * 2) < span ? span : span - Math.PI * 2
  const middle = center.clone().add(new THREE.Vector3(Math.cos(start + sweep / 2), Math.sin(start + sweep / 2), 0)
    .multiplyScalar(radius))
  return { ends, parameters, points, center, middle, start, sweep }
}

// Keep a curve from a point on it to its other end, in its own traversal
// direction, cutting off the given end. Returns the replacement and the
// handle index at the point.
export function trimToPoint(elem, end, parameter, point) {
  if(elem instanceof Line) {
    const points = elem.points.map(point => point.clone())
    points[end] = point
    return { curve: new Line(...points), index: end }
  }
  const range = elem.geom().range()
  const bounds = end ? [range.start, parameter] : [parameter, range.end]
  if(elem instanceof Arc) {
    return { curve: new Arc(elem.center(), elem.radius, bounds[0], bounds[1] - bounds[0]), index: end + 1 }
  }
  return fitSplinePiece(elem, bounds, parameter)
}

// The solver can only attach to the ends of a sketch spline's control
// polygon, so a trimmed piece is refit as a clamped uniform spline.
// Endpoints and end tangents are kept exactly. The fit uses the fewest
// control points that stay within the sketch display tolerance, starting at
// four, the fewest that keep both end tangents, and at most the original
// count. A piece within a single polynomial span is reproduced exactly by at
// most six points. When no count is within tolerance, the closest fit is used.
function fitSplinePiece(spline, bounds, parameter) {
  const basis = spline.geom().curve()
  const original = spline.points.length
  const [low, high] = bounds[0] < bounds[1] ? bounds : [bounds[1], bounds[0]]
  const sampleCount = original * 16
  const targets = Array.from({ length: sampleCount + 1 }, (_, k) =>
    vector(basis.evaluate(low + (high - low) * k / sampleCount).point))
  const tangents = [low, high].map(u => vector(basis.evaluate(u).firstDerivative).normalize())
  const tolerance = getSketchTessellationTolerance()

  let best
  for(let count = Math.min(4, original); count <= original; count++) {
    const fit = fitSpline(targets, tangents, count, tolerance)
    if(!best || fit.error < best.error) best = fit
    if(fit.error <= tolerance) break
  }
  // Clamped uniform knots are symmetric, so reversing the control points
  // reverses the traversal without changing the curve.
  const reversed = bounds[0] > bounds[1]
  const { points } = best
  return {
    curve: new Spline(reversed ? points.toReversed() : points),
    index: (parameter == low) != reversed ? 0 : points.length - 1,
  }
}

// Least-squares fit of a clamped uniform spline with the given control
// point count to samples along a curve, keeping the end points and, from four
// control points, the end tangent directions. Returns the control points and
// the largest distance of a sample from the fit.
function fitSpline(targets, tangents, count, tolerance) {
  const degree = Math.min(5, count - 1), domain = count - degree
  const [values, multiplicities] = Spline.clampedKnots(count, degree)
  const knots = values.flatMap((value, i) => Array(multiplicities[i]).fill(value))
  const first = targets[0], last = targets.at(-1)

  // Each control point is a constant plus a combination of the unknowns.
  const unit = [new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0)]
  let unknowns = 0
  const free = () => ({ constant: new THREE.Vector3(), terms: unit.map(direction => [unknowns++, direction]) })
  const poles = Array(count).fill(null)
  poles[0] = { constant: first, terms: [] }
  poles[count - 1] = { constant: last, terms: [] }
  if(count > 3) {
    poles[1] = { constant: first, terms: [[unknowns++, tangents[0]]] }
    poles[count - 2] = { constant: last, terms: [[unknowns++, tangents[1].clone().negate()]] }
  }
  for(let i = 1; i < count - 1; i++) poles[i] ||= free()

  const samples = targets.map((target, k) => ({ t: domain * k / (targets.length - 1), target }))
  const fit = () => {
    const matrix = Array.from({ length: unknowns }, () => Array(unknowns).fill(0))
    const rhs = Array(unknowns).fill(0)
    samples.forEach(({ t, target }) => {
      const weights = basisFunctions(knots, degree, t)
      const residual = target.clone()
      const gradients = Array.from({ length: unknowns }, () => new THREE.Vector3())
      poles.forEach((pole, i) => {
        residual.addScaledVector(pole.constant, -weights[i])
        pole.terms.forEach(([unknown, direction]) => gradients[unknown].addScaledVector(direction, weights[i]))
      })
      gradients.forEach((gradient, j) => {
        gradients.forEach((other, l) => matrix[j][l] += gradient.dot(other))
        rhs[j] += gradient.dot(residual)
      })
    })
    const solution = solveLinear(matrix, rhs)
    return poles.map(pole => pole.terms.reduce((point, [unknown, direction]) =>
      point.addScaledVector(direction, solution[unknown]), pole.constant.clone()))
  }
  // Reproject the samples onto each fit, so the parametrization stops
  // contributing to the shape error. Stop once that no longer helps.
  let previous = Infinity
  for(let iteration = 0; ; iteration++) {
    const points = fit()
    const geom = new Spline(points).geom()
    const projections = samples.map(sample => projectNear(geom, sample.target, sample.t, domain))
    const error = Math.max(...projections.map(projection => projection.distance))
    if(error <= tolerance || error > previous * 0.95 || iteration == 8) return { points, error }
    previous = error
    samples.slice(1, -1).forEach((sample, k) => sample.t = projections[k + 1].t)
  }
}

// Closest point on a curve near a parameter, by Newton steps. A sample
// starts close to its projection, and this is far cheaper than a global
// closest point search.
function projectNear(geom, target, t, domain) {
  let distance
  for(let step = 0; step < 4; step++) {
    const { point, firstDerivative: d1, secondDerivative: d2 } = geom.evaluate(t)
    const dx = point.x - target.x, dy = point.y - target.y
    distance = Math.hypot(dx, dy)
    const slope = d1.x * d1.x + d1.y * d1.y + dx * d2.x + dy * d2.y
    if(slope <= 0) break
    t = THREE.MathUtils.clamp(t - (dx * d1.x + dy * d1.y) / slope, 0, domain)
  }
  return { t, distance }
}

// Cox-de Boor recursion; the closing knot belongs to the last nonempty span.
function basisFunctions(knots, degree, t) {
  const span = knots.findLastIndex((knot, i) => knot <= t && knot < knots[i + 1])
  let values = knots.slice(0, -1).map((_, i) => i == span ? 1 : 0)
  for(let p = 1; p <= degree; p++) {
    values = values.slice(0, -1).map((value, i) => {
      const left = knots[i + p] - knots[i], right = knots[i + p + 1] - knots[i + 1]
      return (left ? (t - knots[i]) / left * value : 0) + (right ? (knots[i + p + 1] - t) / right * values[i + 1] : 0)
    })
  }
  return values
}

// Gaussian elimination with partial pivoting.
function solveLinear(matrix, rhs) {
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
