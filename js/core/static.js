import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { resolveSimulationFaces } from './simulation.js'
import { findVoxelNode, makeTriangles } from './voxel.js'
import { solveElasticSolid } from './elasticity.js'


export const qualityLevels = {
  1: { resolution: 18, maxCells: 4000, thinLayers: 2 },
  2: { resolution: 24, maxCells: 9000, thinLayers: 3 },
  3: { resolution: 30, maxCells: 16000, thinLayers: 4 },
  4: { resolution: 36, maxCells: 28000, thinLayers: 6 },
  5: { resolution: 42, maxCells: 45000, thinLayers: 8 },
}


export class StaticSimulation {
  constructor(id) {
    this.id = id || makeID()
    this.mode = 'static'
    this.title = 'Static Load'
    this.fixedFaces = []
    this.loadFaces = []
    this.forceX = 0
    this.forceY = -100
    this.forceZ = 0
    this.quality = 3
    this.result = null
    this.revision = 0
    this.deformationFactor = 1
    this.animationRevision = 0
  }

  resolve(tree, bind=true) {
    if(!this.fixedFaces.length || !this.loadFaces.length) return 'Pick fixed and loaded faces'
    const problem = resolveSimulationFaces(tree, [this.fixedFaces, this.loadFaces], bind)
    if(problem) return problem
    if(!this.forceX && !this.forceY && !this.forceZ) return 'Enter a force'
    const material = tree.findChild(this.fixedFaces[0].componentId).getMaterial()
    if(!material) return 'Assign a material to this component'
    if(!(material.elasticModulus > 0) || !Number.isFinite(material.elasticModulus) ||
      !(material.poissonRatio >= 0 && material.poissonRatio < 0.45)) {
      return 'Enter an elastic modulus and Poisson ratio in the material'
    }
  }

  solve(tree) {
    const problem = this.resolve(tree)
    if(problem) {
      this.result = null
      this.revision++
      return problem
    }

    const solid = this.fixedFaces[0].item.solid
    const material = tree.findChild(this.fixedFaces[0].componentId).getMaterial()
    const { resolution, maxCells, thinLayers } = qualityLevels[this.quality]
    this.result = solveElasticSolid(
      solid,
      this.fixedFaces.map(reference => reference.item),
      this.loadFaces.map(reference => reference.item),
      new THREE.Vector3(this.forceX, this.forceY, this.forceZ),
      material, resolution, maxCells, solidGridSteps(solid, thinLayers),
    )
    if(!this.result) {
      this.revision++
      return 'The selected faces do not reach the simulation grid'
    }
    this.result.componentId = this.fixedFaces[0].componentId
    this.result.solidId = solid.id
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
        (coordinates[axis] - result.min[axis]) / result.steps[axis] - (result.nodeCentered ? 0 : 0.5)))
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
        stress: result.stresses[node],
        relativeStress: result.maxStress ? result.stresses[node] / result.maxStress : 0,
      }
    }
    return {
      displacement: displacement.divideScalar(totalWeight),
      stress: stress / totalWeight,
      relativeStress: result.maxStress ? stress / totalWeight / result.maxStress : 0,
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
      quality: this.quality,
    }
  }

  static undump(dump) {
    return Object.assign(new StaticSimulation(dump.id), dump)
  }
}
Serialize.register(StaticSimulation, 'StaticSimulation')


export function solidGridSteps(solid, thinLayers) {
  const faces = solid.faces().filter(face => face.getPlane()).map(face => {
    const triangles = makeTriangles([face])
    const normal = triangles[0].getNormal(new THREE.Vector3())
    const axis = [0, 1, 2].reduce((best, current) =>
      Math.abs(normal.getComponent(current)) > Math.abs(normal.getComponent(best)) ? current : best, 0)
    if(Math.abs(normal.getComponent(axis)) < 0.95) return null
    const bounds = new THREE.Box3()
    triangles.forEach(triangle => {
      bounds.expandByPoint(triangle.a)
      bounds.expandByPoint(triangle.b)
      bounds.expandByPoint(triangle.c)
    })
    return { axis, bounds }
  }).filter(Boolean)
  const steps = [Infinity, Infinity, Infinity]
  for(let i = 0; i < faces.length; i++) {
    for(let j = i + 1; j < faces.length; j++) {
      const a = faces[i]
      const b = faces[j]
      if(a.axis != b.axis) continue
      const axis = a.axis
      const otherAxes = [0, 1, 2].filter(other => other != axis)
      if(otherAxes.some(other => Math.min(a.bounds.max.getComponent(other), b.bounds.max.getComponent(other)) -
        Math.max(a.bounds.min.getComponent(other), b.bounds.min.getComponent(other)) <= 1e-4)) continue
      const thickness = Math.abs(a.bounds.min.getComponent(axis) - b.bounds.min.getComponent(axis))
      if(thickness > 1e-4) steps[axis] = Math.min(steps[axis], thickness / thinLayers)
    }
  }
  return steps
}
