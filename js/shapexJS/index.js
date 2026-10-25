const DEFAULT_TOLERANCE = Object.freeze({
  linear: 1.0e-7,
  relative: 1.0e-12,
  angular: 1.0e-10,
  parameter: 1.0e-12,
})
const WIRE_PROPERTIES = new WeakMap()
const CURVE_MODULES = new WeakMap()
const NATIVE_CURVES = new WeakMap()
const VALUE_MODULES = new WeakMap()

function curveModule(value) {
  const bindings = VALUE_MODULES.get(value) || CURVE_MODULES.get(value.constructor)
  if(!bindings) throw new ShapexError('Curve queries require values from an initialized ShapexJS instance')
  return bindings
}

function curveEvaluation(values, Point = Point2, Vector = Vec2, dimensions = 2) {
  values = Array.from(values)
  return Object.freeze({
    point: new Point(...values.slice(0, dimensions)),
    firstDerivative: new Vector(...values.slice(dimensions, dimensions * 2)),
    secondDerivative: new Vector(...values.slice(dimensions * 2, dimensions * 3)),
  })
}

function nativeCurve(value) {
  if(!NATIVE_CURVES.has(value)) NATIVE_CURVES.set(value, nativeCall(() => new (curveModule(value).NativeCurve2)(value._dto())))
  return NATIVE_CURVES.get(value)
}

function finite(value, name) {
  if(typeof value != 'number' || !Number.isFinite(value)) {
    throw new TypeError(`${name} must be a finite number`)
  }
  return value
}

function positive(value, name) {
  finite(value, name)
  if(value <= 0) throw new RangeError(`${name} must be greater than zero`)
  return value
}

function instance(value, Type, name) {
  if(!(value instanceof Type)) throw new TypeError(`${name} must be a ${Type.name}`)
  return value
}

function oneOf(value, Types, name) {
  if(!Types.some(Type => value instanceof Type)) {
    throw new TypeError(`${name} must be one of ${Types.map(Type => Type.name).join(', ')}`)
  }
  return value
}

function array(value, name) {
  if(value == null || typeof value[Symbol.iterator] != 'function') {
    throw new TypeError(`${name} must be iterable`)
  }
  return Array.from(value)
}

function free(raw) { raw?.free?.() }

function nativeCall(callback) {
  try {
    return callback()
  } catch(error) {
    if(error instanceof ShapexError) throw error
    throw new ShapexError(error?.message || String(error), { cause: error, diagnostics: error?.diagnostics })
  }
}

function triples(values, Type) {
  values = Array.from(values)
  const result = []
  for(let index = 0; index < values.length; index += 3) {
    result.push(new Type(values[index], values[index + 1], values[index + 2]))
  }
  return result
}

export class ShapexError extends Error {
  constructor(message, options) {
    super(message, options)
    this.name = 'ShapexError'
    this.diagnostics = Object.freeze(options?.diagnostics || [])
  }
}

export class Point2 {
  constructor(x = 0, y = 0) {
    this.x = finite(x, 'x')
    this.y = finite(y, 'y')
    Object.freeze(this)
  }

  static new(x, y) { return new this(x, y) }
  toArray() { return [this.x, this.y] }
}

export class Vec2 {
  constructor(x = 0, y = 0) {
    this.x = finite(x, 'x')
    this.y = finite(y, 'y')
    Object.freeze(this)
  }

  static new(x, y) { return new this(x, y) }
  magnitude() { return Math.hypot(this.x, this.y) }
  toArray() { return [this.x, this.y] }
}

export class Point3 {
  constructor(x = 0, y = 0, z = 0) {
    this.x = finite(x, 'x')
    this.y = finite(y, 'y')
    this.z = finite(z, 'z')
    Object.freeze(this)
  }

  static new(x, y, z) { return new this(x, y, z) }
  toArray() { return [this.x, this.y, this.z] }
}

export class Vec3 {
  constructor(x = 0, y = 0, z = 0) {
    this.x = finite(x, 'x')
    this.y = finite(y, 'y')
    this.z = finite(z, 'z')
    Object.freeze(this)
  }

  static new(x, y, z) { return new this(x, y, z) }
  magnitude() { return Math.hypot(this.x, this.y, this.z) }
  dot(other) { instance(other, Vec3, 'other'); return this.x * other.x + this.y * other.y + this.z * other.z }
  cross(other) {
    instance(other, Vec3, 'other')
    return new Vec3(
      this.y * other.z - this.z * other.y,
      this.z * other.x - this.x * other.z,
      this.x * other.y - this.y * other.x,
    )
  }
  normalized() {
    const magnitude = this.magnitude()
    if(magnitude == 0) throw new RangeError('cannot normalize a zero vector')
    return new Vec3(this.x / magnitude, this.y / magnitude, this.z / magnitude)
  }
  scaled(scale) { finite(scale, 'scale'); return new Vec3(this.x * scale, this.y * scale, this.z * scale) }
  toArray() { return [this.x, this.y, this.z] }
}

export { Vec3 as Vector3 }

export class Matrix3 {
  constructor(x, y, z) {
    this.x = instance(x, Vec3, 'x')
    this.y = instance(y, Vec3, 'y')
    this.z = instance(z, Vec3, 'z')
    Object.freeze(this)
  }

  static fromCols(x, y, z) { return new this(x, y, z) }
  static identity() { return new this(new Vec3(1, 0, 0), new Vec3(0, 1, 0), new Vec3(0, 0, 1)) }
}

export class Quaternion {
  constructor(s, x, y, z) {
    this.s = finite(s, 's')
    this.v = new Vec3(x, y, z)
    Object.freeze(this)
  }

  static new(s, x, y, z) { return new this(s, x, y, z) }
}

export class ModelTolerance {
  constructor(
    linear = DEFAULT_TOLERANCE.linear,
    relative = DEFAULT_TOLERANCE.relative,
    angular = DEFAULT_TOLERANCE.angular,
    parameter = DEFAULT_TOLERANCE.parameter,
  ) {
    this.linear = positive(linear, 'linear')
    this.relative = positive(relative, 'relative')
    this.angular = positive(angular, 'angular')
    this.parameter = positive(parameter, 'parameter')
    if(this.angular >= Math.PI) throw new RangeError('angular tolerance must be smaller than pi')
    Object.freeze(this)
  }

  static new(...values) { return new this(...values) }
  static default() { return new this() }
  linearAtScale(scale) { return Math.max(this.linear, this.relative * Math.abs(finite(scale, 'scale'))) }
  toArray() { return [this.linear, this.relative, this.angular, this.parameter] }
}

export class ParameterRange {
  constructor(start, end) {
    this.start = finite(start, 'start')
    this.end = finite(end, 'end')
    if(this.start == this.end) throw new RangeError('a parameter range needs distinct bounds')
    Object.freeze(this)
  }

  static new(start, end) { return new this(start, end) }
  span() { return this.end - this.start }
  reversed() { return new ParameterRange(this.end, this.start) }
  contains(parameter, tolerance) {
    instance(tolerance, ModelTolerance, 'tolerance')
    const low = Math.min(this.start, this.end) - tolerance.parameter
    const high = Math.max(this.start, this.end) + tolerance.parameter
    return low <= parameter && parameter <= high
  }
}

export class Axis3 {
  constructor(origin, direction) {
    this._origin = instance(origin, Point3, 'origin')
    this._direction = instance(direction, Vec3, 'direction').normalized()
    Object.freeze(this)
  }

  static new(origin, direction) { return new this(origin, direction) }
  static through(start, end) {
    instance(start, Point3, 'start'); instance(end, Point3, 'end')
    return new this(start, new Vec3(end.x - start.x, end.y - start.y, end.z - start.z))
  }
  origin() { return this._origin }
  direction() { return this._direction }
  pointAt(distance) {
    finite(distance, 'distance')
    return new Point3(
      this._origin.x + this._direction.x * distance,
      this._origin.y + this._direction.y * distance,
      this._origin.z + this._direction.z * distance,
    )
  }
  _values() { return [...this._origin.toArray(), ...this._direction.toArray()] }
}

export class Plane3 {
  constructor(origin, u, v, tolerance = ModelTolerance.default()) {
    this._origin = instance(origin, Point3, 'origin')
    this._u = instance(u, Vec3, 'u')
    this._v = instance(v, Vec3, 'v')
    this.tolerance = instance(tolerance, ModelTolerance, 'tolerance')
    const cross = this._u.cross(this._v)
    const scale = Math.max(this._u.magnitude(), this._v.magnitude())
    if(cross.magnitude() <= tolerance.linearAtScale(scale) ** 2) throw new RangeError('plane axes must be linearly independent')
    Object.freeze(this)
  }

  static new(origin, u, v, tolerance) { return new this(origin, u, v, tolerance) }
  static xy() { return new this(new Point3(), new Vec3(1, 0, 0), new Vec3(0, 1, 0)) }
  static xz() { return new this(new Point3(), new Vec3(1, 0, 0), new Vec3(0, 0, 1)) }
  static yz() { return new this(new Point3(), new Vec3(0, 1, 0), new Vec3(0, 0, 1)) }
  static fromOriginNormal(origin, normal) {
    instance(origin, Point3, 'origin'); normal = instance(normal, Vec3, 'normal').normalized()
    const reference = Math.abs(normal.x) <= Math.abs(normal.y) && Math.abs(normal.x) <= Math.abs(normal.z)
      ? new Vec3(1, 0, 0)
      : Math.abs(normal.y) <= Math.abs(normal.z) ? new Vec3(0, 1, 0) : new Vec3(0, 0, 1)
    const u = reference.cross(normal).normalized()
    return new this(origin, u, normal.cross(u).normalized())
  }
  static fromFrame(frame) {
    instance(frame, Frame3, 'frame')
    return new this(frame.origin(), frame.x(), frame.y())
  }
  origin() { return this._origin }
  u() { return this._u }
  v() { return this._v }
  normal() { return this._u.cross(this._v).normalized() }
  pointAt(point) {
    const vector = this.vectorAt(point)
    return new Point3(this._origin.x + vector.x, this._origin.y + vector.y, this._origin.z + vector.z)
  }
  vectorAt(vector) { return new Vec3(this._u.x * vector.x + this._v.x * vector.y, this._u.y * vector.x + this._v.y * vector.y, this._u.z * vector.x + this._v.z * vector.y) }
  coordinates(point) {
    const delta = new Vec3(point.x - this._origin.x, point.y - this._origin.y, point.z - this._origin.z)
    const uu = this._u.dot(this._u), vv = this._v.dot(this._v), uv = this._u.dot(this._v), determinant = uu * vv - uv * uv
    return new Point2((delta.dot(this._u) * vv - delta.dot(this._v) * uv) / determinant,
      (delta.dot(this._v) * uu - delta.dot(this._u) * uv) / determinant)
  }
  liftCurve(curve, tolerance = ModelTolerance.default()) { return curve.lifted(this, tolerance) }
  expressCurve(curve, tolerance = ModelTolerance.default()) { return curve.expressedIn(this, tolerance) }
  projectCurve(curve, tolerance = ModelTolerance.default()) { return curve.projectedOnto(this, tolerance) }
  _values() { return [...this._origin.toArray(), ...this._u.toArray(), ...this._v.toArray()] }
}

export class Frame3 {
  constructor(origin, x, y, tolerance = ModelTolerance.default()) {
    this._origin = instance(origin, Point3, 'origin')
    x = instance(x, Vec3, 'x')
    y = instance(y, Vec3, 'y')
    if(x.magnitude() == 0 || y.magnitude() == 0 || Math.abs(x.dot(y)) > tolerance.angular * x.magnitude() * y.magnitude()) {
      throw new RangeError('frame axes must be nonzero and perpendicular')
    }
    this._x = x.normalized()
    this._y = y.normalized()
    this._z = this._x.cross(this._y).normalized()
    Object.freeze(this)
  }
  static new(origin, x, y, tolerance) { return new this(origin, x, y, tolerance) }
  static fromZ(origin, z) {
    const plane = Plane3.fromOriginNormal(origin, z)
    return new this(origin, plane.u(), plane.v())
  }
  origin() { return this._origin }
  x() { return this._x }
  y() { return this._y }
  z() { return this._z }
  axis() { return Axis3.new(this._origin, this._z) }
}

function applyMatrix(matrix, vector) {
  return new Vec3(
    matrix.x.x * vector.x + matrix.y.x * vector.y + matrix.z.x * vector.z,
    matrix.x.y * vector.x + matrix.y.y * vector.y + matrix.z.y * vector.z,
    matrix.x.z * vector.x + matrix.y.z * vector.y + matrix.z.z * vector.z,
  )
}

function multiplyMatrices(left, right) {
  return Matrix3.fromCols(
    applyMatrix(left, right.x),
    applyMatrix(left, right.y),
    applyMatrix(left, right.z),
  )
}

export class RigidTransform3 {
  constructor(rotation, translation, tolerance = ModelTolerance.default()) {
    this._rotation = instance(rotation, Matrix3, 'rotation')
    this._translation = instance(translation, Vec3, 'translation')
    this.tolerance = instance(tolerance, ModelTolerance, 'tolerance')
    const columns = [this._rotation.x, this._rotation.y, this._rotation.z]
    const orthonormal = columns.every(column => Math.abs(column.magnitude() - 1) <= tolerance.angular)
      && Math.abs(columns[0].dot(columns[1])) <= tolerance.angular
      && Math.abs(columns[0].dot(columns[2])) <= tolerance.angular
      && Math.abs(columns[1].dot(columns[2])) <= tolerance.angular
    const determinant = columns[0].dot(columns[1].cross(columns[2]))
    if(!orthonormal || Math.abs(determinant - 1) > tolerance.angular) {
      throw new RangeError('rotation must be right-handed and orthonormal')
    }
    Object.freeze(this)
  }

  static new(rotation, translation, tolerance) { return new this(rotation, translation, tolerance) }
  static identity() { return new this(Matrix3.identity(), new Vec3()) }
  static fromQuaternion(quaternion, translation, tolerance = ModelTolerance.default()) {
    instance(quaternion, Quaternion, 'quaternion')
    const { s: w, v: { x, y, z } } = quaternion
    const length = Math.hypot(w, x, y, z)
    if(length == 0) throw new RangeError('transform quaternion must be nonzero')
    const [qw, qx, qy, qz] = [w / length, x / length, y / length, z / length]
    return new this(Matrix3.fromCols(
      new Vec3(1 - 2 * (qy * qy + qz * qz), 2 * (qx * qy + qz * qw), 2 * (qx * qz - qy * qw)),
      new Vec3(2 * (qx * qy - qz * qw), 1 - 2 * (qx * qx + qz * qz), 2 * (qy * qz + qx * qw)),
      new Vec3(2 * (qx * qz + qy * qw), 2 * (qy * qz - qx * qw), 1 - 2 * (qx * qx + qy * qy)),
    ), translation, tolerance)
  }
  rotation() { return this._rotation }
  translation() { return this._translation }
  transformPoint(point) {
    instance(point, Point3, 'point')
    const value = applyMatrix(this._rotation, new Vec3(point.x, point.y, point.z))
    return new Point3(value.x + this._translation.x, value.y + this._translation.y, value.z + this._translation.z)
  }
  transformVector(vector) { return applyMatrix(this._rotation, instance(vector, Vec3, 'vector')) }
  inverse() {
    const inverseRotation = Matrix3.fromCols(
      new Vec3(this._rotation.x.x, this._rotation.y.x, this._rotation.z.x),
      new Vec3(this._rotation.x.y, this._rotation.y.y, this._rotation.z.y),
      new Vec3(this._rotation.x.z, this._rotation.y.z, this._rotation.z.z),
    )
    return new RigidTransform3(inverseRotation, applyMatrix(inverseRotation, this._translation).scaled(-1), this.tolerance)
  }
  then(next) {
    instance(next, RigidTransform3, 'next')
    const translated = applyMatrix(next._rotation, this._translation)
    return new RigidTransform3(
      multiplyMatrices(next._rotation, this._rotation),
      new Vec3(translated.x + next._translation.x, translated.y + next._translation.y, translated.z + next._translation.z),
      this.tolerance,
    )
  }
  _values() {
    return [
      ...this._rotation.x.toArray(), ...this._rotation.y.toArray(), ...this._rotation.z.toArray(),
      ...this._translation.toArray(),
    ]
  }
}

export class ParameterDomain {
  constructor(bounds = null, period = null) { this.bounds = bounds; this.period = period; Object.freeze(this) }
  contains(parameter, tolerance = ModelTolerance.default()) {
    return Number.isFinite(parameter) && (this.period != null || !this.bounds || this.bounds.contains(parameter, tolerance))
  }
  normalize(parameter) {
    if(this.period == null) return parameter
    const start = this.bounds?.start || 0
    return start + ((parameter - start) % this.period + this.period) % this.period
  }
  nearest(parameter, reference) { return this.period == null ? parameter : parameter + Math.round((reference - parameter) / this.period) * this.period }
}

export class RigidTransform2 {
  constructor(angle = 0, translation = new Vec2(0, 0)) {
    this._angle = finite(angle, 'angle'); this._translation = instance(translation, Vec2, 'translation'); Object.freeze(this)
  }
  static new(angle, translation) { return new this(angle, translation) }
  static identity() { return new this() }
  angle() { return this._angle }
  translation() { return this._translation }
  transformVector(vector) {
    const c = Math.cos(this._angle), s = Math.sin(this._angle)
    return new Vec2(c * vector.x - s * vector.y, s * vector.x + c * vector.y)
  }
  transformPoint(point) { const v = this.transformVector(point); return new Point2(v.x + this._translation.x, v.y + this._translation.y) }
  inverse() { const inverse = new RigidTransform2(-this._angle); const v = inverse.transformVector(this._translation); return new RigidTransform2(-this._angle, new Vec2(-v.x, -v.y)) }
  then(next) { const v = next.transformVector(this._translation); return new RigidTransform2(this._angle + next._angle, new Vec2(v.x + next._translation.x, v.y + next._translation.y)) }
}

export class Curve2 {
  evaluate(parameter) { return curveEvaluation(nativeCall(() => curveModule(this).evaluateCurve2(this._dto(), parameter))) }
  closestPoint(point, tolerance = ModelTolerance.default()) {
    const [parameter, x, y, distance] = nativeCall(() => curveModule(this).projectCurve2(this._dto(), point.x, point.y, tolerance.toArray()))
    return Object.freeze({ parameter, point: new Point2(x, y), distance })
  }
  trimmed(range) { return new (curveModule(this)._curveConstructor)(this, range) }
  transformed(transform, tolerance = ModelTolerance.default()) {
    return this.trimmed(this.domain().bounds || new ParameterRange(0, 1)).transformed(transform, tolerance).curve()
  }
  offset(distance) { return this.trimmed(this.domain().bounds || new ParameterRange(0, 1)).offset(distance).curve() }
}

export class Line2 extends Curve2 {
  constructor(origin, direction) {
    super()
    this._origin = instance(origin, Point2, 'origin')
    this._direction = instance(direction, Vec2, 'direction')
    if(this._direction.magnitude() == 0) throw new RangeError('line direction must be nonzero')
    Object.freeze(this)
  }
  static new(origin, direction) { return new this(origin, direction) }
  origin() { return this._origin }
  direction() { return this._direction }
  domain() { return new ParameterDomain() }
  evaluate(parameter) { return curveEvaluation(nativeCall(() => curveModule(this).evaluateCurve2(this._dto(), parameter))) }
  _dto() { return { kind: 'line', origin: this._origin.toArray(), direction: this._direction.toArray() } }
}

export class Circle2 extends Curve2 {
  constructor(center, radius) {
    super()
    this._center = instance(center, Point2, 'center')
    this._radius = positive(radius, 'radius')
    Object.freeze(this)
  }
  static new(center, radius) { return new this(center, radius) }
  center() { return this._center }
  radius() { return this._radius }
  domain() { return new ParameterDomain(new ParameterRange(0, Math.PI * 2), Math.PI * 2) }
  evaluate(parameter) { return curveEvaluation(nativeCall(() => curveModule(this).evaluateCurve2(this._dto(), parameter))) }
  _dto() { return { kind: 'circle', center: this._center.toArray(), radius: this._radius } }
}

export class Line3 {
  constructor(origin, direction) {
    this._origin = instance(origin, Point3, 'origin')
    this._direction = instance(direction, Vec3, 'direction')
    if(this._direction.magnitude() == 0) throw new RangeError('line direction must be nonzero')
    Object.freeze(this)
  }
  static new(origin, direction) { return new this(origin, direction) }
  static through(start, end) {
    instance(start, Point3, 'start'); instance(end, Point3, 'end')
    return new this(start, new Vec3(end.x - start.x, end.y - start.y, end.z - start.z))
  }
  origin() { return this._origin }
  direction() { return this._direction }
  _dto() { return { kind: 'line', origin: this._origin.toArray(), direction: this._direction.toArray() } }
}

export class Circle3 {
  constructor(center, xRadius, yRadius, tolerance = ModelTolerance.default()) {
    this._center = instance(center, Point3, 'center')
    this._xRadius = instance(xRadius, Vec3, 'xRadius')
    this._yRadius = instance(yRadius, Vec3, 'yRadius')
    instance(tolerance, ModelTolerance, 'tolerance')
    const xLength = this._xRadius.magnitude()
    const yLength = this._yRadius.magnitude()
    const scale = Math.max(xLength, yLength)
    if(xLength <= tolerance.linearAtScale(scale) || yLength <= tolerance.linearAtScale(scale)) {
      throw new RangeError('circle radius must exceed model tolerance')
    }
    const cosine = Math.max(-1, Math.min(1, this._xRadius.dot(this._yRadius) / (xLength * yLength)))
    if(Math.abs(xLength - yLength) > tolerance.linearAtScale(scale)
      || Math.abs(Math.acos(cosine) - Math.PI / 2) > tolerance.angular) {
      throw new RangeError('circle radius vectors must be perpendicular and equally long')
    }
    Object.freeze(this)
  }
  static new(center, xRadius, yRadius, tolerance) { return new this(center, xRadius, yRadius, tolerance) }
  static fromAxis(axis, radius, tolerance = ModelTolerance.default()) {
    instance(axis, Axis3, 'axis')
    positive(radius, 'radius')
    const frame = Frame3.fromZ(axis.origin(), axis.direction())
    return new this(axis.origin(), frame.x().scaled(radius), frame.y().scaled(radius), tolerance)
  }
  center() { return this._center }
  radius() { return this._xRadius.magnitude() }
  xRadius() { return this._xRadius }
  yRadius() { return this._yRadius }
  normal() { return this._xRadius.cross(this._yRadius).normalized() }
  _dto() { return { kind: 'circle', center: this._center.toArray(), x_radius: this._xRadius.toArray(), y_radius: this._yRadius.toArray() } }
}

export class NurbsCurve2 extends Curve2 {
  constructor(degree, controlPoints, weights, knots) {
    super()
    if(!Number.isInteger(degree) || degree < 1) throw new RangeError('degree must be a positive integer')
    controlPoints = array(controlPoints, 'controlPoints')
    controlPoints.forEach((point, index) => instance(point, Point2, `controlPoints[${index}]`))
    weights = array(weights, 'weights').map((value, index) => positive(value, `weights[${index}]`))
    knots = array(knots, 'knots').map((value, index) => finite(value, `knots[${index}]`))
    if(controlPoints.length != weights.length) throw new RangeError('one weight is required per control point')
    this._degree = degree
    this._controlPoints = Object.freeze(controlPoints)
    this._weights = Object.freeze(weights)
    this._knots = Object.freeze(knots)
    Object.freeze(this)
  }
  static new(...values) { return new this(...values) }
  degree() { return this._degree }
  controlPoints() { return this._controlPoints }
  weights() { return this._weights }
  knots() { return this._knots }
  domain() { return new ParameterDomain(new ParameterRange(this._knots[this._degree], this._knots[this._controlPoints.length])) }
  evaluate(parameter) { return curveEvaluation(nativeCall(() => curveModule(this).evaluateCurve2(this._dto(), parameter))) }
  _dto() {
    return {
      kind: 'nurbs', degree: this._degree,
      control_points: this._controlPoints.map(point => point.toArray()),
      weights: this._weights, knots: this._knots,
    }
  }
}

// An exact derived curve whose representation is owned by the native library.
// Consumers use the same evaluation API without inspecting transport details.
class NativeCurve2Value extends Curve2 {
  constructor(native, bindings) { super(); this._native = native; this.kind = native.descriptor().curve.kind; VALUE_MODULES.set(this, bindings); Object.freeze(this) }
  evaluate(parameter) { return curveEvaluation(nativeCall(() => this._native.evaluate(parameter))) }
  domain() { const domain = this._native.domain(); return new ParameterDomain(domain.bounds ? new ParameterRange(...domain.bounds) : null, domain.period) }
  _dto() { return { kind: 'retained', key: this._native.descriptor().curve.key } }
}

export class TrimmedCurve2 {
  constructor(curve, range) {
    this._curve = oneOf(curve, [Line2, Circle2, NurbsCurve2, Curve2], 'curve')
    this._range = instance(range, ParameterRange, 'range')
    Object.freeze(this)
  }
  static new(curve, range) { return new this(curve, range) }
  static line(start, end) {
    instance(start, Point2, 'start'); instance(end, Point2, 'end')
    return new this(
      Line2.new(start, Vec2.new(end.x - start.x, end.y - start.y)),
      ParameterRange.new(0, 1),
    )
  }
  static circle(center, radius) {
    return new this(Circle2.new(center, radius), ParameterRange.new(0, Math.PI * 2))
  }
  static arcThrough(start, interior, end) {
    instance(start, Point2, 'start'); instance(interior, Point2, 'interior'); instance(end, Point2, 'end')
    const denominator = 2 * (start.x * (interior.y - end.y) + interior.x * (end.y - start.y) + end.x * (start.y - interior.y))
    if(Math.abs(denominator) <= Number.EPSILON) throw new RangeError('three arc points must not be collinear')
    const a2 = start.x ** 2 + start.y ** 2
    const b2 = interior.x ** 2 + interior.y ** 2
    const c2 = end.x ** 2 + end.y ** 2
    const center = Point2.new(
      (a2 * (interior.y - end.y) + b2 * (end.y - start.y) + c2 * (start.y - interior.y)) / denominator,
      (a2 * (end.x - interior.x) + b2 * (start.x - end.x) + c2 * (interior.x - start.x)) / denominator,
    )
    const parameter = point => Math.atan2(point.y - center.y, point.x - center.x)
    const startParameter = parameter(start)
    const middleDelta = ((parameter(interior) - startParameter) % (Math.PI * 2) + Math.PI * 2) % (Math.PI * 2)
    const endDelta = ((parameter(end) - startParameter) % (Math.PI * 2) + Math.PI * 2) % (Math.PI * 2)
    const endParameter = middleDelta < endDelta ? startParameter + endDelta : startParameter - (Math.PI * 2 - endDelta)
    return new this(Circle2.new(center, Math.hypot(start.x - center.x, start.y - center.y)), ParameterRange.new(startParameter, endParameter))
  }
  curve() { return this._curve }
  range() { return this._range }
  trimmed(range) {
    if(!this._range.contains(range.start, ModelTolerance.default()) || !this._range.contains(range.end, ModelTolerance.default())) throw new RangeError('new trim lies outside the current trim')
    return new this.constructor(this._curve, range)
  }
  reversed() { return new this.constructor(this._curve, this._range.reversed()) }
  start() { return this.evaluate(this._range.start).point }
  end() { return this.evaluate(this._range.end).point }
  evaluate(parameter) {
    if(!this._range.contains(parameter, ModelTolerance.default())) throw new RangeError('parameter lies outside the trimmed curve')
    return curveEvaluation(nativeCall(() => nativeCurve(this).evaluate(parameter)))
  }
  closestPoint(point, tolerance = ModelTolerance.default()) {
    const [parameter, x, y, distance] = nativeCall(() => nativeCurve(this).closestPoint(point.x, point.y, tolerance.toArray()))
    return Object.freeze({ parameter, point: new Point2(x, y), distance })
  }
  length(tolerance = ModelTolerance.default()) { return nativeCall(() => nativeCurve(this).length(tolerance.toArray())) }
  intersections(other, tolerance = ModelTolerance.default()) {
    return nativeCall(() => nativeCurve(this).intersections(nativeCurve(other), tolerance.toArray())).map(hit => hit.kind == 'point'
      ? Object.freeze({ ...hit, point: new Point2(...hit.point) })
      : Object.freeze({ kind: 'overlap', first: new ParameterRange(...hit.first), second: new ParameterRange(...hit.second) }))
  }
  splitAt(parameters, tolerance = ModelTolerance.default()) {
    return Array.from(nativeCall(() => nativeCurve(this).splitAt(Float64Array.from(parameters), tolerance.toArray())), value => this.constructor._fromNative(value))
  }
  offset(distance) { return this.constructor._fromNative(nativeCall(() => nativeCurve(this).offset(distance))) }
  transformed(transform, tolerance = ModelTolerance.default()) {
    return this.constructor._fromNative(nativeCall(() => nativeCurve(this).transformed(transform.angle(), transform.translation().x, transform.translation().y, tolerance.toArray())))
  }
  approximateNurbs(tolerance = ModelTolerance.default()) {
    const result = nativeCall(() => nativeCurve(this).approximateNurbs(tolerance.toArray()))
    return Object.freeze({ curve: this.constructor._fromNative(result.curve), estimatedError: result.estimatedError })
  }
  selfIntersections(tolerance = ModelTolerance.default()) {
    return nativeCall(() => nativeCurve(this).selfIntersections(tolerance.toArray())).map(hit => hit.kind == 'point'
      ? Object.freeze({ ...hit, point: new Point2(...hit.point) })
      : Object.freeze({ kind: 'overlap', first: new ParameterRange(...hit.first), second: new ParameterRange(...hit.second) }))
  }
  tessellate(options = TessellationOptions.default()) {
    const result = nativeCall(() => nativeCurve(this).tessellate(options.chordTolerance, options.angularTolerance, options.maximumDepth))
    return Object.freeze({ parameters: Object.freeze(result.parameters), points: Object.freeze(result.points.map(point => new Point2(...point))) })
  }
  lifted(plane, tolerance = ModelTolerance.default()) {
    return curveModule(this)._wrapCurve3(nativeCall(() => nativeCurve(this).lift(plane._values(), tolerance.toArray())))
  }
  _dto() { return { curve: this._curve._dto(), range: { start: this._range.start, end: this._range.end } } }
}

export class Wire2 {
  constructor(segments, tolerance = ModelTolerance.default()) {
    segments = array(segments, 'segments')
    if(!segments.length) throw new RangeError('a wire needs at least one segment')
    segments.forEach((segment, index) => instance(segment, TrimmedCurve2, `segments[${index}]`))
    this._segments = Object.freeze(segments)
    this._tolerance = instance(tolerance, ModelTolerance, 'tolerance')
    Object.freeze(this)
  }
  static new(segments, tolerance) { return new this(segments, tolerance) }
  static polygon(vertices, tolerance = ModelTolerance.default()) {
    vertices = array(vertices, 'vertices')
    vertices.forEach((point, index) => instance(point, Point2, `vertices[${index}]`))
    if(vertices.length > 1 && vertices[0].x == vertices.at(-1).x && vertices[0].y == vertices.at(-1).y) vertices.pop()
    if(vertices.length < 3) throw new RangeError('a polygon wire needs at least three vertices')
    return new this(vertices.map((point, index) => TrimmedCurve2.line(point, vertices[(index + 1) % vertices.length])), tolerance)
  }
  segments() { return this._segments }
  _properties() {
    const properties = WIRE_PROPERTIES.get(this)
    if(!properties) throw new ShapexError('Wire2 properties require an initialized ShapexJS instance')
    return properties
  }
  signedArea() { return this._properties()[0] }
  area() { return this._properties()[1] }
  isCounterClockwise() { return Boolean(this._properties()[2]) }
  classifyPoint(point) { return nativeCall(() => curveModule(this).classifyWire2(this._dto(), point.x, point.y)) }
  reversed() { return new this.constructor(this._segments.toReversed().map(segment => segment.reversed()), this._tolerance) }
  _dto() { return { segments: this._segments.map(segment => segment._dto()), tolerance: this._tolerance } }
}

export class TessellationOptions {
  constructor(chordTolerance = 1.0e-3, angularTolerance = 0.1, maximumDepth = 24) {
    this.chordTolerance = positive(chordTolerance, 'chordTolerance')
    this.angularTolerance = positive(angularTolerance, 'angularTolerance')
    if(this.angularTolerance >= Math.PI) throw new RangeError('angularTolerance must be smaller than pi')
    if(!Number.isInteger(maximumDepth) || maximumDepth < 1 || maximumDepth > 255) throw new RangeError('maximumDepth must be between 1 and 255')
    this.maximumDepth = maximumDepth
    Object.freeze(this)
  }
  static new(...values) { return new this(...values) }
  static default() { return new this() }
}

export class FilletOptions {
  constructor(radius, followTangentEdges = true) {
    this.radius = positive(radius, 'radius')
    this.followTangentEdges = Boolean(followTangentEdges)
    Object.freeze(this)
  }
  static new(radius) { return new this(radius) }
  withTangentPropagation(enabled) { return new FilletOptions(this.radius, enabled) }
}

export const BooleanKind = Object.freeze({
  Union: 'union',
  Difference: 'difference',
  Intersection: 'intersection',
})

export class TriangleMesh {
  constructor(positions, normals, triangles) {
    this.positions = Object.freeze(positions)
    this.normals = Object.freeze(normals)
    this.triangles = Object.freeze(triangles)
    Object.freeze(this)
  }
  isEmpty() { return this.triangles.length == 0 }
}

export class Polyline {
  constructor(parameters, points) {
    this.parameters = Object.freeze(parameters)
    this.points = Object.freeze(points)
    Object.freeze(this)
  }
}

function profileDto(profile) {
  return { outer: profile._outer._dto(), holes: profile._holes.map(hole => hole._dto()), tolerance: profile._tolerance }
}

function defaultWasmUrl() {
  return new URL('./pkg/shapex_js_wasm_bg.wasm', import.meta.url).href
}

/** Create the public API around this package's initialized private transport. */
export function createShapex(bindings) {
  const required = ['wireProperties', 'profileArea', 'extrude', 'revolve', 'loft', 'sweep', 'NativeCompound', 'NativeFaceList', 'NativeEdgeList', 'NativeSolidList']
  required.forEach(name => {
    if(typeof bindings?.[name] != 'function') throw new TypeError(`invalid ShapexJS transport: missing ${name}`)
  })

  for(const method of ['chamfer', 'draft']) {
    if(typeof bindings.NativeCompound.prototype[method] != 'function') {
      throw new TypeError(`outdated ShapexJS transport: missing ${method}; rebuild the WASM package`)
    }
  }
  if(bindings.NativeCompound.prototype.chamfer.length < 6) {
    throw new TypeError('outdated ShapexJS transport: chamfer modes are unavailable; rebuild the WASM package')
  }

  function mesh(raw) {
    try {
      const triangles = Array.from(raw.triangles)
      return new TriangleMesh(
        triples(raw.positions, Point3),
        triples(raw.normals, Vec3),
        Array.from({ length: triangles.length / 3 }, (_, index) => triangles.slice(index * 3, index * 3 + 3)),
      )
    } finally { free(raw) }
  }

  function tessellation(options) {
    return options == null ? TessellationOptions.default() : instance(options, TessellationOptions, 'options')
  }

  const curveTransport = { ...bindings, _wrapCurve3: raw => new TrimmedCurve3(raw) }
  function bindCurve(Type) {
    class Bound extends Type {}
    CURVE_MODULES.set(Bound, curveTransport)
    return Bound
  }
  const BoundLine2 = bindCurve(Line2)
  const BoundCircle2 = bindCurve(Circle2)
  class BoundNurbsCurve2 extends NurbsCurve2 {
    constructor(...values) { super(...values); nativeCall(() => bindings.validateCurve2(this._dto())) }
  }
  CURVE_MODULES.set(BoundNurbsCurve2, curveTransport)
  class BoundTrimmedCurve2 extends TrimmedCurve2 {
    constructor(curve, range, raw) {
      if(curve instanceof Line2 && !CURVE_MODULES.has(curve.constructor)) curve = new BoundLine2(curve.origin(), curve.direction())
      if(curve instanceof Circle2 && !CURVE_MODULES.has(curve.constructor)) curve = new BoundCircle2(curve.center(), curve.radius())
      if(curve instanceof NurbsCurve2 && !CURVE_MODULES.has(curve.constructor)) curve = new BoundNurbsCurve2(curve.degree(), curve.controlPoints(), curve.weights(), curve.knots())
      super(curve, range)
      NATIVE_CURVES.set(this, raw || nativeCall(() => new bindings.NativeCurve2(this._dto())))
    }
    static arcThrough(start, interior, end) {
      return this._fromNative(nativeCall(() => bindings.NativeCurve2.arcThrough(start.x, start.y, interior.x, interior.y, end.x, end.y)))
    }
    static _fromNative(raw) {
      const { curve, range } = raw.descriptor()
      const basis = curve.kind == 'line' ? new BoundLine2(new Point2(...curve.origin), new Vec2(...curve.direction))
        : curve.kind == 'circle' ? new BoundCircle2(new Point2(...curve.center), curve.radius)
        : curve.kind == 'nurbs' ? new BoundNurbsCurve2(curve.degree, curve.control_points.map(p => new Point2(...p)), curve.weights, curve.knots)
        : new NativeCurve2Value(raw, curveTransport)
      const result = new this(basis, new ParameterRange(range.start, range.end), raw)
      return result
    }
  }
  CURVE_MODULES.set(BoundTrimmedCurve2, curveTransport)
  curveTransport._curveConstructor = BoundTrimmedCurve2

  class TrimmedCurve3 {
    constructor(curve, range) {
      this._native = range ? nativeCall(() => new bindings.NativeCurve3(curve._dto(), range.start, range.end)) : curve
      Object.freeze(this)
    }
    static new(curve, range) { return new this(curve, range) }
    range() { return new ParameterRange(...this._native.range()) }
    trimmed(range) { return new TrimmedCurve3(nativeCall(() => this._native.trimmed(range.start, range.end))) }
    reversed() { return this.trimmed(this.range().reversed()) }
    evaluate(parameter) {
      if(!this.range().contains(parameter, ModelTolerance.default())) throw new RangeError('parameter lies outside the trimmed curve')
      return curveEvaluation(nativeCall(() => this._native.evaluate(parameter)), Point3, Vec3, 3)
    }
    closestPoint(point, tolerance = ModelTolerance.default()) {
      const [parameter, x, y, z, distance] = nativeCall(() => this._native.closestPoint(point.x, point.y, point.z, tolerance.toArray()))
      return Object.freeze({ parameter, point: new Point3(x, y, z), distance })
    }
    transformed(transform, tolerance = ModelTolerance.default()) {
      return new TrimmedCurve3(nativeCall(() => this._native.transformed(transform._values(), tolerance.toArray())))
    }
    projectedOnto(plane, tolerance = ModelTolerance.default()) {
      const result = nativeCall(() => this._native.project(plane._values(), tolerance.toArray()))
      return Object.freeze(result.kind == 'point' ? { kind: 'point', point: new Point2(...result.point) }
        : { kind: 'curve', curve: BoundTrimmedCurve2._fromNative(result.curve) })
    }
    expressedIn(plane, tolerance = ModelTolerance.default()) {
      return BoundTrimmedCurve2._fromNative(nativeCall(() => this._native.expressedIn(plane._values(), tolerance.toArray())))
    }
  }

  class Path2 {
    constructor(segments, closed = false, tolerance = ModelTolerance.default()) {
      this._segments = Object.freeze(Array.from(segments))
      this._closed = closed
      this._tolerance = tolerance
      nativeCall(() => bindings.validatePath2(this._dto(), closed))
      Object.freeze(this)
    }
    _dto() { return { segments: this._segments.map(curve => curve._dto()), tolerance: this._tolerance } }
    static new(...values) { return new this(...values) }
    segments() { return this._segments }
    isClosed() { return this._closed }
    reversed() { return new Path2(this._segments.toReversed().map(curve => curve.reversed()), this._closed, this._tolerance) }
    validateSimple() { nativeCall(() => bindings.validateSimplePath2(this._dto(), this._closed)) }
    signedArea() { return nativeCall(() => bindings.pathSignedArea2(this._dto())) }
    classifyPoint(point) { return nativeCall(() => bindings.classifyPath2(this._dto(), point.x, point.y)) }
    offset(distance, { join = 'round', miterLimit = 4 } = {}) {
      const result = nativeCall(() => bindings.offsetPath2(this._dto(), this._closed, distance, join, miterLimit))
      return Object.freeze({ path: new Path2(Array.from(result.curves, raw => BoundTrimmedCurve2._fromNative(raw)), this._closed, this._tolerance),
        sources: Object.freeze(result.sources) })
    }
  }

  class BoundWire2 extends Wire2 {
    constructor(segments, tolerance) {
      super(segments, tolerance)
      const values = Array.from(nativeCall(() => bindings.wireProperties(this._dto())))
      WIRE_PROPERTIES.set(this, values)
    }
  }
  CURVE_MODULES.set(BoundWire2, curveTransport)

  class Profile {
    constructor(outer, holes = [], tolerance = ModelTolerance.default()) {
      this._outer = instance(outer, Wire2, 'outer')
      holes = array(holes, 'holes')
      holes.forEach((hole, index) => instance(hole, Wire2, `holes[${index}]`))
      this._holes = Object.freeze(holes)
      this._tolerance = instance(tolerance, ModelTolerance, 'tolerance')
      this._area = nativeCall(() => bindings.profileArea(profileDto(this)))
      Object.freeze(this)
    }
    static new(...values) { return new this(...values) }
    outer() { return this._outer }
    holes() { return this._holes }
    area() { return this._area }
    tolerance() { return this._tolerance }
    approximateRational(tolerance = this._tolerance) {
      let estimatedError = 0
      const wires = [this._outer, ...this._holes].map(wire => new BoundWire2(wire.segments().map(segment => {
        const basis = segment.curve()
        if(basis instanceof Line2 || basis instanceof Circle2 || basis instanceof NurbsCurve2) return segment
        const result = segment.approximateNurbs(tolerance)
        estimatedError = Math.max(estimatedError, result.estimatedError)
        return result.curve
      }), this._tolerance))
      return Object.freeze({ profile: new Profile(wires[0], wires.slice(1), this._tolerance), estimatedError })
    }
    classifyPoint(point) { return nativeCall(() => bindings.classifyProfile2(profileDto(this), point.x, point.y)) }
    offset(distance) {
      return Array.from(nativeCall(() => bindings.offsetProfile2(profileDto(this), distance)), wires => {
        const boundaries = Array.from(wires, curves => new BoundWire2(Array.from(curves, raw => BoundTrimmedCurve2._fromNative(raw)), this._tolerance))
        return new Profile(boundaries[0], boundaries.slice(1), this._tolerance)
      })
    }
    extrude(plane, vector) {
      instance(plane, Plane3, 'plane'); instance(vector, Vec3, 'vector')
      return nativeCall(() => new Compound(bindings.extrude(
        profileDto(this), new Float64Array(plane._values()), new Float64Array(vector.toArray()),
      )))
    }
    revolve(plane, axis, angle) {
      instance(plane, Plane3, 'plane'); instance(axis, Axis3, 'axis'); finite(angle, 'angle')
      return nativeCall(() => new Compound(bindings.revolve(
        profileDto(this), new Float64Array(plane._values()), new Float64Array(axis._values()), angle,
      )))
    }
    extrudeUpToFace(plane, target, limit, offset = 0, endOffset = 0) {
      instance(plane, Plane3, 'plane'); instance(target, Compound, 'target'); instance(limit, Face, 'limit')
      return nativeCall(() => new Compound(bindings.extrudeUpToFace(
        profileDto(this), new Float64Array(plane._values()), target._raw(), limit._raw(),
        finite(offset, 'offset'), finite(endOffset, 'endOffset'),
      )))
    }
    revolveUpToFace(plane, axis, target, limit, forward = true) {
      instance(plane, Plane3, 'plane'); instance(axis, Axis3, 'axis')
      instance(target, Compound, 'target'); instance(limit, Face, 'limit')
      return nativeCall(() => new Compound(bindings.revolveUpToFace(
        profileDto(this), new Float64Array(plane._values()), new Float64Array(axis._values()),
        target._raw(), limit._raw(), Boolean(forward),
      )))
    }
    revolveBetweenFaces(plane, axis, fromBody, from, untilBody, until, forward = true) {
      instance(plane, Plane3, 'plane'); instance(axis, Axis3, 'axis')
      instance(fromBody, Compound, 'fromBody'); instance(from, Face, 'from')
      instance(untilBody, Compound, 'untilBody'); instance(until, Face, 'until')
      return nativeCall(() => new Compound(bindings.revolveBetweenFaces(
        profileDto(this), new Float64Array(plane._values()), new Float64Array(axis._values()),
        fromBody._raw(), from._raw(), untilBody._raw(), until._raw(), Boolean(forward),
      )))
    }
    sweep(plane, railPlane, rail) {
      instance(plane, Plane3, 'plane'); instance(railPlane, Plane3, 'railPlane')
      instance(rail, TrimmedCurve2, 'rail')
      return nativeCall(() => new Compound(bindings.sweep(
        profileDto(this), new Float64Array(plane._values()), rail._dto(),
        new Float64Array(railPlane._values()),
      )))
    }
    sweepAlongEdge(plane, rail) {
      instance(plane, Plane3, 'plane'); instance(rail, Edge, 'rail')
      return nativeCall(() => new Compound(bindings.sweepAlongEdge(
        profileDto(this), new Float64Array(plane._values()), rail._raw(),
      )))
    }
    loft(plane, sections, alignSections = true) {
      instance(plane, Plane3, 'plane')
      sections = [{ profile: this, plane }, ...array(sections, 'sections')]
      const description = sections.map((section, index) => ({
        profile: profileDto(instance(section?.profile, Profile, `sections[${index}].profile`)),
        plane: instance(section?.plane, Plane3, `sections[${index}].plane`)._values(),
      }))
      return nativeCall(() => new Compound(bindings.loft(description, Boolean(alignSections))))
    }
    loftWithAlignment(plane, sections, alignSections) {
      return this.loft(plane, sections, alignSections)
    }
  }

  class NativeValue {
    constructor(raw) {
      if(!raw) throw new TypeError(`${this.constructor.name} needs a native value`)
      this.raw = raw
    }
    free() { free(this.raw); this.raw = null }
    _raw() {
      if(!this.raw) throw new ShapexError(`${this.constructor.name} has been freed`)
      return this.raw
    }
  }

  class Face extends NativeValue {
    id() { return this._raw().id }
    orientation() { return this._raw().orientation }
    surfaceProperties() {
      if(!this._surfaceProperties) {
        const [area, x, y, z] = nativeCall(() => Array.from(this._raw().surfaceProperties()))
        this._surfaceProperties = { area, centroid: new Point3(x, y, z) }
      }
      return this._surfaceProperties
    }
    area() { return this.surfaceProperties().area }
    centroid() { return this.surfaceProperties().centroid }
    surface() {
      const kind = this._raw().surfaceKind()
      const values = Array.from(this._raw().surfaceFrame())
      if(kind == 'plane') return { kind, value: Plane3.new(new Point3(...values.slice(0, 3)), new Vec3(...values.slice(3, 6)), new Vec3(...values.slice(6, 9))) }
      if(kind == 'cylinder') return { kind, axis: Axis3.new(new Point3(...values.slice(0, 3)), new Vec3(...values.slice(3, 6))) }
      if(kind == 'sphere') return { kind, center: new Point3(...values) }
      return { kind }
    }
    boundaryEdges() {
      const raw = nativeCall(() => this._raw().boundaryEdges())
      try { return Array.from({ length: raw.length }, (_, index) => new Edge(raw.get(index))) }
      finally { free(raw) }
    }
    tessellate(options) {
      const value = tessellation(options)
      return nativeCall(() => mesh(this._raw().tessellate(value.chordTolerance, value.angularTolerance, value.maximumDepth)))
    }
  }

  class Edge extends NativeValue {
    id() { return this._raw().id }
    endpoints() { return triples(nativeCall(() => this._raw().endpoints()), Point3) }
    curve() {
      const kind = this._raw().curveKind()
      const values = Array.from(this._raw().curveData())
      if(kind == 'line') return Line3.new(new Point3(...values.slice(0, 3)), new Vec3(...values.slice(3, 6)))
      if(kind == 'circle') return new Circle3(
        new Point3(...values.slice(0, 3)),
        new Vec3(...values.slice(3, 6)),
        new Vec3(...values.slice(6, 9)),
      )
      return this.trimmedCurve()
    }
    trimmedCurve() { return new TrimmedCurve3(nativeCall(() => this._raw().trimmedCurve())) }
    connectedFaces() {
      const raw = nativeCall(() => this._raw().connectedFaces())
      try { return Array.from({ length: raw.length }, (_, index) => new Face(raw.get(index))) }
      finally { free(raw) }
    }
    tessellate(options) {
      const value = tessellation(options)
      const raw = nativeCall(() => this._raw().tessellate(value.chordTolerance, value.angularTolerance, value.maximumDepth))
      try { return new Polyline(Array.from(raw.parameters), triples(raw.points, Point3)) }
      finally { free(raw) }
    }
  }

  class Vertex extends NativeValue {
    id() { return this._raw().id }
    point() { return new Point3(...Array.from(this._raw().point)) }
  }

  class Solid extends NativeValue {
    id() { return this._raw().id }
    volume() { return this._raw().volume() }
    surfaceArea() { return this._raw().surfaceArea() }
    centroid() { return new Point3(...Array.from(this._raw().centroid())) }
    faces() { return Array.from({ length: this._raw().faceCount }, (_, index) => new Face(this._raw().face(index))) }
    edges() { return Array.from({ length: this._raw().edgeCount }, (_, index) => new Edge(this._raw().edge(index))) }
    vertices() { return Array.from({ length: this._raw().vertexCount }, (_, index) => new Vertex(this._raw().vertex(index))) }
    tessellate(options) {
      const value = tessellation(options)
      return nativeCall(() => mesh(this._raw().tessellate(value.chordTolerance, value.angularTolerance, value.maximumDepth)))
    }
    toCompound() { return new Compound(this._raw().toCompound()) }
    union(other) { return this.toCompound().union(instance(other, Solid, 'other').toCompound()) }
    difference(other) { return this.toCompound().difference(instance(other, Solid, 'other').toCompound()) }
    intersection(other) { return this.toCompound().intersection(instance(other, Solid, 'other').toCompound()) }
  }

  class Resolution {
    constructor(kind, entities) {
      this.kind = kind
      this.entities = Object.freeze(entities)
      Object.freeze(this)
    }
  }

  class Compound extends NativeValue {
    static fromStep(text, tolerance = ModelTolerance.default()) {
      instance(tolerance, ModelTolerance, 'tolerance')
      return nativeCall(() => new Compound(bindings.compoundFromStep(String(text), new Float64Array(tolerance.toArray()))))
    }
    volume() { return this._raw().volume() }
    surfaceArea() { return this._raw().surfaceArea() }
    centroid() { return new Point3(...Array.from(this._raw().centroid())) }
    solids() { return Array.from({ length: this._raw().solidCount }, (_, index) => new Solid(this._raw().solid(index))) }
    faces() { return Array.from({ length: this._raw().faceCount }, (_, index) => new Face(this._raw().face(index))) }
    edges() { return Array.from({ length: this._raw().edgeCount }, (_, index) => new Edge(this._raw().edge(index))) }
    vertices() { return Array.from({ length: this._raw().vertexCount }, (_, index) => new Vertex(this._raw().vertex(index))) }
    tessellate(options) {
      const value = tessellation(options)
      return nativeCall(() => mesh(this._raw().tessellate(value.chordTolerance, value.angularTolerance, value.maximumDepth)))
    }
    _boolean(other, operation) {
      instance(other, Compound, 'other')
      return nativeCall(() => new Compound(this._raw().boolean(other._raw(), operation)))
    }
    boolean(other, kind) {
      if(!Object.values(BooleanKind).includes(kind)) throw new RangeError('unknown Boolean kind')
      return this._boolean(other, kind)
    }
    union(other) { return this._boolean(other, 'union') }
    difference(other) { return this._boolean(other, 'difference') }
    intersection(other) { return this._boolean(other, 'intersection') }
    combined(others) {
      return array(others, 'others').reduce((result, other) => {
        instance(other, Compound, 'other')
        return nativeCall(() => new Compound(result._raw().combined(other._raw())))
      }, this)
    }
    dissolveFaces(faces) {
      const list = new bindings.NativeFaceList()
      try {
        array(faces, 'faces').forEach((face, index) => list.add(instance(face, Face, `faces[${index}]`)._raw()))
        return nativeCall(() => new Compound(this._raw().dissolveFaces(list)))
      } finally { free(list) }
    }
    offsetFaces(faces, distance) {
      const list = new bindings.NativeFaceList()
      try {
        array(faces, 'faces').forEach((face, index) => list.add(instance(face, Face, `faces[${index}]`)._raw()))
        return nativeCall(() => new Compound(this._raw().offsetFaces(list, finite(distance, 'distance'))))
      } finally { free(list) }
    }
    shell(openings, distance, joinType = 'rounded') {
      if(!['sharp', 'rounded'].includes(joinType)) throw new TypeError('Shell join type must be sharp or rounded')
      const list = new bindings.NativeFaceList()
      try {
        array(openings, 'openings').forEach((face, index) => list.add(instance(face, Face, `openings[${index}]`)._raw()))
        return nativeCall(() => new Compound(this._raw().shell(list, finite(distance, 'distance'), joinType)))
      } finally { free(list) }
    }
    fillet(edges, radius) { return this.filletWithOptions(edges, FilletOptions.new(radius)) }
    filletWithOptions(edges, options) {
      instance(options, FilletOptions, 'options')
      const list = new bindings.NativeEdgeList()
      try {
        array(edges, 'edges').forEach((edge, index) => list.add(instance(edge, Edge, `edges[${index}]`)._raw()))
        return nativeCall(() => new Compound(this._raw().fillet(list, options.radius, options.followTangentEdges)))
      } finally { free(list) }
    }
    chamfer(edges, distance, followTangentEdges=false, { type = 'equal', referenceFaces = [], value = distance } = {}) {
      const list = new bindings.NativeEdgeList()
      const faces = new bindings.NativeFaceList()
      try {
        array(edges, 'edges').forEach((edge, index) => list.add(instance(edge, Edge, `edges[${index}]`)._raw()))
        array(referenceFaces, 'referenceFaces').forEach((face, index) => faces.add(instance(face, Face, `referenceFaces[${index}]`)._raw()))
        return nativeCall(() => new Compound(this._raw().chamfer(list, finite(distance, 'distance'), Boolean(followTangentEdges), faces, type, finite(value, 'value'))))
      } finally { free(list); free(faces) }
    }
    draft(faces, plane, angle) {
      instance(plane, Plane3, 'plane')
      const list = new bindings.NativeFaceList()
      try {
        array(faces, 'faces').forEach((face, index) => list.add(instance(face, Face, `faces[${index}]`)._raw()))
        return nativeCall(() => new Compound(this._raw().draft(
          list, new Float64Array(plane._values()), finite(angle, 'angle'), new Float64Array(plane.tolerance.toArray()),
        )))
      } finally { free(list) }
    }
    split(plane, keep) {
      instance(plane, Plane3, 'plane')
      return nativeCall(() => new Compound(this._raw().split(
        new Float64Array(plane._values()), keep, new Float64Array(plane.tolerance.toArray()),
      )))
    }
    withoutSolids(solids) {
      const list = new bindings.NativeSolidList()
      try {
        array(solids, 'solids').forEach((solid, index) => list.add(instance(solid, Solid, `solids[${index}]`)._raw()))
        return nativeCall(() => new Compound(this._raw().withoutSolids(list)))
      } finally { free(list) }
    }
    moveFaces(faces, transform) {
      instance(transform, RigidTransform3, 'transform')
      const list = new bindings.NativeFaceList()
      try {
        array(faces, 'faces').forEach((face, index) => list.add(instance(face, Face, `faces[${index}]`)._raw()))
        return nativeCall(() => new Compound(this._raw().moveFaces(
          list, new Float64Array(transform._values()), new Float64Array(transform.tolerance.toArray()),
        )))
      } finally { free(list) }
    }
    replaceFace(target, replacement) {
      instance(target, Face, 'target'); instance(replacement, Face, 'replacement')
      return nativeCall(() => new Compound(this._raw().replaceFace(target._raw(), replacement._raw())))
    }
    transformed(transform) {
      instance(transform, RigidTransform3, 'transform')
      return nativeCall(() => new Compound(this._raw().transformed(
        new Float64Array(transform._values()), new Float64Array(transform.tolerance.toArray()),
      )))
    }
    mirrored(plane) {
      instance(plane, Plane3, 'plane')
      return nativeCall(() => new Compound(this._raw().mirrored(
        new Float64Array(plane._values()), new Float64Array(plane.tolerance.toArray()),
      )))
    }
    resolve(entity) {
      let raw
      let wrap
      if(entity instanceof Face) { raw = this._raw().resolveFace(entity._raw()); wrap = value => new Face(value) }
      else if(entity instanceof Edge) { raw = this._raw().resolveEdge(entity._raw()); wrap = value => new Edge(value) }
      else if(entity instanceof Vertex) { raw = this._raw().resolveVertex(entity._raw()); wrap = value => new Vertex(value) }
      else throw new TypeError('entity must be a Face, Edge, or Vertex')
      try {
        const getter = entity instanceof Face ? 'face' : entity instanceof Edge ? 'edge' : 'vertex'
        return new Resolution(raw.kind, Array.from({ length: raw.length }, (_, index) => wrap(raw[getter](index))))
      } finally { free(raw) }
    }
    isSemanticEdge(edge) { return this._raw().isSemanticEdge(instance(edge, Edge, 'edge')._raw()) }
    topologyHistory() { return JSON.parse(this._raw().topologyHistory()) }
    creationRole(entity) {
      let raw
      if(entity instanceof Face) raw = this._raw().creationRoleFace(entity._raw())
      else if(entity instanceof Edge) raw = this._raw().creationRoleEdge(entity._raw())
      else throw new TypeError('entity must be a Face or Edge')
      if(!raw) return undefined
      try {
        return Object.freeze({
          kind: raw.kind, boundary: raw.boundary, holeIndex: raw.holeIndex,
          segment: raw.segment, part: raw.part,
          generators: JSON.parse(raw.generators),
          chart: raw.chart,
        })
      } finally { free(raw) }
    }
    facesGeneratedFrom(entity) {
      let raw
      if(entity instanceof Face) raw = this._raw().facesGeneratedFromFace(entity._raw())
      else if(entity instanceof Edge) raw = this._raw().facesGeneratedFromEdge(entity._raw())
      else throw new TypeError('entity must be a Face or Edge')
      try { return Array.from({ length: raw.length }, (_, index) => new Face(raw.get(index))) }
      finally { free(raw) }
    }
  }

  // STEP product structures as plain data: products with a name, their own
  // bodies or null, an [r, g, b] colour or null per body, and occurrences
  // placing other products by index. Written files use mm or in.
  const StepAssembly = Object.freeze({
    fromStep(text, tolerance = ModelTolerance.default()) {
      instance(tolerance, ModelTolerance, 'tolerance')
      const raw = nativeCall(() => bindings.NativeStepAssembly.fromStep(String(text), new Float64Array(tolerance.toArray())))
      const placement = values => new RigidTransform3(Matrix3.fromCols(
        new Vec3(values[0], values[1], values[2]),
        new Vec3(values[3], values[4], values[5]),
        new Vec3(values[6], values[7], values[8]),
      ), new Vec3(values[9], values[10], values[11]), tolerance)
      try {
        const products = Array.from({ length: raw.productCount }, (_, index) => {
          const bodies = raw.productBodies(index)
          const colorValues = raw.productColors(index)
          const colors = Array.from({ length: colorValues.length / 3 }, (_, solid) =>
            Number.isNaN(colorValues[solid * 3]) ? null : Object.freeze(Array.from(colorValues.subarray(solid * 3, solid * 3 + 3))))
          const occurrences = Array.from({ length: raw.occurrenceCount(index) }, (_, occurrence) => Object.freeze({
            product: raw.occurrenceProduct(index, occurrence),
            placement: placement(raw.occurrenceTransform(index, occurrence)),
          }))
          return Object.freeze({
            name: raw.productName(index),
            bodies: bodies ? new Compound(bodies) : null,
            colors: Object.freeze(colors),
            occurrences: Object.freeze(occurrences),
          })
        })
        return Object.freeze({ products: Object.freeze(products), roots: Object.freeze(Array.from(raw.roots())) })
      } finally { free(raw) }
    },
    toStep({ products, roots, unit = 'mm' }, timestamp = new Date().toISOString().slice(0, 19)) {
      const raw = new bindings.NativeStepAssembly()
      try {
        nativeCall(() => raw.setUnit(String(unit)))
        products = array(products, 'products')
        products.forEach((product, index) => {
          raw.addProduct(String(product.name))
          if(product.bodies) nativeCall(() => raw.setBodies(index, instance(product.bodies, Compound, `products[${index}].bodies`)._raw()))
          if(product.colors) raw.setColors(index, new Float64Array(array(product.colors, `products[${index}].colors`)
            .flatMap(color => color ? Array.from(color, Number) : [NaN, NaN, NaN])))
        })
        products.forEach((product, index) => array(product.occurrences, `products[${index}].occurrences`).forEach(occurrence => {
          const placement = instance(occurrence.placement, RigidTransform3, 'occurrence.placement')
          nativeCall(() => raw.addOccurrence(index, occurrence.product,
            new Float64Array(placement._values()), new Float64Array(placement.tolerance.toArray())))
        }))
        array(roots, 'roots').forEach(root => nativeCall(() => raw.addRoot(root)))
        return nativeCall(() => raw.toStep(String(timestamp)))
      } finally { free(raw) }
    },
  })

  return Object.freeze({
    ShapexError,
    Point2, Vec2, Point3, Vec3, Vector3: Vec3, Matrix3, Quaternion,
    ModelTolerance, ParameterRange, ParameterDomain, Axis3, Frame3, Plane3, RigidTransform2, RigidTransform3,
    Line2: BoundLine2, Circle2: BoundCircle2, NurbsCurve2: BoundNurbsCurve2, Line3, Circle3,
    Curve2, TrimmedCurve2: BoundTrimmedCurve2, TrimmedCurve3, Path2, Wire2: BoundWire2, Profile,
    TessellationOptions, FilletOptions, TriangleMesh, Polyline,
    BooleanKind,
    Face, Edge, Vertex, Solid, Compound, Resolution, StepAssembly,
  })
}

/** Load ShapexJS's own WebAssembly transport. */
export async function initShapex({ bindings, moduleUrl, wasmUrl, wasm } = {}) {
  const transport = bindings || (moduleUrl
    ? await import(/* @vite-ignore */ moduleUrl)
    : await import('./pkg/shapex_js_wasm.js'))
  if(typeof transport.default != 'function') throw new TypeError('ShapexJS transport has no initializer')
  await transport.default({ module_or_path: wasm || wasmUrl || defaultWasmUrl() })
  return createShapex(transport)
}

export default initShapex
