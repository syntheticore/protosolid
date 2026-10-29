import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { resolveSimulationFaces } from './simulation.js'
import { cellLevel, coarsenCells, makeConjugateGradient, makeMultigrid } from './multigrid.js'
import { findVoxelNode, fixFloatingNodes, forLatticeNear, makeTriangles, voxelizeSolid } from './voxel.js'


export class ThermalSimulation {
  constructor(id) {
    this.id = id || makeID()
    this.mode = 'thermal'
    this.title = 'Heat Flow'
    this.coldTemperature = 20
    this.hotTemperature = 100
    this.coldFaces = []
    this.hotFaces = []
    this.result = null
    this.revision = 0
  }

  resolve(tree, bind=true) {
    if(!this.coldFaces.length || !this.hotFaces.length) return 'Pick hot and cold faces'
    return resolveSimulationFaces(tree, [this.coldFaces, this.hotFaces], bind)
  }

  solve(tree) {
    const problem = this.resolve(tree)
    if(problem) {
      this.result = null
      this.revision++
      return problem
    }

    const solid = this.coldFaces[0].item.solid
    this.result = solveVoxelHeat(
      solid,
      this.coldFaces.map(reference => reference.item),
      this.hotFaces.map(reference => reference.item),
      this.coldTemperature,
      this.hotTemperature,
    )
    this.result.componentId = this.coldFaces[0].componentId
    this.result.solidId = solid.id
    this.revision++
  }

  temperatureAt(position) {
    const result = this.result
    if(!result) return
    const node = findVoxelNode(result, position)
    if(node !== undefined) return result.temperatures[node]
  }

  fluxSamples() {
    const result = this.result
    if(!result) return []
    const samples = []
    const temperature = (i, j, k, fallback) => {
      if(i < 0 || j < 0 || k < 0 || i >= result.nx || j >= result.ny || k >= result.nz) return fallback
      const node = result.grid[i + result.nx * (j + result.ny * k)]
      return node < 0 ? fallback : result.temperatures[node]
    }

    for(let k = 1; k < result.nz; k += 3) {
      for(let j = 1; j < result.ny; j += 3) {
        for(let i = 1; i < result.nx; i += 3) {
          const node = result.grid[i + result.nx * (j + result.ny * k)]
          if(node < 0) continue
          const center = result.temperatures[node]
          const flux = new THREE.Vector3(
            temperature(i - 1, j, k, center) - temperature(i + 1, j, k, center),
            temperature(i, j - 1, k, center) - temperature(i, j + 1, k, center),
            temperature(i, j, k - 1, center) - temperature(i, j, k + 1, center),
          ).divideScalar(2 * result.step)
          if(!flux.lengthSq()) continue
          samples.push({
            position: new THREE.Vector3(
              result.min[0] + (i + 0.5) * result.step,
              result.min[1] + (j + 0.5) * result.step,
              result.min[2] + (k + 0.5) * result.step,
            ),
            flux,
          })
        }
      }
    }
    return samples
  }

  dump() {
    return {
      id: this.id,
      title: this.title,
      coldTemperature: this.coldTemperature,
      hotTemperature: this.hotTemperature,
      coldFaces: this.coldFaces,
      hotFaces: this.hotFaces,
    }
  }

  static undump(dump) {
    return Object.assign(new ThermalSimulation(dump.id), dump)
  }
}
Serialize.register(ThermalSimulation, 'ThermalSimulation')


export function solveVoxelHeat(solid, coldFaces, hotFaces, coldTemperature, hotTemperature, onProgress=() => {}) {
  const coldTriangles = makeTriangles(coldFaces)
  const hotTriangles = makeTriangles(hotFaces)
  const { bounds, step, nx, ny, nz, grid, points, coordinates } = voxelizeSolid(solid, 14, Infinity, null,
    value => onProgress(value * 20))
  onProgress(20)

  const offsets = new Int32Array(points.length + 1)
  const neighbors = []
  const directions = [[-1,0,0], [1,0,0], [0,-1,0], [0,1,0], [0,0,-1], [0,0,1]]
  for(let node = 0; node < points.length; node++) {
    offsets[node] = neighbors.length
    const [i, j, k] = coordinates.subarray(node * 3, node * 3 + 3)
    for(const [di, dj, dk] of directions) {
      const x = i + di
      const y = j + dj
      const z = k + dk
      if(x < 0 || y < 0 || z < 0 || x >= nx || y >= ny || z >= nz) continue
      const other = grid[x + nx * (y + ny * z)]
      if(other >= 0) neighbors.push(other)
    }
  }
  offsets[points.length] = neighbors.length

  const temperatures = new Float64Array(points.length).fill((coldTemperature + hotTemperature) / 2)
  const fixed = new Uint8Array(points.length)
  const closest = new THREE.Vector3()
  const limit = step * step * 1.1
  const origin = bounds.min.toArray().map(value => value + step / 2)
  const tag = (triangles, temperature) => triangles.forEach(triangle =>
    forLatticeNear(triangle, Math.sqrt(limit), origin, [step, step, step], [nx, ny, nz], (i, j, k) => {
      const node = grid[i + nx * (j + ny * k)]
      if(node < 0 || triangle.closestPointToPoint(points[node], closest).distanceToSquared(points[node]) >= limit) return
      fixed[node] = 1
      temperatures[node] = temperature
    }))
  tag(coldTriangles, coldTemperature)
  tag(hotTriangles, hotTemperature)
  fixFloatingNodes(fixed, union => {
    for(let node = 0; node < points.length; node++) {
      for(let slot = offsets[node]; slot < offsets[node + 1]; slot++) union(node, neighbors[slot])
    }
  })

  // Steady state: every free cell holds the mean of its neighbors. Fixed
  // neighbors move to the right-hand side.
  const free = new Int32Array(points.length).fill(-1)
  const freeNodes = []
  fixed.forEach((value, node) => { if(!value) free[node] = freeNodes.push(node) - 1 })
  const diagonal = new Float64Array(freeNodes.length)
  const right = new Float64Array(freeNodes.length)
  const rowOffsets = new Int32Array(freeNodes.length + 1)
  const columns = []
  freeNodes.forEach((node, row) => {
    rowOffsets[row] = columns.length
    diagonal[row] = offsets[node + 1] - offsets[node]
    for(let slot = offsets[node]; slot < offsets[node + 1]; slot++) {
      const other = neighbors[slot]
      if(fixed[other]) right[row] += temperatures[other]
      else columns.push(free[other])
    }
  })
  rowOffsets[freeNodes.length] = columns.length
  const level = cellLevel({
    diagonal,
    offsets: rowOffsets,
    columns: Int32Array.from(columns),
    values: new Float64Array(columns.length).fill(-1),
    coordinates: Int32Array.from(freeNodes.flatMap(node => [...coordinates.subarray(node * 3, node * 3 + 3)])),
  })
  const solve = makeConjugateGradient(level.apply, makeMultigrid(level, coarsenCells), freeNodes.length)
  onProgress(30)
  const solution = Float64Array.from(freeNodes, node => temperatures[node])
  solve(right, solution, 1e-8, 500, fraction => onProgress(30 + fraction * 65))
  freeNodes.forEach((node, row) => temperatures[node] = solution[row])

  onProgress(98)
  return {
    min: bounds.min.toArray(),
    step,
    nx,
    ny,
    nz,
    grid,
    temperatures,
  }
}
