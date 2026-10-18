const DEFAULT_TOLERANCE = Object.freeze({
  linear: 1.0e-7,
  relative: 1.0e-12,
  angular: 1.0e-10,
  parameter: 1.0e-12,
})
const WIRE_PROPERTIES = new WeakMap()

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

function integer(value, name) {
  if(!Number.isInteger(value) || value < 0) throw new RangeError(`${name} must be a non-negative integer`)
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
    throw new ShapexError(error?.message || String(error), { cause: error })
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

export class Line2 {
  constructor(origin, direction) {
    this._origin = instance(origin, Point2, 'origin')
    this._direction = instance(direction, Vec2, 'direction')
    if(this._direction.magnitude() == 0) throw new RangeError('line direction must be nonzero')
    Object.freeze(this)
  }
  static new(origin, direction) { return new this(origin, direction) }
  origin() { return this._origin }
  direction() { return this._direction }
  _dto() { return { kind: 'line', origin: this._origin.toArray(), direction: this._direction.toArray() } }
}

export class Circle2 {
  constructor(center, radius) {
    this._center = instance(center, Point2, 'center')
    this._radius = positive(radius, 'radius')
    Object.freeze(this)
  }
  static new(center, radius) { return new this(center, radius) }
  center() { return this._center }
  radius() { return this._radius }
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
}

export class NurbsCurve2 {
  constructor(degree, controlPoints, weights, knots) {
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
  _dto() {
    return {
      kind: 'nurbs', degree: this._degree,
      control_points: this._controlPoints.map(point => point.toArray()),
      weights: this._weights, knots: this._knots,
    }
  }
}

export class TrimmedCurve2 {
  constructor(curve, range) {
    this._curve = oneOf(curve, [Line2, Circle2, NurbsCurve2], 'curve')
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
  reversed() { return new TrimmedCurve2(this._curve, this._range.reversed()) }
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

  class BoundWire2 extends Wire2 {
    constructor(segments, tolerance) {
      super(segments, tolerance)
      const values = Array.from(nativeCall(() => bindings.wireProperties(this._dto())))
      WIRE_PROPERTIES.set(this, values)
    }
  }

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
      return { kind }
    }
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

  return Object.freeze({
    Point2, Vec2, Point3, Vec3, Vector3: Vec3, Matrix3, Quaternion,
    ModelTolerance, ParameterRange, Axis3, Frame3, Plane3, RigidTransform3,
    Line2, Circle2, NurbsCurve2, Line3, Circle3, TrimmedCurve2, Wire2: BoundWire2, Profile,
    TessellationOptions, FilletOptions, TriangleMesh, Polyline,
    BooleanKind,
    Face, Edge, Vertex, Solid, Compound, Resolution,
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
