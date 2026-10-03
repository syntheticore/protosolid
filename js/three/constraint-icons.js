import { Line, Arc, Spline } from '../core/geom2d.js'
import { cross2d } from '../core/utils.js'

export function perpendicularPoint(constraint) {
  const spline = constraint.items.find(item => item.curve() instanceof Spline)
  if(spline) return spline.curve().handles()[spline.index]
  const curves = constraint.items.map(item => item.curve())
  const line = curves.find(curve => curve instanceof Line)
  const round = curves.find(curve => curve !== line)
  if(round instanceof Line) {
    const start = line.points[0]
    const direction = line.direction()
    const otherDirection = round.direction()
    const parameter = cross2d(round.points[0].clone().sub(start), otherDirection) / cross2d(direction, otherDirection)
    return start.clone().addScaledVector(direction, parameter)
  }
  const center = round.center()
  const direction = line.direction().normalize()
  const intersections = [-1, 1].map(sign => center.clone().addScaledVector(direction, sign * round.radius))
  const onArc = round instanceof Arc ? intersections.filter(point => round.distanceTo(point).almost(0.0)) : intersections
  const candidates = onArc.length ? onArc : intersections
  const distance = point => Math.min(...line.endpoints().map(endpoint => endpoint.distanceToSquared(point)))
  return candidates.reduce((nearest, point) => distance(point) < distance(nearest) ? point : nearest)
}

export function tangentPoint(constraint) {
  const splineItem = constraint.items.find(item => item.curve() instanceof Spline)
  if(splineItem) {
    const spline = splineItem.curve()
    if(splineItem.index !== undefined) return spline.handles()[splineItem.index]
    const other = constraint.items.find(item => item !== splineItem).curve()
    return spline.endpoints().reduce((nearest, point) =>
      other.distanceTo(point) < other.distanceTo(nearest) ? point : nearest
    )
  }

  const curves = constraint.items.map(item => item.curve())
  const line = curves.find(curve => curve instanceof Line)
  if(line) {
    const round = curves.find(curve => curve !== line)
    const direction = line.direction().normalize()
    return line.points[0].clone().addScaledVector(direction,
      round.center().sub(line.points[0]).dot(direction))
  }

  // The contact lies on the centers' axis, including internal tangency.
  const [first, second] = curves
  const center = first.center()
  const direction = second.center().sub(center).normalize()
  const candidates = [
    center.clone().addScaledVector(direction, first.radius),
    center.clone().addScaledVector(direction, -first.radius),
  ]
  return candidates.reduce((nearest, point) =>
    Math.abs(point.distanceTo(second.center()) - second.radius) <
    Math.abs(nearest.distanceTo(second.center()) - second.radius) ? point : nearest
  )
}

export function layoutConstraintIcons(entries) {
  // Icons are 20px including padding; leave a small gap between neighbors.
  const spacing = 24
  const groups = entries.flatMap(([id, proxies]) => proxies.map((proxy, index) => ({
    x: proxy.coords.x,
    y: proxy.coords.y,
    icons: [{ id, index, proxy }],
  })))

  // Recheck expanded rows so that a row cannot cover a neighboring icon.
  for(let i = 0; i < groups.length; i++) {
    for(let j = i + 1; j < groups.length; j++) {
      const a = groups[i]
      const b = groups[j]
      if(Math.abs(a.y - b.y) >= spacing ||
        a.x + a.icons.length * spacing <= b.x ||
        b.x + b.icons.length * spacing <= a.x) continue
      a.x = Math.min(a.x, b.x)
      a.y = Math.min(a.y, b.y)
      a.icons.push(...b.icons)
      groups.splice(j, 1)
      i = -1
      break
    }
  }

  const result = new Map(entries.map(([id]) => [id, []]))
  for(const group of groups) {
    group.icons.forEach(({ id, index, proxy }, column) => {
      result.get(id)[index] = {
        ...proxy,
        coords: { x: group.x + column * spacing, y: group.y },
      }
    })
  }
  return result
}
