import * as THREE from 'three'


export function voxelizeSolid(solid, resolution=14, maxCells=Infinity, targetSteps=null, onProgress=() => {}) {
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
  const coordinates = []

  // Cast one +x ray per (j, k) column of cell centers and fill between
  // sorted surface crossings instead of ray casting every cell.
  const columns = Array.from({ length: ny * nz }, () => [])
  for(const { a, b, c } of triangles) {
    const normal = new THREE.Vector3().crossVectors(b.clone().sub(a), c.clone().sub(a))
    if(!normal.x) continue
    const positive = normal.x > 0
    const [jLow, jHigh] = latticeRange(Math.min(a.y, b.y, c.y), Math.max(a.y, b.y, c.y), bounds.min.y + steps[1] / 2, steps[1], ny)
    const [kLow, kHigh] = latticeRange(Math.min(a.z, b.z, c.z), Math.max(a.z, b.z, c.z), bounds.min.z + steps[2] / 2, steps[2], nz)
    for(let k = kLow; k <= kHigh; k++) {
      const z = bounds.min.z + (k + 0.5) * steps[2]
      for(let j = jLow; j <= jHigh; j++) {
        const y = bounds.min.y + (j + 0.5) * steps[1]
        if(edgeSide(a, b, y, z) != positive || edgeSide(b, c, y, z) != positive || edgeSide(c, a, y, z) != positive) continue
        columns[j + ny * k].push(a.x - (normal.y * (y - a.y) + normal.z * (z - a.z)) / normal.x)
      }
    }
  }

  for(let k = 0; k < nz; k++) {
    onProgress(k / nz)
    for(let j = 0; j < ny; j++) {
      const crossings = columns[j + ny * k].sort((a, b) => a - b)
      let crossed = 0
      for(let i = 0; i < nx; i++) {
        const x = bounds.min.x + (i + 0.5) * steps[0]
        while(crossed < crossings.length && crossings[crossed] < x) crossed++
        if(crossed % 2 == 0) continue
        grid[i + nx * (j + ny * k)] = points.length
        points.push(new THREE.Vector3(x, bounds.min.y + (j + 0.5) * steps[1], bounds.min.z + (k + 0.5) * steps[2]))
        coordinates.push(i, j, k)
      }
    }
  }

  onProgress(1)
  return { bounds, size, step, steps, nx, ny, nz, grid, points, coordinates: Int32Array.from(coordinates) }
}


// Fixes every node whose connected component holds no fixed node, so floating
// voxel islands keep their initial value instead of leaving the solve singular.
export function fixFloatingNodes(fixed, connect) {
  const parents = Int32Array.from(fixed, (_, node) => node)
  const find = node => {
    while(parents[node] != node) node = parents[node] = parents[parents[node]]
    return node
  }
  connect((a, b) => parents[find(a)] = find(b))
  const anchored = new Uint8Array(fixed.length)
  fixed.forEach((value, node) => { if(value) anchored[find(node)] = 1 })
  fixed.forEach((_, node) => { if(!anchored[find(node)]) fixed[node] = 1 })
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


// Visits lattice points origin + index * steps within distance of the
// triangle's bounding box, so callers only test nearby candidates.
export function forLatticeNear(triangle, distance, origin, steps, dimensions, visit) {
  const ranges = [0, 1, 2].map(axis => {
    const values = [triangle.a, triangle.b, triangle.c].map(point => point.getComponent(axis))
    return latticeRange(Math.min(...values) - distance, Math.max(...values) + distance, origin[axis], steps[axis], dimensions[axis])
  })
  for(let k = ranges[2][0]; k <= ranges[2][1]; k++) {
    for(let j = ranges[1][0]; j <= ranges[1][1]; j++) {
      for(let i = ranges[0][0]; i <= ranges[0][1]; i++) visit(i, j, k)
    }
  }
}


function latticeRange(low, high, origin, step, count) {
  return [
    Math.max(0, Math.ceil((low - origin) / step)),
    Math.min(count - 1, Math.floor((high - origin) / step)),
  ]
}


// Side of a projected (y, z) point relative to edge pq. Evaluating every edge in
// one canonical direction gives neighbors sharing an edge exactly opposite answers,
// so a ray through a shared edge counts exactly one of the two triangles.
function edgeSide(p, q, y, z) {
  const flip = p.y > q.y || (p.y == q.y && p.z > q.z)
  if(flip) [p, q] = [q, p]
  return ((q.y - p.y) * (z - p.z) - (q.z - p.z) * (y - p.y) >= 0) != flip
}
