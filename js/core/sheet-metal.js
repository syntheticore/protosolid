import * as THREE from 'three'
import { markRaw } from 'vue'

import { SolidFeature } from './features.js'
import { Compound } from './geom3d.js'
import { Line } from './geom2d.js'
import { shapex as sx, modelTolerance, point2, point3, vector3, threeVector, planeFromMatrix } from './shapex.js'
import { normalFromMatrix, rad } from './utils.js'
import { FeatureGizmos } from '../three/gizmos.js'
import { worldTransform } from './assembly.js'
import Serialize from './serialize.js'
import { fail, featureError } from './errors.js'


// Sheet metal rules are ordinary component parameters. Features bind to them
// through expressions, so variants and parent components can change them.
export const sheetMetalRules = [
  { name: 'thickness', value: '1mm' },
  { name: 'bendRadius', value: '1mm' },
]

export function addSheetMetalRules(component) {
  const names = component.getParameters().map(parameter => parameter.name)
  sheetMetalRules
    .filter(rule => !names.includes(rule.name))
    .forEach(rule => component.creator.addParameter({ ...rule }))
}

// Binds feature settings to rule parameters of the active component, if defined
function bindRules(feature, bindings) {
  const names = feature.document.activeComponent.getParameters().map(parameter => parameter.name)
  Object.entries(bindings)
    .filter(([_, name]) => names.includes(name))
    .forEach(([key, name]) => feature.expressions[key] = name)
}

function checkRules(thickness, radius) {
  if(!(thickness > 0)) fail('Thickness must be positive')
  if(radius !== undefined && !(radius > 0)) fail('Bend radius must be positive')
}

const tolerance = () => modelTolerance.linear * 10

const v2 = (x, y) => new THREE.Vector2(x, y)

const featureComponent = feature => feature.document.top().findChild(feature.componentOccurrenceId) ||
  feature.document.top().findChild(feature.componentId)

// Builds a body from a closed chain of 2D curves, naming its faces after the
// curves. `build` turns the profile into geometry, like an extrusion.
function namedBody(componentId, curves, names, build, owner) {
  let wire = sx.Wire2.new(curves, modelTolerance)
  if(wire.signedArea() < 0) {
    curves = curves.map(curve => curve.reversed()).reverse()
    names = [...names].reverse()
    wire = sx.Wire2.new(curves, modelTolerance)
  }
  const geometry = build(sx.Profile.new(wire, [], modelTolerance))
  const generator = { rings: [{ segments: names }], getBaseId: name => name }
  return new Compound(componentId, geometry).seedProfileTopology([generator], owner)
}

function polygon(points, tag) {
  return {
    curves: points.map((point, i) => sx.TrimmedCurve2.line(point2(point), point2(points[(i + 1) % points.length]))),
    names: points.map((_, i) => `${tag}/${i}`),
  }
}

const bodyFrom = (bodies, componentId) =>
  bodies.reduce((tool, body) => tool.boolean(body, 'join'), new Compound(componentId))

// Matrix of the plane through a point with the given normal
function planeMatrix(origin, normal) {
  const u = new THREE.Vector3(1, 0, 0).cross(normal)
  if(u.lengthSq() < 1e-6) u.set(0, 1, 0).cross(normal)
  u.normalize()
  return new THREE.Matrix4().makeBasis(u, normal.clone().cross(u), normal).setPosition(origin)
}

function rotationAbout(point, axis, angle) {
  return new THREE.Matrix4().makeTranslation(point.x, point.y, point.z)
    .multiply(new THREE.Matrix4().makeRotationAxis(axis, angle))
    .multiply(new THREE.Matrix4().makeTranslation(-point.x, -point.y, -point.z))
}

// Closed cross section of a sheet following a polyline with sharp corners.
// The polyline runs along one face of the sheet, which extends by its
// thickness to the right. Corners become bends with the given inside radius.
function sheetSection(points, thickness, radius, tag) {
  const dirs = points.slice(1).map((point, i) => point.clone().sub(points[i]).normalize())
  const normals = dirs.map(dir => v2(-dir.y, dir.x))
  const faceOffsets = [0, -thickness]

  const corners = dirs.slice(1).map((dir, i) => {
    const turn = Math.atan2(dirs[i].cross(dir), dirs[i].dot(dir))
    if(Math.abs(turn) > Math.PI - 1e-6) fail('Sheet cannot fold back onto itself')
    const sign = Math.sign(turn)
    // The bend center lies on the inner side, offset like a face of the sheet
    const inner = sign > 0 ? Math.max(...faceOffsets) : Math.min(...faceOffsets)
    const offset = inner + sign * radius
    const [n1, n2] = [normals[i], normals[i + 1]]
    const center = points[i + 1].clone().addScaledVector(n1.clone().add(n2), offset / (1 + n1.dot(n2)))
    return { center, offset, n1, n2, bisector: n1.clone().add(n2).normalize() }
  })

  // Segments of one face, given by its offset from the polyline
  const faceSegments = h => {
    const segments = []
    let start = points[0].clone().addScaledVector(normals[0], h)
    dirs.forEach((dir, i) => {
      const corner = corners[i]
      let end = corner ?
        corner.center.clone().addScaledVector(corner.n1, h - corner.offset) :
        points.at(-1).clone().addScaledVector(normals.at(-1), h)
      const length = end.clone().sub(start).dot(dir)
      if(length < -modelTolerance.linear) fail('Lines are too short for the bend radius')
      if(length > modelTolerance.linear) segments.push({ name: `line/${i}`, start, end })
      else end = start
      if(!corner) return
      start = corner.center.clone().addScaledVector(corner.n2, h - corner.offset)
      const mid = corner.center.clone().addScaledVector(corner.bisector, h - corner.offset)
      segments.push({ name: `bend/${i}`, start: end, mid, end: start })
    })
    return segments
  }

  const [a, b] = faceOffsets.map(faceSegments)
  const segments = [
    ...a.map(segment => ({ ...segment, name: `${tag}/a/${segment.name}` })),
    { name: `${tag}/end`, start: a.at(-1).end, end: b.at(-1).end },
    ...[...b].reverse().map(segment => ({ ...segment, name: `${tag}/b/${segment.name}`, start: segment.end, end: segment.start })),
    { name: `${tag}/start`, start: b[0].start, end: a[0].start },
  ]
  return {
    curves: segments.map(segment => segment.mid ?
      sx.TrimmedCurve2.arcThrough(point2(segment.start), point2(segment.mid), point2(segment.end)) :
      sx.TrimmedCurve2.line(point2(segment.start), point2(segment.end))),
    names: segments.map(segment => segment.name),
  }
}

// Ordered corner points of a chain of sketch lines
function chainPoints(lines) {
  if(lines.some(line => !(line instanceof Line))) fail('Contours may only consist of straight lines')
  const sketch = lines[0].sketch
  if(lines.some(line => line.sketch !== sketch)) fail('Contour lines must lie in one sketch')

  const close = (p, q) => p.distanceTo(q) <= modelTolerance.linear
  const ends = lines.map(line => line.endpoints().map(point => v2(point.x, point.y)))
  const isFree = point => ends.filter(other => other.some(q => close(point, q))).length == 1
  const first = ends.find(points => points.some(isFree))
  if(!first) fail('Closed contours are not supported yet')

  let point = first.find(isFree)
  const points = [point]
  const remaining = new Set(ends)
  while(remaining.size) {
    const next = [...remaining].find(points => points.some(q => close(q, point)))
    if(!next) fail('Contour lines must form a single chain')
    remaining.delete(next)
    point = next.find(q => !close(q, point))
    points.push(point)
  }

  // Collinear lines form a single flat
  const corners = points.filter((point, i) => i == 0 || i == points.length - 1 ||
    Math.abs(point.clone().sub(points[i - 1]).normalize().cross(points[i + 1].clone().sub(point).normalize())) > 1e-9)
  return { sketch, points: corners }
}


// Plate of sheet thickness from closed sketch profiles
export class SheetFaceFeature extends SolidFeature {
  static icon = 'sheet-face'
  constructor(document) {
    super(document, false, 'Sheet Face', {
      profiles: {
        title: 'Profiles',
        type: 'profile',
        multi: true,
      },
      thickness: {
        title: 'Thickness',
        type: 'length',
        gizmo: true,
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-up', 'caret-down'],
      },
    })

    // Sheet metal always adds material
    this.operation = 'join'
    this.profiles = null
    this.thickness = 1.0
    this.side = true
    bindRules(this, { thickness: 'thickness' })
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    this.previewBody = null
    try {
      checkRules(this.thickness)
      const thickness = this.thickness * (this.side ? 1 : -1)
      const tool = bodyFrom(references.profiles.map(profile =>
        profile.extrude(this.componentId, thickness, this.id)), this.componentId)
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = featureError(err, 'Sheet face could not be created') }
  }

  getGizmo() {
    const profile = this.profiles().at(-1).getItem()
    return {
      distance: {
        center: profile.center(),
        direction: profile.normal(),
        distance: this.thickness,
        side: this.side,
        cb: (thickness, side) => {
          this.setExpression('thickness', null)
          this.thickness = thickness
          this.side = side
        },
      },
    }
  }

  preview() {
    return this.isComplete() && this.error?.type != 'error' ? this.previewBody : null
  }
}

Serialize.register(SheetFaceFeature, 'SheetFaceFeature')


// Bent sheet following an open chain of sketch lines, extruded along the sketch normal
export class ContourFlangeFeature extends SolidFeature {
  static icon = 'sheet-contour'
  constructor(document) {
    super(document, false, 'Contour Flange', {
      lines: {
        title: 'Contour',
        type: 'curve',
        multi: true,
      },
      thickness: {
        title: 'Thickness',
        type: 'length',
      },
      flip: {
        title: 'Thickness side',
        type: 'bool',
        icons: ['caret-left', 'caret-right'],
      },
      radius: {
        title: 'Bend radius',
        type: 'length',
      },
      side: {
        title: 'Width direction',
        type: 'select',
        options: {
          forward: { title: 'Forward', icon: 'caret-up' },
          reverse: { title: 'Reverse', icon: 'caret-down' },
          both: { title: 'Both directions', icon: 'arrows-alt-v' },
        },
      },
      width: {
        title: 'Width',
        type: 'length',
        gizmo: true,
      },
    })

    this.operation = 'join'
    this.lines = null
    this.thickness = 1.0
    this.flip = false
    this.radius = 1.0
    this.side = 'forward'
    this.width = 10.0
    bindRules(this, { thickness: 'thickness', radius: 'bendRadius' })
  }

  // Sketch plane, moved to where the extrusion starts
  startPlane(sketch) {
    const offset = this.side == 'both' ? -this.width : 0
    const normal = normalFromMatrix(sketch.workplane).multiplyScalar(offset)
    return new THREE.Matrix4().makeTranslation(normal.x, normal.y, normal.z).multiply(sketch.workplane)
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    this.previewBody = null
    try {
      checkRules(this.thickness, this.radius)
      const { sketch, points } = chainPoints(references.lines)
      const { curves, names } = sheetSection(this.flip ? points.reverse() : points, this.thickness, this.radius, 'contour')
      const plane = planeFromMatrix(this.startPlane(sketch))
      const width = this.width * (this.side == 'reverse' ? -1 : this.side == 'both' ? 2 : 1)
      const body = namedBody(this.componentId, curves, names,
        profile => profile.extrude(plane, plane.normal().scaled(width)), this.id)
      comp.compound = comp.compound.boolean(body, this.operation)
      this.previewBody = body
    } catch(err) { this.error = featureError(err, 'Contour flange could not be created') }
  }

  getGizmo() {
    const line = this.lines().at(-1).getItem()
    const center = line.center().clone().applyMatrix4(line.sketch.workplane)
    return {
      distance: {
        center,
        direction: normalFromMatrix(line.sketch.workplane),
        distance: this.width,
        side: this.side != 'reverse',
        cb: (width, side) => {
          this.setExpression('width', null)
          this.width = width
          if(this.side != 'both') this.side = side ? 'forward' : 'reverse'
        },
      },
    }
  }

  preview() {
    return this.isComplete() && this.error?.type != 'error' ? this.previewBody : null
  }
}

Serialize.register(ContourFlangeFeature, 'ContourFlangeFeature')


// Where a straight sheet edge is, and which way its faces point. The narrow
// face at the edge gives the sheet's thickness.
function flangeFrame(edge) {
  if(!edge.getAxis()) fail('Flanges need straight edges')
  const [start, end] = edge.vertices()
  const along = end.clone().sub(start)
  const span = along.length()
  along.normalize()

  const faces = edge.connectedFaces()
  const planes = faces.map(face => face.getPlane())
  if(faces.length != 2 || planes.some(plane => !plane)) fail('Flanges need an edge between two planar faces')
  const [rim, sheet] = faces.map((face, i) => {
    const normal = normalFromMatrix(planes[i])
    const across = normal.clone().cross(along)
    const extent = Math.max(...face.boundaryEdges().flatMap(other => other.vertices())
      .map(vertex => Math.abs(vertex.clone().sub(start).dot(across))))
    return { face, normal, extent }
  }).sort((a, b) => a.extent - b.extent)
  if(Math.abs(rim.normal.dot(sheet.normal)) > 1e-6) fail('Flanges need a square sheet edge')
  return { edge, sheetFace: sheet.face, start, end, along, span, out: rim.normal, normal: sheet.normal, thickness: rim.extent }
}

// Point of a flange's cross section at a distance along its edge
const framePoint = (frame, point, distance) => frame.start.clone()
  .addScaledVector(frame.along, distance)
  .addScaledVector(frame.out, point.x)
  .addScaledVector(frame.normal, point.y)

// Cross section of a flange, with x pointing out of the edge face and y out of
// the sheet face the edge lies on. Depending on the position, the bend starts
// at the edge or early enough to put the inner or outer face of the wall on
// the edge face. The length is measured from the outer virtual sharp.
function flangeLayout(thickness, { length, angle, radius, up, position }) {
  checkRules(thickness, radius)
  if(!(angle > 0 && angle < 180)) fail('Angle must lie between 0° and 180°')
  const half = Math.tan(rad(angle) / 2)
  const inset = {
    'bend-outside': 0,
    'material-outside': radius * half,
    'material-inside': (radius + thickness) * half,
  }[position]
  const straight = length - (radius + thickness) * half
  if(straight < -modelTolerance.linear) fail('Length must exceed the bend')
  const turn = rad(angle) * (up ? 1 : -1)
  const direction = v2(Math.cos(turn), Math.sin(turn))
  // Distance from the start of the bend to the virtual sharp of the path's face
  const setback = (up ? radius : radius + thickness) * half
  const corner = v2(setback - inset, 0)
  return {
    inset,
    direction,
    path: [v2(-inset, 0), corner, corner.clone().addScaledVector(direction, setback + Math.max(straight, 0))],
    outerSharp: v2((radius + thickness) * half - inset, up ? -thickness : 0),
  }
}

// Cross section with the bounds of its curves, which edge tools need to reach their end planes
function section({ curves, names }) {
  const points = curves.flatMap(curve => {
    const { start, end } = curve.range()
    return Array.from({ length: 33 }, (_, i) => curve.evaluate(start + (end - start) * i / 32).point)
  })
  const xs = points.map(point => point.x)
  const ys = points.map(point => point.y)
  return { curves, names, min: v2(Math.min(...xs), Math.min(...ys)), max: v2(Math.max(...xs), Math.max(...ys)) }
}

// Sheet material that a flange bending early replaces. It reaches past the
// sheet's faces, as faces of tools lying on them are hard to cut with.
function insetSection(frame, inset, tag) {
  const margin = frame.thickness
  const [low, high] = [-frame.thickness - margin, margin]
  return section(polygon([v2(-inset, low), v2(margin, low), v2(margin, high), v2(-inset, high)], tag))
}

// Outward normal of the side face next to an edge's vertex
function neighborRim(frame, vertex) {
  const edge = frame.sheetFace.boundaryEdges().find(other => other.kernelId != frame.edge.kernelId &&
    other.vertices().some(point => point.distanceTo(vertex) <= tolerance()))
  const plane = edge?.connectedFaces().find(face => face.kernelId != frame.sheetFace.kernelId)?.getPlane()
  return plane && normalFromMatrix(plane)
}

// Square end closest to a vertex that keeps a cross section extruded along an
// edge clear of a plane. Square ends flatten exactly. The clearance keeps
// tools from touching the plane's face along their edges. Planes square to
// the edge already are kept, so tools stay flush with faces on them. Sections
// count as reaching the edge face, as the sheet ends there.
function squareEnd(frame, vertex, inward, plane, { min, max }) {
  if(plane.normal.dot(inward) > 1 - 1e-9) return plane
  const clearance = frame.thickness
  const crossings = [Math.min(min.x, 0) - clearance, Math.max(max.x, 0) + clearance].flatMap(x => [min.y, max.y].map(y => {
    const point = vertex.clone().addScaledVector(frame.out, x).addScaledVector(frame.normal, y)
    return plane.point.clone().sub(point).dot(plane.normal) / inward.dot(plane.normal)
  }))
  const distance = Math.max(...crossings)
  if(Math.abs(distance) < tolerance()) return null
  return { point: vertex.clone().addScaledVector(inward, distance), normal: inward }
}

// Planes bounding the ends of the tools along a feature's edges, given as
// { cut, body, relief } for the sheet material removed, the material added
// and the relief next to it. Each plane faces its tool. Tools end square
// short of the side faces next to their edges. Where two of them meet, they
// end on the plane bisecting the corner, or square short of the other's
// cross section.
function edgeEnds(frames, sections, index, corners, gap) {
  const frame = frames[index]
  return [frame.start, frame.end].map((vertex, end) => {
    const inward = frame.along.clone().multiplyScalar(end ? -1 : 1)
    const neighbor = frames.findIndex((other, i) => i != index && other.normal.dot(frame.normal) > 1 - 1e-6 &&
      [other.start, other.end].some(point => point.distanceTo(vertex) <= tolerance()))
    const out = neighbor == -1 ? neighborRim(frame, vertex) : frames[neighbor].out
    // Side faces continuing the edge leave its end square
    if(!out || Math.abs(out.dot(inward)) < 0.05) return { cut: null, body: null, relief: null }
    const facing = normal => normal.clone().multiplyScalar(Math.sign(normal.dot(inward)))
    const rim = { point: vertex, normal: facing(out) }
    const convex = out.dot(inward) < 0
    const square = plane => squareEnd(frame, vertex, inward, plane, sections[index])
    // Sheet material between the square end and the side face stays. At
    // concave corners the sheet continues past the side face. A relief keeps
    // the tool from fusing with it.
    if(neighbor == -1) {
      const end = square(rim)
      return { cut: end, body: end, relief: convex ? null : rim }
    }
    // At convex corners, nothing lies beyond the side face for cuts to remove
    const cut = convex ? { point: vertex.clone().addScaledVector(rim.normal, -frame.thickness), normal: rim.normal } : rim
    if(corners == 'miter') {
      const normal = facing(frame.out.clone().sub(out).normalize())
      return { cut, body: { point: vertex.clone().addScaledVector(normal, gap / 2), normal }, relief: null }
    }
    const { min, max } = sections[neighbor]
    const reach = out.dot(rim.normal) > 0 ? max.x : -min.x
    return { cut, body: square({ point: vertex.clone().addScaledVector(rim.normal, reach + gap / 2), normal: rim.normal }), relief: null }
  })
}

// How far a tool must extend past a vertex, away from the inward direction, to reach an end plane
function endExtension(frame, vertex, inward, plane, { min, max }) {
  if(!plane) return 0
  const crossings = [min.x, max.x].flatMap(x => [min.y, max.y].map(y => {
    const point = vertex.clone().addScaledVector(frame.out, x).addScaledVector(frame.normal, y)
    return plane.point.clone().sub(point).dot(plane.normal) / inward.dot(plane.normal)
  }))
  return Math.max(0, -Math.min(...crossings)) + frame.thickness
}

// Extrudes a cross section in flange coordinates along an edge from start to
// end, bounded by end planes
function edgeTool(componentId, frame, section, ends, owner, start = frame.start, end = frame.end) {
  const [before, after] = ends.map((plane, index) =>
    endExtension(frame, index ? end : start, frame.along.clone().multiplyScalar(index ? -1 : 1), plane, section))
  const origin = start.clone().addScaledVector(frame.along, -before)
  const plane = sx.Plane3.new(point3(origin), vector3(frame.out), vector3(frame.normal), modelTolerance)
  const vector = vector3(frame.along.clone().multiplyScalar(end.clone().sub(start).dot(frame.along) + before + after))
  const body = ends.reduce((body, end) => end ? body.split(planeMatrix(end.point, end.normal), 'right') : body,
    namedBody(componentId, section.curves, section.names, profile => profile.extrude(plane, vector), owner))
  if(!body.solids().length) fail('Edges are too short for their corners')
  return body
}

// Slot through the sheet past the end of a tool, as wide as the sheet is thick
function reliefTool(componentId, frame, bounds, end, rim, tag, owner) {
  const vertex = end ? frame.end : frame.start
  const width = frame.thickness
  const { min, max } = bounds
  const outline = polygon([v2(min.x - width, -frame.thickness - width), v2(max.x + width, -frame.thickness - width),
    v2(max.x + width, width), v2(min.x - width, width)], tag)
  const near = { point: rim.point, normal: rim.normal.clone().negate() }
  const far = { point: rim.point.clone().addScaledVector(rim.normal, -width), normal: rim.normal }
  return edgeTool(componentId, frame, section(outline), end ? [near, far] : [far, near], owner, vertex, vertex)
}

// Removes the sheet material where tools bend early and next to their ends,
// then adds the tools. Booleans apply one at a time, as tools touching at
// corners cannot be united.
function addEdgeTools(feature, comp, frames, sections, insets, corners, gap) {
  const ends = frames.map((_, index) => edgeEnds(frames, sections, index, corners, gap))
  const cut = tool => comp.compound = comp.compound.boolean(tool, 'cut')
  frames.forEach((frame, index) => {
    const tag = sections[index].tag
    if(insets[index] > 0) cut(edgeTool(feature.componentId, frame, insetSection(frame, insets[index], `${tag}/inset`),
      ends[index].map(end => end.cut), feature.id))
    ends[index].forEach((end, side) => {
      if(end.relief) cut(reliefTool(feature.componentId, frame, sections[index], side, end.relief, `${tag}/relief/${side}`, feature.id))
    })
  })
  frames.forEach((frame, index) => comp.compound = comp.compound.boolean(edgeTool(feature.componentId, frame,
    sections[index], ends[index].map(end => end.body), feature.id), 'join'))
}

// Sheet metal features showing length and angle gizmos
class SheetGizmoFeature extends SolidFeature {
  updateGizmos() {
    this.sheetGizmos ||= markRaw(new FeatureGizmos())
    let gizmos = {}
    // Settings the feature cannot be built with show no gizmos
    if(this.isComplete()) try { gizmos = this.getGizmo() } catch(err) { featureError(err) }
    this.sheetGizmos.update(gizmos, worldTransform(featureComponent(this)))
  }

  dispose() {
    super.dispose()
    this.sheetGizmos?.dispose()
    this.sheetGizmos = null
  }
}

export class FlangeFeature extends SheetGizmoFeature {
  static icon = 'sheet-flange'
  constructor(document) {
    super(document, false, 'Flange', {
      edges: {
        title: 'Edges',
        type: 'edge',
        multi: true,
      },
      position: {
        title: 'Position',
        type: 'select',
        options: {
          'material-inside': { title: 'Material inside — outer face on the edge', icon: 'flange-material-inside' },
          'material-outside': { title: 'Material outside — inner face on the edge', icon: 'flange-material-outside' },
          'bend-outside': { title: 'Bend outside — bend starts at the edge', icon: 'flange-bend-outside' },
        },
      },
      length: {
        title: 'Length',
        type: 'length',
        gizmo: true,
      },
      angle: {
        title: 'Angle',
        type: 'angle',
        gizmo: true,
      },
      up: {
        title: 'Direction',
        type: 'bool',
        icons: ['caret-up', 'caret-down'],
      },
      radius: {
        title: 'Bend radius',
        type: 'length',
      },
      corners: {
        title: 'Corners',
        type: 'select',
        options: {
          open: { title: 'Open', icon: 'corner-open' },
          miter: { title: 'Mitered', icon: 'corner-miter' },
        },
      },
      gap: {
        title: 'Corner gap',
        type: 'length',
      },
    })

    this.operation = 'join'
    this.edges = null
    this.position = 'material-inside'
    this.length = 10.0
    this.angle = 90
    this.up = true
    this.radius = 1.0
    this.corners = 'miter'
    this.gap = 0.2
    bindRules(this, { radius: 'bendRadius' })
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      if(!(this.gap >= 0)) fail('Corner gap must not be negative')
      const frames = references.edges.map(flangeFrame)
      const layouts = frames.map(frame => flangeLayout(frame.thickness, this))
      const sections = frames.map((frame, index) => ({ tag: `flange/${index}`,
        ...section(sheetSection(layouts[index].path, frame.thickness, this.radius, `flange/${index}`)) }))
      addEdgeTools(this, comp, frames, sections, layouts.map(layout => layout.inset), this.corners, this.gap)
    } catch(err) { this.error = featureError(err, 'Flange could not be created') }
  }

  // Both gizmos sit at the outer virtual sharp in the middle of the last edge
  getGizmo() {
    const edge = this.inputItems('edges').at(-1)
    if(!edge) return {}
    const frame = flangeFrame(edge)
    const layout = flangeLayout(frame.thickness, this)
    const center = framePoint(frame, layout.outerSharp, frame.span / 2)
    const wall = frame.out.clone().multiplyScalar(layout.direction.x).addScaledVector(frame.normal, layout.direction.y)
    return {
      length: {
        type: 'length', center, direction: wall, value: this.length,
        cb: length => {
          this.setExpression('length', null)
          this.length = length
        },
      },
      angle: {
        type: 'angle', center, size: 0.65,
        rotation: new THREE.Matrix4().makeBasis(frame.out, frame.normal, frame.out.clone().cross(frame.normal)),
        value: this.angle * (this.up ? 1 : -1),
        cb: angle => {
          this.setExpression('angle', null)
          this.angle = Math.min(179, Math.max(1, Math.abs(angle)))
          if(angle) this.up = angle > 0
        },
      },
    }
  }

}

Serialize.register(FlangeFeature, 'FlangeFeature')


// Cross section of a hem in flange coordinates. The sheet turns by 180°
// around a bend whose inside radius is half the gap and returns over itself.
function hemLayout(thickness, { position, length, gap, up }) {
  checkRules(thickness)
  if(!(gap > 0)) fail('Gap must be positive')
  if(!(length > 0)) fail('Length must be positive')
  const radius = gap / 2
  const start = position == 'material-inside' ? -(radius + thickness) : 0
  const point = (x, y) => v2(start + x, up ? y : -thickness - y)
  return {
    inset: -start,
    radius,
    point,
    outline: [
      [point(0, 0), point(radius, radius), point(0, gap)],
      [point(0, gap), point(-length, gap)],
      [point(-length, gap), point(-length, gap + thickness)],
      [point(-length, gap + thickness), point(0, gap + thickness)],
      [point(0, gap + thickness), point(radius + thickness, radius), point(0, -thickness)],
      [point(0, -thickness), point(0, 0)],
    ],
  }
}

function hemSection(frame, index, settings) {
  const { outline } = hemLayout(frame.thickness, settings)
  const curves = outline.map(points => points.length == 3 ?
    sx.TrimmedCurve2.arcThrough(...points.map(point2)) :
    sx.TrimmedCurve2.line(...points.map(point2)))
  const names = ['inner', 'return/inner', 'end', 'return/outer', 'outer', 'start'].map(name => `hem/${index}/${name}`)
  return { tag: `hem/${index}`, ...section({ curves, names }) }
}

export class HemFeature extends SheetGizmoFeature {
  static icon = 'sheet-hem'
  constructor(document) {
    super(document, false, 'Hem', {
      edges: {
        title: 'Edges',
        type: 'edge',
        multi: true,
      },
      position: {
        title: 'Position',
        type: 'select',
        options: {
          'material-inside': { title: 'Material inside — hem stays within the edge', icon: 'hem-material-inside' },
          'bend-outside': { title: 'Bend outside — hem starts at the edge', icon: 'hem-bend-outside' },
        },
      },
      length: {
        title: 'Length',
        type: 'length',
        gizmo: true,
      },
      gap: {
        title: 'Gap',
        type: 'length',
      },
      up: {
        title: 'Direction',
        type: 'bool',
        icons: ['caret-up', 'caret-down'],
      },
    })

    this.operation = 'join'
    this.edges = null
    this.position = 'material-inside'
    this.length = 5.0
    this.gap = 0.5
    this.up = true
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      const frames = references.edges.map(flangeFrame)
      // Hems meeting at a corner are mitered
      addEdgeTools(this, comp, frames, frames.map((frame, index) => hemSection(frame, index, this)),
        frames.map(frame => hemLayout(frame.thickness, this).inset), 'miter', 0)
    } catch(err) { this.error = featureError(err, 'Hem could not be created') }
  }

  // The length gizmo runs along the outside of the returning leg
  getGizmo() {
    const edge = this.inputItems('edges').at(-1)
    if(!edge) return {}
    const frame = flangeFrame(edge)
    const layout = hemLayout(frame.thickness, this)
    return {
      length: {
        type: 'length',
        center: framePoint(frame, layout.point(0, this.gap + frame.thickness), frame.span / 2),
        direction: frame.out.clone().negate(),
        value: this.length,
        cb: length => {
          this.setExpression('length', null)
          this.length = length
        },
      },
    }
  }
}

Serialize.register(HemFeature, 'HemFeature')


// Thickness of a sheet at a planar face, from the nearest face facing the other way
function sheetThickness(face) {
  const normal = normalFromMatrix(face.getPlane())
  const center = face.center()
  const distances = face.solid.faces()
    .filter(other => other.getPlane() && normalFromMatrix(other.getPlane()).dot(normal) < -1 + 1e-6)
    .map(other => center.clone().sub(other.center()).dot(normal))
    .filter(distance => distance > tolerance())
  if(!distances.length) fail('The face has no opposite sheet face')
  return Math.min(...distances)
}

// Where a sketched bend line lies on a sheet face. The material on the out
// side of the line moves, the face's normal points up.
function bendFrame(face, line, flip) {
  const plane = face.getPlane()
  if(!plane) fail('Bends need a planar face')
  if(!(line instanceof Line)) fail('Bend lines must be straight')
  const normal = normalFromMatrix(plane)
  const origin = new THREE.Vector3().setFromMatrixPosition(plane)
  const [start, end] = line.endpoints().map(point => new THREE.Vector3(point.x, point.y, 0).applyMatrix4(line.sketch.workplane))
  if([start, end].some(point => Math.abs(point.clone().sub(origin).dot(normal)) > tolerance())) fail('The bend line must lie on the face')
  const along = end.clone().sub(start).normalize()
  const out = normal.clone().cross(along).multiplyScalar(flip ? -1 : 1)
  return { start, end, along, out, normal, thickness: sheetThickness(face) }
}

// Removes a strip as wide as the bend allowance at the line, then wraps the
// fixed side's cut face around the bend axis and rotates the moving side to
// the end of the bend.
function bendBody(sheet, frame, settings, owner) {
  const { thickness, out, normal } = frame
  const { radius, kFactor, up } = settings
  const componentId = sheet.componentId
  checkRules(thickness, radius)
  if(!(settings.angle > 0 && settings.angle <= 180)) fail('Angle must lie between 0° and 180°')
  const angle = rad(settings.angle)
  const allowance = angle * (radius + kFactor * thickness)
  const offset = { centerline: -allowance / 2, start: 0, end: -allowance }[settings.position]
  const cut = frame.start.clone().addScaledVector(out, offset)
  const fixed = sheet.split(planeMatrix(cut, out.clone().negate()), 'right')
  const moving = sheet.split(planeMatrix(cut.clone().addScaledVector(out, allowance), out), 'right')

  // Rotating about out × normal turns out towards normal
  const axisPoint = cut.clone().addScaledVector(normal, up ? radius : -thickness - radius)
  const axis = out.clone().cross(normal)
  const sweep = up ? angle : -angle
  const radial = cut.clone().sub(axisPoint).normalize()
  const plane = sx.Plane3.new(point3(axisPoint), vector3(radial), vector3(axis), modelTolerance)
  const faces = fixed.faces().filter(face => {
    const facePlane = face.getPlane()
    return facePlane && normalFromMatrix(facePlane).dot(out) > 1 - 1e-6 && Math.abs(face.center().sub(cut).dot(out)) < tolerance()
  })
  if(!faces.length) fail('The bend line must cross the sheet')
  const bends = faces.map((face, index) => {
    const curves = chainCurves(face.boundaryEdges().map(edge => plane.expressCurve(edge.geom().trimmedCurve(), modelTolerance)))
    return namedBody(componentId, curves, curves.map((_, i) => `bend/${index}/${i}`),
      profile => profile.revolve(plane, sx.Axis3.new(point3(axisPoint), vector3(axis)), sweep), owner)
  })
  const shift = out.clone().multiplyScalar(-allowance)
  const fold = rotationAbout(axisPoint, axis, sweep).multiply(new THREE.Matrix4().makeTranslation(shift.x, shift.y, shift.z))
  return bodyFrom([fixed, ...bends, ...(moving.solids().length ? [moving.transform(fold)] : [])], componentId)
}

export class BendFeature extends SheetGizmoFeature {
  static icon = 'sheet-bend'
  constructor(document) {
    super(document, false, 'Bend', {
      face: {
        title: 'Sheet face',
        type: 'face',
      },
      line: {
        title: 'Bend line',
        type: 'curve',
      },
      position: {
        title: 'Line position',
        type: 'select',
        options: {
          centerline: { title: 'Bend centered on the line', icon: 'bend-centerline' },
          start: { title: 'Bend starts at the line', icon: 'bend-start' },
          end: { title: 'Bend ends at the line', icon: 'bend-end' },
        },
      },
      flip: {
        title: 'Moving side',
        type: 'bool',
        icons: ['caret-left', 'caret-right'],
      },
      angle: {
        title: 'Angle',
        type: 'angle',
        gizmo: true,
      },
      up: {
        title: 'Direction',
        type: 'bool',
        icons: ['caret-up', 'caret-down'],
      },
      radius: {
        title: 'Bend radius',
        type: 'length',
      },
      kFactor: {
        title: 'K-factor',
        type: 'number',
        min: 0,
        max: 1,
        step: 0.01,
      },
    })

    this.face = null
    this.line = null
    this.position = 'centerline'
    this.flip = false
    this.angle = 90
    this.up = true
    this.radius = 1.0
    this.kFactor = 0.44
    bindRules(this, { radius: 'bendRadius' })
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      const solid = references.face.solid
      const frame = bendFrame(references.face, references.line, this.flip)
      const others = comp.compound.solids().filter(other => other.id != solid.id)
      const bent = bendBody(comp.compound.subset([solid]), frame, this, this.id)
      comp.compound = others.length ? comp.compound.subset(others).merge([bent]) : bent
    } catch(err) { this.error = featureError(err, 'Sheet could not be bent') }
  }

  getGizmo() {
    const frame = bendFrame(this.inputItems('face')[0], this.inputItems('line')[0], this.flip)
    return {
      angle: {
        type: 'angle', size: 0.65,
        center: frame.start.clone().add(frame.end).multiplyScalar(0.5),
        rotation: new THREE.Matrix4().makeBasis(frame.out, frame.normal, frame.out.clone().cross(frame.normal)),
        value: this.angle * (this.up ? 1 : -1),
        cb: angle => {
          this.setExpression('angle', null)
          this.angle = Math.min(180, Math.max(1, Math.abs(angle)))
          if(angle) this.up = angle > 0
        },
      },
    }
  }
}

Serialize.register(BendFeature, 'BendFeature')


// Unfolding

const radialOffset = (point, bend) => {
  const offset = point.clone().sub(bend.a)
  return offset.addScaledVector(bend.e, -offset.dot(bend.e))
}

// Bends are pairs of coaxial cylindrical faces, whose radii differ by the
// sheet thickness. Lines along the axis bound them where they meet their flats.
function findBends(solid) {
  const groups = []
  for(const face of solid.faces()) {
    const surface = face.geom().surface()
    if(surface.kind != 'cylinder') continue
    const a = threeVector(surface.axis.origin())
    const e = threeVector(surface.axis.direction()).normalize()
    const vertices = face.boundaryEdges().flatMap(edge => edge.vertices())
    let group = groups.find(group => Math.abs(group.e.dot(e)) > 1 - 1e-9 && radialOffset(a, group).length() < tolerance())
    if(!group) groups.push(group = { a, e, faces: [] })
    group.faces.push({ face, vertices, radius: radialOffset(vertices[0], group).length() })
  }

  return groups.flatMap(group => {
    const radii = group.faces.map(face => face.radius).sort((a, b) => a - b)
      .filter((radius, i, all) => !i || radius - all[i - 1] > tolerance())
    // Holes and bosses have a single radius
    if(radii.length != 2) return []

    const directions = []
    for(const { face } of group.faces) for(const edge of face.boundaryEdges()) {
      const axis = edge.getAxis()
      if(!axis || Math.abs(normalFromMatrix(axis).dot(group.e)) < 1 - 1e-9) continue
      const direction = radialOffset(edge.vertices()[0], group).normalize()
      if(!directions.some(other => other.dot(direction) > 1 - 1e-9)) directions.push(direction)
    }
    if(directions.length != 2) fail('Bends must be bounded by straight tangent lines')

    // Signed angle from the first to the second tangent line, through the bend
    const [w0, w1] = directions
    const angleOf = w => Math.atan2(w.dot(group.e.clone().cross(w0)), w.dot(w0))
    const middle = angleOf(radialOffset(group.faces[0].face.center(), group))
    let sweep = angleOf(w1)
    if(Math.sign(middle) != Math.sign(sweep) || Math.abs(middle) > Math.abs(sweep)) sweep -= Math.sign(sweep) * 2 * Math.PI

    const vertices = group.faces.flatMap(face => face.vertices)
    const heights = vertices.map(vertex => vertex.clone().sub(group.a).dot(group.e))

    // Planar faces closing the bend meet its faces along arcs. Corners can
    // cut them at an angle, so they bound the bend better than its height range.
    const center = vertices.reduce((sum, vertex) => sum.add(vertex), new THREE.Vector3()).divideScalar(vertices.length)
    const bendFaces = new Set(group.faces.map(({ face }) => face.kernelId))
    const ends = []
    for(const { face } of group.faces) for(const edge of face.boundaryEdges()) {
      if(edge.getAxis()) continue
      for(const other of edge.connectedFaces()) {
        const plane = !bendFaces.has(other.kernelId) && other.getPlane()
        if(!plane) continue
        const point = new THREE.Vector3().setFromMatrixPosition(plane)
        const normal = normalFromMatrix(plane)
        normal.multiplyScalar(Math.sign(center.clone().sub(point).dot(normal)))
        if(!ends.some(end => end.normal.dot(normal) > 1 - 1e-9 && Math.abs(end.point.clone().sub(point).dot(normal)) < tolerance())) {
          ends.push({ point, normal })
        }
      }
    }

    return [{
      a: group.a, e: group.e, w: [w0, w1], sweep,
      radius: radii[0], thickness: radii[1] - radii[0],
      range: [Math.min(...heights), Math.max(...heights)],
      ends,
    }]
  })
}

// Region swept by a bend, slightly oversized radially and bounded by its end
// faces, or slightly past them
function bendWedge(componentId, bend, tag, owner, bounded = true) {
  const margin = 0.1 * Math.min(bend.radius, bend.thickness)
  const [inner, outer] = [bend.radius - margin, bend.radius + bend.thickness + margin]
  const [low, high] = [bend.range[0] - margin, bend.range[1] + margin]
  const { curves, names } = polygon([v2(inner, low), v2(outer, low), v2(outer, high), v2(inner, high)], tag)
  const plane = sx.Plane3.new(point3(bend.a), vector3(bend.w[0]), vector3(bend.e), modelTolerance)
  const axis = sx.Axis3.new(point3(bend.a), vector3(bend.e))
  const wedge = namedBody(componentId, curves, names, profile => profile.revolve(plane, axis, bend.sweep), owner)
  return bounded ? bend.ends.reduce((wedge, end) => wedge.split(planeMatrix(end.point, end.normal), 'right'), wedge) : wedge
}

// Direction from a tangent line into the bend, along the flat on that side
const bendTangent = (bend, side) => bend.e.clone().cross(bend.w[side])
  .multiplyScalar(Math.sign(bend.sweep) * (side ? -1 : 1))

// Planar faces of a flat where it was cut from a bend's side
function bendSideFaces(solid, bend, side) {
  const w = bend.w[side]
  const normal = bend.e.clone().cross(w)
  return solid.faces().filter(face => {
    const plane = face.getPlane()
    if(!plane || Math.abs(normalFromMatrix(plane).dot(normal)) < 1 - 1e-6) return false
    const offset = face.center().sub(bend.a)
    const radial = offset.dot(w)
    return Math.abs(offset.dot(normal)) < tolerance() &&
      radial > bend.radius - tolerance() && radial < bend.radius + bend.thickness + tolerance()
  })
}

// Orders 2D curves into a closed chain
function chainCurves(curves) {
  const close = (p, q) => Math.hypot(p.x - q.x, p.y - q.y) <= tolerance()
  const chain = [curves[0]]
  const remaining = curves.slice(1)
  while(remaining.length) {
    const end = chain.at(-1).end()
    const index = remaining.findIndex(curve => close(curve.start(), end) || close(curve.end(), end))
    if(index == -1) fail('Bend cross sections must be single loops')
    const [curve] = remaining.splice(index, 1)
    chain.push(close(curve.start(), end) ? curve : curve.reversed())
  }
  return chain
}

// Cross section of a bend where it meets the flat on the given side
function bendSection(face, bend, side) {
  const plane = sx.Plane3.new(point3(bend.a), vector3(bend.w[side]), vector3(bend.e), modelTolerance)
  const curves = chainCurves(face.boundaryEdges().map(edge => plane.expressCurve(edge.geom().trimmedCurve(), modelTolerance)))
  return { plane, curves, names: curves.map((_, i) => i) }
}

// Trims a body to the end faces of a bend, where flats continue past them
function clipToBend(body, bend) {
  const vertices = body.solids().flatMap(solid => solid.edges()).flatMap(edge => edge.vertices())
  return bend.ends.reduce((body, end) =>
    vertices.some(vertex => vertex.clone().sub(end.point).dot(end.normal) < -tolerance()) ?
      body.split(planeMatrix(end.point, end.normal), 'right') : body, body)
}

// Flat strip replacing a bend, extruding the flat's cross section by the bend allowance
function bendStrip(componentId, face, bend, side, allowance, tag, owner) {
  const { plane, curves, names } = bendSection(face, bend, side)
  const vector = vector3(bendTangent(bend, side).multiplyScalar(allowance))
  return clipToBend(namedBody(componentId, curves, names.map(name => `${tag}/${name}`),
    profile => profile.extrude(plane, vector), owner), bend)
}

// Moves the flat on the other side of a bend into the plane of the flat on
// the given side, leaving room for the flattened bend
function unbendTransform(bend, side, allowance) {
  const shift = bendTangent(bend, side).multiplyScalar(allowance)
  return new THREE.Matrix4().makeTranslation(shift.x, shift.y, shift.z)
    .multiply(rotationAbout(bend.a, bend.e, side ? bend.sweep : -bend.sweep))
}

// Cuts every bend out of a sheet body, then lays out the remaining flats in
// the plane of the fixed face, connected by strips as long as the bend
// allowances. Returns where each flat and strip went and the bends as they
// were, so they can be refolded.
function unfoldBody(body, fixedFace, kFactor, owner) {
  const componentId = body.componentId
  if(!fixedFace.getPlane()) fail('The fixed face must be flat')
  const bends = findBends(fixedFace.solid)
  if(!bends.length) fail('The body has no bends')

  const wedges = bends.map((bend, index) => bendWedge(componentId, bend, `unfold/${index}/wedge`, owner))
  // Wedges of bends meeting at a corner overlap, which their union does not support
  const cut = wedges.reduce((flats, wedge) => flats.boolean(wedge, 'cut'), body)
  const solids = cut.solids()
  const center = fixedFace.center()
  const fixed = solids.find(solid => solid.faces().some(face => face.center().distanceTo(center) < tolerance()))
  if(!fixed) fail('The fixed face must not lie on a bend')

  const placements = new Map([[fixed, new THREE.Matrix4()]])
  const strips = []
  const unfolded = new Set()
  const queue = [fixed]
  while(queue.length) {
    const solid = queue.shift()
    const placement = placements.get(solid)
    bends.forEach((bend, index) => {
      if(unfolded.has(bend)) return
      const side = [0, 1].find(side => bendSideFaces(solid, bend, side).length)
      if(side === undefined) return
      unfolded.add(bend)
      const allowance = Math.abs(bend.sweep) * (bend.radius + kFactor * bend.thickness)
      bendSideFaces(solid, bend, side).forEach((face, faceIndex) => strips.push({
        bend, side, face, allowance, placement,
        body: bendStrip(componentId, face, bend, side, allowance, `unfold/${index}/strip/${faceIndex}`, owner).transform(placement),
      }))
      const unbend = placement.clone().multiply(unbendTransform(bend, side, allowance))
      solids
        .filter(other => !placements.has(other) && bendSideFaces(other, bend, 1 - side).length)
        .forEach(other => {
          placements.set(other, unbend)
          queue.push(other)
        })
    })
  }
  if(placements.size < solids.length) fail('Some flats are not connected to the fixed face')

  const flats = solids.map(solid => {
    const flat = cut.subset([solid])
    const placement = placements.get(solid)
    return { placement, body: placement.equals(new THREE.Matrix4()) ? flat : flat.transform(placement) }
  })
  return {
    body: bodyFrom([...flats, ...strips].map(part => part.body), componentId),
    flats,
    strips,
    // Intersections with wedges ending on faces of the body are unreliable
    bends: bends.map((bend, index) => ({ bend,
      body: clipToBend(body.boolean(bendWedge(componentId, bend, `unfold/${index}/bend`, owner, false), 'intersect'), bend) })),
    plane: fixedFace.getPlane(),
  }
}

// Centerlines of the flattened bends
function bendLines(record) {
  return record.strips.map(({ bend, side, face, allowance, placement }) => {
    const heights = face.boundaryEdges().flatMap(edge => edge.vertices()).map(vertex => vertex.clone().sub(bend.a).dot(bend.e))
    const middle = bend.a.clone().addScaledVector(bendTangent(bend, side), allowance / 2)
    return [Math.min(...heights), Math.max(...heights)]
      .map(height => middle.clone().addScaledVector(bend.e, height).applyMatrix4(placement))
  })
}

// Flat pattern of the largest body of a compound, unfolded at its largest
// planar face and laid into the XY plane with that face on top
export function flatPattern(compound, kFactor) {
  const solid = compound.solids().sort((a, b) => b.volume() - a.volume())[0]
  if(!solid) fail('The component has no body')
  const face = solid.faces().filter(face => face.getPlane()).sort((a, b) => b.area() - a.area())[0]
  if(!face) fail('The body has no flat faces')
  const body = compound.subset([solid])
  const record = findBends(solid).length ? unfoldBody(body, face, kFactor) : { body, strips: [], plane: face.getPlane() }
  const toXY = record.plane.clone().invert()
  return {
    // Puts the pattern back onto the fixed face
    placement: record.plane,
    unfolded: record.body,
    body: record.body.transform(toXY),
    bendLines: bendLines(record).map(line => line.map(point => point.applyMatrix4(toXY).setZ(0))),
  }
}

// Region of a bend that corresponds to material removed from its flat strip.
// Cuts map to the bend by their extent across and along the strip.
function removedWedge(componentId, strip, solid, tag, owner) {
  const { bend, side, allowance } = strip
  const tangent = bendTangent(bend, side)
  const margin = 0.1 * Math.min(bend.radius, bend.thickness)
  const points = solid.edges().flatMap(edge => edge.tesselate()).map(point => point.clone().sub(bend.a))
  // Cuts reaching the strip's boundaries continue past the bend's
  const range = (values, low, high) => [
    Math.min(...values) < low + tolerance() ? low - margin : Math.min(...values),
    Math.max(...values) > high - tolerance() ? high + margin : Math.max(...values),
  ]
  const [s0, s1] = range(points.map(point => point.dot(tangent)), 0, allowance)
  const [z0, z1] = range(points.map(point => point.dot(bend.e)), ...bend.range)
  const neutral = allowance / Math.abs(bend.sweep)
  const direction = Math.sign(bend.sweep) * (side ? -1 : 1)
  const [inner, outer] = [bend.radius - margin, bend.radius + bend.thickness + margin]
  const { curves, names } = polygon([v2(inner, z0), v2(outer, z0), v2(outer, z1), v2(inner, z1)], tag)
  const plane = sx.Plane3.new(point3(bend.a), vector3(bend.w[side]), vector3(bend.e), modelTolerance)
  const axis = sx.Axis3.new(point3(bend.a), vector3(bend.e))
  return namedBody(componentId, curves, names, profile => profile.revolve(plane, axis, direction * (s1 - s0) / neutral), owner)
    .transform(rotationAbout(bend.a, bend.e, direction * s0 / neutral))
}

// Moves the flats of an unfolded body back into place and restores their
// bends, without what was cut from their strips
function refoldBody(body, record, owner) {
  const componentId = body.componentId
  // Flats nothing was cut from keep their exact geometry, which their bends join best
  const flats = record.flats.map(flat => {
    const removed = flat.body.boolean(body, 'cut')
    return (removed.solids().length ? flat.body.boolean(removed, 'cut') : flat.body).transform(flat.placement.clone().invert())
  })
  const bends = record.bends.map(({ bend, body: bent }, index) => record.strips
    .filter(strip => strip.bend == bend)
    .flatMap(strip => strip.body.boolean(body, 'cut').transform(strip.placement.clone().invert()).solids()
      .map(solid => ({ strip, solid })))
    .reduce((bent, { strip, solid }, cutIndex) =>
      bent.boolean(removedWedge(componentId, strip, solid, `refold/${index}/cut/${cutIndex}`, owner), 'cut'), bent))
  return bodyFrom([...flats, ...bends].filter(part => part.solids().length), componentId)
}

export class UnfoldFeature extends SolidFeature {
  static icon = 'sheet-unfold'
  constructor(document) {
    super(document, false, 'Unfold', {
      face: {
        title: 'Fixed face',
        type: 'face',
      },
      kFactor: {
        title: 'K-factor',
        type: 'number',
        min: 0,
        max: 1,
        step: 0.01,
      },
    })

    this.face = null
    this.kFactor = 0.44
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    this.record = null
    try {
      const sheet = references.face.solid
      const others = comp.compound.solids().filter(solid => solid.id != sheet.id)
      const record = unfoldBody(comp.compound.subset([sheet]), references.face, this.kFactor, this.id)
      comp.compound = others.length ? comp.compound.subset(others).merge([record.body]) : record.body
      // Kept for refolding, which needs to know where every flat and strip went
      this.record = markRaw(record)
    } catch(err) { this.error = featureError(err, 'Body could not be unfolded') }
  }
}

Serialize.register(UnfoldFeature, 'UnfoldFeature')


// Folds the body of the latest open Unfold back, including what features
// in between changed on its flats and strips
export class RefoldFeature extends SolidFeature {
  static icon = 'sheet-refold'
  constructor(document) {
    super(document, false, 'Refold', {})
  }

  unfoldFeature() {
    const features = this.document.timeline.features
    let refolds = 0
    for(const feature of features.slice(0, features.indexOf(this)).reverse()) {
      if(feature.componentId != this.componentId) continue
      if(feature instanceof RefoldFeature) refolds++
      if(feature instanceof UnfoldFeature && !refolds--) return feature
    }
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      const record = this.unfoldFeature()?.record
      if(!record) fail('Refold needs an Unfold before it')
      const solids = comp.compound.solids()
      const sheets = solids.filter(solid =>
        comp.compound.subset([solid]).boolean(record.body, 'intersect').volume() > tolerance())
      if(!sheets.length) fail('The unfolded body is gone')
      const others = solids.filter(solid => !sheets.includes(solid))
      const refolded = refoldBody(comp.compound.subset(sheets), record, this.id)
      comp.compound = others.length ? comp.compound.subset(others).merge([refolded]) : refolded
    } catch(err) { this.error = featureError(err, 'Body could not be refolded') }
  }
}

Serialize.register(RefoldFeature, 'RefoldFeature')
