import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { resolveSimulationFaces } from './simulation.js'
import { findVoxelNode, makeTriangles, voxelizeSolid } from './voxel.js'


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

  resolve(tree) {
    if(!this.coldFaces.length || !this.hotFaces.length) return 'Pick hot and cold faces'
    return resolveSimulationFaces(tree, [this.coldFaces, this.hotFaces])
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
    this.result.solidId = this.coldFaces[0].solidId
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


function solveVoxelHeat(solid, coldFaces, hotFaces, coldTemperature, hotTemperature) {
  const coldTriangles = makeTriangles(coldFaces)
  const hotTriangles = makeTriangles(hotFaces)
  const { bounds, step, nx, ny, nz, grid, points } = voxelizeSolid(solid)

  const neighbors = points.map(() => [])
  const directions = [[-1,0,0], [1,0,0], [0,-1,0], [0,1,0], [0,0,-1], [0,0,1]]
  for(let k = 0; k < nz; k++) {
    for(let j = 0; j < ny; j++) {
      for(let i = 0; i < nx; i++) {
        const node = grid[i + nx * (j + ny * k)]
        if(node < 0) continue
        directions.forEach(([di, dj, dk]) => {
          const x = i + di
          const y = j + dj
          const z = k + dk
          if(x < 0 || y < 0 || z < 0 || x >= nx || y >= ny || z >= nz) return
          const other = grid[x + nx * (y + ny * z)]
          if(other >= 0) neighbors[node].push(other)
        })
      }
    }
  }

  const temperatures = new Float64Array(points.length).fill((coldTemperature + hotTemperature) / 2)
  const fixed = new Map()
  const closest = new THREE.Vector3()
  const limit = step * step * 1.1
  points.forEach((point, node) => {
    if(coldTriangles.some(triangle => triangle.closestPointToPoint(point, closest).distanceToSquared(point) < limit)) {
      fixed.set(node, coldTemperature)
    }
    if(hotTriangles.some(triangle => triangle.closestPointToPoint(point, closest).distanceToSquared(point) < limit)) {
      fixed.set(node, hotTemperature)
    }
  })
  fixed.forEach((temperature, node) => temperatures[node] = temperature)

  for(let iteration = 0; iteration < 400; iteration++) {
    points.forEach((_, node) => {
      if(fixed.has(node)) return
      const adjacent = neighbors[node]
      temperatures[node] = adjacent.reduce((sum, other) => sum + temperatures[other], 0) / adjacent.length
    })
  }

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
