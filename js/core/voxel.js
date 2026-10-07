import * as THREE from 'three'


export function voxelizeSolid(solid, resolution=14, maxCells=Infinity, targetSteps=null) {
  const triangles = makeTriangles(solid.faces())
  const bounds = new THREE.Box3()
  triangles.forEach(triangle => {
    bounds.expandByPoint(triangle.a)
    bounds.expandByPoint(triangle.b)
    bounds.expandByPoint(triangle.c)
  })

  const size = bounds.getSize(new THREE.Vector3())
  const baseStep = Math.max(size.x, size.y, size.z) / resolution
  const steps = [0, 1, 2].map(axis => Math.min(baseStep, targetSteps?.[axis] ?? Infinity))
  while(Math.ceil(size.x / steps[0]) * Math.ceil(size.y / steps[1]) * Math.ceil(size.z / steps[2]) > maxCells) {
    for(let axis = 0; axis < 3; axis++) steps[axis] *= 1.05
  }
  const step = Math.max(...steps)
  const nx = Math.max(1, Math.ceil(size.x / steps[0]))
  const ny = Math.max(1, Math.ceil(size.y / steps[1]))
  const nz = Math.max(1, Math.ceil(size.z / steps[2]))
  const grid = new Int32Array(nx * ny * nz).fill(-1)
  const points = []

  for(let k = 0; k < nz; k++) {
    for(let j = 0; j < ny; j++) {
      for(let i = 0; i < nx; i++) {
        const point = new THREE.Vector3(
          bounds.min.x + (i + 0.5) * steps[0],
          bounds.min.y + (j + 0.5) * steps[1],
          bounds.min.z + (k + 0.5) * steps[2],
        )
        if(!isInside(point, triangles)) continue
        grid[i + nx * (j + ny * k)] = points.length
        points.push(point)
      }
    }
  }

  return { bounds, size, step, steps, nx, ny, nz, grid, points }
}


export function findVoxelNode(result, position) {
  const steps = result.steps || [result.step, result.step, result.step]
  const i = Math.floor((position.x - result.min[0]) / steps[0])
  const j = Math.floor((position.y - result.min[1]) / steps[1])
  const k = Math.floor((position.z - result.min[2]) / steps[2])

  for(let radius = 0; radius < 3; radius++) {
    for(let z = k - radius; z <= k + radius; z++) {
      for(let y = j - radius; y <= j + radius; y++) {
        for(let x = i - radius; x <= i + radius; x++) {
          if(x < 0 || y < 0 || z < 0 || x >= result.nx || y >= result.ny || z >= result.nz) continue
          const node = result.grid[x + result.nx * (y + result.ny * z)]
          if(node >= 0) return node
        }
      }
    }
  }
}


export function makeTriangles(faces) {
  return faces.flatMap(face => {
    const positions = face.tesselate().positions
    const triangles = []
    for(let i = 0; i < positions.length; i += 9) {
      triangles.push(new THREE.Triangle(
        new THREE.Vector3(...positions.slice(i, i + 3)),
        new THREE.Vector3(...positions.slice(i + 3, i + 6)),
        new THREE.Vector3(...positions.slice(i + 6, i + 9)),
      ))
    }
    return triangles
  })
}


function isInside(point, triangles) {
  let intersections = 0
  triangles.forEach(triangle => {
    if(rayIntersectsTriangle(point, triangle)) intersections++
  })
  return intersections % 2 == 1
}


function rayIntersectsTriangle(origin, triangle) {
  const dx = 1
  const dy = 0.371390676
  const dz = 0.557086014
  const e1x = triangle.b.x - triangle.a.x
  const e1y = triangle.b.y - triangle.a.y
  const e1z = triangle.b.z - triangle.a.z
  const e2x = triangle.c.x - triangle.a.x
  const e2y = triangle.c.y - triangle.a.y
  const e2z = triangle.c.z - triangle.a.z
  const px = dy * e2z - dz * e2y
  const py = dz * e2x - dx * e2z
  const pz = dx * e2y - dy * e2x
  const det = e1x * px + e1y * py + e1z * pz
  if(Math.abs(det) < 1e-10) return false
  const inverse = 1 / det
  const tx = origin.x - triangle.a.x
  const ty = origin.y - triangle.a.y
  const tz = origin.z - triangle.a.z
  const u = (tx * px + ty * py + tz * pz) * inverse
  if(u < 0 || u > 1) return false
  const qx = ty * e1z - tz * e1y
  const qy = tz * e1x - tx * e1z
  const qz = tx * e1y - ty * e1x
  const v = (dx * qx + dy * qy + dz * qz) * inverse
  if(v < 0 || u + v > 1) return false
  return (e2x * qx + e2y * qy + e2z * qz) * inverse > 1e-8
}
