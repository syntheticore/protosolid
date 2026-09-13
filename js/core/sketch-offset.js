import * as THREE from 'three'
import { Line, Circle, Arc, Spline, OffsetCurve } from './geom2d.js'
import { EPSILON, cross2d, ocPnt2dFromVec, vecFromOc } from './utils.js'

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
      if(!supportsOffset(curve)) throw new Error('This profile contains unsupported offset geometry')
      if(curve instanceof Circle && segment.isClosed()) return { curve, start: 0, end: 1 }
      const parameter = point => {
        const geom = curve.geom().get()
        const value = (curve.unsample(point) - geom.FirstParameter()) / (geom.LastParameter() - geom.FirstParameter())
        return curve instanceof Arc && !curve.forward ? 1 - value : value
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

export function offsetFrame(entry, parameter) {
  const { curve, start, end } = entry
  const t = start + (end - start) * parameter
  const point = curve.sample(t)
  const direction = Math.sign(end - start) * (curve instanceof Arc && !curve.forward ? -1 : 1)
  const tangent = vecFromOc(curve.geom().get().DN(rawParameter(entry, t), 1)).multiplyScalar(direction)
  if(tangent.lengthSq() < 1e-20) throw new Error('The curve has no offset direction here')
  tangent.normalize()
  return { point, tangent, normal: new THREE.Vector3(-tangent.y, tangent.x, 0) }
}

export function nearestOffsetFrame(groups, position) {
  let nearest
  groups.forEach((entries, group) => entries.forEach((entry, index) => {
    // OC can return no perpendicular projection beyond a bounded curve's
    // ends. Always compare the boundary points as well as its projection.
    const geom = entry.curve.geom().get()
    let t = (entry.curve.unsample(position) - geom.FirstParameter()) / (geom.LastParameter() - geom.FirstParameter())
    if(entry.curve instanceof Arc && !entry.curve.forward) t = 1 - t
    if(entry.curve instanceof Circle) t += Math.round((entry.start + entry.end) / 2 - t)
    const parameter = Math.max(0, Math.min(1, (t - entry.start) / (entry.end - entry.start)))
    const candidates = [...new Set([0, 1, parameter].filter(Number.isFinite))]
    candidates.forEach(parameter => {
      const frame = offsetFrame(entry, parameter)
      const distance = frame.point.distanceToSquared(position)
      if(!nearest || distance < nearest.distance) nearest = { ...frame, group, index, parameter, distance }
    })
  }))
  if(!nearest) throw new Error('Could not locate the offset on this profile')
  return nearest
}

function rawParameter(entry, t) {
  const geom = entry.curve.geom().get()
  if(entry.curve instanceof Arc && !entry.curve.forward) t = 1 - t
  return geom.FirstParameter() + t * (geom.LastParameter() - geom.FirstParameter())
}

function offsetSegment(entry, distance) {
  const oc = window.oc.oc
  if(entry.curve instanceof Line) {
    const frame = offsetFrame(entry, 0)
    const a = frame.point.clone().addScaledVector(frame.normal, distance)
    const b = entry.curve.sample(entry.end).addScaledVector(frame.normal, distance)
    const line = new Line(a, b)
    const geom = line.geom().get()
    const segment = { entry, handle: geom.BasisCurve(), start: geom.FirstParameter(), end: geom.LastParameter(), a, b }
    line.clear()
    return segment
  }
  // Reverse the basis itself so OC's derivative follows the chain direction.
  const first = rawParameter(entry, entry.start)
  const last = rawParameter(entry, entry.end)
  const basis = new oc.Geom2d_TrimmedCurve(entry.curve.geom(), Math.min(first, last), Math.max(first, last), first < last, true)
  const basisHandle = new oc.Handle_Geom2d_Curve_2(basis)
  // OC uses the right normal; the tool consistently measures to the left.
  const offset = new oc.Geom2d_OffsetCurve(basisHandle, -distance, false)
  const handle = new oc.Handle_Geom2d_Curve_2(offset)
  const start = offset.FirstParameter(), end = offset.LastParameter()
  const a = vecFromOc(offset.Value(start)), b = vecFromOc(offset.Value(end))
  if(![...a.toArray(), ...b.toArray()].every(Number.isFinite)) throw new Error('Offset is not finite')
  if(entry.curve instanceof Circle || entry.curve instanceof Arc) {
    const frame = offsetFrame(entry, 0.5)
    if(frame.point.clone().addScaledVector(frame.normal, distance).sub(entry.curve.center())
      .dot(frame.point.clone().sub(entry.curve.center())) <= EPSILON) throw new Error('Offset collapses a circular edge')
  }
  return { entry, handle, start, end, a, b }
}

function parameterAt(segment, point) {
  const projection = new window.oc.oc.Geom2dAPI_ProjectPointOnCurve_2(ocPnt2dFromVec(point), segment.handle)
  try {
    if(!projection.NbPoints()) throw new Error('Offset edges could not be joined')
    return projection.LowerDistanceParameter()
  } finally {
    projection.delete()
  }
}

function joinSegments(left, right, distance) {
  if(left.b.almost(right.a)) return
  const incoming = offsetFrame(left.entry, 1), outgoing = offsetFrame(right.entry, 0)
  // Straight edges meet at a miter, including outside corners.
  if(left.entry.curve instanceof Line && right.entry.curve instanceof Line) {
    const divisor = cross2d(incoming.tangent, outgoing.tangent)
    if(Math.abs(divisor) < 1e-10) throw new Error('Offset has a reversing corner')
    const point = left.b.clone().addScaledVector(incoming.tangent, cross2d(right.a.clone().sub(left.b), outgoing.tangent) / divisor)
    // Project onto unbounded lines rather than the trimmed OC offset curves.
    const leftLength = left.a.distanceTo(left.b), rightLength = right.a.distanceTo(right.b)
    left.end = left.start + point.clone().sub(left.a).dot(incoming.tangent) / leftLength * (left.end - left.start)
    right.start = right.end - right.b.clone().sub(point).dot(outgoing.tangent) / rightLength * (right.end - right.start)
    left.b = point
    right.a = point
    return
  }
  const intersections = new window.oc.oc.Geom2dAPI_InterCurveCurve_2(left.handle, right.handle, EPSILON / 10)
  const points = Array.from({ length: intersections.NbPoints() }, (_, i) => vecFromOc(intersections.Point(i + 1)))
  intersections.delete()
  if(points.length) {
    const point = points.minMaxBy(Math.min, point => point.distanceToSquared(left.b) + point.distanceToSquared(right.a))
    left.end = parameterAt(left, point)
    right.start = parameterAt(right, point)
    left.b = point
    right.a = point
    return
  }
  // A convex outside corner leaves a gap; a circular join has the same
  // distance to the original vertex as the two adjacent offset curves.
  if(cross2d(incoming.tangent, outgoing.tangent) * distance >= -EPSILON) throw new Error('Offset is too large to join these edges')
  const center = incoming.point
  const start = left.b.clone().sub(center).normalize()
  const end = right.a.clone().sub(center).normalize()
  const middle = start.clone().add(end).normalize().multiplyScalar(Math.abs(distance)).add(center)
  const arc = Arc.fromPoints([left.b, middle, right.a])
  if(!arc) throw new Error('Offset corner could not be created')
  const geom = arc.geom().get()
  const join = new OffsetCurve(new window.oc.oc.Geom2d_TrimmedCurve(arc.geom(), geom.FirstParameter(), geom.LastParameter(), true, true))
  if(!join.sample(0).almost(left.b)) join.flip()
  arc.clear()
  return join
}

export function buildOffsetCurves(groups, distance) {
  if(!Number.isFinite(distance) || Math.abs(distance) <= EPSILON) throw new Error('Offset must be nonzero')
  const output = []
  const pendingJoins = new Set()
  try {
    groups.forEach(entries => {
      const segments = entries.map(entry => offsetSegment(entry, distance))
      const closed = entries.closed ?? entries[0].curve.sample(entries[0].start).almost(entries.at(-1).curve.sample(entries.at(-1).end))
      entries.forEach((entry, i) => {
        if(i + 1 < entries.length || closed) {
          const next = entries[(i + 1) % entries.length]
          if(!entry.curve.sample(entry.end).almost(next.curve.sample(next.start))) throw new Error('Offset source edges are no longer connected')
        }
      })
      const joins = segments.map((segment, i) => {
        const join = i + 1 < segments.length || closed ?
          joinSegments(segment, segments[(i + 1) % segments.length], distance) : undefined
        if(join) pendingJoins.add(join)
        return join
      })
      segments.forEach((segment, i) => {
        if(segment.end - segment.start <= EPSILON) throw new Error('Offset collapses an edge')
        const curve = new OffsetCurve(new window.oc.oc.Geom2d_TrimmedCurve(segment.handle, segment.start, segment.end, true, true))
        // An inward miter must not invert a segment when the profile collapses.
        if(segment.entry.curve instanceof Line && curve.sample(1).sub(curve.sample(0)).dot(offsetFrame(segment.entry, 0.5).tangent) <= EPSILON) {
          curve.clear()
          throw new Error('Offset collapses an edge')
        }
        output.push(curve)
        if(joins[i]) {
          output.push(joins[i])
          pendingJoins.delete(joins[i])
        }
      })
    })
    validateOffsetCurves(output)
    return output
  } catch(error) {
    output.forEach(curve => curve.clear())
    pendingJoins.forEach(curve => curve.clear())
    throw error
  }
}

function validateOffsetCurves(curves) {
  const oc = window.oc.oc
  curves.forEach((curve, index) => {
    const vertices = curve.tesselate()
    if(!vertices || !vertices.flat().every(Number.isFinite)) throw new Error('Offset contains a singular curve')
    const self = new oc.Geom2dAPI_InterCurveCurve_3(curve.geom(), EPSILON / 10)
    const crossesItself = Array.from({ length: self.NbPoints() }, (_, i) => vecFromOc(self.Point(i + 1)))
      .some(point => !curve.endpoints().some(endpoint => endpoint.almost(point)))
    self.delete()
    if(crossesItself) throw new Error('Offset intersects itself')
    curves.slice(0, index).forEach(other => {
      const intersections = new oc.Geom2dAPI_InterCurveCurve_2(curve.geom(), other.geom(), EPSILON / 10)
      try {
        for(let i = 1; i <= intersections.NbPoints(); i++) {
          const point = vecFromOc(intersections.Point(i))
          if(!curve.endpoints().some(endpoint => endpoint.almost(point)) ||
            !other.endpoints().some(endpoint => endpoint.almost(point))) throw new Error('Offset boundaries intersect')
        }
      } finally {
        intersections.delete()
      }
    })
  })
}
