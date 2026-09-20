import * as THREE from 'three'
import { markRaw, toRaw } from 'vue'

import initShapex from '../shapexJS/index.js'
import { rotationFromNormal } from '../core/utils.js'

const LINEAR_TOLERANCE = 0.1
const ANGULAR_TOLERANCE = 0.1
const MAXIMUM_TESSELLATION_DEPTH = 24

function unsupported(operation) {
  throw {
    type: 'error',
    msg: `Shapex does not support ${operation} yet`,
    kernel: 'shapex',
    capability: operation,
  }
}

function featureResult(operation, create) {
  try {
    return new ShapexGeometry(create())
  } catch(error) {
    throw { type: 'error', msg: `Shapex ${operation} failed: ${error?.message || error?.msg || error}`, kernel: 'shapex' }
  }
}

function planeFrame(matrix) {
  const elements = matrix.elements
  const origin = new THREE.Vector3().setFromMatrixPosition(matrix)
  const u = new THREE.Vector3(elements[0], elements[1], elements[2]).normalize()
  const v = new THREE.Vector3(elements[4], elements[5], elements[6])
  v.addScaledVector(u, -v.dot(u)).normalize()
  const normal = new THREE.Vector3().crossVectors(u, v).normalize()
  return { origin, u, v, normal }
}

function meshFromShapex(mesh) {
  const positions = []
  const normals = []
  mesh.triangles.forEach(triangle => triangle.forEach(index => {
    positions.push(...mesh.positions[index].toArray())
    normals.push(...mesh.normals[index].toArray())
  }))
  return { positions, normals }
}

function shapexValue(value) {
  return toRaw(value)
}

function geometryValue(geometry) {
  const value = toRaw(geometry)
  return shapexValue(value instanceof ShapexGeometry ? value.raw : value)
}

export class ShapexGeometry {
  constructor(raw) {
    this.kernel = 'shapex'
    this.raw = markRaw(raw)
    markRaw(this)
  }
}

export class ShapexBackend {
  constructor(api) {
    this.id = 'shapex'
    this.title = 'Shapex'
    this.api = api
    this.tolerance = api.ModelTolerance.default()
    this.tessellationOptions = api.TessellationOptions.new(
      LINEAR_TOLERANCE,
      ANGULAR_TOLERANCE,
      MAXIMUM_TESSELLATION_DEPTH,
    )
    this.capabilities = Object.freeze({
      extrude: true,
      extrudeUpToFace: true,
      boolean: true,
      transform: true,
      tessellate: true,
      revolve: true,
      revolveUpToFace: true,
      revolveBetweenFaces: true,
      loft: true,
      sweep: true,
      fillet: true,
      chamfer: true,
      draft: true,
      offset: true,
      split: true,
      compoundMerge: true,
      multiSolidBoolean: true,
    })
  }

  point2(point) {
    return this.api.Point2.new(point.x, point.y)
  }

  point3(point) {
    return this.api.Point3.new(point.x, point.y, point.z)
  }

  vector3(vector) {
    return this.api.Vec3.new(vector.x, vector.y, vector.z)
  }

  plane(matrix) {
    const { origin, u, v } = planeFrame(matrix)
    return this.api.Plane3.new(
      this.point3(origin),
      this.vector3(u),
      this.vector3(v),
      this.tolerance,
    )
  }

  transformValue(matrix) {
    const position = new THREE.Vector3().setFromMatrixPosition(matrix)
    const rotation = new THREE.Quaternion().setFromRotationMatrix(matrix)
    const x = new THREE.Vector3(1, 0, 0).applyQuaternion(rotation)
    const y = new THREE.Vector3(0, 1, 0).applyQuaternion(rotation)
    const z = new THREE.Vector3(0, 0, 1).applyQuaternion(rotation)
    return this.api.RigidTransform3.new(
      this.api.Matrix3.fromCols(this.vector3(x), this.vector3(y), this.vector3(z)),
      this.vector3(position),
      this.tolerance,
    )
  }

  segment(source) {
    const type = source.typename().replace(' Projection', '')
    if(type == 'Line') {
      const [start, end] = source.endpoints()
      return this.api.TrimmedCurve2.line(this.point2(start), this.point2(end))
    }
    if(type == 'Circle') {
      return this.api.TrimmedCurve2.circle(this.point2(source.center()), source.radius)
    }
    if(type == 'Arc') {
      return this.api.TrimmedCurve2.arcThrough(
        this.point2(source.sample(0.0)),
        this.point2(source.sample(0.5)),
        this.point2(source.sample(1.0)),
      )
    }
    if(type == 'Spline') return this.splineGeometry(source)
    if(type == 'Offset Curve') return this.offsetGeometry(source)
    unsupported(`${type.toLowerCase()} profile curves`)
  }

  splineGeometry(source) {
    // Profile splitting stores only endpoints on the SketchElement. Transfer
    // the actual B-spline poles and trim, including reversed profile edges.
    const geom = source.geom().get()
    let basis = geom
    const oc = window.oc.oc
    while(basis instanceof oc.Geom2d_TrimmedCurve) basis = basis.BasisCurve().get()
    const knots = []
    for(let i = 1; i <= basis.NbKnots(); i++) {
      knots.push(...Array(basis.Multiplicity(i)).fill(basis.Knot(i)))
    }
    const points = [], weights = []
    for(let i = 1; i <= basis.NbPoles(); i++) {
      const point = basis.Pole(i)
      points.push(this.api.Point2.new(point.X(), point.Y()))
      weights.push(basis.Weight(i))
      point.delete()
    }
    return this.api.TrimmedCurve2.new(
      this.api.NurbsCurve2.new(basis.Degree(), points, weights, knots),
      this.api.ParameterRange.new(geom.FirstParameter(), geom.LastParameter()),
    )
  }

  offsetGeometry(source) {
    const oc = window.oc.oc
    let basis = source.geom().get()
    while(basis instanceof oc.Geom2d_TrimmedCurve || basis instanceof oc.Geom2d_OffsetCurve) {
      basis = basis.BasisCurve().get()
    }
    if(basis instanceof oc.Geom2d_Line) {
      return this.api.TrimmedCurve2.line(...source.endpoints().map(point => this.point2(point)))
    }
    if(basis instanceof oc.Geom2d_Circle) {
      const arc = this.api.TrimmedCurve2.arcThrough(
        this.point2(source.sample(0)), this.point2(source.sample(0.25)), this.point2(source.sample(0.5)),
      )
      return this.api.TrimmedCurve2.new(arc.curve(), this.api.ParameterRange.new(
        arc.range().start, arc.range().start + 2 * (arc.range().end - arc.range().start),
      ))
    }
    // An offset of a spline is generally not rational. Approximate its exact
    // OC evaluator with adaptive cubic Hermite spans at model tolerance, as
    // one NURBS edge (not a faceted wire). Endpoints remain exact.
    const geom = source.geom().get()
    const start = geom.FirstParameter(), end = geom.LastParameter()
    const at = u => {
      const point = geom.Value(u), tangent = geom.DN(u, 1)
      const result = { p: new THREE.Vector2(point.X(), point.Y()), d: new THREE.Vector2(tangent.X(), tangent.Y()) }
      point.delete()
      tangent.delete()
      return result
    }
    const spans = []
    const fit = (a, b, left, right, depth) => {
      const controls = [left.p, left.p.clone().addScaledVector(left.d, (b - a) / 3),
        right.p.clone().addScaledVector(right.d, -(b - a) / 3), right.p]
      const accurate = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875].every(t => {
        const s = 1 - t
        const point = controls[0].clone().multiplyScalar(s ** 3)
          .addScaledVector(controls[1], 3 * s * s * t)
          .addScaledVector(controls[2], 3 * s * t * t)
          .addScaledVector(controls[3], t ** 3)
        return point.distanceTo(at(a + t * (b - a)).p) <= this.tolerance.linear * 0.25
      })
      if(accurate) { spans.push({ b, controls }); return }
      if(depth >= 20) throw new Error('Offset curve could not be fitted within model tolerance')
      const middle = (a + b) / 2, sample = at(middle)
      fit(a, middle, left, sample, depth + 1)
      fit(middle, b, sample, right, depth + 1)
    }
    // Start at each basis knot so short spans cannot hide between fit probes.
    const breaks = [start]
    if(basis instanceof oc.Geom2d_BSplineCurve) {
      for(let i = 1; i <= basis.NbKnots(); i++) {
        const knot = basis.Knot(i)
        if(knot > start && knot < end) breaks.push(knot)
      }
    }
    breaks.push(end)
    for(let i = 1; i < breaks.length; i++) {
      fit(breaks[i - 1], breaks[i], at(breaks[i - 1]), at(breaks[i]), 0)
    }
    const controls = [spans[0].controls[0]], knots = Array(4).fill(start)
    spans.forEach((span, index) => {
      controls.push(...span.controls.slice(1))
      knots.push(...Array(index == spans.length - 1 ? 4 : 3).fill(span.b))
    })
    return this.api.TrimmedCurve2.new(
      this.api.NurbsCurve2.new(3, controls.map(point => this.point2(point)), Array(controls.length).fill(1), knots),
      this.api.ParameterRange.new(source.forward === false ? end : start, source.forward === false ? start : end),
    )
  }

  wire(source) {
    return this.api.Wire2.new(
      source.segments.map(segment => this.segment(segment)),
      this.tolerance,
    )
  }

  profile(source) {
    const rings = source.rings.map(ring => this.wire(ring))
    return this.api.Profile.new(rings[0], rings.slice(1), this.tolerance)
  }

  extrude(profile, height) {
    const frame = planeFrame(profile.sketch.workplane)
    try {
      const raw = this.profile(profile).extrude(
        this.plane(profile.sketch.workplane),
        this.vector3(frame.normal.multiplyScalar(height)),
      )
      return new ShapexGeometry(raw)
    } catch(error) {
      throw {
        type: 'error',
        msg: `Shapex extrusion failed: ${error?.message || error?.msg || error}`,
      }
    }
  }

  extrusionRange(geometry, origin, direction) {
    // This mesh only sizes a disposable prism; the Boolean uses the original
    // analytic body. Pad beyond the tessellation's chord tolerance so curved
    // extrema between samples cannot leave the end cap inside the body.
    const mesh = geometryValue(geometry).tessellate(this.tessellationOptions)
    let minimum = Infinity
    let maximum = -Infinity
    for(const point of mesh.positions) {
      const distance = new THREE.Vector3(...point.toArray()).sub(origin).dot(direction)
      minimum = Math.min(minimum, distance)
      maximum = Math.max(maximum, distance)
    }
    if(!Number.isFinite(minimum) || !Number.isFinite(maximum)) {
      throw { type: 'error', msg: 'Through all could not determine the body extent' }
    }
    const padding = Math.max(this.tessellationOptions.chordTolerance * 4,
      this.tolerance.linear * 16, (maximum - minimum) * 0.01)
    return [minimum - padding, maximum + padding]
  }

  revolve(profile, axis, angle) {
    const { origin, normal } = planeFrame(axis)
    return featureResult('revolve', () => this.profile(profile).revolve(
      this.plane(profile.sketch.workplane),
      this.api.Axis3.new(this.point3(origin), this.vector3(normal)),
      angle,
    ))
  }

  sweep(profile, rail) {
    return featureResult('sweep', () => this.profile(profile).sweep(
      this.plane(profile.sketch.workplane), this.plane(rail.sketch.workplane), this.segment(rail),
    ))
  }

  extrudeUpToFace(profile, target, offset) {
    const body = shapexValue(target.solid.geom()).toCompound()
    try {
      return featureResult('extrusion up to face', () => this.profile(profile).extrudeUpToFace(
        this.plane(profile.sketch.workplane), body, shapexValue(target.geom()), offset,
      ))
    } finally { body.free() }
  }

  revolveUpToFace(profile, axis, target, forward) {
    const { origin, normal } = planeFrame(axis)
    const body = shapexValue(target.solid.geom()).toCompound()
    try {
      return featureResult('revolution up to face', () => this.profile(profile).revolveUpToFace(
        this.plane(profile.sketch.workplane), this.api.Axis3.new(this.point3(origin), this.vector3(normal)),
        body, shapexValue(target.geom()), forward,
      ))
    } finally { body.free() }
  }

  revolveBetweenFaces(profile, axis, from, until, forward) {
    const { origin, normal } = planeFrame(axis)
    const fromBody = shapexValue(from.solid.geom()).toCompound()
    const untilBody = shapexValue(until.solid.geom()).toCompound()
    try {
      return featureResult('revolution between faces', () => this.profile(profile).revolveBetweenFaces(
        this.plane(profile.sketch.workplane), this.api.Axis3.new(this.point3(origin), this.vector3(normal)),
        fromBody, shapexValue(from.geom()), untilBody, shapexValue(until.geom()), forward,
      ))
    } finally { fromBody.free(); untilBody.free() }
  }

  loft(profiles, alignSections) {
    const [first, ...others] = profiles
    return featureResult('loft', () => this.profile(first).loft(
      this.plane(first.sketch.workplane),
      others.map(profile => ({ profile: this.profile(profile), plane: this.plane(profile.sketch.workplane) })),
      alignSections,
    ))
  }

  boolean(first, second, operation) {
    const method = { join: 'union', cut: 'difference', intersect: 'intersection' }[operation]
    try {
      return new ShapexGeometry(geometryValue(first)[method](geometryValue(second)))
    } catch(error) {
      throw {
        type: 'error',
        msg: `Shapex ${operation} failed: ${error?.message || error}`,
      }
    }
  }

  combine(first, others) {
    return new ShapexGeometry(
      geometryValue(first).combined(others.map(geometryValue))
    )
  }

  offsetFaces(geometry, selected, distance) {
    return featureResult('face offset', () => geometryValue(geometry).offsetFaces(selected.map(shapexValue), distance))
  }

  shell(geometry, selected, distance, joinType = 'rounded') {
    try {
      return new ShapexGeometry(
        geometryValue(geometry).shell(selected.map(shapexValue), distance, joinType)
      )
    } catch(error) {
      throw {
        type: 'error',
        msg: `Shapex shell failed: ${error?.message || error}`,
      }
    }
  }

  fillet(geometry, selected, radius, followTangentEdges=true) {
    try {
      const options = this.api.FilletOptions.new(radius)
        .withTangentPropagation(followTangentEdges)
      return new ShapexGeometry(
        geometryValue(geometry).filletWithOptions(selected.map(shapexValue), options)
      )
    } catch(error) {
      throw {
        type: 'error',
        msg: `Shapex fillet failed: ${error?.message || error}`,
      }
    }
  }

  chamfer(geometry, selected, distance, followTangentEdges=true, { type = 'equal', referenceFaces = [], value = distance } = {}) {
    return featureResult('chamfer', () => geometryValue(geometry).chamfer(
      selected.map(shapexValue), distance, followTangentEdges,
      { type, referenceFaces: referenceFaces.map(shapexValue), value },
    ))
  }

  draft(geometry, selected, plane, angle) {
    return featureResult('draft', () => geometryValue(geometry).draft(selected.map(shapexValue), this.plane(plane), angle))
  }

  split(geometry, plane, keep) {
    return featureResult('split', () => geometryValue(geometry).split(this.plane(plane), keep))
  }

  halfCut(geometry, plane, side) {
    return this.split(geometry, plane, side ? 'left' : 'right')
  }

  removeSolids(geometry, removed) {
    return new ShapexGeometry(
      geometryValue(geometry).withoutSolids(removed.map(shapexValue))
    )
  }

  moveFaces(geometry, selected, matrix) {
    return new ShapexGeometry(
      geometryValue(geometry).moveFaces(selected.map(shapexValue), this.transformValue(matrix))
    )
  }

  replaceFace(geometry, target, replacement) {
    return new ShapexGeometry(
      geometryValue(geometry).replaceFace(shapexValue(target), shapexValue(replacement))
    )
  }

  transform(geometry, matrix) {
    return new ShapexGeometry(
      geometryValue(geometry).transformed(this.transformValue(matrix))
    )
  }

  volume(raw) {
    return shapexValue(raw).volume()
  }

  center(raw) {
    return new THREE.Vector3(...shapexValue(raw).centroid().toArray())
  }

  entityId(raw) {
    return shapexValue(raw).id()
  }

  compoundSolids(raw) {
    return shapexValue(raw).solids().map(markRaw)
  }

  solidFaces(raw) {
    return shapexValue(raw).faces().map(markRaw)
  }

  solidEdges(raw) {
    return shapexValue(raw).edges().map(markRaw)
  }

  solidToCompound(raw) {
    return markRaw(shapexValue(raw).toCompound())
  }

  facePlane(raw) {
    const surface = shapexValue(raw).surface()
    if(surface.kind != 'plane') return
    const plane = surface.value
    const u = new THREE.Vector3(...plane.u().toArray()).normalize()
    const v = new THREE.Vector3(...plane.v().toArray())
    v.addScaledVector(u, -v.dot(u)).normalize()
    return new THREE.Matrix4()
      .makeBasis(u, v, new THREE.Vector3().crossVectors(u, v).normalize())
      .setPosition(new THREE.Vector3(...plane.origin().toArray()))
  }

  faceAxis(raw) {
    const surface = shapexValue(raw).surface()
    if(surface.kind != 'cylinder') return
    const location = new THREE.Vector3(...surface.axis.origin().toArray())
    const direction = new THREE.Vector3(...surface.axis.direction().toArray()).normalize()
    return rotationFromNormal(direction).setPosition(location)
  }

  facePoint(raw) {
    const surface = shapexValue(raw).surface()
    if(surface.kind != 'sphere') return
    return new THREE.Matrix4().setPosition(new THREE.Vector3(...surface.center.toArray()))
  }

  faceBoundaryEdgeIds(raw) {
    return shapexValue(raw).boundaryEdges().map(edge => {
      try { return edge.id() }
      finally { edge.free() }
    })
  }

  edgeAxis(raw) {
    const curve = shapexValue(raw).curve()
    if(!(curve instanceof this.api.Line3)) return
    const location = new THREE.Vector3(...curve.origin().toArray())
    const direction = new THREE.Vector3(...curve.direction().toArray()).normalize()
    return rotationFromNormal(direction).setPosition(location)
  }

  edgeConnectedFaceIds(raw) {
    return shapexValue(raw).connectedFaces().map(face => {
      try { return face.id() }
      finally { face.free() }
    })
  }

  edgeEndpoints(raw) {
    return shapexValue(raw).endpoints().map(point => new THREE.Vector3(...point.toArray()))
  }

  isSemanticEdge(compound, edge) {
    const compoundValue = shapexValue(compound)
    const edgeValue = shapexValue(edge)
    return compoundValue.isSemanticEdge(edgeValue)
  }

  creationRole(compound, face) {
    return shapexValue(compound).creationRole(shapexValue(face))
  }

  resolveFace(compound, face) {
    const resolution = shapexValue(compound).resolve(shapexValue(face))
    return {
      kind: resolution.kind,
      entityIds: resolution.entities.map(entity => {
        try { return entity.id() }
        finally { entity.free() }
      }),
    }
  }

  mesh(raw, options = {}) {
    const value = shapexValue(raw)
    const tessellationOptions = options.chordTolerance || options.angularTolerance || options.maximumDepth
      ? this.api.TessellationOptions.new(
        options.chordTolerance ?? LINEAR_TOLERANCE,
        options.angularTolerance ?? ANGULAR_TOLERANCE,
        options.maximumDepth ?? MAXIMUM_TESSELLATION_DEPTH,
      )
      : this.tessellationOptions
    return {
      ...meshFromShapex(value.tessellate(tessellationOptions)),
      chordTolerance: tessellationOptions.chordTolerance,
    }
  }

  edgePolyline(raw) {
    return shapexValue(raw).tessellate(this.tessellationOptions).points
      .map(point => new THREE.Vector3(...point.toArray()))
  }

  unsupported(operation) {
    unsupported(operation)
  }
}

export async function loadShapexBackend() {
  try {
    return new ShapexBackend(await initShapex())
  } catch(error) {
    throw new Error(
      `Could not load ShapexJS. Run npm run build-shapex before selecting Shapex. ${error}`
    )
  }
}
