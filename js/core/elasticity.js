import * as THREE from 'three'

import { makeTriangles, voxelizeSolid } from './voxel.js'


const tetraCorners = [
  [0, 1, 2, 6],
  [0, 2, 3, 6],
  [0, 3, 7, 6],
  [0, 7, 4, 6],
  [0, 4, 5, 6],
  [0, 5, 1, 6],
]
const cubeCorners = [
  [0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0],
  [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1],
]


function tetraTemplates(steps) {
  const corners = cubeCorners.map(([x, y, z]) =>
    new THREE.Vector3(x * steps[0], y * steps[1], z * steps[2]))
  return tetraCorners.map(indices => {
    const [a, b, c, d] = indices.map(index => corners[index])
    const e1 = b.clone().sub(a)
    const e2 = c.clone().sub(a)
    const e3 = d.clone().sub(a)
    const determinant = e1.dot(new THREE.Vector3().crossVectors(e2, e3))
    const gradients = [
      new THREE.Vector3(),
      new THREE.Vector3().crossVectors(e2, e3).divideScalar(determinant),
      new THREE.Vector3().crossVectors(e3, e1).divideScalar(determinant),
      new THREE.Vector3().crossVectors(e1, e2).divideScalar(determinant),
    ]
    gradients[0].copy(gradients[1]).add(gradients[2]).add(gradients[3]).negate()
    return { indices, gradients: gradients.flatMap(gradient => gradient.toArray()), volume: Math.abs(determinant) / 6 }
  })
}


function strainAndStress(input, nodes, offset, template, lambda, mu) {
  const { indices, gradients } = template
  let uxx = 0, uxy = 0, uxz = 0
  let uyx = 0, uyy = 0, uyz = 0
  let uzx = 0, uzy = 0, uzz = 0
  for(let corner = 0; corner < 4; corner++) {
    const node = nodes[offset + indices[corner]] * 3
    const gx = gradients[corner * 3]
    const gy = gradients[corner * 3 + 1]
    const gz = gradients[corner * 3 + 2]
    uxx += input[node] * gx
    uxy += input[node] * gy
    uxz += input[node] * gz
    uyx += input[node + 1] * gx
    uyy += input[node + 1] * gy
    uyz += input[node + 1] * gz
    uzx += input[node + 2] * gx
    uzy += input[node + 2] * gy
    uzz += input[node + 2] * gz
  }
  const trace = uxx + uyy + uzz
  return [
    lambda * trace + 2 * mu * uxx,
    lambda * trace + 2 * mu * uyy,
    lambda * trace + 2 * mu * uzz,
    mu * (uxy + uyx),
    mu * (uyz + uzy),
    mu * (uxz + uzx),
  ]
}


function solveConjugateGradient(apply, right, diagonal, maxIterations) {
  const displacement = new Float64Array(right.length)
  const residual = right.slice()
  const direction = new Float64Array(right.length)
  let rz = 0
  let initialSquared = 0
  for(let i = 0; i < right.length; i++) {
    direction[i] = residual[i] / diagonal[i]
    rz += residual[i] * direction[i]
    initialSquared += residual[i] * residual[i]
  }
  if(!initialSquared) return { displacement, relativeResidual: 0 }
  const target = initialSquared * 1e-8
  let residualSquared = initialSquared
  for(let iteration = 0; iteration < maxIterations && residualSquared > target; iteration++) {
    const applied = apply(direction)
    let product = 0
    for(let i = 0; i < right.length; i++) product += direction[i] * applied[i]
    if(!(product > 0)) break
    const alpha = rz / product
    residualSquared = 0
    let nextRz = 0
    for(let i = 0; i < right.length; i++) {
      displacement[i] += alpha * direction[i]
      residual[i] -= alpha * applied[i]
      residualSquared += residual[i] * residual[i]
      nextRz += residual[i] * residual[i] / diagonal[i]
    }
    const beta = nextRz / rz
    for(let i = 0; i < right.length; i++) direction[i] = residual[i] / diagonal[i] + beta * direction[i]
    rz = nextRz
  }
  const applied = apply(displacement)
  residualSquared = 0
  for(let i = 0; i < right.length; i++) residualSquared += (right[i] - applied[i]) ** 2
  return { displacement, relativeResidual: Math.sqrt(residualSquared / initialSquared) }
}


export function solveElasticSolid(solid, fixedFaces, loadFaces, force, material, resolution, maxCells, targetSteps) {
  const { bounds, size, step, steps, nx, ny, nz, grid: cells } =
    voxelizeSolid(solid, resolution, maxCells, targetSteps)
  const nodeDimensions = [nx + 1, ny + 1, nz + 1]
  const nodeGrid = new Int32Array(nodeDimensions[0] * nodeDimensions[1] * nodeDimensions[2]).fill(-1)
  const positions = []
  const elements = []
  const nodeAt = (i, j, k) => {
    const index = i + nodeDimensions[0] * (j + nodeDimensions[1] * k)
    if(nodeGrid[index] >= 0) return nodeGrid[index]
    const node = positions.length
    nodeGrid[index] = node
    positions.push(new THREE.Vector3(
      bounds.min.x + i * steps[0], bounds.min.y + j * steps[1], bounds.min.z + k * steps[2],
    ))
    return node
  }
  for(let k = 0; k < nz; k++) {
    for(let j = 0; j < ny; j++) {
      for(let i = 0; i < nx; i++) {
        if(cells[i + nx * (j + ny * k)] < 0) continue
        for(const [di, dj, dk] of cubeCorners) elements.push(nodeAt(i + di, j + dj, k + dk))
      }
    }
  }
  if(!elements.length) return

  const fixedTriangles = makeTriangles(fixedFaces)
  const loadTriangles = makeTriangles(loadFaces)
  const boundary = triangles => triangles.map(triangle => {
    const normal = triangle.getNormal(new THREE.Vector3())
    const distance = 0.6 * Math.hypot(normal.x * steps[0], normal.y * steps[1], normal.z * steps[2])
    return { triangle, limit: distance * distance }
  })
  const fixedBoundary = boundary(fixedTriangles)
  const loadBoundary = boundary(loadTriangles)
  const isSurfaceNode = (i, j, k) => {
    for(let dk = -1; dk <= 0; dk++) {
      for(let dj = -1; dj <= 0; dj++) {
        for(let di = -1; di <= 0; di++) {
          const x = i + di, y = j + dj, z = k + dk
          if(x < 0 || y < 0 || z < 0 || x >= nx || y >= ny || z >= nz ||
            cells[x + nx * (y + ny * z)] < 0) return true
        }
      }
    }
    return false
  }
  const fixed = new Uint8Array(positions.length)
  const loaded = []
  const closest = new THREE.Vector3()
  for(let k = 0; k < nodeDimensions[2]; k++) {
    for(let j = 0; j < nodeDimensions[1]; j++) {
      for(let i = 0; i < nodeDimensions[0]; i++) {
        const node = nodeGrid[i + nodeDimensions[0] * (j + nodeDimensions[1] * k)]
        if(node < 0 || !isSurfaceNode(i, j, k)) continue
        const point = positions[node]
        if(fixedBoundary.some(({ triangle, limit }) => triangle.closestPointToPoint(point, closest).distanceToSquared(point) <= limit)) fixed[node] = 1
        if(loadBoundary.some(({ triangle, limit }) => triangle.closestPointToPoint(point, closest).distanceToSquared(point) <= limit)) loaded.push(node)
      }
    }
  }
  const freeLoaded = loaded.filter(node => !fixed[node])
  if(!fixed.some(Boolean) || !freeLoaded.length) return

  const young = material.elasticModulus
  const poisson = material.poissonRatio
  const mu = young / (2 * (1 + poisson))
  const lambda = young * poisson / ((1 + poisson) * (1 - 2 * poisson))
  const templates = tetraTemplates(steps)
  const nodeIndices = Int32Array.from(elements)
  const diagonal = new Float64Array(positions.length * 3)
  for(let offset = 0; offset < nodeIndices.length; offset += 8) {
    for(const { indices, gradients, volume } of templates) {
      for(let corner = 0; corner < 4; corner++) {
        const node = nodeIndices[offset + indices[corner]] * 3
        const gx = gradients[corner * 3], gy = gradients[corner * 3 + 1], gz = gradients[corner * 3 + 2]
        const squared = gx * gx + gy * gy + gz * gz
        diagonal[node] += volume * (mu * squared + (lambda + mu) * gx * gx)
        diagonal[node + 1] += volume * (mu * squared + (lambda + mu) * gy * gy)
        diagonal[node + 2] += volume * (mu * squared + (lambda + mu) * gz * gz)
      }
    }
  }
  const right = new Float64Array(diagonal.length)
  for(const node of freeLoaded) {
    right[node * 3] += force.x / freeLoaded.length
    right[node * 3 + 1] += force.y / freeLoaded.length
    right[node * 3 + 2] += force.z / freeLoaded.length
  }
  for(let node = 0; node < fixed.length; node++) {
    if(!fixed[node]) continue
    right.fill(0, node * 3, node * 3 + 3)
    diagonal.fill(1, node * 3, node * 3 + 3)
  }
  const apply = input => {
    const output = new Float64Array(input.length)
    for(let offset = 0; offset < nodeIndices.length; offset += 8) {
      for(const template of templates) {
        const [xx, yy, zz, xy, yz, zx] = strainAndStress(input, nodeIndices, offset, template, lambda, mu)
        const { indices, gradients, volume } = template
        for(let corner = 0; corner < 4; corner++) {
          const node = nodeIndices[offset + indices[corner]]
          if(fixed[node]) continue
          const gx = gradients[corner * 3], gy = gradients[corner * 3 + 1], gz = gradients[corner * 3 + 2]
          output[node * 3] += volume * (xx * gx + xy * gy + zx * gz)
          output[node * 3 + 1] += volume * (xy * gx + yy * gy + yz * gz)
          output[node * 3 + 2] += volume * (zx * gx + yz * gy + zz * gz)
        }
      }
    }
    for(let node = 0; node < fixed.length; node++) {
      if(!fixed[node]) continue
      output[node * 3] = input[node * 3]
      output[node * 3 + 1] = input[node * 3 + 1]
      output[node * 3 + 2] = input[node * 3 + 2]
    }
    return output
  }
  const { displacement, relativeResidual } = solveConjugateGradient(apply, right, diagonal, 5000)
  if(!Number.isFinite(relativeResidual)) return
  const stresses = new Float64Array(positions.length)
  const weights = new Float64Array(positions.length)
  for(let offset = 0; offset < nodeIndices.length; offset += 8) {
    for(const template of templates) {
      const [xx, yy, zz, xy, yz, zx] = strainAndStress(displacement, nodeIndices, offset, template, lambda, mu)
      const equivalent = Math.sqrt(
        ((xx - yy) ** 2 + (yy - zz) ** 2 + (zz - xx) ** 2) / 2 + 3 * (xy * xy + yz * yz + zx * zx),
      )
      for(const corner of template.indices) {
        const node = nodeIndices[offset + corner]
        stresses[node] += equivalent * template.volume
        weights[node] += template.volume
      }
    }
  }
  let maxStress = 0
  let maxDisplacement = 0
  for(let node = 0; node < positions.length; node++) {
    stresses[node] /= weights[node]
    maxStress = Math.max(maxStress, stresses[node])
    maxDisplacement = Math.max(maxDisplacement, Math.hypot(...displacement.subarray(node * 3, node * 3 + 3)))
  }
  return {
    min: bounds.min.toArray(), step, steps,
    nx: nodeDimensions[0], ny: nodeDimensions[1], nz: nodeDimensions[2],
    grid: nodeGrid, nodeCentered: true,
    displacements: displacement, stresses, maxStress, maxDisplacement,
    cellCount: elements.length / 8, relativeResidual,
    displayScale: maxDisplacement ? Math.max(size.x, size.y, size.z) * 0.12 / maxDisplacement : 0,
  }
}
