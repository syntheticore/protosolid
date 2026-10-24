import * as THREE from 'three'
import Serialize from './serialize.js'
import { makeID } from './id.js'
import { AxialReference, CurveReference, PointReference } from './references.js'
import { EPSILON, arrayRange, rotationFromNormal } from './utils.js'
import { shapex as sx, point2, modelTolerance, sketchTessellationOptions } from './shapex.js'
import { isModelingFailure } from './errors.js'

const vector = point => new THREE.Vector3(point.x, point.y, 0)

export class SketchElement {
  constructor() {
    this.id = makeID()
    this.geom = null
    this.fullyConstrained = false
  }

  typename(base) { return base + (this.projection ? ' Projection' : '') }

  clear() { this.geom = null }

  update(geom) {
    this.clear()
    geom = geom || this.geometry()
    this.geom = geom ? () => geom : null // Hide geometry from Vue
  }

  remove() {
    this.sketch.remove(this)
    this.clear()
  }

  isClosed() {
    return false
  }

  sample(u) {
    if(!this.geom) return
    const curve = this.geom(), range = curve.range()
    const point = curve.curve().evaluate(range.start + u * range.span()).point
    return new THREE.Vector3(point.x, point.y, 0)
  }

  unsample(point) { return this.geom().closestPoint(point2(point), modelTolerance).parameter }

  distanceTo(point) { return this.geom().closestPoint(point2(point), modelTolerance).distance }

  endpoints() {
    return [this.sample(0.0), this.sample(1.0)]
  }

  otherBound(bound) {
    const points = this.endpoints()
    return bound.almost(points[0]) ? points[1] : points[0]
  }

  commonHandle(other) {
    if(!other.handles) return
    return this.handles().find(p => other.handles().some(op => p.almost(op) ) )
  }

  length() { return this.geom ? this.geom().length(modelTolerance) : 0 }


  tesselate() {
    if(!this.geom) return
    return this.geom().tessellate(sketchTessellationOptions).points.map(point => [point.x, point.y, 0])
  }

  pathReference() {
    return new CurveReference(this)
  }

  // Polyline in component space
  getPath() {
    return this.tesselate().map(([x, y]) => new THREE.Vector3(x, y, 0).applyMatrix4(this.sketch.workplane))
  }

  intersect(others) {
    if(!this.geom) return []
    return others.filter(other => other != this && other.geom).flatMap(other =>
      this.geom().intersections(other.geom(), modelTolerance).filter(hit => hit.kind == 'point').map(hit => hit.point))
  }

  splitIntersections(others) {
    return [...this.intersect(others), ...others.filter(other => other != this && other.geom)
      .flatMap(other => other.endpoints().filter(point => this.distanceTo(point) <= EPSILON))]
  }

  split(others) {
    if(!this.geom) return [this]
    const curve = this.geom(), range = curve.range()
    const parameters = this.splitIntersections(others).map(point => this.unsample(point))
    if(this instanceof Circle) {
      const period = Math.abs(range.span()), direction = Math.sign(range.span())
      const progress = parameters.map(u => ((direction * (u - range.start) % period) + period) % period)
        .map(u => u <= EPSILON || period - u <= EPSILON ? 0 : u)
        .sort((a, b) => a - b).filter((u, i, values) => !i || Math.abs(u - values[i - 1]) > EPSILON)
      if(progress.length < 2) return [this]
      return progress.map((u, i) => {
        const end = i + 1 < progress.length ? progress[i + 1] : progress[0] + period
        const piece = sx.TrimmedCurve2.new(curve.curve(), sx.ParameterRange.new(range.start + direction * u, range.start + direction * end))
        return Arc.fromGeometry(piece, this.id + '/' + i)
      })
    }
    const cutters = others.filter(other => other != this)
    const cuts = parameters.filter(u => Math.abs(u - range.start) > EPSILON && Math.abs(u - range.end) > EPSILON)
    if(!cuts.length) return [this]
    return curve.splitAt([cuts[0]], modelTolerance).flatMap((piece, i) =>
      this.constructor.fromGeometry(piece, this.id + '/' + (i + 1)).split(cutters))
  }

  constraints() {
    return this.sketch.constraints.filter(constraint => constraint.items.some(item => item.curve() == this ) )
  }

  clone() {
    const clone = this._clone()
    clone.id = this.id
    clone.sketch = this.sketch
    clone.isReference = this.isReference
    return clone
  }

}


export class SketchPoint extends SketchElement {
  typename() { return 'Point' }

  constructor(point) {
    super()
    this.point = point
  }

  center() { return this.point }
  endpoints() { return [this.point] }
  snapPoints() { return [this.point] }
  handles() { return [this.point] }
  setHandles(handles) { this.point = handles[0] }
  _clone() { return new SketchPoint(this.point.clone()) }

  pointReference() {
    return new PointReference(new CurveReference(this))
  }

  getPoint() {
    return new THREE.Matrix4().setPosition(this.point.clone().applyMatrix4(this.sketch.workplane))
  }

  dump() {
    return {
      id: this.id,
      point: this.point,
      isReference: this.isReference,
    }
  }

  static undump(dump) {
    const point = new SketchPoint(dump.point)
    point.id = dump.id
    point.isReference = dump.isReference || false
    return point
  }
}
Serialize.register(SketchPoint, 'SketchPoint')


export class Line extends SketchElement {
  typename() { return super.typename('Line') }

  constructor(p1, p2, geom) {
    super()
    this.points = [p1, p2]
    this.update(geom)
  }

  static fromGeometry(geom, id) {
    const line = new Line(vector(geom.start()), vector(geom.end()), geom)
    line.id = id
    return line
  }

  snapPoints() {
    return [...this.points, this.midpoint()]
  }

  endpoints() {
    return this.points
  }

  midpoint() {
    return this.points[0].clone().add(this.points[1]).divideScalar(2.0)
  }

  center() { return this.midpoint() }

  handles() {
    return this.points
  }

  setHandles(handles) {
    this.points = handles
    this.update()
  }

  geometry() {
    const [start, end] = this.points
    return start.almost(end) ? null : sx.TrimmedCurve2.line(point2(start), point2(end))
  }

  flip() {
    this.points = [this.points[1], this.points[0]]
    this.update()
  }


  direction() {
    return this.points[1].clone().sub(this.points[0])
  }

  // Mirror image of a point across the infinite extension of this line
  reflect(point) {
    const direction = this.direction().normalize()
    const offset = point.clone().sub(this.points[0])
    return this.points[0].clone().add(direction.multiplyScalar(2 * offset.dot(direction))).sub(offset)
  }

  axialReference() {
    return new AxialReference(new CurveReference(this))
  }

  getAxis() {
    return rotationFromNormal(this.direction().normalize()).setPosition(this.points[0]).premultiply(this.sketch.workplane)
  }

  _clone() {
    return new Line(...this.points)
  }

  dump() {
    return {
      id: this.id,
      points: this.points,
      isReference: this.isReference,
    }
  }

  static undump(dump) {
    const line = new Line(...dump.points)
    line.id = dump.id
    line.isReference = dump.isReference || false
    return line
  }
}

Serialize.register(Line, 'Line')


export class Circle extends SketchElement {
  typename() { return super.typename('Circle') }

  constructor(center, radius, geom) {
    super()
    this._center = center
    this.radius = radius
    this.update(geom)
  }

  static fromGeometry(geom, id) {
    const basis = geom.curve()
    const circle = new Circle(vector(basis.center()), basis.radius(), geom)
    circle.id = id
    return circle
  }

  snapPoints() {
    return [this._center]
  }

  center() { return this._center.clone() }

  handles() {
    return [this._center]
  }

  setHandles(handles) {
    this._center = handles[0]
    if(handles[1]) this.radius = handles[0].distanceTo(handles[1])
    this.update()
  }

  geometry() {
    if(this.radius <= EPSILON) return null
    return sx.TrimmedCurve2.circle(point2(this._center), this.radius).reversed()
  }

  isClosed() {
    return true
  }


  area() {
    return Math.PI * Math.pow(this.radius, 2.0)
  }

  axialReference() {
    return new AxialReference(new CurveReference(this))
  }

  getAxis() {
    return new THREE.Matrix4().setPosition(this._center).premultiply(this.sketch.workplane)
  }

  _clone() {
    return new Circle(this._center, this.radius)
  }

  dump() {
    return {
      id: this.id,
      _center: this._center,
      radius: this.radius,
      isReference: this.isReference,
    }
  }

  static undump(dump) {
    const circle = new Circle(dump._center, dump.radius)
    circle.id = dump.id
    circle.isReference = dump.isReference || false
    return circle
  }
}

Serialize.register(Circle, 'Circle')


// An arc starts at the Cartesian angle `start` and turns by the signed
// `sweep`, matching Circle2's counter-clockwise parameter. The sign of the
// sweep is the traversal direction.
export class Arc extends SketchElement {
  typename() { return super.typename('Arc') }

  constructor(center, radius, start, sweep, geom) {
    super()
    this._center = center
    this.radius = radius
    this.start = start
    this.sweep = sweep
    this.update(geom)
  }

  static fromPoints(points) {
    try { return Arc.fromGeometry(sx.TrimmedCurve2.arcThrough(...points.map(point2))) }
    catch(error) {
      if(isModelingFailure(error)) return
      throw error
    }
  }

  static fromGeometry(geom, id) {
    const basis = geom.curve(), range = geom.range()
    const arc = new Arc(vector(basis.center()), basis.radius(), range.start, range.span(), geom)
    if(id) arc.id = id
    return arc
  }


  center() { return this._center.clone() }

  snapPoints() {
    return [this._center, ...this.endpoints()]
  }

  handles() {
    return [this._center, ...this.endpoints()]
  }

  setHandles(handles) {
    return this.setSolvedHandles(handles, this.radius)
  }

  setSolvedHandles(handles, radius) {
    radius = Math.abs(radius)
    if(!Number.isFinite(radius) || radius <= EPSILON) return false
    if(handles.some(point => !point || ![point.x, point.y, point.z].every(Number.isFinite))) return false
    const center = handles[0]
    const [first, last] = handles.slice(1).map(point => Math.atan2(point.y - center.y, point.x - center.x))
    // Keep the traversal direction; the solver only moves the endpoints.
    const direction = Math.sign(this.sweep)
    const span = THREE.MathUtils.euclideanModulo(direction * (last - first), Math.PI * 2)
    if(Math.min(span, Math.PI * 2 - span) * radius <= EPSILON) return false
    this._center = center
    this.radius = radius
    this.start = first
    this.sweep = direction * span
    this.update()
    return true
  }

  setPoints(points) {
    const arc = Arc.fromPoints(points)
    if(!arc) return false
    this.clear()
    this._center = arc._center
    this.radius = arc.radius
    this.start = arc.start
    this.sweep = arc.sweep
    this.geom = arc.geom
    arc.geom = null
    return true
  }

  geometry() {
    return sx.TrimmedCurve2.new(sx.Circle2.new(point2(this._center), this.radius),
      sx.ParameterRange.new(this.start, this.start + this.sweep))
  }

  isClosed() {
    return false
  }


  flip() {
    this.start += this.sweep
    this.sweep = -this.sweep
    this.update()
  }


  _clone() {
    return new Arc(this._center, this.radius, this.start, this.sweep)
  }

  dump() {
    return {
      id: this.id,
      _center: this._center,
      radius: this.radius,
      start: this.start,
      sweep: this.sweep,
      isReference: this.isReference,
    }
  }

  static undump(dump) {
    const arc = new Arc(dump._center, dump.radius, dump.start, dump.sweep)
    arc.id = dump.id
    arc.isReference = dump.isReference || false
    return arc
  }
}
Serialize.register(Arc, 'Arc')


// Keep snapping and solver point-on-curve support on the same boundary.
export function supportsPointOnCurveConstraint(elem) {
  return elem instanceof Line || elem instanceof Circle || elem instanceof Arc || elem instanceof Spline ||
    (elem instanceof DerivedCurve && !!elem.geom && !elem.invalid)
}


// A clamped uniform B-spline through its control points. Split and trimmed
// pieces keep the whole control polygon and restrict it to `trim`, a
// parameter range whose direction is the traversal direction.
export class Spline extends SketchElement {
  typename() { return super.typename('Spline') }

  constructor(points, trim = null, geom) {
    super()
    this.points = points
    this.trim = trim
    this.update(geom)
  }

  static fromGeometry(geom, id) {
    const points = geom.curve().controlPoints().map(vector)
    const range = geom.range()
    const spline = new Spline(points, [range.start, range.end], geom)
    spline.id = id
    return spline
  }

  static clampedKnots(n, degree) {
    if(degree >= n) { return [] }
    const d = degree + 1
    const uniform = arrayRange(1, n - d)
    const last = n - d + 1
    return [
      [0.0, ...uniform, last],
      [d, ...Array(uniform.length).fill(1), d],
    ]
  }

  endpoints() {
    return this.trim ? [this.sample(0), this.sample(1)] : [this.points[0], this.points.at(-1)]
  }

  snapPoints() {
    return this.endpoints()
  }

  handles() {
    return [...this.points]
  }

  setHandles(handles) {
    this.points = handles
    this.update()
  }

  // Inserting or removing a control point changes the knot vector, so a
  // previous trim no longer describes the same piece.
  setControlPoints(points) {
    this.trim = null
    this.setHandles(points)
  }

  // Clamped uniform knots span [0, n - degree].
  domain() { return [0, this.points.length - Math.min(5, this.points.length - 1)] }

  center() { return this.sample(0.5) }

  geometry() {
    // The first drawing click has only coincident preview control points.
    if(this.points.every(point => point.almost(this.points[0]))) return null
    const degree = Math.min(5, this.points.length - 1)
    const [values, multiplicities] = Spline.clampedKnots(this.points.length, degree)
    const knots = values.flatMap((value, i) => Array(multiplicities[i]).fill(value))
    const basis = sx.NurbsCurve2.new(degree, this.points.map(point2), this.points.map(() => 1), knots)
    return sx.TrimmedCurve2.new(basis, sx.ParameterRange.new(...(this.trim || this.domain())))
  }

  flip() {
    this.trim = (this.trim || this.domain()).toReversed()
    this.update()
  }

  _clone() { return new Spline([...this.points], this.trim) }

  dump() {
    return {
      id: this.id,
      points: this.points,
      trim: this.trim,
      isReference: this.isReference,
    }
  }

  static undump(dump) {
    const line = new Spline(dump.points, dump.trim)
    line.id = dump.id
    line.isReference = dump.isReference || false
    return line
  }
}

Serialize.register(Spline, 'Spline')

// Exact derived geometry regenerated by an offset constraint or projection.
// Its source owns edits; the curve has no independent control points.
export class DerivedCurve extends SketchElement {
  constructor(geom) { super(); if(geom) this.update(geom) }

  typename() { return super.typename('Curve') }
  handles() { return this.geom && !this.invalid ? this.endpoints() : [] }
  snapPoints() { return this.handles() }
  closestFrame(point) {
    const curve = this.geom(), closest = curve.closestPoint(point2(point), modelTolerance)
    const derivative = curve.evaluate(closest.parameter).firstDerivative
    return { point: vector(closest.point), tangent: vector(derivative).multiplyScalar(Math.sign(curve.range().span())).normalize() }
  }
  center() { return this.sample(0.5) }
  isClosed() { return !this.invalid && !!this.geom && this.endpoints()[0].almost(this.endpoints()[1]) }
  length() { return this.invalid ? 0 : super.length() }


  geometry() { return null }


  // Reverse traversal without mutating the shared immutable basis.
  flip() { this.update(this.geom().reversed()) }

  _clone() { return new this.constructor(this.geom()) }

  static fromGeometry(geom, id) {
    const curve = new this(geom)
    curve.id = id || curve.id
    return curve
  }

  dump() { return { id: this.id, isReference: this.isReference } }

  static undump(dump) {
    return Object.assign(new this(), dump)
  }
}
export class OffsetCurve extends DerivedCurve {
  typename() { return 'Offset Curve' }
}
Serialize.register(OffsetCurve, 'OffsetCurve')
