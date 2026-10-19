import * as THREE from 'three'
import initShapex from '../shapexJS/index.js'

export let shapex
export let modelTolerance
export let tessellationOptions

export async function initializeShapex(options) {
  shapex = await initShapex(options)
  modelTolerance = shapex.ModelTolerance.default()
  tessellationOptions = shapex.TessellationOptions.new(0.1, 0.1, 24)
}

export const point2 = point => shapex.Point2.new(point.x, point.y)
export const point3 = point => shapex.Point3.new(point.x, point.y, point.z)
export const vector3 = vector => shapex.Vec3.new(vector.x, vector.y, vector.z)
export const threeVector = point => new THREE.Vector3(...point.toArray())

export function planeFromMatrix(matrix) {
  const elements = matrix.elements
  const origin = new THREE.Vector3().setFromMatrixPosition(matrix)
  const u = new THREE.Vector3(elements[0], elements[1], elements[2]).normalize()
  const v = new THREE.Vector3(elements[4], elements[5], elements[6])
  v.addScaledVector(u, -v.dot(u)).normalize()
  return shapex.Plane3.new(point3(origin), vector3(u), vector3(v), modelTolerance)
}

export function axisFromMatrix(matrix) {
  const plane = planeFromMatrix(matrix)
  return shapex.Axis3.new(plane.origin(), plane.normal())
}

export function transformFromMatrix(matrix) {
  const position = new THREE.Vector3().setFromMatrixPosition(matrix)
  const rotation = new THREE.Quaternion().setFromRotationMatrix(matrix)
  const x = new THREE.Vector3(1, 0, 0).applyQuaternion(rotation)
  const y = new THREE.Vector3(0, 1, 0).applyQuaternion(rotation)
  const z = new THREE.Vector3(0, 0, 1).applyQuaternion(rotation)
  return shapex.RigidTransform3.new(
    shapex.Matrix3.fromCols(vector3(x), vector3(y), vector3(z)),
    vector3(position), modelTolerance,
  )
}

// Consume temporary topology handles when only their IDs are needed.
export function consumeEntityIds(entities) {
  return entities.map(entity => {
    try { return entity.id() }
    finally { entity.free() }
  })
}

// Three.js displays and exporters use flat triangle buffers.
export function tessellateMesh(geometry, options = {}) {
  const settings = Object.keys(options).length ? shapex.TessellationOptions.new(
    options.chordTolerance ?? tessellationOptions.chordTolerance,
    options.angularTolerance ?? tessellationOptions.angularTolerance,
    options.maximumDepth ?? tessellationOptions.maximumDepth,
  ) : tessellationOptions
  const mesh = geometry.tessellate(settings)
  const positions = []
  const normals = []
  for(const triangle of mesh.triangles) for(const index of triangle) {
    positions.push(...mesh.positions[index].toArray())
    normals.push(...mesh.normals[index].toArray())
  }
  return { positions, normals, chordTolerance: settings.chordTolerance }
}
