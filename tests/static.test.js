import assert from 'node:assert/strict'
import test from 'node:test'
import * as THREE from 'three'

import { StaticSimulation } from '../js/core/static.js'

function reference(identity) {
  return {
    componentId: 'component',
    selection: { identity },
    clone() { return reference(identity) },
    getItem() { return this.item },
    update(tree) {
      this.item = tree.findChild(this.componentId).compound.solids()
        .flatMap(solid => solid.faces()).find(face => face.id == identity)
      if(!this.item) return { type: 'error' }
    },
  }
}


function steppedCantilever(plateThickness) {
  const solid = { id: 'solid' }
  const faces = []
  const addFace = (id, a, b, c, d) => {
    const face = {
      id,
      solid,
      getPlane: () => true,
      center: () => new THREE.Vector3(...a).add(new THREE.Vector3(...b))
        .add(new THREE.Vector3(...c)).add(new THREE.Vector3(...d)).divideScalar(4),
      tesselate: () => ({ positions: [...a, ...b, ...c, ...a, ...c, ...d] }),
    }
    faces.push(face)
    return face
  }
  const p = plateThickness
  addFace('fixed', [-40, -25, 0], [-40, 25, 0], [40, 25, 0], [40, -25, 0])
  addFace('plate-left', [-40, -25, 0], [-40, -25, p], [-40, 25, p], [-40, 25, 0])
  addFace('plate-right', [40, -25, 0], [40, 25, 0], [40, 25, p], [40, -25, p])
  addFace('plate-front', [-40, -25, 0], [40, -25, 0], [40, -25, p], [-40, -25, p])
  addFace('plate-back', [-40, 25, 0], [-40, 25, p], [40, 25, p], [40, 25, 0])
  addFace('plate-top-left', [-40, -25, p], [-28, -25, p], [-28, 25, p], [-40, 25, p])
  addFace('plate-top-right', [28, -25, p], [40, -25, p], [40, 25, p], [28, 25, p])
  addFace('plate-top-front', [-28, -25, p], [28, -25, p], [28, -7.5, p], [-28, -7.5, p])
  addFace('plate-top-back', [-28, 7.5, p], [28, 7.5, p], [28, 25, p], [-28, 25, p])
  addFace('arm-left', [-28, -7.5, p], [-28, -7.5, p + 197], [-28, 7.5, p + 197], [-28, 7.5, p])
  addFace('arm-right', [28, -7.5, p], [28, 7.5, p], [28, 7.5, p + 197], [28, -7.5, p + 197])
  addFace('arm-front', [-28, -7.5, p], [28, -7.5, p], [28, -7.5, p + 197], [-28, -7.5, p + 197])
  addFace('arm-back', [-28, 7.5, p], [-28, 7.5, p + 197], [28, 7.5, p + 197], [28, 7.5, p])
  addFace('loaded', [-28, -7.5, p + 197], [28, -7.5, p + 197], [28, 7.5, p + 197], [-28, 7.5, p + 197])
  solid.faces = () => faces
  return { solid, faces }
}


test('static load resolves a thin fixed plate instead of locking its entire thickness', () => {
  const { solid } = steppedCantilever(8)
  const tree = { findChild: () => ({
    compound: { solids: () => [solid] },
    getMaterial: () => ({ elasticModulus: 69000, poissonRatio: 0.33 }),
  }) }
  const simulation = new StaticSimulation('test')
  assert.equal(simulation.forceY, -100)
  assert.equal(simulation.forceZ, 0)
  // The layer bounds below assume quality 3 rather than the default.
  simulation.quality = 3
  simulation.fixedFaces = [reference('fixed')]
  simulation.loadFaces = [reference('loaded')]
  assert.equal(simulation.resolve(tree, false), undefined)
  assert.equal(simulation.fixedFaces[0].item, undefined, 'render validation must not bind face references')
  simulation.solve(tree)

  const result = simulation.result
  assert.ok(result)
  assert.ok(result.relativeResidual < 0.001, `solver residual ${result.relativeResidual}`)
  assert.ok(result.steps[2] <= 2.5, 'plate thickness needs several grid layers')
  assert.ok(result.steps[1] <= 4.5, 'the arm needs several grid layers across the load direction')
  const plateDisplacements = []
  const plateStresses = []
  for(let k = 0; k < result.nz; k++) {
    const z = result.min[2] + k * result.steps[2]
    if(z >= 8) continue
    for(let j = 0; j < result.ny; j++) {
      for(let i = 0; i < result.nx; i++) {
        const node = result.grid[i + result.nx * (j + result.ny * k)]
        if(node < 0) continue
        plateDisplacements.push(Math.hypot(...result.displacements.slice(node * 3, node * 3 + 3)))
        plateStresses.push(result.stresses[node])
      }
    }
  }
  assert.ok(Math.max(...plateDisplacements) > 1e-6, 'free plate layers must deform')
  assert.ok(Math.max(...plateStresses) > 0, 'the plate must carry strain')

  const meanStress = (from, to) => {
    let sum = 0
    let count = 0
    for(let k = 0; k < result.nz; k++) {
      const z = result.min[2] + k * result.steps[2]
      if(z < from || z >= to) continue
      for(let j = 0; j < result.ny; j++) {
        for(let i = 0; i < result.nx; i++) {
          const node = result.grid[i + result.nx * (j + result.ny * k)]
          if(node < 0) continue
          sum += result.stresses[node]
          count++
        }
      }
    }
    return sum / count
  }
  assert.ok(meanStress(8, 30) > meanStress(175, 205), 'the arm should carry more bending strain near the plate')

  const alternate = new StaticSimulation('alternate')
  alternate.quality = 3
  alternate.fixedFaces = [reference('plate-left')]
  alternate.loadFaces = [reference('arm-front')]
  alternate.solve(tree)
  assert.ok(alternate.result)
  assert.deepEqual(alternate.result.steps, result.steps, 'grid spacing must not depend on selected faces')

  simulation.quality = 5
  simulation.solve(tree)
  assert.ok(simulation.result)
  assert.ok(simulation.result.relativeResidual < 0.001)
  assert.ok(simulation.result.cellCount > result.cellCount)
})


test('linear elastic bar has physical stress and displacement units', () => {
  const solid = { id: 'bar' }
  const faces = []
  const addFace = (id, a, b, c, d) => {
    faces.push({
      id, solid,
      getPlane: () => true,
      tesselate: () => ({ positions: [...a, ...b, ...c, ...a, ...c, ...d] }),
    })
  }
  addFace('fixed', [0, 0, 0], [0, 0, 10], [0, 10, 10], [0, 10, 0])
  addFace('loaded', [100, 0, 0], [100, 10, 0], [100, 10, 10], [100, 0, 10])
  addFace('front', [0, 0, 0], [100, 0, 0], [100, 0, 10], [0, 0, 10])
  addFace('back', [0, 10, 0], [0, 10, 10], [100, 10, 10], [100, 10, 0])
  addFace('bottom', [0, 0, 0], [0, 10, 0], [100, 10, 0], [100, 0, 0])
  addFace('top', [0, 0, 10], [100, 0, 10], [100, 10, 10], [0, 10, 10])
  solid.faces = () => faces
  const material = { elasticModulus: 1000, poissonRatio: 0 }
  const tree = { findChild: () => ({
    compound: { solids: () => [solid] },
    getMaterial: () => material,
  }) }
  const simulation = new StaticSimulation('bar')
  simulation.fixedFaces = [reference('fixed')]
  simulation.loadFaces = [reference('loaded')]
  simulation.forceX = 1000
  simulation.forceY = 0
  simulation.solve(tree)

  assert.ok(simulation.result)
  assert.ok(simulation.result.relativeResidual < 0.001)
  const displacement = simulation.sampleAt(new THREE.Vector3(100, 5, 5)).displacement.x
  const stress = simulation.sampleAt(new THREE.Vector3(50, 5, 5)).stress
  assert.ok(Math.abs(displacement - 1) < 0.15, `expected approximately 1 mm, got ${displacement}`)
  assert.ok(Math.abs(stress - 10) < 2, `expected approximately 10 MPa, got ${stress}`)

  material.elasticModulus = 2000
  simulation.solve(tree)
  const stifferDisplacement = simulation.sampleAt(new THREE.Vector3(100, 5, 5)).displacement.x
  const stifferStress = simulation.sampleAt(new THREE.Vector3(50, 5, 5)).stress
  assert.ok(Math.abs(stifferDisplacement / displacement - 0.5) < 0.05)
  assert.ok(Math.abs(stifferStress / stress - 1) < 0.05)

  simulation.quality = 1
  simulation.solve(tree)
  const draftCells = simulation.result.cellCount
  simulation.quality = 5
  simulation.solve(tree)
  assert.ok(simulation.result.cellCount > draftCells, 'quality must increase voxel resolution')

  material.elasticModulus = 1000
  simulation.quality = 3
  simulation.forceX = 0
  simulation.forceY = -1
  simulation.solve(tree)
  const tipBending = simulation.sampleAt(new THREE.Vector3(100, 5, 5)).displacement.y
  assert.ok(Math.abs(tipBending + 0.4) < 0.15, `expected approximately 0.4 mm bending, got ${tipBending}`)
})
