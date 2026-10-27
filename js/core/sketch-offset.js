import * as THREE from 'three'
import { Line, Circle, Arc, Spline, OffsetCurve } from './geom2d.js'
import { EPSILON, cross2d } from './utils.js'
import { shapex as sx, modelTolerance } from './shapex.js'
import { fail } from './errors.js'

export const supportsOffset = curve =>
  curve instanceof Line || curve instanceof Circle || curve instanceof Arc || curve instanceof Spline

// Stop at branches; a filled profile selects an explicit boundary at a junction.
export function connectedOffsetChain(source, elements) {
  const entries = [{ curve: source, start: 0, end: 1 }]
  if(source.isClosed()) return entries
  const used = new Set([source])
  for(const prepend of [false, true]) {
    while(true) {
      const entry = prepend ? entries[0] : entries.at(-1)
      const point = entry.curve.sample(prepend ? entry.start : entry.end)
      const matches = elements.filter(curve => supportsOffset(curve) && !curve.isClosed() && !!curve.isReference == !!source.isReference)
        .flatMap(curve => curve.endpoints().flatMap((p, index) =>
          p.almost(point) && curve !== entry.curve ? [{ curve, index }] : []))
      if(matches.length !== 1 || used.has(matches[0].curve)) break
      const { curve, index } = matches[0]
      used.add(curve)
      const start = prepend ? 1 - index : index
      const next = { curve, start, end: 1 - start }
      if(prepend) entries.unshift(next)
      else entries.push(next)
    }
  }
  return entries
}

export function profileOffsetGroups(profile, elements) {
  return profile.rings.map((ring, ringIndex) => {
    const entries = ring.segments.map(segment => {
      const curve = elements.find(curve => segment.id == curve.id || segment.id.startsWith(curve.id + '/'))
      if(!supportsOffset(curve)) fail('This profile contains unsupported offset geometry')
      if(curve instanceof Circle && segment.isClosed()) return { curve, start: 0, end: 1 }
      const parameter = point => {
        const range = curve.geom().range()
        return (curve.unsample(point) - range.start) / range.span()
      }
      const start = parameter(segment.endpoints()[0])
      let end = parameter(segment.endpoints()[1])
      if(curve instanceof Circle) {
        const point = segment.sample(0)
        const sameDirection = segment.sample(1e-5).sub(point).dot(curve.sample(start + 1e-5).sub(point)) > 0
        if(sameDirection && end <= start) end += 1
        if(!sameDirection && end >= start) end -= 1
      }
      return { curve, start, end }
    })
    // Keep the filled area to the left: inward offsets shrink the outer
    // boundary and expand holes, regardless of their original curve directions.
    const points = entries.flatMap(entry => [0, .25, .5, .75].map(t =>
      entry.curve.sample(entry.start + (entry.end - entry.start) * t)))
    const area = points.reduce((sum, point, i) => sum + cross2d(point, points[(i + 1) % points.length]), 0)
    return (area > 0) == (ringIndex == 0) ? entries :
      entries.reverse().map(entry => ({ ...entry, start: entry.end, end: entry.start }))
  })
}

function geometry(entry) {
  const source = entry.curve.geom(), range = source.range()
  return sx.TrimmedCurve2.new(source.curve(), sx.ParameterRange.new(
    range.start + range.span() * entry.start, range.start + range.span() * entry.end))
}

export function offsetFrame(entry, parameter) {
  const curve = geometry(entry), range = curve.range()
  const value = curve.evaluate(range.start + range.span() * parameter)
  const tangent = new THREE.Vector3(value.firstDerivative.x, value.firstDerivative.y, 0).multiplyScalar(Math.sign(range.span())).normalize()
  return { point: new THREE.Vector3(value.point.x, value.point.y, 0), tangent, normal: new THREE.Vector3(-tangent.y, tangent.x, 0) }
}

export function nearestOffsetFrame(groups, position) {
  let nearest
  groups.forEach((entries, group) => entries.forEach((entry, index) => {
    const curve = geometry(entry), range = curve.range()
    const closest = curve.closestPoint(sx.Point2.new(position.x, position.y), modelTolerance)
    if(!nearest || closest.distance < nearest.distance) {
      const parameter = (closest.parameter - range.start) / range.span()
      nearest = { ...offsetFrame(entry, parameter), group, index, parameter, distance: closest.distance }
    }
  }))
  if(!nearest) fail('Could not locate the offset on this profile')
  return nearest
}

export function buildOffsetCurves(groups, distance) {
  if(!Number.isFinite(distance) || Math.abs(distance) <= EPSILON) fail('Offset must be nonzero')
  const paths = groups.map(entries => {
    const curves = entries.map(geometry)
    const wire = sx.Wire2.new(curves, modelTolerance)
    const joins = curves.slice(0, wire.isClosed() ? curves.length : -1).map((curve, index) =>
      curve.curve() instanceof sx.Line2 && curves[(index + 1) % curves.length].curve() instanceof sx.Line2 ? 'miter' : 'round')
    const result = wire.offset(distance, { join: joins, miterLimit: Infinity })
    result.wire.validateSimple()
    return result.wire
  })
  for(let i = 0; i < paths.length; i++) for(const other of paths.slice(0, i)) {
    for(const curve of paths[i].segments()) for(const boundary of other.segments()) {
      if(curve.intersections(boundary, modelTolerance).length) fail('Offset boundaries intersect')
    }
  }
  return paths.flatMap(path => path.segments().map(curve => new OffsetCurve(curve)))
}
