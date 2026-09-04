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
      boolean: true,
      transform: true,
      tessellate: true,
      revolve: false,
      loft: false,
      fillet: true,
      offset: true,
      split: false,
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
    if(type == 'Spline') {
      const count = source.points.length
      const degree = Math.min(5, count - 1)
      const interior = count - degree - 1
      const end = interior + 1
      const knots = [
        ...Array(degree + 1).fill(0),
        ...Array.from({ length: interior }, (_, index) => index + 1),
        ...Array(degree + 1).fill(end),
      ]
      return this.api.TrimmedCurve2.new(
        this.api.NurbsCurve2.new(
          degree,
          source.points.map(point => this.point2(point)),
          Array(count).fill(1),
          knots,
        ),
        this.api.ParameterRange.new(knots[degree], knots[count]),
      )
    }
    unsupported(`${type.toLowerCase()} profile curves`)
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
        msg: `Shapex extrusion failed: ${error?.message || error}`,
      }
    }
  }

  revolve() {
    unsupported('analytical revolutions yet')
  }

  loft() {
    unsupported('analytical lofts yet')
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

  shell(geometry, selected, distance) {
    try {
      return new ShapexGeometry(
        geometryValue(geometry).shell(selected.map(shapexValue), distance)
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

  split() {
    unsupported('analytical splitting yet')
  }

  halfCut(geometry, plane, side) {
    const halfSize = 500
    const height = 1000
    const wire = this.api.Wire2.polygon([
      this.api.Point2.new(-halfSize, -halfSize),
      this.api.Point2.new(halfSize, -halfSize),
      this.api.Point2.new(halfSize, halfSize),
      this.api.Point2.new(-halfSize, halfSize),
    ], this.tolerance)
    const profile = this.api.Profile.new(wire, [], this.tolerance)
    const { normal } = planeFrame(plane)
    const cutter = profile.extrude(
      this.plane(plane),
      this.vector3(normal.multiplyScalar(side ? height : -height)),
    )
    try {
      return new ShapexGeometry(geometryValue(geometry).difference(cutter))
    } finally {
      cutter.free()
    }
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
    return meshFromShapex(value.tessellate(tessellationOptions))
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
