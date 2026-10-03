import { EPSILON } from './utils.js'

// Return the tangent points and arc midpoint without changing either line.
export function lineFillet(left, right, radius) {
  if(!Number.isFinite(radius) || radius <= EPSILON) throw new Error('Enter a positive fillet radius')
  const indices = left.points.flatMap((point, i) => right.points
    .flatMap((other, j) => point.almost(other) ? [[i, j]] : []))[0]
  if(!indices) throw new Error('Select two lines with a shared endpoint')
  const corner = left.points[indices[0]]
  const directions = [left, right].map((line, i) =>
    line.points[1 - indices[i]].clone().sub(corner).normalize())
  const cosine = Math.max(-1, Math.min(1, directions[0].dot(directions[1])))
  const halfAngle = Math.acos(cosine) / 2
  if(halfAngle <= EPSILON || Math.PI / 2 - halfAngle <= EPSILON) {
    throw new Error('Parallel lines cannot form a fillet')
  }
  const trim = radius / Math.tan(halfAngle)
  if([left, right].some(line => trim >= line.points[0].distanceTo(line.points[1]) - EPSILON)) {
    throw new Error('The fillet radius is too large for this corner')
  }
  const bisector = directions[0].clone().add(directions[1]).normalize()
  const center = corner.clone().addScaledVector(bisector, radius / Math.sin(halfAngle))
  const points = directions.map(direction => corner.clone().addScaledVector(direction, trim))
  const middle = center.clone().addScaledVector(bisector, -radius)
  return { indices, points, middle, center }
}
