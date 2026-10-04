import * as THREE from 'three'

import { makeTriangles, voxelizeSolid } from './voxel.js'


const boundaryTypes = { wall: 0, inlet: 1, interior: 2, outlet: 3 }


// Staggered-grid incompressible Navier–Stokes: upwind advection, explicit
// viscosity, then a pressure projection. Geometry is in mm; physics is in SI.
// Unpicked boundaries are stationary no-slip walls; outlets fix gauge pressure.
// Pressure discretization: https://www.cs.ubc.ca/~rbridson/fluidsimulation/fluids_notes.pdf
export function solveVoxelFlow(solid, inletFaces, outletFaces, settings) {
  const { bounds, step, nx, ny, nz, grid, points } = voxelizeSolid(solid, settings.resolution)
  if(!points.length) return 'The fluid volume is too thin for this grid'
  const h = step / 1000
  const nu = settings.viscosity / settings.density
  const cell = (i, j, k) => i < 0 || j < 0 || k < 0 || i >= nx || j >= ny || k >= nz
    ? -1 : grid[i + nx * (j + ny * k)]
  const surfaces = solid.faces().flatMap(face => makeTriangles([face]).map(triangle => ({
    triangle,
    normal: triangle.getNormal(new THREE.Vector3()),
    type: inletFaces.includes(face) ? 'inlet' : outletFaces.includes(face) ? 'outlet' : 'wall',
  })))
  const closest = new THREE.Vector3()
  const axes = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
  const dimensions = [[nx + 1, ny, nz], [nx, ny + 1, nz], [nx, ny, nz + 1]]
  const fields = dimensions.map(([x, y, z]) => ({
    dimensions: [x, y, z],
    indices: new Int32Array(x * y * z).fill(-1),
  }))
  const faces = []
  const cellFaces = points.map(() => [])
  let inletCount = 0
  let outletCount = 0
  fields.forEach((field, axis) => {
    const [sx, sy, sz] = field.dimensions
    const offset = axes[axis]
    for(let k = 0; k < sz; k++) {
      for(let j = 0; j < sy; j++) {
        for(let i = 0; i < sx; i++) {
          const a = cell(i - offset[0], j - offset[1], k - offset[2])
          const b = cell(i, j, k)
          if(a < 0 && b < 0) continue
          const coordinates = [i + 0.5, j + 0.5, k + 0.5]
          coordinates[axis] -= 0.5
          let type = 'interior'
          let alignment = 1
          if(a < 0 || b < 0) {
            const position = new THREE.Vector3(...coordinates).multiplyScalar(step).add(bounds.min)
            let distance = Infinity
            type = 'wall'
            surfaces.forEach(surface => {
              const dot = Math.abs(surface.normal.getComponent(axis))
              if(dot < 0.1) return
              const d = surface.triangle.closestPointToPoint(position, closest).distanceToSquared(position)
              if(d < distance) {
                distance = d
                type = surface.type
                alignment = dot
              }
            })
            if(type == 'inlet') inletCount++
            if(type == 'outlet') outletCount++
          }
          const index = faces.length
          const sign = a < 0 ? 1 : -1
          faces.push({ a, b, axis, coordinates, type, inletVelocity: sign * settings.inletSpeed * alignment })
          field.indices[i + sx * (j + sy * k)] = index
          if(a >= 0) cellFaces[a].push({ index, sign: 1, other: b })
          if(b >= 0) cellFaces[b].push({ index, sign: -1, other: a })
        }
      }
    }
  })
  if(!inletCount || !outletCount) return 'The inlet or outlet is too small for this grid; increase grid cells'

  // Each connected region needs a pressure outlet to anchor its Poisson solve.
  const reached = new Set()
  const queue = faces.filter(face => face.type == 'outlet').map(face => Math.max(face.a, face.b))
  queue.forEach(node => reached.add(node))
  for(let cursor = 0; cursor < queue.length; cursor++) {
    cellFaces[queue[cursor]].forEach(({ other }) => {
      if(other >= 0 && !reached.has(other)) {
        reached.add(other)
        queue.push(other)
      }
    })
  }
  if(reached.size != points.length) return 'Every connected fluid region needs an outlet; check passages or increase grid cells'

  // The grid never moves. Precompute the neighbor and interpolation indices
  // once instead of rebuilding trilinear samples for every timestep.
  // An extra zero velocity slot represents absent neighbors in these stencils.
  const empty = faces.length
  const neighborFaces = new Int32Array(faces.length * 6).fill(empty)
  const transportFaces = new Int32Array(faces.length * 12).fill(empty)
  const faceTypes = new Uint8Array(faces.length)
  const inletVelocities = new Float64Array(faces.length)
  const faceIndex = (field, i, j, k) => {
    const [sx, sy, sz] = field.dimensions
    if(i < 0 || j < 0 || k < 0 || i >= sx || j >= sy || k >= sz) return empty
    const index = field.indices[i + sx * (j + sy * k)]
    return index < 0 ? empty : index
  }
  faces.forEach((face, index) => {
    faceTypes[index] = boundaryTypes[face.type]
    inletVelocities[index] = face.inletVelocity
    const base = face.coordinates.map((value, axis) => value - (axis == face.axis ? 0 : 0.5))
    axes.forEach((offset, axis) => {
      const minus = base.map((value, d) => value - offset[d])
      const plus = base.map((value, d) => value + offset[d])
      neighborFaces[index * 6 + axis * 2] = faceIndex(fields[face.axis], ...minus)
      neighborFaces[index * 6 + axis * 2 + 1] = faceIndex(fields[face.axis], ...plus)
      if(face.type == 'outlet' && axis == face.axis) {
        neighborFaces[index * 6 + axis * 2 + (face.a < 0 ? 0 : 1)] = index
      }
      if(axis == face.axis) return
      const q = face.coordinates.map((value, d) => value - (d == axis ? 0 : 0.5))
      const lower = q.map(Math.floor)
      let slot = index * 12 + axis * 4
      // Cross-component samples lie halfway between four staggered faces.
      for(let z = 0; z <= (q[2] == lower[2] ? 0 : 1); z++) {
        for(let y = 0; y <= (q[1] == lower[1] ? 0 : 1); y++) {
          for(let x = 0; x <= (q[0] == lower[0] ? 0 : 1); x++) {
            transportFaces[slot++] = faceIndex(fields[axis], lower[0] + x, lower[1] + y, lower[2] + z)
          }
        }
      }
    })
  })

  const cellFaceIndices = new Int32Array(points.length * 6)
  const cellFaceSigns = new Int8Array(points.length * 6)
  const diagonal = new Float64Array(points.length)
  const pressureOffsets = new Int32Array(points.length + 1)
  const pressureNeighbors = []
  cellFaces.forEach((adjacent, node) => {
    pressureOffsets[node] = pressureNeighbors.length
    adjacent.forEach(({ index, sign, other }, slot) => {
      cellFaceIndices[node * 6 + slot] = index
      cellFaceSigns[node * 6 + slot] = sign
      if(faces[index].type == 'interior') {
        diagonal[node]++
        pressureNeighbors.push(other)
      } else if(faces[index].type == 'outlet') diagonal[node] += 2
    })
  })
  pressureOffsets[points.length] = pressureNeighbors.length
  const neighbors = new Int32Array(pressureNeighbors)
  const applyPressure = (input, output) => {
    for(let node = 0; node < points.length; node++) {
      let value = diagonal[node] * input[node]
      for(let slot = pressureOffsets[node]; slot < pressureOffsets[node + 1]; slot++) value -= input[neighbors[slot]]
      output[node] = value
    }
  }
  const pressureSolver = makePressureSolver(applyPressure, diagonal)
  let velocity = new Float64Array(faces.length + 1)
  let next = new Float64Array(faces.length + 1)
  const right = new Float64Array(points.length)
  let pressure

  let time = 0
  let converged = false
  let relativeChange = Infinity
  let iterations = 0
  let continuityError = 0
  const flowTime = Math.max(nx, ny, nz) * h / settings.inletSpeed
  for(; iterations < 300; iterations++) {
    let maximum = settings.inletSpeed
    for(let index = 0; index < faces.length; index++) maximum = Math.max(maximum, Math.abs(velocity[index]))
    const dt = 0.8 / (3 * maximum / h + 6 * nu / (h * h))
    for(let index = 0; index < faces.length; index++) {
      const type = faceTypes[index]
      if(type == boundaryTypes.wall) continue
      if(type == boundaryTypes.inlet) {
        next[index] = inletVelocities[index]
        continue
      }
      const center = velocity[index]
      let advection = 0
      let laplacian = 0
      for(let axis = 0; axis < 3; axis++) {
        const slot = index * 6 + axis * 2
        const left = velocity[neighborFaces[slot]]
        const right = velocity[neighborFaces[slot + 1]]
        const cross = index * 12 + axis * 4
        const transport = axis == faces[index].axis ? center : (
          velocity[transportFaces[cross]] + velocity[transportFaces[cross + 1]] +
          velocity[transportFaces[cross + 2]] + velocity[transportFaces[cross + 3]]
        ) * 0.25
        advection += transport * (transport >= 0 ? center - left : right - center) / h
        laplacian += (left - 2 * center + right) / (h * h)
      }
      next[index] = center + dt * (nu * laplacian - advection)
    }
    for(let node = 0; node < points.length; node++) {
      let divergence = 0
      for(let slot = node * 6; slot < node * 6 + 6; slot++) {
        divergence += cellFaceSigns[slot] * next[cellFaceIndices[slot]]
      }
      right[node] = -settings.density * h / dt * divergence
    }
    pressure = pressureSolver(right)
    for(let index = 0; index < faces.length; index++) {
      const face = faces[index]
      if(faceTypes[index] == boundaryTypes.interior) next[index] -= dt / (settings.density * h) * (pressure[face.b] - pressure[face.a])
      if(faceTypes[index] == boundaryTypes.outlet) {
        const gradient = face.a < 0 ? 2 * pressure[face.b] : -2 * pressure[face.a]
        next[index] -= dt / (settings.density * h) * gradient
      }
    }
    let change = 0
    let magnitude = 0
    for(let index = 0; index < faces.length; index++) {
      const value = next[index]
      change += (value - velocity[index]) ** 2
      magnitude += value * value
    }
    // Normalize by the timestep so a small timestep cannot imply steady flow.
    relativeChange = Math.sqrt(change / magnitude) * flowTime / dt
    const previous = velocity
    velocity = next
    next = previous
    time += dt
    if(time > flowTime && relativeChange < 0.005) {
      converged = true
      iterations++
      break
    }
  }
  const velocities = new Float64Array(points.length * 3)
  const pressures = pressure.map(value => value + settings.outletPressure)
  let maxSpeed = 0
  cellFaces.forEach((adjacent, node) => {
    let divergence = 0
    adjacent.forEach(({ index, sign }) => {
      velocities[node * 3 + faces[index].axis] += velocity[index] / 2
      divergence += sign * velocity[index]
    })
    continuityError = Math.max(continuityError, Math.abs(divergence) / settings.inletSpeed)
    maxSpeed = Math.max(maxSpeed, Math.hypot(velocities[node * 3], velocities[node * 3 + 1], velocities[node * 3 + 2]))
  })
  let inletFlow = 0, outletFlow = 0
  faces.forEach((face, index) => {
    const outward = face.a < 0 ? -velocity[index] : velocity[index]
    if(face.type == 'inlet') inletFlow -= outward * h * h
    if(face.type == 'outlet') outletFlow += outward * h * h
  })
  const inletNodes = new Set(faces.filter(face => face.type == 'inlet').map(face => Math.max(face.a, face.b)))
  const inletPositions = Float64Array.from([...inletNodes].flatMap(node => points[node].toArray()))
  return {
    min: bounds.min.toArray(), step, nx, ny, nz, grid, velocities, pressures,
    inletPositions,
    maxSpeed, inletFlow, outletFlow, time, iterations, converged, relativeChange, continuityError,
  }
}


function makePressureSolver(apply, diagonal) {
  // Reuse PCG workspace across all pressure projections in this solve.
  const solution = new Float64Array(diagonal.length)
  const residual = new Float64Array(diagonal.length)
  const preconditioned = new Float64Array(diagonal.length)
  const direction = new Float64Array(diagonal.length)
  const applied = new Float64Array(diagonal.length)
  return right => {
    solution.fill(0)
    residual.set(right)
    for(let i = 0; i < right.length; i++) {
      preconditioned[i] = residual[i] / diagonal[i]
      direction[i] = preconditioned[i]
    }
    let product = dot(residual, preconditioned)
    const target = dot(right, right) * 1e-12
    for(let iteration = 0; iteration < 200 && dot(residual, residual) > target; iteration++) {
      apply(direction, applied)
      const alpha = product / dot(direction, applied)
      for(let i = 0; i < right.length; i++) {
        solution[i] += alpha * direction[i]
        residual[i] -= alpha * applied[i]
        preconditioned[i] = residual[i] / diagonal[i]
      }
      const nextProduct = dot(residual, preconditioned)
      const beta = nextProduct / product
      for(let i = 0; i < right.length; i++) direction[i] = preconditioned[i] + beta * direction[i]
      product = nextProduct
    }
    return solution
  }
}

function dot(a, b) {
  let value = 0
  for(let i = 0; i < a.length; i++) value += a[i] * b[i]
  return value
}
