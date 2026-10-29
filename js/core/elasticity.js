import * as THREE from 'three'

import { dot, makeConjugateGradient, makeMultigrid } from './multigrid.js'
import { fixFloatingNodes, forLatticeNear, makeTriangles, voxelizeSolid } from './voxel.js'


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


// Every voxel shares one 24×24 stiffness matrix over its corner displacements.
function elementMatrix(templates, lambda, mu) {
  const matrix = new Float64Array(576)
  for(const { indices, gradients, volume } of templates) {
    for(let a = 0; a < 4; a++) {
      for(let b = 0; b < 4; b++) {
        const ga = gradients.slice(a * 3, a * 3 + 3)
        const gb = gradients.slice(b * 3, b * 3 + 3)
        const dot = ga[0] * gb[0] + ga[1] * gb[1] + ga[2] * gb[2]
        for(let p = 0; p < 3; p++) {
          for(let q = 0; q < 3; q++) {
            matrix[(indices[a] * 3 + p) * 24 + indices[b] * 3 + q] +=
              volume * (lambda * ga[p] * gb[q] + mu * ga[q] * gb[p] + (p == q ? mu * dot : 0))
          }
        }
      }
    }
  }
  return matrix
}


// Hexahedral mesh whose elements reference shared 24×24 matrices. Fixed dofs
// act as identity rows; their element rows and columns carry no coupling.
function elasticLevel(level) {
  const { elements, matrixIndex, matrices, fixed } = level
  const size = fixed.length
  const diagonal = new Float64Array(size)
  for(let element = 0; element < matrixIndex.length; element++) {
    const matrix = matrices[matrixIndex[element]]
    for(let row = 0; row < 24; row++) diagonal[elements[element * 8 + (row / 3 | 0)] * 3 + row % 3] += matrix[row * 25]
  }
  for(let dof = 0; dof < size; dof++) {
    if(!diagonal[dof]) fixed[dof] = 1
    if(fixed[dof]) diagonal[dof] = 1
  }
  const local = new Float64Array(24)
  const dofs = new Int32Array(24)
  const gather = element => {
    for(let corner = 0; corner < 8; corner++) {
      const node = elements[element * 8 + corner] * 3
      dofs[corner * 3] = node
      dofs[corner * 3 + 1] = node + 1
      dofs[corner * 3 + 2] = node + 2
    }
  }
  return {
    ...level, size, diagonal,
    apply(input, output) {
      output.fill(0)
      for(let element = 0; element < matrixIndex.length; element++) {
        const matrix = matrices[matrixIndex[element]]
        gather(element)
        for(let column = 0; column < 24; column++) local[column] = input[dofs[column]]
        for(let row = 0; row < 24; row++) {
          let value = 0
          for(let column = 0; column < 24; column++) value += matrix[row * 24 + column] * local[column]
          output[dofs[row]] += value
        }
      }
      for(let dof = 0; dof < size; dof++) if(fixed[dof]) output[dof] = input[dof]
    },
    dense() {
      const dense = new Float64Array(size * size)
      for(let element = 0; element < matrixIndex.length; element++) {
        const matrix = matrices[matrixIndex[element]]
        gather(element)
        for(let row = 0; row < 24; row++) {
          if(fixed[dofs[row]]) continue
          for(let column = 0; column < 24; column++) {
            if(!fixed[dofs[column]]) dense[dofs[row] * size + dofs[column]] += matrix[row * 24 + column]
          }
        }
      }
      for(let dof = 0; dof < size; dof++) if(fixed[dof]) dense[dof * size + dof] = 1
      return dense
    },
  }
}


// Merges 2×2×2 elements with trilinear prolongation between node lattices.
// Coarse element matrices are Galerkin products PᵀKP of their children,
// with prolongation rows of fixed fine dofs removed; identical children
// and identical coarse elements share one matrix.
function coarsenElastic(level) {
  const elementDimensions = level.nodeDimensions.map(count => Math.ceil((count - 1) / 2))
  const nodeDimensions = elementDimensions.map(count => count + 1)
  const elementGrid = new Int32Array(elementDimensions[0] * elementDimensions[1] * elementDimensions[2]).fill(-1)
  const elementCoordinates = []
  const children = []
  const contributions = new Map()
  for(let element = 0; element < level.matrixIndex.length; element++) {
    const [i, j, k] = level.elementCoordinates.subarray(element * 3, element * 3 + 3)
    const cell = (i >> 1) + elementDimensions[0] * ((j >> 1) + elementDimensions[1] * (k >> 1))
    if(elementGrid[cell] < 0) {
      elementGrid[cell] = children.length
      elementCoordinates.push(i >> 1, j >> 1, k >> 1)
      children.push([])
    }
    const position = (i & 1) + 2 * (j & 1) + 4 * (k & 1)
    let mask = 0
    for(let dof = 0; dof < 24; dof++) {
      if(level.fixed[level.elements[element * 8 + (dof / 3 | 0)] * 3 + dof % 3]) mask |= 1 << dof
    }
    const key = `${level.matrixIndex[element]},${position},${mask}`
    if(!contributions.has(key)) {
      contributions.set(key, galerkinElement(level.matrices[level.matrixIndex[element]], position, mask))
    }
    children[elementGrid[cell]].push(key)
  }

  const matrices = []
  const matrixIndex = new Int32Array(children.length)
  const shared = new Map()
  children.forEach((keys, element) => {
    const key = keys.join(' ')
    if(!shared.has(key)) {
      const matrix = new Float64Array(576)
      for(const child of keys) contributions.get(child).forEach((value, index) => matrix[index] += value)
      shared.set(key, matrices.push(matrix) - 1)
    }
    matrixIndex[element] = shared.get(key)
  })

  const nodeGrid = new Int32Array(nodeDimensions[0] * nodeDimensions[1] * nodeDimensions[2]).fill(-1)
  const nodeCoordinates = []
  const elements = new Int32Array(children.length * 8)
  for(let element = 0; element < children.length; element++) {
    cubeCorners.forEach(([di, dj, dk], corner) => {
      const i = elementCoordinates[element * 3] + di
      const j = elementCoordinates[element * 3 + 1] + dj
      const k = elementCoordinates[element * 3 + 2] + dk
      const index = i + nodeDimensions[0] * (j + nodeDimensions[1] * k)
      if(nodeGrid[index] < 0) {
        nodeGrid[index] = nodeCoordinates.length / 3
        nodeCoordinates.push(i, j, k)
      }
      elements[element * 8 + corner] = nodeGrid[index]
    })
  }

  const fineCount = level.nodeCoordinates.length / 3
  const offsets = new Int32Array(fineCount + 1)
  const coarseNodes = []
  const weights = []
  for(let node = 0; node < fineCount; node++) {
    offsets[node] = coarseNodes.length
    const [axisX, axisY, axisZ] = [0, 1, 2].map(axis => {
      const value = level.nodeCoordinates[node * 3 + axis]
      return value % 2 ? [[value - 1 >> 1, 0.5], [value + 1 >> 1, 0.5]] : [[value >> 1, 1]]
    })
    for(const [k, wz] of axisZ) {
      for(const [j, wy] of axisY) {
        for(const [i, wx] of axisX) {
          coarseNodes.push(nodeGrid[i + nodeDimensions[0] * (j + nodeDimensions[1] * k)])
          weights.push(wx * wy * wz)
        }
      }
    }
  }
  offsets[fineCount] = coarseNodes.length

  const transfer = (from, to, toFine) => {
    for(let node = 0; node < fineCount; node++) {
      for(let component = 0; component < 3; component++) {
        const dof = node * 3 + component
        if(level.fixed[dof]) continue
        for(let slot = offsets[node]; slot < offsets[node + 1]; slot++) {
          const coarse = coarseNodes[slot] * 3 + component
          if(toFine) to[dof] += weights[slot] * from[coarse]
          else to[coarse] += weights[slot] * from[dof]
        }
      }
    }
  }
  level.restrict = (fine, coarse) => {
    coarse.fill(0)
    transfer(fine, coarse, false)
  }
  level.prolong = (coarse, fine) => transfer(coarse, fine, true)

  return elasticLevel({
    nodeDimensions,
    nodeCoordinates: Int32Array.from(nodeCoordinates),
    elementCoordinates: Int32Array.from(elementCoordinates),
    elements, matrixIndex, matrices,
    fixed: new Uint8Array(nodeCoordinates.length),
  })
}


// PᵀKP for a child element at sub-position (bit per axis) within its parent.
// Fine and coarse corners sit at 0..2 and 0, 2 in fine lattice units.
function galerkinElement(matrix, position, mask) {
  const offsets = [position & 1, position >> 1 & 1, position >> 2 & 1]
  const prolongation = new Float64Array(576)
  cubeCorners.forEach((fine, a) => cubeCorners.forEach((coarse, b) => {
    const weight = fine.reduce((product, value, axis) =>
      product * Math.max(0, 1 - Math.abs(offsets[axis] + value - 2 * coarse[axis]) / 2), 1)
    for(let component = 0; component < 3; component++) {
      if(!(mask >> (a * 3 + component) & 1)) prolongation[(a * 3 + component) * 24 + b * 3 + component] = weight
    }
  }))
  const product = new Float64Array(576)
  for(let row = 0; row < 24; row++) {
    for(let column = 0; column < 24; column++) {
      let value = 0
      for(let inner = 0; inner < 24; inner++) value += matrix[row * 24 + inner] * prolongation[inner * 24 + column]
      product[row * 24 + column] = value
    }
  }
  const result = new Float64Array(576)
  for(let row = 0; row < 24; row++) {
    for(let column = 0; column < 24; column++) {
      let value = 0
      for(let inner = 0; inner < 24; inner++) value += prolongation[inner * 24 + row] * product[inner * 24 + column]
      result[row * 24 + column] = value
    }
  }
  return result
}


export function solveElasticSolid(solid, fixedFaces, loadFaces, force, material, resolution, maxCells, targetSteps, onProgress=() => {}) {
  const { bounds, size, step, steps, nx, ny, nz, grid: cells } =
    voxelizeSolid(solid, resolution, maxCells, targetSteps,
      value => onProgress(value * 20))
  onProgress(20)
  const nodeDimensions = [nx + 1, ny + 1, nz + 1]
  const nodeGrid = new Int32Array(nodeDimensions[0] * nodeDimensions[1] * nodeDimensions[2]).fill(-1)
  const positions = []
  const nodeCoordinates = []
  const elements = []
  const elementCoordinates = []
  const nodeAt = (i, j, k) => {
    const index = i + nodeDimensions[0] * (j + nodeDimensions[1] * k)
    if(nodeGrid[index] >= 0) return nodeGrid[index]
    const node = positions.length
    nodeGrid[index] = node
    nodeCoordinates.push(i, j, k)
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
        elementCoordinates.push(i, j, k)
      }
    }
  }
  if(!elements.length) return

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
  const closest = new THREE.Vector3()
  const tag = faces => {
    const marks = new Uint8Array(positions.length)
    makeTriangles(faces).forEach(triangle => {
      const normal = triangle.getNormal(new THREE.Vector3())
      const distance = 0.6 * Math.hypot(normal.x * steps[0], normal.y * steps[1], normal.z * steps[2])
      forLatticeNear(triangle, distance, bounds.min.toArray(), steps, nodeDimensions, (i, j, k) => {
        const node = nodeGrid[i + nodeDimensions[0] * (j + nodeDimensions[1] * k)]
        if(node < 0 || marks[node] || !isSurfaceNode(i, j, k)) return
        const point = positions[node]
        if(triangle.closestPointToPoint(point, closest).distanceToSquared(point) <= distance * distance) marks[node] = 1
      })
    })
    return marks
  }
  const fixed = tag(fixedFaces)
  const loaded = tag(loadFaces)
  if(!fixed.some(Boolean)) return
  const nodeIndices = Int32Array.from(elements)
  fixFloatingNodes(fixed, union => {
    for(let offset = 0; offset < nodeIndices.length; offset += 8) {
      for(let corner = 1; corner < 8; corner++) union(nodeIndices[offset], nodeIndices[offset + corner])
    }
  })
  const freeLoaded = []
  loaded.forEach((value, node) => { if(value && !fixed[node]) freeLoaded.push(node) })
  if(!freeLoaded.length) return

  const young = material.elasticModulus
  const poisson = material.poissonRatio
  const mu = young / (2 * (1 + poisson))
  const lambda = young * poisson / ((1 + poisson) * (1 - 2 * poisson))
  const templates = tetraTemplates(steps)
  const dofCount = positions.length * 3
  const right = new Float64Array(dofCount)
  for(const node of freeLoaded) {
    right[node * 3] += force.x / freeLoaded.length
    right[node * 3 + 1] += force.y / freeLoaded.length
    right[node * 3 + 2] += force.z / freeLoaded.length
  }
  const level = elasticLevel({
    nodeDimensions,
    nodeCoordinates: Int32Array.from(nodeCoordinates),
    elementCoordinates: Int32Array.from(elementCoordinates),
    elements: nodeIndices,
    matrixIndex: new Int32Array(nodeIndices.length / 8),
    matrices: [elementMatrix(templates, lambda, mu)],
    fixed: Uint8Array.from({ length: dofCount }, (_, dof) => fixed[dof / 3 | 0]),
  })
  const solve = makeConjugateGradient(level.apply, makeMultigrid(level, coarsenElastic), dofCount)
  onProgress(35)
  const displacement = new Float64Array(dofCount)
  solve(right, displacement, 1e-4, 1000, fraction => onProgress(35 + fraction * 55))
  onProgress(90)
  const applied = new Float64Array(dofCount)
  level.apply(displacement, applied)
  let residualSquared = 0
  for(let i = 0; i < dofCount; i++) residualSquared += (right[i] - applied[i]) ** 2
  const relativeResidual = Math.sqrt(residualSquared / dot(right, right))
  if(!Number.isFinite(relativeResidual)) return
  const stresses = new Float64Array(positions.length)
  const weights = new Float64Array(positions.length)
  for(let offset = 0; offset < nodeIndices.length; offset += 8) {
    if(offset % 8000 == 0) onProgress(90 + offset / nodeIndices.length * 8)
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
