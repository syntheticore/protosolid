import * as THREE from 'three'

const LINEAR_TOLERANCE = 0.1
const ANGULAR_TOLERANCE = 0.1

function unsupported(operation) {
  throw {
    type: 'error',
    msg: `Shapex does not support ${operation} yet`,
    kernel: 'shapex',
    capability: operation,
  }
}

function toArray(values) {
  return Array.from(values)
}

function meshFromBridge(mesh) {
  return {
    positions: toArray(mesh.positions),
    normals: toArray(mesh.normals),
  }
}

function addSegment(wire, segment) {
  const type = segment.typename().replace(' Projection', '')
  if(type == 'Line') {
    const [start, end] = segment.endpoints()
    wire.addLine(start.x, start.y, end.x, end.y)
    return
  }
  if(type == 'Circle') {
    const center = segment.center()
    wire.addCircle(center.x, center.y, segment.radius)
    return
  }
  if(type == 'Arc') {
    const start = segment.sample(0.0)
    const middle = segment.sample(0.5)
    const end = segment.sample(1.0)
    wire.addArcThrough(start.x, start.y, middle.x, middle.y, end.x, end.y)
    return
  }
  unsupported(`${type.toLowerCase()} profile curves`)
}

function buildWire(api, source) {
  const wire = new api.Wire2()
  source.segments.forEach(segment => addSegment(wire, segment))
  return wire
}

export class ShapexGeometry {
  constructor(raw) {
    this.kernel = 'shapex'
    this.raw = raw
  }
}

export class ShapexBackend {
  constructor(api) {
    this.id = 'shapex'
    this.title = 'Shapex'
    this.api = api
    this.capabilities = Object.freeze({
      extrude: true,
      boolean: true,
      transform: true,
      tessellate: true,
      revolve: true,
      loft: true,
      fillet: true,
      offset: true,
      split: true,
      compoundMerge: true,
      multiSolidBoolean: true,
    })
  }

  extrude(profile, height) {
    const built = profile.rings.map(ring => buildWire(this.api, ring))
    const kernelProfile = new this.api.Profile(built[0])
    built.slice(1).forEach(hole => kernelProfile.addHole(hole))

    const origin = new THREE.Vector3().setFromMatrixPosition(profile.sketch.workplane)
    const rotation = new THREE.Quaternion().setFromRotationMatrix(profile.sketch.workplane)
    const u = new THREE.Vector3(1, 0, 0).applyQuaternion(rotation)
    const v = new THREE.Vector3(0, 1, 0).applyQuaternion(rotation)
    const vector = new THREE.Vector3(0, 0, height).applyQuaternion(rotation)
    let raw
    try {
      raw = kernelProfile.extrude(
        origin.x, origin.y, origin.z,
        u.x, u.y, u.z,
        v.x, v.y, v.z,
        vector.x, vector.y, vector.z,
      )
    } catch(error) {
      throw {
        type: 'error',
        msg: `Shapex extrusion failed: ${error?.message || error}`,
      }
    }
    return new ShapexGeometry(raw)
  }

  revolve(profile, axis, angle) {
    const built = profile.rings.map(ring => buildWire(this.api, ring))
    const kernelProfile = new this.api.Profile(built[0])
    built.slice(1).forEach(hole => kernelProfile.addHole(hole))
    const profileOrigin = new THREE.Vector3().setFromMatrixPosition(profile.sketch.workplane)
    const profileRotation = new THREE.Quaternion().setFromRotationMatrix(profile.sketch.workplane)
    const u = new THREE.Vector3(1, 0, 0).applyQuaternion(profileRotation)
    const v = new THREE.Vector3(0, 1, 0).applyQuaternion(profileRotation)
    const axisOrigin = new THREE.Vector3().setFromMatrixPosition(axis)
    const axisDirection = new THREE.Vector3(0, 0, 1)
      .applyQuaternion(new THREE.Quaternion().setFromRotationMatrix(axis))
    try {
      return new ShapexGeometry(kernelProfile.revolve(
        profileOrigin.x, profileOrigin.y, profileOrigin.z,
        u.x, u.y, u.z,
        v.x, v.y, v.z,
        axisOrigin.x, axisOrigin.y, axisOrigin.z,
        axisDirection.x, axisDirection.y, axisDirection.z,
        angle,
      ))
    } catch(error) {
      throw { type: 'error', msg: `Shapex revolution failed: ${error?.message || error}` }
    }
  }

  loft(profiles, _untwist) {
    const loft = new this.api.Loft()
    for(const profile of profiles) {
      const built = profile.rings.map(ring => buildWire(this.api, ring))
      const kernelProfile = new this.api.Profile(built[0])
      built.slice(1).forEach(hole => kernelProfile.addHole(hole))
      const origin = new THREE.Vector3().setFromMatrixPosition(profile.sketch.workplane)
      const rotation = new THREE.Quaternion().setFromRotationMatrix(profile.sketch.workplane)
      const u = new THREE.Vector3(1, 0, 0).applyQuaternion(rotation)
      const v = new THREE.Vector3(0, 1, 0).applyQuaternion(rotation)
      loft.addProfile(
        kernelProfile,
        origin.x, origin.y, origin.z,
        u.x, u.y, u.z,
        v.x, v.y, v.z,
      )
    }
    try {
      return new ShapexGeometry(loft.build(_untwist))
    } catch(error) {
      throw { type: 'error', msg: `Shapex loft failed: ${error?.message || error}` }
    }
  }

  boolean(first, second, operation) {
    return new ShapexGeometry(first.raw.boolean(second.raw, operation))
  }

  combine(first, others) {
    return others.reduce(
      (combined, other) => new ShapexGeometry(combined.raw.combined(other.raw)),
      first,
    )
  }

  shell(geometry, openings, distance) {
    const faces = new this.api.FaceList()
    openings.forEach(face => faces.add(face))
    try {
      return new ShapexGeometry(geometry.raw.shell(faces, distance))
    } catch(error) {
      throw { type: 'error', msg: `Shapex shell failed: ${error?.message || error}` }
    }
  }

  fillet(geometry, selected, radius) {
    const edges = new this.api.EdgeList()
    selected.forEach(edge => edges.add(edge))
    try {
      return new ShapexGeometry(geometry.raw.fillet(edges, radius))
    } catch(error) {
      throw { type: 'error', msg: `Shapex fillet failed: ${error?.message || error}` }
    }
  }

  split(geometry, plane, keep) {
    const origin = new THREE.Vector3().setFromMatrixPosition(plane)
    const rotation = new THREE.Quaternion().setFromRotationMatrix(plane)
    const u = new THREE.Vector3(1, 0, 0).applyQuaternion(rotation)
    const v = new THREE.Vector3(0, 1, 0).applyQuaternion(rotation)
    try {
      return new ShapexGeometry(geometry.raw.split(
        origin.x, origin.y, origin.z,
        u.x, u.y, u.z,
        v.x, v.y, v.z,
        keep,
      ))
    } catch(error) {
      throw { type: 'error', msg: `Shapex split failed: ${error?.message || error}` }
    }
  }

  removeSolids(geometry, removed) {
    const solids = new this.api.SolidList()
    removed.forEach(solid => solids.add(solid))
    return new ShapexGeometry(geometry.raw.withoutSolids(solids))
  }

  moveFaces(geometry, selected, matrix) {
    const faces = new this.api.FaceList()
    selected.forEach(face => faces.add(face))
    const position = new THREE.Vector3().setFromMatrixPosition(matrix)
    const rotation = new THREE.Quaternion().setFromRotationMatrix(matrix)
    const x = new THREE.Vector3(1, 0, 0).applyQuaternion(rotation)
    const y = new THREE.Vector3(0, 1, 0).applyQuaternion(rotation)
    const z = new THREE.Vector3(0, 0, 1).applyQuaternion(rotation)
    return new ShapexGeometry(geometry.raw.moveFaces(
      faces,
      x.x, x.y, x.z,
      y.x, y.y, y.z,
      z.x, z.y, z.z,
      position.x, position.y, position.z,
    ))
  }

  replaceFace(geometry, target, replacement) {
    return new ShapexGeometry(geometry.raw.replaceFace(target, replacement))
  }

  transform(geometry, matrix) {
    const position = new THREE.Vector3().setFromMatrixPosition(matrix)
    const rotation = new THREE.Quaternion().setFromRotationMatrix(matrix)
    const x = new THREE.Vector3(1, 0, 0).applyQuaternion(rotation)
    const y = new THREE.Vector3(0, 1, 0).applyQuaternion(rotation)
    const z = new THREE.Vector3(0, 0, 1).applyQuaternion(rotation)
    return new ShapexGeometry(geometry.raw.transformed(
      x.x, x.y, x.z,
      y.x, y.y, y.z,
      z.x, z.y, z.z,
      position.x, position.y, position.z,
    ))
  }

  mesh(raw) {
    return meshFromBridge(raw.tessellate(LINEAR_TOLERANCE, ANGULAR_TOLERANCE))
  }

  edgePolyline(raw) {
    const values = toArray(raw.tessellate(LINEAR_TOLERANCE, ANGULAR_TOLERANCE))
    const points = []
    for(let index = 0; index < values.length; index += 3) {
      points.push(new THREE.Vector3(values[index], values[index + 1], values[index + 2]))
    }
    return points
  }

  unsupported(operation) {
    unsupported(operation)
  }
}

export async function loadShapexBackend() {
  const moduleUrl = new URL('kernels/shapex/shapex_wasm.js', document.baseURI).href
  const wasmUrl = new URL('kernels/shapex/shapex_wasm_bg.wasm', document.baseURI).href
  let api
  try {
    api = await import(/* @vite-ignore */ moduleUrl)
    await api.default({ module_or_path: wasmUrl })
  } catch(error) {
    throw new Error(
      `Could not load the Shapex bridge. Run npm run build-shapex before selecting Shapex. ${error}`
    )
  }
  return new ShapexBackend(api)
}
