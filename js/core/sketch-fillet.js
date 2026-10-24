import * as THREE from 'three'
import { Line, Arc, Spline } from './geom2d.js'
import { EPSILON, cross2d } from './utils.js'
import { modelTolerance, point2 } from './shapex.js'
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

// Keep a curve from its tangent point to its far end, in its own traversal
// direction. Returns the replacement and the handle index meeting the fillet.
export function trimToFillet(elem, end, parameter, point) {
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
// polygon, so a trimmed piece is refit as a clamped uniform spline with the
// same control point count. Endpoints and end tangents are kept exactly. With
// at most degree + 1 control points the fit is the exact Bezier subdivision;
// with interior knots it is a least-squares approximation of the piece.
function fitSplinePiece(spline, bounds, parameter) {
  const basis = spline.geom().curve()
  const degree = basis.degree(), knots = basis.knots(), count = spline.points.length
  const domain = count - degree
  const [low, high] = bounds[0] < bounds[1] ? bounds : [bounds[1], bounds[0]]
  const first = vector(basis.evaluate(low).point), last = vector(basis.evaluate(high).point)

  // Each control point is a constant plus a combination of the unknowns.
  const unit = [new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0)]
  let unknowns = 0
  const free = () => ({ constant: new THREE.Vector3(), terms: unit.map(direction => [unknowns++, direction]) })
  const poles = Array(count).fill(null)
  poles[0] = { constant: first, terms: [] }
  poles[count - 1] = { constant: last, terms: [] }
  if(count > 3) {
    const tangent = u => vector(basis.evaluate(u).firstDerivative).normalize()
    poles[1] = { constant: first, terms: [[unknowns++, tangent(low)]] }
    poles[count - 2] = { constant: last, terms: [[unknowns++, tangent(high).negate()]] }
  }
  for(let i = 1; i < count - 1; i++) poles[i] ||= free()

  const sampleCount = count * 16
  const samples = Array.from({ length: sampleCount + 1 }, (_, k) => ({
    t: domain * k / sampleCount,
    target: vector(basis.evaluate(low + (high - low) * k / sampleCount).point),
  }))
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
  let points = fit()
  // Reproject the samples onto each fit, so the parametrization stops
  // contributing to the shape error.
  for(let iteration = 0; iteration < 8 && count > degree + 1; iteration++) {
    const geom = new Spline(points).geom()
    samples.slice(1, -1).forEach(sample => sample.t = geom.closestPoint(point2(sample.target), modelTolerance).parameter)
    points = fit()
  }
  const reversed = bounds[0] > bounds[1]
  return {
    curve: new Spline(points, reversed ? [domain, 0] : null),
    index: parameter == low ? 0 : count - 1,
  }
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
