import * as THREE from 'three'
import { toRaw } from 'vue'

import { SketchPoint, Line, Circle, Arc, Spline, SketchText } from '../core/geom2d.js'
import {
  ElemRef,
  SketchOrigin,
  ProjectedPoint,
  Projection,
  Intersection,
  CoincidentConstraint,
  HorVertConstraint,
  PerpendicularConstraint,
  ParallelConstraint,
  TangentConstraint,
  EqualConstraint,
  ConcentricConstraint,
  SymmetricConstraint,
  FixConstraint,
  MidpointConstraint,
  TouchConstraint,
  IntersectionConstraint,
  OffsetConstraint,
  ProfileOffsetConstraint,
  Dimension,
} from '../core/sketch.js'
import { Profile } from '../core/geom3d.js'
import { supportsFillet, curveFillet, trimToPoint } from '../core/sketch-fillet.js'
import { supportsOffset, connectedOffsetChain, profileOffsetGroups, nearestOffsetFrame, buildOffsetCurves } from '../core/sketch-offset.js'
import { listSystemFonts, findFont, preferredFont, loadSystemFont, textPieces } from '../core/sketch-text.js'
import Expression from '../core/expression.js'
import { ToolError, resolveItem } from './describe.js'

// Sketch edits as the drawing and constraint tools make them, with explicit
// coordinates instead of pointer input. Points are [x, y] on the sketch plane.

const constraintTypes = {
  coincident: CoincidentConstraint,
  horizontal: HorVertConstraint,
  vertical: HorVertConstraint,
  parallel: ParallelConstraint,
  perpendicular: PerpendicularConstraint,
  tangent: TangentConstraint,
  equal: EqualConstraint,
  concentric: ConcentricConstraint,
  symmetric: SymmetricConstraint,
  fix: FixConstraint,
  midpoint: MidpointConstraint,
  touch: TouchConstraint,
}

export const constraintTypeNames = Object.keys(constraintTypes)

function point(value, label) {
  if(!Array.isArray(value) || value.length != 2 || !value.every(Number.isFinite)) {
    throw new ToolError(`${label} must be a point given as [x, y]`)
  }
  return new THREE.Vector3(value[0], value[1], 0)
}

function positive(value, label) {
  if(!Number.isFinite(value) || value <= 0) throw new ToolError(`${label} must be a positive number`)
  return value
}

// Handles that take point constraints, like isConstrainablePoint of the tools
function constrainableHandles(elem) {
  if(elem instanceof SketchOrigin) return [0]
  if(elem instanceof SketchPoint || elem instanceof SketchText || elem instanceof ProjectedPoint) return [0]
  if(elem instanceof Line) return [0, 1]
  if(elem instanceof Circle) return [0]
  if(elem instanceof Arc) return [0, 1, 2]
  if(elem instanceof Spline) return [0, elem.handles().length - 1]
  return []
}

function handleIndex(elem, name) {
  if(Number.isInteger(name)) return name
  const names = elem instanceof Line ? { start: 0, end: 1 } :
    elem instanceof Arc ? { center: 0, start: 1, end: 2 } :
    elem instanceof Circle ? { center: 0 } :
    elem instanceof Spline ? { start: 0, end: elem.handles().length - 1 } :
    { point: 0, center: 0, start: 0 }
  if(!(name in names)) throw new ToolError(`Element ${elem.id} has no point '${name}', use one of ${Object.keys(names).join(', ')}`)
  return names[name]
}

class SketchEdit {
  constructor(doc, sketch) {
    this.doc = doc
    this.sketch = sketch
    this.tree = doc.top(sketch.creator)
    this.names = {}
    this.created = []
  }

  get component() {
    return this.tree.findChild(this.sketch.creator.componentId)
  }

  element(ref) {
    if(ref == 'origin') return this.sketch.origin()
    const id = this.names[ref] || ref
    const elem = [...this.sketch.elements, ...this.sketch.projectedElements()].find(elem => elem.id == id)
    if(!elem) throw new ToolError(`Sketch ${this.sketch.id} has no element '${ref}'`)
    return elem
  }

  // A curve by its id or name, or a point as { element, point }
  item(ref) {
    if(typeof ref == 'string') return this.element(ref)
    if(ref && typeof ref == 'object' && ref.element) {
      const elem = this.element(ref.element)
      return new ElemRef(elem, handleIndex(elem, ref.point ?? 'point'))
    }
    throw new ToolError(`Invalid sketch item ${JSON.stringify(ref)}, use an element id or { element, point }`)
  }

  add(elem, name, construction) {
    if(construction) elem.isReference = true
    this.sketch.add(elem)
    this.created.push(elem)
    if(name) this.names[name] = elem.id
    return elem
  }

  name(name, elements, labels = []) {
    if(!name) return
    elements.forEach((elem, index) => {
      this.names[`${name}.${index}`] = elem.id
      if(labels[index]) this.names[`${name}.${labels[index]}`] = elem.id
    })
  }

  connect(first, firstIndex, second, secondIndex) {
    this.sketch.addConstraint(new CoincidentConstraint(new ElemRef(first, firstIndex), new ElemRef(second, secondIndex)))
  }

  // Like snapping, points placed onto existing points stay attached to them
  snap(elem, indices, excluded = []) {
    const others = [this.sketch.origin(), ...this.sketch.elements, ...this.sketch.projectedElements()]
      .filter(other => toRaw(other) != toRaw(elem) && !excluded.some(item => toRaw(item) == toRaw(other)))
    indices.forEach(index => {
      const position = elem.handles()[index]
      for(const other of others) {
        const handles = other instanceof SketchOrigin ? [other.center()] : other.handles()
        const match = constrainableHandles(other).find(otherIndex => handles[otherIndex]?.distanceTo(position) < 1e-6)
        if(match === undefined) continue
        if(other instanceof SketchOrigin) this.sketch.addConstraint(new CoincidentConstraint(new ElemRef(elem, index), new ElemRef(other)))
        else this.connect(elem, index, other, match)
        return
      }
    })
  }

  point({ at, name, construction }) {
    const elem = this.add(new SketchPoint(point(at, 'at')), name, construction)
    this.snap(elem, [0])
  }

  line({ from, to, name, construction }) {
    const elem = this.add(new Line(point(from, 'from'), point(to, 'to')), name, construction)
    this.snap(elem, [0, 1])
  }

  polyline({ points, closed, name, construction }) {
    if(!Array.isArray(points) || points.length < 2) throw new ToolError('A polyline needs at least two points')
    const corners = points.map((corner, index) => point(corner, `points[${index}]`))
    if(closed) corners.push(corners[0])
    const lines = corners.slice(1).map((corner, index) => this.add(new Line(corners[index].clone(), corner.clone()), null, construction))
    lines.slice(1).forEach((line, index) => this.connect(lines[index], 1, line, 0))
    if(closed) this.connect(lines.at(-1), 1, lines[0], 0)
    lines.forEach((line, index) => this.snap(line, [
      ...(!closed && index == 0 ? [0] : []),
      ...(!closed && index == lines.length - 1 ? [1] : []),
      ...(closed ? [0] : []),
    ], lines))
    this.name(name, lines)
  }

  // Lines and constraints as the rectangle tool creates them
  rectangle({ from, to, center, width, height, name, construction }) {
    let start, end
    if(center) {
      const middle = point(center, 'center')
      const half = new THREE.Vector3(positive(width, 'width') / 2, positive(height, 'height') / 2, 0)
      start = middle.clone().sub(half)
      end = middle.clone().add(half)
    } else {
      start = point(from, 'from')
      end = point(to, 'to')
      if(Math.abs(start.x - end.x) < 1e-9 || Math.abs(start.y - end.y) < 1e-9) throw new ToolError('Rectangle corners must differ in x and y')
    }
    const corners = [start, new THREE.Vector3(end.x, start.y, 0), end, new THREE.Vector3(start.x, end.y, 0)]
    const sides = corners.map((corner, index) => this.add(new Line(corner.clone(), corners[(index + 1) % 4].clone()), null, construction))
    const [bottom, right, top, left] = sides
    ;[[bottom, right], [right, top], [top, left], [left, bottom]].forEach(([first, second]) => {
      this.connect(first, 1, second, 0)
      this.sketch.addConstraint(new PerpendicularConstraint(first, second))
    })
    this.sketch.addConstraint(new HorVertConstraint(bottom))
    this.name(name, sides, ['bottom', 'right', 'top', 'left'])

    if(center) {
      const rising = this.add(new Line(corners[0].clone(), corners[2].clone()), null, true)
      const falling = this.add(new Line(corners[1].clone(), corners[3].clone()), null, true)
      ;[[rising, 0, bottom, 0], [rising, 1, top, 0], [falling, 0, right, 0], [falling, 1, left, 0]]
        .forEach(([diagonal, diagonalIndex, side, sideIndex]) => this.connect(diagonal, diagonalIndex, side, sideIndex))
      const centerPoint = this.add(new SketchPoint(point(center, 'center')), name && `${name}.center`, true)
      this.sketch.addConstraint(new IntersectionConstraint(new ElemRef(centerPoint, 0), rising, falling))
      this.snap(centerPoint, [0], [...sides, rising, falling])
    } else {
      sides.forEach(side => this.snap(side, [0], sides))
    }
  }

  circle({ center, radius, name, construction }) {
    const elem = this.add(new Circle(point(center, 'center'), positive(radius, 'radius')), name, construction)
    this.snap(elem, [0])
  }

  // Counterclockwise from startAngle to endAngle, or through three points
  arc({ center, radius, startAngle, endAngle, start, through, end, name, construction }) {
    let elem
    if(center) {
      if(!Number.isFinite(startAngle) || !Number.isFinite(endAngle)) throw new ToolError('An arc around a center needs startAngle and endAngle')
      const sweep = THREE.MathUtils.euclideanModulo(endAngle - startAngle, 360) || 360
      elem = new Arc(point(center, 'center'), positive(radius, 'radius'), THREE.MathUtils.degToRad(startAngle), THREE.MathUtils.degToRad(sweep))
    } else {
      elem = Arc.fromPoints([point(start, 'start'), point(through, 'through'), point(end, 'end')])
      if(!elem) throw new ToolError('No arc passes through these points')
    }
    this.add(elem, name, construction)
    this.snap(elem, [0, 1, 2])
  }

  spline({ points, name, construction }) {
    if(!Array.isArray(points) || points.length < 2) throw new ToolError('A spline needs at least two control points')
    const elem = this.add(new Spline(points.map((control, index) => point(control, `points[${index}]`))), name, construction)
    this.snap(elem, constrainableHandles(elem))
  }

  constraint({ type, items }) {
    const Type = constraintTypes[type]
    if(!Type) throw new ToolError(`Unknown constraint type '${type}', use one of ${constraintTypeNames.join(', ')}`)
    if(!Array.isArray(items) || !items.length) throw new ToolError('A constraint needs items')
    const constraint = new Type(...items.map(item => this.item(item)))
    if(Type === HorVertConstraint) constraint.isVertical = type == 'vertical'
    this.sketch.addConstraint(constraint)
  }

  dimension({ items, value, orientation, position }) {
    if(!Array.isArray(items) || !items.length || items.length > 2) throw new ToolError('A dimension measures one or two items')
    const resolved = items.map(item => this.item(item))
      .map(item => item instanceof SketchOrigin ? new ElemRef(item) : item)
    const dimension = new Dimension(resolved, position ? point(position, 'position') : defaultDimensionPosition(resolved))
    if(orientation) {
      if(!dimension.measuredPoints()) throw new ToolError('Only distances between points and line lengths can be oriented')
      dimension.orient(orientation)
    }
    if(typeof value == 'string') {
      const expression = new Expression(value, this.component.getParameters(), dimension.isAngular() ? 'angle' : 'length')
      dimension.expression = expression.expression
      dimension.distance = expression.getBase()
    } else if(value !== undefined) {
      dimension.distance = positive(value, 'value')
    }
    this.sketch.addConstraint(dimension)
  }

  delete({ items }) {
    items.forEach(id => {
      const constraint = this.sketch.constraints.find(constraint => constraint.id == id)
      if(constraint) return this.sketch.remove(constraint)
      const elem = this.element(id)
      if(elem.projection) return this.sketch.remove(elem)
      elem.remove()
    })
  }

  construction({ items, construction = true }) {
    items.forEach(id => {
      const elem = this.element(id)
      elem.isReference = construction
      if(elem.projection) elem.projection.isReference = construction
    })
  }

  // Rounds the corner between two curves, given as both curves or as their shared end point
  fillet({ items, radius, name }) {
    radius = positive(radius, 'radius')
    if(!Array.isArray(items) || !items.length || items.length > 2) throw new ToolError('A fillet takes two curves or one corner point')
    let curves = items.map(item => this.item(item))
    if(curves.length == 1) {
      const ref = curves[0]
      if(!(ref instanceof ElemRef) || ref.index === undefined) throw new ToolError('A single fillet item must be a corner point like { element, point: "end" }')
      const curve = ref.curve()
      const corner = curve.handles()[ref.index]
      if(!curve.endpoints().some(end => end.almost(corner))) throw new ToolError('The fillet point must be the end of a curve')
      const neighbors = this.sketch.elements.filter(other => supportsFillet(other) && other != curve && !other.projection &&
        other.isReference == curve.isReference && other.endpoints().some(end => end.almost(corner)))
      if(neighbors.length != 1) throw new ToolError('The corner must join exactly two curves')
      curves = [curve, neighbors[0]]
    }
    if(curves.some(curve => curve instanceof ElemRef || !supportsFillet(curve) || curve.projection)) {
      throw new ToolError('Fillets round lines, arcs and splines of the sketch')
    }
    if(curves[0] == curves[1]) throw new ToolError('Select a different curve for the fillet')
    if(curves[0].isReference != curves[1].isReference) throw new ToolError('Both curves must have the same construction status')
    const fillet = curveFillet(...curves, radius)
    const arc = this.add(new Arc(fillet.center, radius, fillet.start, fillet.sweep), name, curves[0].isReference)
    // As the fillet tool trims the curves to the arc
    curves.forEach((curve, i) => {
      const { curve: replacement, index } = trimToPoint(curve, fillet.ends[i], fillet.parameters[i], fillet.points[i])
      replacement.isReference = curve.isReference
      this.sketch.replaceElement(curve, [replacement])
      this.sketch.addConstraint(new CoincidentConstraint(new ElemRef(replacement, index), new ElemRef(arc, i + 1)))
      this.sketch.addConstraint(new TangentConstraint(replacement instanceof Spline ? new ElemRef(replacement, index) : replacement, arc))
    })
    const direction = fillet.middle.clone().sub(fillet.center).normalize()
    this.sketch.addConstraint(new Dimension([arc], fillet.middle.clone().addScaledVector(direction, 10), radius))
  }

  // Offsets a curve, the chain it connects to, or a profile towards a point, like the offset tool
  offset({ items, through, distance, name }) {
    if(!Array.isArray(items) || items.length != 1) throw new ToolError('An offset takes one curve or profile')
    const position = point(through, 'through')
    const source = String(items[0]).startsWith('profile:') ? resolveItem(this.doc, items[0], this.tree) : this.element(items[0])
    if(!(source instanceof Profile) && !supportsOffset(source)) throw new ToolError('Offsets take lines, circles, arcs, splines and profiles')
    const elements = [...this.sketch.elements, ...this.sketch.projectedElements()]
    const groups = source instanceof Profile ? profileOffsetGroups(source, elements) : [connectedOffsetChain(source, elements)]
    // The point picks the side, an explicit distance replaces its distance
    const side = value => distance === undefined ? value : (Math.sign(value) || 1) * positive(distance, 'distance')
    let constraint, curves
    if(source instanceof Profile || source instanceof Spline || groups.flat().length > 1) {
      const frame = nearestOffsetFrame(groups, position)
      const offset = side(position.clone().sub(frame.point).dot(frame.normal))
      curves = buildOffsetCurves(groups, offset)
      curves.forEach(curve => this.add(curve, null, groups[0][0].curve.isReference))
      constraint = new ProfileOffsetConstraint(groups, curves, position.clone(), offset)
    } else {
      let curve
      if(source instanceof Line) {
        const direction = source.direction().normalize()
        const normal = new THREE.Vector3(-direction.y, direction.x, 0)
        const offset = side(position.clone().sub(source.points[0]).dot(normal))
        if(Math.abs(offset) < 1e-9) throw new ToolError('The offset must not be zero')
        curve = new Line(...source.points.map(point => point.clone().addScaledVector(normal, offset)))
      } else {
        const radius = source.radius + side(position.distanceTo(source.center()) - source.radius)
        if(!(radius > 1e-9) || Math.abs(radius - source.radius) < 1e-9) throw new ToolError('The offset must not be zero or collapse the curve')
        curve = source instanceof Circle ? new Circle(source.center(), radius) :
          Arc.fromPoints([0, 0.5, 1].map(u => source.sample(u).sub(source.center()).multiplyScalar(radius / source.radius).add(source.center())))
      }
      this.add(curve, null, source.isReference)
      curves = [curve]
      constraint = new OffsetConstraint([source, curve], position.clone())
    }
    this.sketch.addConstraint(constraint)
    constraint.active = true
    this.name(name, curves)
  }

  // Outlined text, its font loaded by prepareSketchOperations
  text({ at, content, size = 10, explode, name, construction, font }) {
    if(typeof content != 'string' || !content.trim()) throw new ToolError('Text needs content')
    const elem = this.add(new SketchText(point(at, 'at'), content, positive(size, 'size'), font.pieces, font.family, font.style), name, construction)
    this.snap(elem, [0])
    if(explode) {
      const curves = this.sketch.materializeText(elem)
      this.created.splice(this.created.indexOf(elem), 1, ...curves)
      this.name(name, curves)
    }
  }

  project({ items, intersect }) {
    items.forEach(id => {
      const projection = intersect ? new Intersection(resolveItem(this.doc, id, this.tree)) : new Projection(resolveItem(this.doc, id, this.tree))
      projection.sketch = this.sketch
      if(!projection.update(this.tree)) throw new ToolError(`${id}: ${projection.error}`)
      this.sketch.addProjection(projection)
      this.created.push(...projection.geometries())
    })
  }
}

function itemPoint(item) {
  if(item instanceof ElemRef) return item.index === undefined ? item.curve().center() : item.curve().handles()[item.index]
  return item.center()
}

function defaultDimensionPosition(items) {
  if(items.length == 1 && items[0] instanceof Line) {
    const line = items[0]
    const direction = line.direction().normalize()
    return line.midpoint().addScaledVector(new THREE.Vector3(-direction.y, direction.x, 0), Math.max(line.length() * 0.15, 2))
  }
  if(items.length == 1 && (items[0] instanceof Circle || items[0] instanceof Arc)) {
    return items[0].center().add(new THREE.Vector3(items[0].radius, items[0].radius, 0).multiplyScalar(0.7))
  }
  const center = items.map(itemPoint).reduce((sum, p) => sum.add(p), new THREE.Vector3()).divideScalar(items.length)
  return center.add(new THREE.Vector3(0, 3, 0))
}

export const operationTypes = [
  'point', 'line', 'polyline', 'rectangle', 'circle', 'arc', 'spline', 'text', 'fillet', 'offset',
  'constraint', 'dimension', 'delete', 'construction', 'project',
]

// Text outlines come from system fonts, which load asynchronously
export async function prepareSketchOperations(operations) {
  if(!operations.some(operation => operation?.op == 'text')) return operations
  const fonts = await listSystemFonts()
  if(!fonts.length) throw new ToolError('No system fonts are available for text')
  return Promise.all(operations.map(async operation => {
    if(operation?.op != 'text') return operation
    const data = operation.fontFamily ?
      findFont(fonts, operation.fontFamily, operation.fontStyle || 'Regular') :
      preferredFont(fonts)
    if(!data) {
      const families = [...new Set(fonts.map(font => font.family))]
      throw new ToolError(`No font ${operation.fontFamily} ${operation.fontStyle || 'Regular'}. Families include ${families.slice(0, 40).join(', ')}.`)
    }
    const font = await loadSystemFont(data)
    return { ...operation, font: { family: data.family, style: data.style, pieces: textPieces(font, String(operation.content ?? '')) } }
  }))
}

// Applies all operations and solves the sketch. A failing operation or an
// unsolvable result restores the sketch as it was, using a history of its own.
export function editSketch(doc, sketch, operations) {
  const edit = new SketchEdit(doc, sketch)
  const history = sketch.history
  sketch.startHistory()
  try {
    operations.forEach((operation, index) => {
      if(!operationTypes.includes(operation?.op)) {
        throw new ToolError(`Operation ${index}: unknown op '${operation?.op}', use one of ${operationTypes.join(', ')}`)
      }
      try {
        edit[operation.op](operation)
      } catch(error) {
        throw new ToolError(`Operation ${index} (${operation.op}): ${error.message}`)
      }
    })
    if(sketch.solve(edit.tree) === false) {
      throw new ToolError('The sketch could not be solved with these changes. It is probably over-constrained or the constraints contradict each other.')
    }
  } catch(error) {
    sketch.recordHistory()
    if(sketch.restoreHistory(-1, edit.tree)) sketch.restoringHistory = false
    throw new ToolError(`${error.message} The sketch was left unchanged.`)
  } finally {
    sketch.history = history
  }
  sketch.profileUpdateNeeded = true
  return {
    names: edit.names,
    created: edit.created.map(elem => elem.id),
  }
}
