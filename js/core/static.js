import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { resolveSimulationFaces } from './simulation.js'
import { findVoxelNode, makeTriangles, voxelizeSolid } from './voxel.js'


export class StaticSimulation {
  constructor(id) {
    this.id = id || makeID()
    this.mode = 'static'
    this.title = 'Static Load'
    this.fixedFaces = []
    this.loadFaces = []
    this.forceX = 0
    this.forceY = 0
    this.forceZ = -100
    this.result = null
    this.revision = 0
    this.deformationFactor = 1
    this.animationRevision = 0
  }

  resolve(tree) {
    if(!this.fixedFaces.length || !this.loadFaces.length) return 'Pick fixed and loaded faces'
    const problem = resolveSimulationFaces(tree, [this.fixedFaces, this.loadFaces])
    if(problem) return problem
    if(!this.forceX && !this.forceY && !this.forceZ) return 'Enter a force'
  }

  solve(tree) {
    const problem = this.resolve(tree)
    if(problem) {
      this.result = null
      this.revision++
      return problem
    }

    const solid = this.fixedFaces[0].item.solid
    this.result = solveSpringSolid(
      solid,
      this.fixedFaces.map(reference => reference.item),
      this.loadFaces.map(reference => reference.item),
      new THREE.Vector3(this.forceX, this.forceY, this.forceZ),
    )
    if(!this.result) {
      this.revision++
      return 'The selected faces do not reach the simulation grid'
    }
    this.result.componentId = this.fixedFaces[0].componentId
    this.result.solidId = this.fixedFaces[0].solidId
    this.revision++
  }

  nodeAt(position) {
    const result = this.result
    if(!result) return
    return findVoxelNode(result, position)
  }

  displacementAt(position) {
    return this.sampleAt(position)?.displacement
  }

  stressAt(position) {
    return this.sampleAt(position)?.stress
  }

  sampleAt(position) {
    const result = this.result
    if(!result) return
    const coordinates = [position.x, position.y, position.z]
    const dimensions = [result.nx, result.ny, result.nz]
    const lower = []
    const fractions = []
    for(let axis = 0; axis < 3; axis++) {
      const coordinate = Math.max(0, Math.min(dimensions[axis] - 1,
        (coordinates[axis] - result.min[axis]) / result.step - 0.5))
      lower[axis] = Math.floor(coordinate)
      fractions[axis] = coordinate - lower[axis]
    }
    const displacement = new THREE.Vector3()
    let stress = 0
    let totalWeight = 0
    for(let z = 0; z <= 1; z++) {
      for(let y = 0; y <= 1; y++) {
        for(let x = 0; x <= 1; x++) {
          const i = lower[0] + x
          const j = lower[1] + y
          const k = lower[2] + z
          if(i >= result.nx || j >= result.ny || k >= result.nz) continue
          const node = result.grid[i + result.nx * (j + result.ny * k)]
          if(node < 0) continue
          const weight = (x ? fractions[0] : 1 - fractions[0]) *
            (y ? fractions[1] : 1 - fractions[1]) *
            (z ? fractions[2] : 1 - fractions[2])
          displacement.x += weight * result.displacements[node * 3]
          displacement.y += weight * result.displacements[node * 3 + 1]
          displacement.z += weight * result.displacements[node * 3 + 2]
          stress += weight * result.stresses[node]
          totalWeight += weight
        }
      }
    }
    if(!totalWeight) {
      const node = this.nodeAt(position)
      if(node === undefined) return
      return {
        displacement: new THREE.Vector3(
          result.displacements[node * 3],
          result.displacements[node * 3 + 1],
          result.displacements[node * 3 + 2],
        ),
        stress: result.stresses[node] / result.maxStress,
      }
    }
    return {
      displacement: displacement.divideScalar(totalWeight),
      stress: stress / totalWeight / result.maxStress,
    }
  }

  dump() {
    return {
      id: this.id,
      title: this.title,
      fixedFaces: this.fixedFaces,
      loadFaces: this.loadFaces,
      forceX: this.forceX,
      forceY: this.forceY,
      forceZ: this.forceZ,
    }
  }

  static undump(dump) {
    return Object.assign(new StaticSimulation(dump.id), dump)
  }
}
Serialize.register(StaticSimulation, 'StaticSimulation')


function solveSpringSolid(solid, fixedFaces, loadFaces, force) {
  const fixedTriangles = makeTriangles(fixedFaces)
  const loadTriangles = makeTriangles(loadFaces)
  const { bounds, size, step, nx, ny, nz, grid, points } = voxelizeSolid(solid, 28, 4000)

  const fixed = new Set()
  const loaded = []
  const closest = new THREE.Vector3()
  const limit = step * step * 1.1
  points.forEach((point, node) => {
    if(fixedTriangles.some(triangle => triangle.closestPointToPoint(point, closest).distanceToSquared(point) < limit)) fixed.add(node)
    if(loadTriangles.some(triangle => triangle.closestPointToPoint(point, closest).distanceToSquared(point) < limit)) loaded.push(node)
  })
  if(!points.length || !fixed.size || !loaded.length) return

  const springs = []
  const offsets = []
  for(let dk = -1; dk <= 1; dk++) {
    for(let dj = -1; dj <= 1; dj++) {
      for(let di = -1; di <= 1; di++) {
        if(!di && !dj && !dk) continue
        if(dk > 0 || (dk == 0 && dj > 0) || (dk == 0 && dj == 0 && di > 0)) offsets.push([di, dj, dk])
      }
    }
  }
  for(let k = 0; k < nz; k++) {
    for(let j = 0; j < ny; j++) {
      for(let i = 0; i < nx; i++) {
        const a = grid[i + nx * (j + ny * k)]
        if(a < 0) continue
        offsets.forEach(([di, dj, dk]) => {
          const x = i + di
          const y = j + dj
          const z = k + dk
          if(x < 0 || y < 0 || z < 0 || x >= nx || y >= ny || z >= nz) return
          const b = grid[x + nx * (y + ny * z)]
          if(b < 0) return
          const direction = points[b].clone().sub(points[a])
          const length = direction.length()
          springs.push({ a, b, direction: direction.divideScalar(length), stiffness: 1 / length })
        })
      }
    }
  }

  const count = points.length * 3
  const right = new Float64Array(count)
  loaded.forEach(node => {
    right[node * 3] += force.x / loaded.length
    right[node * 3 + 1] += force.y / loaded.length
    right[node * 3 + 2] += force.z / loaded.length
  })
  const apply = input => {
    const output = new Float64Array(count)
    springs.forEach(spring => {
      const ai = spring.a * 3
      const bi = spring.b * 3
      const ax = fixed.has(spring.a) ? 0 : input[ai]
      const ay = fixed.has(spring.a) ? 0 : input[ai + 1]
      const az = fixed.has(spring.a) ? 0 : input[ai + 2]
      const bx = fixed.has(spring.b) ? 0 : input[bi]
      const by = fixed.has(spring.b) ? 0 : input[bi + 1]
      const bz = fixed.has(spring.b) ? 0 : input[bi + 2]
      const extension = ((ax - bx) * spring.direction.x + (ay - by) * spring.direction.y + (az - bz) * spring.direction.z) * spring.stiffness
      if(!fixed.has(spring.a)) {
        output[ai] += extension * spring.direction.x
        output[ai + 1] += extension * spring.direction.y
        output[ai + 2] += extension * spring.direction.z
      }
      if(!fixed.has(spring.b)) {
        output[bi] -= extension * spring.direction.x
        output[bi + 1] -= extension * spring.direction.y
        output[bi + 2] -= extension * spring.direction.z
      }
    })
    fixed.forEach(node => {
      output[node * 3] = input[node * 3]
      output[node * 3 + 1] = input[node * 3 + 1]
      output[node * 3 + 2] = input[node * 3 + 2]
    })
    return output
  }

  const displacements = conjugateGradient(apply, right)
  const stresses = new Float64Array(points.length)
  const dimensions = [nx, ny, nz]
  const strides = [1, nx, nx * ny]
  const gradient = (index, node, coordinates, axis, component) => {
    const stride = strides[axis]
    const minus = coordinates[axis] > 0 ? grid[index - stride] : -1
    const plus = coordinates[axis] + 1 < dimensions[axis] ? grid[index + stride] : -1
    if(minus >= 0 && plus >= 0) {
      return (displacements[plus * 3 + component] - displacements[minus * 3 + component]) / (2 * step)
    }
    if(plus >= 0) return (displacements[plus * 3 + component] - displacements[node * 3 + component]) / step
    if(minus >= 0) return (displacements[node * 3 + component] - displacements[minus * 3 + component]) / step
    return 0
  }
  for(let k = 0; k < nz; k++) {
    for(let j = 0; j < ny; j++) {
      for(let i = 0; i < nx; i++) {
        const index = i + nx * (j + ny * k)
        const node = grid[index]
        if(node < 0) continue
        const coordinates = [i, j, k]
        const exx = gradient(index, node, coordinates, 0, 0)
        const eyy = gradient(index, node, coordinates, 1, 1)
        const ezz = gradient(index, node, coordinates, 2, 2)
        const exy = (gradient(index, node, coordinates, 0, 1) + gradient(index, node, coordinates, 1, 0)) / 2
        const eyz = (gradient(index, node, coordinates, 1, 2) + gradient(index, node, coordinates, 2, 1)) / 2
        const ezx = (gradient(index, node, coordinates, 2, 0) + gradient(index, node, coordinates, 0, 2)) / 2
        // For a uniform isotropic material, von Mises stress is proportional to this strain measure.
        stresses[node] = Math.sqrt(
          ((exx - eyy) ** 2 + (eyy - ezz) ** 2 + (ezz - exx) ** 2) / 2 +
          3 * (exy ** 2 + eyz ** 2 + ezx ** 2)
        )
      }
    }
  }
  const maxStress = Math.max(...stresses) || 1
  let maxDisplacement = 0
  points.forEach((_, node) => {
    maxDisplacement = Math.max(maxDisplacement, Math.hypot(
      displacements[node * 3], displacements[node * 3 + 1], displacements[node * 3 + 2]
    ))
  })

  return {
    min: bounds.min.toArray(),
    step,
    nx,
    ny,
    nz,
    grid,
    displacements,
    stresses,
    maxStress,
    displayScale: maxDisplacement ? Math.max(size.x, size.y, size.z) * 0.12 / maxDisplacement : 0,
  }
}


function conjugateGradient(apply, right) {
  const solution = new Float64Array(right.length)
  const residual = right.slice()
  const direction = residual.slice()
  let residualSquared = dot(residual, residual)
  const target = residualSquared * 1e-12
  for(let iteration = 0; iteration < 600 && residualSquared > target; iteration++) {
    const applied = apply(direction)
    const alpha = residualSquared / dot(direction, applied)
    for(let i = 0; i < solution.length; i++) {
      solution[i] += alpha * direction[i]
      residual[i] -= alpha * applied[i]
    }
    const nextResidualSquared = dot(residual, residual)
    const beta = nextResidualSquared / residualSquared
    for(let i = 0; i < direction.length; i++) direction[i] = residual[i] + beta * direction[i]
    residualSquared = nextResidualSquared
  }
  return solution
}


function dot(a, b) {
  let result = 0
  for(let i = 0; i < a.length; i++) result += a[i] * b[i]
  return result
}
