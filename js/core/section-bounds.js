import * as THREE from 'three'

const meshBounds = new WeakMap()

// Keep the error allowance with the cached mesh: preferences may have changed
// since it was generated. Unknown or incomplete meshes cannot prove separation.
function faceBounds(mesh) {
  if(meshBounds.has(mesh)) return meshBounds.get(mesh)
  const { positions, chordTolerance } = mesh
  let bounds = null
  if(Number.isFinite(chordTolerance) && chordTolerance > 0 &&
    positions.length > 0 && positions.length % 3 === 0 &&
    positions.every(Number.isFinite)) {
    bounds = new THREE.Box3().setFromArray(positions)
      .expandByScalar(chordTolerance + 5e-3)
  }
  meshBounds.set(mesh, bounds)
  return bounds
}

export function sectionBounds(compound) {
  const bounds = new THREE.Box3()
  for(const solid of compound.solids()) {
    const faces = solid.faces()
    if(!faces.length) return null
    for(const face of faces) {
      const box = faceBounds(face.tesselate())
      if(!box) return null
      bounds.union(box)
    }
  }
  return bounds.isEmpty() ? null : bounds
}

export function sectionPlaneRelation(bounds, plane) {
  if(!bounds) return 0
  const origin = new THREE.Vector3().setFromMatrixPosition(plane)
  const normal = new THREE.Vector3().setFromMatrixColumn(plane, 2).normalize()
  const center = bounds.getCenter(new THREE.Vector3())
  const halfSize = bounds.getSize(new THREE.Vector3()).multiplyScalar(0.5)
  const distance = center.sub(origin).dot(normal)
  const radius = Math.abs(normal.x) * halfSize.x +
    Math.abs(normal.y) * halfSize.y + Math.abs(normal.z) * halfSize.z
  if(distance + radius < 0) return -1
  if(distance - radius > 0) return 1
  return 0
}
