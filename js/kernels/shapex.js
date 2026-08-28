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

function planeFrame(matrix) {
  const elements = matrix.elements
  const origin = new THREE.Vector3().setFromMatrixPosition(matrix)
  const u = new THREE.Vector3(elements[0], elements[1], elements[2]).normalize()
  const v = new THREE.Vector3(elements[4], elements[5], elements[6])
  v.addScaledVector(u, -v.dot(u)).normalize()
  const normal = new THREE.Vector3().crossVectors(u, v).normalize()
  return { origin, u, v, normal }
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
  if(type == 'Spline') {
    const count = segment.points.length
    const degree = Math.min(5, count - 1)
    const interior = count - degree - 1
    const end = interior + 1
    const knots = [
      ...Array(degree + 1).fill(0),
      ...Array.from({ length: interior }, (_, index) => index + 1),
      ...Array(degree + 1).fill(end),
    ]
    wire.addNurbs(
      degree,
      new Float64Array(segment.points.flatMap(point => [point.x, point.y])),
      new Float64Array(Array(count).fill(1)),
      new Float64Array(knots),
    )
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
      revolve: false,
      loft: false,
      fillet: false,
      offset: false,
      split: false,
      compoundMerge: true,
      multiSolidBoolean: true,
    })
  }

  extrude(profile, height) {
    const built = profile.rings.map(ring => buildWire(this.api, ring))
    const kernelProfile = new this.api.Profile(built[0])
    built.slice(1).forEach(hole => kernelProfile.addHole(hole))

    const { origin, u, v, normal } = planeFrame(profile.sketch.workplane)
    const vector = normal.multiplyScalar(height)
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

  revolve() {
    unsupported('analytical revolutions yet')
  }

  loft() {
    unsupported('analytical lofts yet')
  }

  boolean(first, second, operation) {
    try {
      return new ShapexGeometry(first.raw.boolean(second.raw, operation))
    } catch(error) {
      throw {
        type: 'error',
        msg: `Shapex ${operation} failed: ${error?.message || error}`,
      }
    }
  }

  combine(first, others) {
    return others.reduce(
      (combined, other) => new ShapexGeometry(combined.raw.combined(other.raw)),
      first,
    )
  }

  shell() {
    unsupported('analytical shelling yet')
  }

  fillet() {
    unsupported('analytical fillets yet')
  }

  split() {
    unsupported('analytical splitting yet')
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
