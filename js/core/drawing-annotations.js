import * as THREE from 'three'

import { relativeComponentTransform } from './assembly.js'
import { shapex } from './shapex.js'
import { isInside } from './drawing-geometry.js'


export const THIN = { width: 0.25 }
export const CHAIN = { width: 0.25, dash: [8, 1.5, 1, 1.5] }
const TEXT = 3.5
const ARROW = 3
const GAP = 1
const OVERSHOOT = 2

const gdtCharacteristics = [
  { value: 'straightness', label: 'Straightness' },
  { value: 'flatness', label: 'Flatness' },
  { value: 'circularity', label: 'Circularity' },
  { value: 'cylindricity', label: 'Cylindricity' },
  { value: 'profileLine', label: 'Profile of a Line' },
  { value: 'profileSurface', label: 'Profile of a Surface' },
  { value: 'parallelism', label: 'Parallelism' },
  { value: 'perpendicularity', label: 'Perpendicularity' },
  { value: 'angularity', label: 'Angularity' },
  { value: 'position', label: 'Position' },
  { value: 'concentricity', label: 'Concentricity' },
  { value: 'symmetry', label: 'Symmetry' },
  { value: 'runout', label: 'Circular Runout' },
  { value: 'totalRunout', label: 'Total Runout' },
]

const weldTypes = [
  { value: 'fillet', label: 'Fillet' },
  { value: 'square', label: 'Square Butt' },
  { value: 'v', label: 'V Butt' },
]


// Model entities

// An edge or vertex in the drawn component's frame. Circle centers count as points.
export function entity(item, occurrence, component, center) {
  const transform = relativeComponentTransform(occurrence, component)
  if(item.typename() == 'Vertex') return { kind: 'point', point: item.point.clone().applyMatrix4(transform), item, occurrence }
  const curve = item.geom().curve()
  if(curve instanceof shapex.Line3) {
    const [a, b] = item.vertices().map(point => point.applyMatrix4(transform))
    return { kind: 'line', a, b, item, occurrence }
  }
  if(curve instanceof shapex.Circle3) {
    const circle = {
      kind: 'circle',
      center: new THREE.Vector3(...curve.center().toArray()).applyMatrix4(transform),
      normal: new THREE.Vector3(curve.normal().x, curve.normal().y, curve.normal().z).transformDirection(transform),
      radius: curve.radius(),
      full: Math.abs(item.geom().trimmedCurve().range().span()) > 2 * Math.PI - 1e-6,
      item,
      occurrence,
    }
    return center ? { kind: 'point', point: circle.center, item, occurrence } : circle
  }
  return { kind: 'curve', points: item.tesselate().map(point => point.clone().applyMatrix4(transform)), item, occurrence }
}

// Stored references resolve against the current model, once per regeneration
export function resolveEntity(context, stored) {
  if(!context.entities.has(stored)) {
    const reference = stored.ref.clone()
    const error = reference.update(context.tree)
    const item = !error && reference.getItem()
    context.entities.set(stored, item ? entity(item, context.tree.findChild(reference.componentId), context.component, stored.center) : null)
  }
  return context.entities.get(stored)
}


// Ink collects primitives on the sheet in mm, with y pointing down

export class Ink {
  constructor(style = THIN) {
    this.style = style
    this.paths = []
    this.texts = []
    this.fields = []
    this.box = new THREE.Box2()
  }

  line(points, style = this.style) {
    this.paths.push({ ...style, points })
    points.forEach(point => this.box.expandByPoint(new THREE.Vector2(...point)))
  }

  fill(points) {
    this.paths.push({ width: 0.1, fill: true, points: [...points, points[0]] })
    points.forEach(point => this.box.expandByPoint(new THREE.Vector2(...point)))
  }

  circle(center, radius, style = this.style, from = 0, to = 2 * Math.PI) {
    const count = Math.max(8, Math.ceil(48 * Math.abs(to - from) / (2 * Math.PI)))
    this.line(Array.from({ length: count + 1 }, (_, i) => {
      const angle = from + (to - from) * i / count
      return [center[0] + radius * Math.cos(angle), center[1] - radius * Math.sin(angle)]
    }), style)
  }

  arrow(tip, direction) {
    const [dx, dy] = normalize(direction)
    const base = [tip[0] - dx * ARROW, tip[1] - dy * ARROW]
    const side = [-dy * ARROW / 6, dx * ARROW / 6]
    this.fill([tip, [base[0] + side[0], base[1] + side[1]], [base[0] - side[0], base[1] - side[1]]])
  }

  dot(point) {
    this.circle(point, 0.4, { width: 0.8 })
  }

  // Angle in degrees counterclockwise on the sheet
  text(point, text, { size = TEXT, anchor = 'start', angle = 0 } = {}) {
    this.texts.push({ x: point[0], y: point[1], size, text, anchor, angle })
    const width = textWidth(text, size)
    const offset = { start: 0, middle: -width / 2, end: -width }[anchor]
    const along = [Math.cos(angle * Math.PI / 180), -Math.sin(angle * Math.PI / 180)]
    const up = textUp(angle)
    const corner = (s, h) => new THREE.Vector2(
      point[0] + along[0] * s + up[0] * h,
      point[1] + along[1] * s + up[1] * h,
    )
    ;[corner(offset, 0), corner(offset + width, 0), corner(offset, size), corner(offset + width, size)]
      .forEach(point => this.box.expandByPoint(point))
  }

  // A value of the annotation that is typed or picked in place, laid out
  // like text of its current width at point
  field(key, point, width, { size = TEXT, anchor = 'start', angle = 0, placeholder, choices, lines = 1, multiline = false } = {}) {
    this.fields.push({ key, point, width, size, anchor, angle, placeholder, choices, lines, multiline })
  }

  merge(ink) {
    this.paths.push(...ink.paths)
    this.texts.push(...ink.texts)
    this.fields.push(...ink.fields)
    this.box.union(ink.box)
  }
}

// Rectangle of a field on the sheet for an input of the given width, before
// rotating it by -angle about its corner
export function fieldRect(field, width) {
  const pad = field.size * 0.3
  const offset = { start: 0, middle: -width / 2, end: -width }[field.anchor] - pad
  const along = [Math.cos(field.angle * Math.PI / 180), -Math.sin(field.angle * Math.PI / 180)]
  const corner = add(add(field.point, scale(along, offset)), scale(textUp(field.angle), field.size * 1.05))
  return { x: corner[0], y: corner[1], width: width + 2 * pad, height: field.size * 1.4 * field.lines, pad }
}


// Helvetica widths in 1/1000 em, for anchoring text in PDFs
const helvetica = [
  278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
  556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
  1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
  667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
  333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
  556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
]
const helveticaExtra = { 'Ø': 778, '°': 400, '±': 584 }

export function textWidth(text, size) {
  let width = 0
  for(const char of String(text)) {
    const code = char.charCodeAt(0)
    width += helveticaExtra[char] || (code >= 32 && code < 127 ? helvetica[code - 32] : 556)
  }
  return width * size / 1000
}


// Vector helpers on sheet points

const add = (a, b) => [a[0] + b[0], a[1] + b[1]]
const sub = (a, b) => [a[0] - b[0], a[1] - b[1]]
const scale = (a, s) => [a[0] * s, a[1] * s]
const dot = (a, b) => a[0] * b[0] + a[1] * b[1]
const length = a => Math.hypot(a[0], a[1])
const normalize = a => scale(a, 1 / (length(a) || 1))
const perpendicular = a => [-a[1], a[0]]
const mix = (a, b, t) => add(a, scale(sub(b, a), t))

function formatNumber(value) {
  return String(+value.toFixed(2))
}

// Readable text angle along a sheet direction
function textAngle(direction) {
  let angle = Math.atan2(-direction[1], direction[0]) * 180 / Math.PI
  if(angle > 90) angle -= 180
  if(angle <= -90) angle += 180
  return angle
}

function textUp(angle) {
  const radians = angle * Math.PI / 180
  return [-Math.sin(radians), -Math.cos(radians)]
}

// Value text with an optional tolerance, centered on point
function dimensionText(ink, point, text, tolerance, angle) {
  const along = [Math.cos(angle * Math.PI / 180), -Math.sin(angle * Math.PI / 180)]
  const up = textUp(angle)
  tolerance = (tolerance || '').trim()
  const stacked = tolerance.includes('/') && tolerance.split('/')
  const main = stacked || !tolerance ? text : tolerance.startsWith('±') ? text + tolerance : `${text} ${tolerance}`
  const small = TEXT * 0.7
  const width = textWidth(main, TEXT) + (stacked ? 1 + Math.max(...stacked.map(part => textWidth(part.trim(), small))) : 0)
  const start = add(point, scale(along, -width / 2))
  ink.text(start, main, { angle })
  ink.field('tolerance', add(start, scale(along, textWidth(text, TEXT) + 1.2)), textWidth(tolerance, TEXT), {
    angle, placeholder: '±0.1, +0.1/-0.05 or H7...',
  })
  if(stacked) {
    const left = add(start, scale(along, textWidth(main, TEXT) + 1))
    ink.text(add(left, scale(up, TEXT * 0.55)), stacked[0].trim(), { size: small, angle })
    ink.text(add(left, scale(up, -0.6)), stacked[1].trim(), { size: small, angle })
  }
}

// Linear dimension between sheet points p1 and p2, measured along t
function linear(ink, p1, p2, t, position, text, tolerance) {
  const n = perpendicular(t)
  const ends = [p1, p2].map(point => {
    const offset = dot(sub(position, point), n)
    const end = add(point, scale(n, offset))
    const sign = Math.sign(offset)
    if(Math.abs(offset) > GAP) ink.line([add(point, scale(n, sign * GAP)), add(end, scale(n, sign * OVERSHOOT))])
    return end
  })
  ink.line(ends)
  ink.arrow(ends[0], sub(ends[0], ends[1]))
  ink.arrow(ends[1], sub(ends[1], ends[0]))
  const angle = textAngle(t)
  dimensionText(ink, add(mix(ends[0], ends[1], 0.5), scale(textUp(angle), 1)), text, tolerance, angle)
}

// Arrow leader from the text side to a target
function leader(ink, from, target, arrow = true) {
  ink.line([from, target])
  if(arrow) ink.arrow(target, sub(target, from))
  else ink.dot(target)
}


// Annotations

export function orientationAt(p1, p2, position) {
  const low = [Math.min(p1[0], p2[0]), Math.min(p1[1], p2[1])]
  const high = [Math.max(p1[0], p2[0]), Math.max(p1[1], p2[1])]
  const outsideX = position[0] < low[0] || position[0] > high[0]
  const outsideY = position[1] < low[1] || position[1] > high[1]
  if(outsideY && !outsideX) return 'h'
  if(outsideX && !outsideY) return 'v'
  return 'aligned'
}

// Kind of dimension the picked entities give
export function dimensionKind(entities) {
  if(entities.length == 1) {
    if(entities[0].kind == 'circle') return entities[0].full ? 'diameter' : 'radius'
    return entities[0].kind == 'line' ? 'linear' : null
  }
  const [a, b] = entities
  if(a.kind == 'line' && b.kind == 'line') {
    const da = a.b.clone().sub(a.a).normalize()
    const db = b.b.clone().sub(b.a).normalize()
    return Math.abs(da.dot(db)) > 0.9998 ? 'linear' : 'angle'
  }
  return entities.every(entity => entity.kind != 'curve' && entity.kind != 'circle') ? 'linear' : null
}

// Sheet geometry of a dimension's entities, also used while placing it
export function dimensionGeometry(view, entities, kind) {
  const sheet = point => view.project(point)
  const model = point => [point.dot(view.frame.u), point.dot(view.frame.v)]
  if(kind == 'diameter' || kind == 'radius') {
    const circle = entities[0]
    const faceOn = Math.abs(circle.normal.dot(view.frame.dir)) > 0.3
    if(faceOn) return { kind, faceOn, center: sheet(circle.center), radius: circle.radius * view.factor, value: circle.radius }
    const across = new THREE.Vector3().crossVectors(view.frame.dir, circle.normal).normalize().multiplyScalar(circle.radius)
    const ends = [circle.center.clone().sub(across), circle.center.clone().add(across)]
    return { kind, faceOn, points: ends.map(sheet), model: ends.map(model) }
  }
  if(kind == 'angle') {
    return { kind, lines: entities.map(line => [sheet(line.a), sheet(line.b)]) }
  }
  // Lines stand for both ends, or for the line itself when measured from
  const anchors = entities.length == 1 ? [entities[0].a, entities[0].b] : entities.map(entity => entity.kind == 'line' ? null : entity.point)
  const line = entities.find(entity => entity.kind == 'line' && entities.length == 2)
  return {
    kind,
    points: anchors.map(point => point && sheet(point)),
    model: anchors.map(point => point && model(point)),
    line: line && { sheet: [sheet(line.a), sheet(line.b)], model: [model(line.a), model(line.b)] },
    other: line && entities.find(entity => entity !== line),
  }
}

export function drawDimension(ink, view, geometry, annotation, position) {
  const prefix = { diameter: 'Ø', radius: 'R' }[geometry.kind] || ''
  const tolerance = annotation.tolerance

  if(geometry.kind == 'angle') return drawAngle(ink, geometry.lines, position, tolerance)

  if(geometry.faceOn) {
    const direction = normalize(sub(position, geometry.center))
    const rim = add(geometry.center, scale(direction, geometry.radius))
    const text = prefix + formatNumber(geometry.kind == 'diameter' ? 2 * geometry.value : geometry.value)
    const right = position[0] >= geometry.center[0]
    const shoulder = add(position, [right ? 3 : -3, 0])
    if(geometry.kind == 'diameter') {
      const opposite = sub(geometry.center, scale(direction, geometry.radius))
      ink.line([opposite, position, shoulder])
      ink.arrow(opposite, scale(direction, -1))
      ink.arrow(rim, direction)
    } else {
      ink.line([geometry.center, position, shoulder])
      ink.arrow(rim, direction)
    }
    dimensionText(ink, add(shoulder, [right ? textWidth(text, TEXT) / 2 + 1 : -textWidth(text, TEXT) / 2 - 1, 1]), text, tolerance, 0)
    return
  }

  // Distance between parallel lines, or from a point to a line
  if(geometry.line) {
    const [a, b] = geometry.line.sheet
    const t = perpendicular(normalize(sub(b, a)))
    const otherSheet = geometry.other.kind == 'line' ? geometry.other : null
    const foot = point => add(a, scale(normalize(sub(b, a)), dot(sub(point, a), normalize(sub(b, a)))))
    const p2 = otherSheet ? view.project(otherSheet.a) : view.project(geometry.other.point)
    const p1 = foot(p2)
    const [ma, mb] = geometry.line.model
    const mOther = geometry.other.kind == 'line' ? geometry.other.a : geometry.other.point
    const mPoint = [mOther.dot(view.frame.u), mOther.dot(view.frame.v)]
    const mDirection = normalize(sub(mb, ma))
    const value = Math.abs(dot(sub(mPoint, ma), perpendicular(mDirection)))
    linear(ink, p1, p2, t, position, prefix + formatNumber(value), tolerance)
    return
  }

  const [p1, p2] = geometry.points
  const [m1, m2] = geometry.model
  const orientation = annotation.orientation
  const t = orientation == 'h' ? [1, 0] : orientation == 'v' ? [0, 1] : normalize(sub(p2, p1))
  const delta = sub(m2, m1)
  const value = orientation == 'h' ? Math.abs(delta[0]) : orientation == 'v' ? Math.abs(delta[1]) : length(delta)
  linear(ink, p1, p2, t, position, prefix + formatNumber(value), tolerance)
}

function drawAngle(ink, lines, position, tolerance) {
  const [[a1, b1], [a2, b2]] = lines
  const d1 = normalize(sub(b1, a1))
  const d2 = normalize(sub(b2, a2))
  const cross = d1[0] * d2[1] - d1[1] * d2[0]
  if(Math.abs(cross) < 1e-9) return
  const s = (dot(sub(a2, a1), perpendicular(d2))) / dot(d1, perpendicular(d2))
  const center = add(a1, scale(d1, s))
  const radius = Math.max(length(sub(position, center)), 4)
  const angleOf = vector => Math.atan2(-vector[1], vector[0])
  const target = angleOf(sub(position, center))
  const modulo = angle => ((angle % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI)
  let sweep = null
  for(const r1 of [d1, scale(d1, -1)]) for(const r2 of [d2, scale(d2, -1)]) {
    const start = angleOf(r1)
    const span = modulo(angleOf(r2) - start)
    if(span < Math.PI && modulo(target - start) <= span) sweep = { start, span, rays: [r1, r2] }
  }
  if(!sweep) return
  const { start, span } = sweep
  ink.circle(center, radius, THIN, start, start + span)
  const at = angle => add(center, [radius * Math.cos(angle), -radius * Math.sin(angle)])
  const tangent = angle => [-Math.sin(angle), -Math.cos(angle)]
  ink.arrow(at(start), scale(tangent(start), -1))
  ink.arrow(at(start + span), tangent(start + span))
  // Extension lines from the lines' ends out to the arc
  ;[[a1, b1, sweep.rays[0], start], [a2, b2, sweep.rays[1], start + span]].forEach(([a, b, ray, angle]) => {
    const reach = Math.max(dot(sub(a, center), ray), dot(sub(b, center), ray))
    if(reach < radius - GAP) ink.line([add(center, scale(ray, reach + GAP)), add(at(angle), scale(ray, OVERSHOOT))])
  })
  const middle = start + span / 2
  const label = add(center, [(radius + 2) * Math.cos(middle), -(radius + 2) * Math.sin(middle)])
  dimensionText(ink, add(label, [0, 1.2]), formatNumber(span * 180 / Math.PI) + '°', tolerance, 0)
}

// Dimensions from one base point to several others
export function drawBaseline(ink, view, entities, annotation, position) {
  const points = entities.map(entity => view.project(entity.point))
  const models = entities.map(entity => [entity.point.dot(view.frame.u), entity.point.dot(view.frame.v)])
  const k = annotation.orientation == 'h' ? 0 : 1
  const t = k == 0 ? [1, 0] : [0, 1]
  const n = perpendicular(t)
  const sign = Math.sign(dot(sub(position, points[0]), n)) || 1
  points.slice(1).forEach((point, i) => {
    const value = Math.abs(models[i + 1][k] - models[0][k])
    linear(ink, points[0], point, t, add(position, scale(n, sign * 7 * i)), formatNumber(value), '')
  })
}

// Distances from an origin, labeled where the leaders end
export function drawOrdinate(ink, view, entities, annotation, position) {
  const k = annotation.orientation == 'h' ? 0 : 1
  const origin = entities[0].point.dot(k == 0 ? view.frame.u : view.frame.v)
  entities.forEach(entity => {
    const point = view.project(entity.point)
    const value = Math.abs(entity.point.dot(k == 0 ? view.frame.u : view.frame.v) - origin)
    const across = 1 - k
    const sign = Math.sign(position[across] - point[across]) || 1
    const start = [...point]
    start[across] += sign * GAP
    const end = [...point]
    end[across] = position[across]
    ink.line([start, end])
    const label = [...end]
    label[across] += sign * 1
    if(k == 0) ink.text(add(label, [1.2, 0]), formatNumber(value), { angle: 90, anchor: sign > 0 ? 'end' : 'start' })
    else ink.text(add(label, [0, 1.2]), formatNumber(value), { anchor: sign < 0 ? 'end' : 'start' })
  })
}

export function drawCenterline(ink, view, entities) {
  const [a, b] = entities
  if(a.kind == 'line' && b.kind == 'line') {
    const [a1, a2, b1, b2] = [a.a, a.b, b.a, b.b].map(point => view.project(point))
    const direction = normalize(sub(a2, a1))
    const normal = perpendicular(direction)
    const offset = (dot(a1, normal) + dot(b1, normal)) / 2
    const along = [a1, a2, b1, b2].map(point => dot(point, direction))
    const base = scale(normal, offset)
    ink.line([
      add(base, scale(direction, Math.min(...along) - 3)),
      add(base, scale(direction, Math.max(...along) + 3)),
    ], CHAIN)
    return
  }
  const points = entities.map(entity => view.project(entity.kind == 'line' ? entity.a.clone().lerp(entity.b, 0.5) : entity.point))
  const direction = normalize(sub(points[1], points[0]))
  ink.line([sub(points[0], scale(direction, 3)), add(points[1], scale(direction, 3))], CHAIN)
}

export function drawCenterMark(ink, view, circle) {
  const center = view.project(circle.center)
  const faceOn = Math.abs(circle.normal.dot(view.frame.dir)) > 0.3
  const reach = circle.radius * view.factor + 3
  if(faceOn) {
    ink.line([add(center, [-reach, 0]), add(center, [reach, 0])], CHAIN)
    ink.line([add(center, [0, -reach]), add(center, [0, reach])], CHAIN)
  } else {
    const axis = normalize([circle.normal.dot(view.frame.u), -circle.normal.dot(view.frame.v)])
    ink.line([sub(center, scale(axis, reach)), add(center, scale(axis, reach))], CHAIN)
  }
}

export function drawNote(ink, annotation, position, target) {
  const lines = String(annotation.text).split('\n')
  lines.forEach((line, i) => ink.text(add(position, [0, i * TEXT * 1.4]), line))
  ink.field('text', position, Math.max(...lines.map(line => textWidth(line, TEXT))), { placeholder: 'Note text...', lines: lines.length, multiline: true })
  if(target) {
    const right = target[0] > position[0] + textWidth(lines[0], TEXT) / 2
    const anchor = add(position, [right ? textWidth(lines[0], TEXT) + 1 : -1, -TEXT / 3])
    leader(ink, anchor, target)
  }
}

export function drawHoleCallout(ink, view, circle, model, position) {
  const center = view.project(circle.center)
  const faceOn = Math.abs(circle.normal.dot(view.frame.dir)) > 0.3
  let target
  if(faceOn) {
    target = add(center, scale(normalize(sub(position, center)), circle.radius * view.factor))
  } else {
    const across = new THREE.Vector3().crossVectors(view.frame.dir, circle.normal).normalize().multiplyScalar(circle.radius)
    const ends = [circle.center.clone().sub(across), circle.center.clone().add(across)].map(point => view.project(point))
    target = ends.sort((a, b) => length(sub(a, position)) - length(sub(b, position)))[0]
  }
  const text = holeText(circle, model)
  const right = position[0] >= target[0]
  ink.line([target, position, add(position, [right ? 3 : -3, 0])])
  ink.arrow(target, sub(target, position))
  ink.text(add(position, [right ? 4 : -4, 1.2]), text, { anchor: right ? 'start' : 'end' })
}

// Diameter and depth from the hole's cylinder, or THRU when it leaves the material
function holeText(circle, model) {
  const diameter = 'Ø' + formatNumber(2 * circle.radius)
  const transform = relativeComponentTransform(circle.occurrence, model.component)
  const cylinder = circle.item.connectedFaces().find(face => face.geom().surface().kind == 'cylinder')
  if(!cylinder) return diameter
  const positions = cylinder.tesselate().positions
  const point = new THREE.Vector3()
  let low = Infinity
  let high = -Infinity
  for(let i = 0; i < positions.length; i += 3) {
    const along = point.fromArray(positions, i).applyMatrix4(transform).sub(circle.center).dot(circle.normal)
    low = Math.min(low, along)
    high = Math.max(high, along)
  }
  // Open on both ends when probes just past them are outside the material
  const margin = Math.max(model.near * 4, 0.05)
  const open = [low - margin, high + margin].every(along =>
    !isInside(model, circle.center.clone().addScaledVector(circle.normal, along)))
  return open ? `${diameter} THRU` : `${diameter} x ${formatNumber(high - low)} DEEP`
}

export function drawBalloon(ink, number, position, target) {
  const radius = 4
  ink.circle(position, radius)
  ink.text(add(position, [0, TEXT / 2.8]), String(number), { anchor: 'middle' })
  const direction = normalize(sub(target, position))
  leader(ink, add(position, scale(direction, radius)), target, false)
}

export function drawFinish(ink, annotation, target) {
  const left = add(target, [-2.5, -4.33])
  const right = add(target, [5, -8.66])
  ink.line([left, target, right, add(right, [10, 0])])
  ink.text(add(right, [1, -0.8]), annotation.text, { size: 2.5 })
  ink.field('text', add(right, [1, -0.8]), textWidth(annotation.text, 2.5), { size: 2.5, placeholder: 'Ra 3.2...' })
}

export function drawDatum(ink, annotation, position, target) {
  const direction = normalize(sub(position, target))
  const side = perpendicular(direction)
  const apex = add(target, scale(direction, 2.6))
  ink.fill([add(target, scale(side, 1.6)), sub(target, scale(side, 1.6)), apex])
  const size = 6
  const box = [[-1, -1], [1, -1], [1, 1], [-1, 1], [-1, -1]].map(([x, y]) => add(position, [x * size / 2, y * size / 2]))
  const entry = Math.abs(direction[0]) > Math.abs(direction[1])
    ? add(position, [-Math.sign(direction[0]) * size / 2, 0])
    : add(position, [0, -Math.sign(direction[1]) * size / 2])
  ink.line([apex, entry])
  ink.line(box)
  ink.text(add(position, [0, TEXT / 2.8]), annotation.letter, { anchor: 'middle' })
  ink.field('letter', add(position, [0, TEXT / 2.8]), size - 1.5, { anchor: 'middle', placeholder: 'A...' })
}

export function drawFrame(ink, annotation, position, target) {
  const height = 8
  const cells = [
    { width: height, symbol: annotation.characteristic },
    { width: textWidth(annotation.tolerance, TEXT) + 4, text: annotation.tolerance },
    ...annotation.datums.split(/\s+/).filter(Boolean).map(text => ({ width: Math.max(height, textWidth(text, TEXT) + 4), text })),
  ]
  const total = cells.reduce((sum, cell) => sum + cell.width, 0)
  const [x, y] = position
  ink.line([[x, y], [x + total, y], [x + total, y + height], [x, y + height], [x, y]])
  let left = x
  cells.forEach((cell, i) => {
    if(i) ink.line([[left, y], [left, y + height]])
    const middle = [left + cell.width / 2, y + height / 2]
    if(cell.symbol) gdtSymbol(ink, cell.symbol, middle)
    else ink.text(add(middle, [0, TEXT / 2.8]), cell.text, { anchor: 'middle' })
    left += cell.width
  })
  // Fields cover their cells, the datums share one
  const baseline = y + height / 2 + TEXT / 2.8
  ink.field('characteristic', [x + height / 2, baseline], height - 3, { anchor: 'middle', choices: gdtCharacteristics })
  ink.field('tolerance', [x + height + cells[1].width / 2, baseline], cells[1].width - 2, { anchor: 'middle', placeholder: '0.1...' })
  const datums = x + height + cells[1].width
  ink.field('datums', cells.length > 2 ? [(datums + x + total) / 2, baseline] : [datums + 1.5, baseline], Math.max(0, x + total - datums - 2), {
    anchor: cells.length > 2 ? 'middle' : 'start', placeholder: 'A B C...',
  })
  const right = target[0] > x + total / 2
  leader(ink, [right ? x + total : x, y + height / 2], target)
}

// Geometric characteristic symbols, drawn in a 5 mm box around center
function gdtSymbol(ink, symbol, center) {
  const p = (x, y) => add(center, [x, -y])
  const circle = radius => ink.circle(center, radius)
  const arrow = (x, y, dx, dy) => {
    ink.line([p(x, y), p(x + dx, y + dy)])
    ink.arrow(p(x + dx, y + dy), [dx, -dy])
  }
  switch(symbol) {
    case 'straightness': return ink.line([p(-2.5, 0), p(2.5, 0)])
    case 'flatness': return ink.line([p(-2.5, -1.5), p(1, -1.5), p(2.5, 1.5), p(-1, 1.5), p(-2.5, -1.5)])
    case 'circularity': return circle(2.2)
    case 'cylindricity':
      circle(1.8)
      ink.line([p(-2.8, -2), p(-0.8, 2.2)])
      return ink.line([p(0.8, -2.2), p(2.8, 2)])
    case 'profileLine': return ink.circle(add(center, [0, 1]), 2.3, THIN, 0, Math.PI)
    case 'profileSurface':
      ink.circle(add(center, [0, 1]), 2.3, THIN, 0, Math.PI)
      return ink.line([p(-2.3, -1), p(2.3, -1)])
    case 'parallelism':
      ink.line([p(-2.2, -2), p(-0.4, 2)])
      return ink.line([p(0.4, -2), p(2.2, 2)])
    case 'perpendicularity':
      ink.line([p(-2.5, -2), p(2.5, -2)])
      return ink.line([p(0, -2), p(0, 2.5)])
    case 'angularity': return ink.line([p(2.5, 2), p(-2.5, -2), p(2.5, -2)])
    case 'position':
      circle(1.6)
      ink.line([p(-2.6, 0), p(2.6, 0)])
      return ink.line([p(0, -2.6), p(0, 2.6)])
    case 'concentricity':
      circle(1)
      return circle(2.2)
    case 'symmetry':
      ink.line([p(-2.6, 0), p(2.6, 0)])
      ink.line([p(-1.6, 1.4), p(1.6, 1.4)])
      return ink.line([p(-1.6, -1.4), p(1.6, -1.4)])
    case 'runout': return arrow(-1.8, -2.2, 3.2, 3.8)
    case 'totalRunout':
      ink.line([p(-2.8, -2.4), p(1.6, -2.4)])
      arrow(-2.6, -2.4, 2.4, 4.4)
      return arrow(-0.2, -2.4, 2.4, 4.4)
  }
}

export function drawWeld(ink, annotation, position, target) {
  const right = target[0] < position[0]
  const end = add(position, [right ? 16 : -16, 0])
  ink.line([target, position, end])
  ink.arrow(target, sub(target, position))
  const middle = mix(position, end, 0.5)
  const p = (x, y) => add(middle, [x, y])
  if(annotation.weldType == 'fillet') ink.line([p(-1.5, 0), p(-1.5, 3), p(1.5, 0)])
  else if(annotation.weldType == 'square') {
    ink.line([p(-0.8, 0), p(-0.8, 3)])
    ink.line([p(0.8, 0), p(0.8, 3)])
  } else ink.line([p(-2, 3.5), p(0, 0), p(2, 3.5)])
  if(annotation.size) ink.text(p(-3, 3.2), annotation.size, { anchor: 'end' })
  ink.field('weldType', p(0, 2.9), 3, { anchor: 'middle', choices: weldTypes })
  ink.field('size', p(-3, 3.2), textWidth(annotation.size, TEXT), { anchor: 'end', placeholder: 'a3...' })
}
