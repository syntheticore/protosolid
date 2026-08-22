import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'


export class ThermalSimulation {
  constructor(id) {
    this.id = id || makeID()
    this.title = 'Heat Flow'
    this.coldTemperature = 20
    this.hotTemperature = 100
    this.coldFaces = []
    this.hotFaces = []
    this.result = null
    this.revision = 0
  }

  resolve(tree) {
    const references = [...this.coldFaces, ...this.hotFaces]
    if(!this.coldFaces.length || !this.hotFaces.length) return 'Pick hot and cold faces'

    for(const reference of references) {
      const component = tree.findChild(reference.componentId)
      if(!component) return 'A picked face does not exist here'
      const solid = component.compound.solids().find(item => item.id == reference.solidId)
      if(!solid) return 'A picked face does not exist here'
      const face = solid.faces().find(item => item.id == reference.topoId)
      if(!face) return 'A picked face does not exist here'
      reference.item = face
    }

    const first = references[0]
    if(references.some(reference =>
      reference.componentId != first.componentId || reference.solidId != first.solidId
    )) return 'Pick faces on one solid'
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
    const i = Math.floor((position.x - result.min[0]) / result.step)
    const j = Math.floor((position.y - result.min[1]) / result.step)
    const k = Math.floor((position.z - result.min[2]) / result.step)

    for(let radius = 0; radius < 3; radius++) {
      for(let z = k - radius; z <= k + radius; z++) {
        for(let y = j - radius; y <= j + radius; y++) {
          for(let x = i - radius; x <= i + radius; x++) {
            if(x < 0 || y < 0 || z < 0 || x >= result.nx || y >= result.ny || z >= result.nz) continue
            const node = result.grid[x + result.nx * (y + result.ny * z)]
            if(node >= 0) return result.temperatures[node]
          }
        }
      }
    }
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
  const triangles = makeTriangles(solid.faces())
  const coldTriangles = makeTriangles(coldFaces)
  const hotTriangles = makeTriangles(hotFaces)
  const bounds = new THREE.Box3()
  triangles.forEach(triangle => {
    bounds.expandByPoint(triangle.a)
    bounds.expandByPoint(triangle.b)
    bounds.expandByPoint(triangle.c)
  })

  const size = bounds.getSize(new THREE.Vector3())
  const step = Math.max(size.x, size.y, size.z) / 14
  const nx = Math.ceil(size.x / step)
  const ny = Math.ceil(size.y / step)
  const nz = Math.ceil(size.z / step)
  const grid = new Int32Array(nx * ny * nz).fill(-1)
  const points = []

  for(let k = 0; k < nz; k++) {
    for(let j = 0; j < ny; j++) {
      for(let i = 0; i < nx; i++) {
        const point = new THREE.Vector3(
          bounds.min.x + (i + 0.5) * step,
          bounds.min.y + (j + 0.5) * step,
          bounds.min.z + (k + 0.5) * step,
        )
        if(!isInside(point, triangles)) continue
        grid[i + nx * (j + ny * k)] = points.length
        points.push(point)
      }
    }
  }

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


function makeTriangles(faces) {
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

  const tx = origin.x - triangle.a.x
  const ty = origin.y - triangle.a.y
  const tz = origin.z - triangle.a.z
  const u = (tx * px + ty * py + tz * pz) / det
  if(u < 0 || u > 1) return false

  const qx = ty * e1z - tz * e1y
  const qy = tz * e1x - tx * e1z
  const qz = tx * e1y - ty * e1x
  const v = (dx * qx + dy * qy + dz * qz) / det
  if(v < 0 || u + v > 1) return false

  const distance = (e2x * qx + e2y * qy + e2z * qz) / det
  return distance > 1e-8
}
