import * as THREE from 'three'

import Serialize from './serialize.js'
import { makeID } from './id.js'
import { saveFile } from '../utils.js'
import { componentTriangles } from './export.js'
import { relativeComponentTransform } from './assembly.js'
import { threeVector } from './shapex.js'


// Program start, tool change and end for each supported machine controller
export const postProcessors = {
  grbl: {
    label: 'GRBL',
    percent: false,
    header: ['G21 G90 G94', 'G17', 'G54'],
    // GRBL has no tool changer, so it pauses for a manual change. The new
    // tool is longer or shorter, so Z has to be zeroed again.
    toolChange: (number, description) => [
      'M5',
      `(Change to T${number}: ${description}, then zero Z on the stock top)`,
      'M0',
    ],
    footer: ['M5', 'M30'],
  },
  linuxcnc: {
    label: 'LinuxCNC',
    percent: true,
    header: ['G21 G90 G94', 'G17 G40 G49 G80', 'G54'],
    toolChange: number => [`T${number} M6`, `G43 H${number}`],
    footer: ['M5', 'M9', 'M2'],
  },
}

// Listed in machining order: each stage removes what the next one cannot
export const operationTypes = {
  face: {
    label: 'Face',
    icon: 'layer-group',
    defaults: { toolDiameter: 10, stepover: 6, stepdown: 1, feed: 800, plungeFeed: 300, spindleSpeed: 10000 },
  },
  rough: {
    label: 'Rough',
    icon: 'cubes',
    defaults: {
      toolDiameter: 6, shape: 'flat', direction: 'x', stepover: 2.4, stepdown: 2, leave: 0.3,
      feed: 1000, plungeFeed: 300, spindleSpeed: 12000,
    },
  },
  contour: {
    label: 'Contour',
    icon: 'draw-polygon',
    defaults: {
      toolDiameter: 6, shape: 'flat', stepdown: 1, leave: 0,
      feed: 800, plungeFeed: 300, spindleSpeed: 12000,
    },
  },
  finish: {
    label: 'Finish',
    icon: 'water',
    defaults: {
      toolDiameter: 4, shape: 'ball', direction: 'xy', stepover: 0.4,
      feed: 1200, plungeFeed: 400, spindleSpeed: 15000,
    },
  },
  drill: {
    label: 'Drill',
    icon: 'dot-circle',
    defaults: { toolDiameter: 5, peck: 2, plungeFeed: 150, spindleSpeed: 5000 },
  },
}

export const toolShapes = {
  flat: 'Flat end mill',
  ball: 'Ball end mill',
}

export const cutDirections = {
  x: 'Along X',
  y: 'Along Y',
  xy: 'Along X and Y',
}

// The side of the part facing the spindle, with the machine X and Y axes
// that go with it
export const orientations = {
  '+Y': [[1, 0, 0], [0, 0, -1], [0, 1, 0]],
  '-Y': [[1, 0, 0], [0, 0, 1], [0, -1, 0]],
  '+Z': [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
  '-Z': [[1, 0, 0], [0, -1, 0], [0, 0, -1]],
  '+X': [[0, 0, -1], [0, 1, 0], [1, 0, 0]],
  '-X': [[0, 0, 1], [0, 1, 0], [-1, 0, 0]],
}

export const origins = {
  corner: 'Top front left corner',
  center: 'Top center',
}

// New operations go behind those of the same or an earlier stage
export function addOperation(setup, type) {
  const stages = Object.keys(operationTypes)
  const stage = stages.indexOf(type)
  const index = setup.operations.findIndex(operation => stages.indexOf(operation.type) > stage)
  const operation = { id: makeID(), type, ...operationTypes[type].defaults }
  // Drills start out fitting the smallest hole
  if(type == 'drill' && setup.result?.holes.length) {
    operation.toolDiameter = Math.min(...setup.result.holes.map(hole => hole.diameter))
  }
  setup.operations.splice(index < 0 ? setup.operations.length : index, 0, operation)
}

// Drills go into holes as wide as them, or slightly wider for clearance
export function drillFits(hole, operation) {
  return hole.diameter > operation.toolDiameter - 0.05 && hole.diameter < operation.toolDiameter + 0.6
}


// A machining setup of a component. While its treelet is open, the
// document is in CAM mode.
export class CamSetup {
  constructor(id) {
    this.id = id || makeID()
    this.title = 'Setup'
    this.up = '+Y'
    this.origin = 'corner'
    this.stockOffset = 1
    this.clampHeight = 6
    this.quality = 3
    this.operations = []
    this.postProcessor = 'grbl'
    this.path = null
    this.autoSave = false
    this.result = null
  }

  dump() {
    return {
      id: this.id,
      title: this.title,
      up: this.up,
      origin: this.origin,
      stockOffset: this.stockOffset,
      clampHeight: this.clampHeight,
      quality: this.quality,
      operations: this.operations,
      postProcessor: this.postProcessor,
      path: this.path,
      autoSave: this.autoSave,
    }
  }

  static undump(dump) {
    return Object.assign(new CamSetup(dump.id), dump)
  }
}
Serialize.register(CamSetup, 'CamSetup')


// Above the stock top
const safeHeight = 5
// Typical rapid rate of hobby machines, for time estimates
const rapidRate = 3000
// Length of a 118° drill point per unit of radius
const drillTip = 1 / Math.tan(THREE.MathUtils.degToRad(59))

// Machine coordinates have Z pointing out of the side of the part facing the
// spindle. Internally their origin is the front left corner of the stock
// top; G-code shifts it to the chosen origin.
export function generateToolpaths(setup, component) {
  const triangles = componentTriangles(component, 0.05, THREE.MathUtils.degToRad(10))
  if(!triangles.length) return null

  const rotation = new THREE.Matrix4().makeBasis(
    ...orientations[setup.up].map(axis => new THREE.Vector3(...axis))
  )
  const unrotation = rotation.clone().transpose()
  const bounds = new THREE.Box3()
  triangles.forEach(triangle => triangle.forEach(vertex =>
    bounds.expandByPoint(vertex.clone().applyMatrix4(unrotation))
  ))
  // Stock surrounds the part by the offset, plus the height held by the vise
  bounds.min.x -= setup.stockOffset
  bounds.min.y -= setup.stockOffset
  bounds.max.x += setup.stockOffset
  bounds.max.y += setup.stockOffset
  bounds.max.z += setup.stockOffset
  bounds.min.z -= setup.clampHeight
  const size = bounds.getSize(new THREE.Vector3())

  // Quality refines the grid that paths and material removal are sampled on
  const cell = Math.max(Math.max(size.x, size.y) / (50 * (setup.quality + 1)), 0.75 / (setup.quality + 1))
  const nx = Math.ceil(size.x / cell)
  const ny = Math.ceil(size.y / cell)
  const corner = new THREE.Vector3(bounds.min.x, bounds.min.y, bounds.max.z).applyMatrix4(rotation)
  const machineToLocal = rotation.clone().setPosition(corner)
  const localToMachine = machineToLocal.clone().invert()

  const grid = {
    cell,
    nx,
    ny,
    width: nx * cell,
    depth: ny * cell,
    height: size.z,
    partTop: -setup.stockOffset,
    // The tool stays above the vise jaws, at the bottom of the part
    floor: -size.z + setup.clampHeight,
  }
  grid.heights = partHeights(triangles.map(triangle =>
    triangle.map(vertex => vertex.clone().applyMatrix4(localToMachine))
  ), grid)
  const holes = findHoles(component, localToMachine, grid)

  const operations = setup.operations.map((operation, index) => {
    const moves = operationMoves(operation, grid, holes)
    moves.forEach(move => move.op = index)
    return {
      operation,
      tool: {
        radius: operation.toolDiameter / 2,
        ball: operation.shape == 'ball',
        drill: operation.type == 'drill',
      },
      moves,
    }
  })
  const moves = operations.flatMap(operation => operation.moves)

  // Machining time on reaching each move
  const times = new Float64Array(moves.length)
  for(let i = 1; i < moves.length; i++) {
    const a = moves[i - 1]
    const b = moves[i]
    const operation = setup.operations[b.op]
    const rate = b.rapid ? rapidRate : isPlunge(a, b) ? operation.plungeFeed : operation.feed
    times[i] = times[i - 1] + distance(a, b) / rate
  }
  // Moving to its first point belongs to an operation
  let first = 0
  operations.forEach(operation => {
    operation.start = first ? times[first - 1] : 0
    first += operation.moves.length
    operation.end = first ? times[first - 1] : 0
  })

  const origin = setup.origin == 'center' ? { x: grid.width / 2, y: grid.depth / 2 } : { x: 0, y: 0 }
  const minutes = moves.length ? times.at(-1) : 0
  return { grid, machineToLocal, origin, holes, operations, moves, times, minutes }
}

function isPlunge(a, b) {
  return b.z < a.z && a.x == b.x && a.y == b.y
}

// Highest point of the part above each cell center, or the floor
function partHeights(triangles, grid) {
  const { cell, nx, ny, floor } = grid
  const heights = new Float32Array(nx * ny).fill(floor)
  for(const [a, b, c] of triangles) {
    const area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)
    if(Math.abs(area) < 1e-12) continue
    const i0 = Math.max(0, Math.ceil(Math.min(a.x, b.x, c.x) / cell - 0.5))
    const i1 = Math.min(nx - 1, Math.floor(Math.max(a.x, b.x, c.x) / cell - 0.5))
    const j0 = Math.max(0, Math.ceil(Math.min(a.y, b.y, c.y) / cell - 0.5))
    const j1 = Math.min(ny - 1, Math.floor(Math.max(a.y, b.y, c.y) / cell - 0.5))
    for(let j = j0; j <= j1; j++) {
      const y = (j + 0.5) * cell
      for(let i = i0; i <= i1; i++) {
        const x = (i + 0.5) * cell
        const u = ((b.x - x) * (c.y - y) - (c.x - x) * (b.y - y)) / area
        const v = ((c.x - x) * (a.y - y) - (a.x - x) * (c.y - y)) / area
        const w = 1 - u - v
        if(u < -1e-9 || v < -1e-9 || w < -1e-9) continue
        const z = u * a.z + v * b.z + w * c.z
        const index = i + nx * j
        if(z > heights[index]) heights[index] = z
      }
    }
  }
  return heights
}

function heightAt(grid, x, y) {
  const i = THREE.MathUtils.clamp(Math.floor(x / grid.cell), 0, grid.nx - 1)
  const j = THREE.MathUtils.clamp(Math.floor(y / grid.cell), 0, grid.ny - 1)
  return grid.heights[i + grid.nx * j]
}

// Round holes along the spindle axis, from the cylindrical faces of the
// part. Faces of one hole can be split at seams, so they are merged by axis.
function findHoles(component, localToMachine, grid) {
  const groups = new Map()
  component.getChildren().forEach(occurrence => {
    if(!occurrence.compound.geom) return
    const transform = localToMachine.clone().multiply(relativeComponentTransform(occurrence, component))
    occurrence.compound.faces().forEach(face => {
      const surface = face.geom().surface()
      if(surface.kind != 'cylinder') return
      const direction = threeVector(surface.axis.direction()).transformDirection(transform)
      if(Math.abs(direction.z) < 0.999) return
      const axis = threeVector(surface.axis.origin()).applyMatrix4(transform)
      const positions = face.tesselate().positions
      const points = []
      for(let k = 0; k < positions.length; k += 3) {
        points.push(new THREE.Vector3(positions[k], positions[k + 1], positions[k + 2]).applyMatrix4(transform))
      }
      const radius = Math.hypot(points[0].x - axis.x, points[0].y - axis.y)
      const key = [axis.x, axis.y, radius].map(value => value.toFixed(2)).join()
      if(!groups.has(key)) groups.set(key, { x: axis.x, y: axis.y, radius, top: -Infinity, bottom: Infinity, angles: [] })
      const group = groups.get(key)
      points.forEach(point => {
        group.top = Math.max(group.top, point.z)
        group.bottom = Math.min(group.bottom, point.z)
        group.angles.push(Math.atan2(point.y - axis.y, point.x - axis.x))
      })
    })
  })

  return [...groups.values()].filter(group => {
    // Partial cylinders are rounded corners
    const angles = group.angles.sort((a, b) => a - b)
    const gaps = angles.map((angle, index) => (index + 1 < angles.length ? angles[index + 1] : angles[0] + 2 * Math.PI) - angle)
    if(Math.max(...gaps) > Math.PI / 2) return false
    // Material at the axis makes it a boss
    return heightAt(grid, group.x, group.y) < group.top - 1e-3
  }).map(({ x, y, radius, top, bottom }) => {
    // Through holes end at the part bottom, or have a lower surface below
    const below = heightAt(grid, x, y)
    const through = bottom < grid.floor + 1e-3 || below < bottom - 1e-3
    return {
      x, y, top, bottom, through,
      diameter: Number((radius * 2).toFixed(2)),
      // Lowest the drill tip may go
      limit: !through ? bottom : below > grid.floor + 1e-3 ? below : -grid.height + 0.5,
    }
  })
}

// Lowest tool tip height at each cell center that does not cut into the
// part, on a grid extended by the tool radius on every side.
function dropCutter(grid, radius, ball) {
  const { cell, nx, ny, heights, floor } = grid
  const reach = Math.ceil(radius / cell)
  const offsets = []
  for(let dj = -reach; dj <= reach; dj++) {
    for(let di = -reach; di <= reach; di++) {
      const d = Math.hypot(di, dj) * cell
      if(d > radius) continue
      offsets.push(di, dj, ball ? radius - Math.sqrt(radius * radius - d * d) : 0)
    }
  }
  const ex = nx + 2 * reach
  const ey = ny + 2 * reach
  const values = new Float32Array(ex * ey)
  for(let J = 0; J < ey; J++) {
    for(let I = 0; I < ex; I++) {
      const i = I - reach
      const j = J - reach
      let best = floor
      for(let k = 0; k < offsets.length; k += 3) {
        const si = i + offsets[k]
        const sj = j + offsets[k + 1]
        if(si < 0 || sj < 0 || si >= nx || sj >= ny) continue
        const value = heights[si + nx * sj] - offsets[k + 2]
        if(value > best) best = value
      }
      values[I + ex * J] = best
    }
  }
  return { reach, ex, ey, values }
}

function operationMoves(operation, grid, holes) {
  if(operation.type == 'contour') return contourMoves(operation, grid)
  if(operation.type == 'drill') return drillMoves(operation, holes.filter(hole => drillFits(hole, operation)))
  // Rows along X, along Y or both, so walls of either direction get the
  // tool alongside them
  const passes = { x: [false], y: [true], xy: [false, true] }[operation.direction || 'x']
  return passes.flatMap(transposed => transposed ?
    rasterMoves(operation, transpose(grid)).map(move => ({ ...move, x: move.y, y: move.x })) :
    rasterMoves(operation, grid)
  )
}

function transpose(grid) {
  const { nx, ny } = grid
  const heights = new Float32Array(nx * ny)
  for(let j = 0; j < ny; j++) {
    for(let i = 0; i < nx; i++) heights[j + ny * i] = grid.heights[i + nx * j]
  }
  return { ...grid, nx: ny, ny: nx, width: grid.depth, depth: grid.width, heights }
}

// Zigzag raster in layers from the stock top down
function rasterMoves(operation, grid) {
  const radius = operation.toolDiameter / 2
  let levels
  let reach
  let tipAt
  if(operation.type == 'face') {
    // Facing stops at the top of the part
    reach = Math.ceil(radius / grid.cell)
    tipAt = () => grid.partTop
    levels = layers(grid.partTop, operation.stepdown)
  } else {
    const rough = operation.type == 'rough'
    const surface = rough ?
      dropCutter(grid, radius + operation.leave, operation.shape == 'ball') :
      dropCutter(grid, radius, operation.shape == 'ball')
    const leave = rough ? operation.leave : 0
    reach = surface.reach
    tipAt = (I, J) => surface.values[I + surface.ex * J] + leave
    levels = rough ? layers(grid.floor, operation.stepdown) : [-Infinity]
  }

  // Rows and columns reach past the stock, so the tool clears its walls
  const columns = grid.nx + 2 * reach
  const lastRow = grid.ny + 2 * reach - 1
  const rowStep = Math.max(1, Math.round(operation.stepover / grid.cell))
  const rows = []
  for(let J = 0; J < lastRow; J += rowStep) rows.push(J)
  rows.push(lastRow)

  const x = I => (I - reach + 0.5) * grid.cell
  const y = J => (J - reach + 0.5) * grid.cell
  const moves = []
  for(const level of levels) {
    const points = []
    const add = (I, J) => points.push({ x: x(I), y: y(J), z: Math.max(level, tipAt(I, J)) })
    rows.forEach((row, index) => {
      const forward = index % 2 == 0
      for(let k = 0; k < columns; k++) add(forward ? k : columns - 1 - k, row)
      // Link to the next row along the stock edge
      const next = rows[index + 1]
      if(next !== undefined) {
        for(let J = row + 1; J < next; J++) add(forward ? columns - 1 : 0, J)
      }
    })
    const path = simplify(steps(points, grid.cell))
    moves.push(
      { x: path[0].x, y: path[0].y, z: safeHeight, rapid: true },
      ...path.map(point => ({ ...point, rapid: false })),
      { x: path.at(-1).x, y: path.at(-1).y, z: safeHeight, rapid: true },
    )
  }
  return moves
}

// Waterline: at each level the tool goes around the outlines of the part,
// keeping them on its right, so it climb mills.
function contourMoves(operation, grid) {
  const radius = operation.toolDiameter / 2 + operation.leave
  const ball = operation.shape == 'ball'
  const surface = dropCutter(grid, radius, ball)
  const moves = []
  let position = { x: 0, y: 0 }
  for(const level of layers(grid.floor, operation.stepdown)) {
    const loops = isolines(surface, grid, Math.max(level, grid.floor + 1e-3), radius, ball)
    while(loops.length) {
      const loop = takeClosest(loops, position)
      const path = simplify(loop.map(point => ({ x: point.x, y: point.y, z: level })))
      moves.push(
        { x: path[0].x, y: path[0].y, z: safeHeight, rapid: true },
        ...path.map(point => ({ ...point, rapid: false })),
        { x: path[0].x, y: path[0].y, z: safeHeight, rapid: true },
      )
      position = path[0]
    }
  }
  return moves
}

// Closed outlines where the drop cutter surface rises above the threshold,
// with the higher side on their right. Marching squares on the extended
// grid, whose border counts as low, so every outline closes.
function isolines(surface, grid, threshold, radius, ball) {
  const { reach, ex, ey, values } = surface
  const { cell, nx, ny, heights } = grid
  const high = (I, J) => I > 0 && J > 0 && I < ex - 1 && J < ey - 1 && values[I + ex * J] > threshold

  // Where the tool, moving from a low to a high sample, first touches the
  // part. Interpolating would cut into the part by up to half a cell.
  const crossing = (I0, J0, I1, J1) => {
    const [from, to] = high(I0, J0) ? [[I1, J1], [I0, J0]] : [[I0, J0], [I1, J1]]
    const a = { x: (from[0] - reach + 0.5) * cell, y: (from[1] - reach + 0.5) * cell }
    const dx = (to[0] - from[0]) * cell
    const dy = (to[1] - from[1]) * cell
    const i0 = Math.max(0, Math.floor((Math.min(a.x, a.x + dx) - radius) / cell - 0.5))
    const i1 = Math.min(nx - 1, Math.ceil((Math.max(a.x, a.x + dx) + radius) / cell - 0.5))
    const j0 = Math.max(0, Math.floor((Math.min(a.y, a.y + dy) - radius) / cell - 0.5))
    const j1 = Math.min(ny - 1, Math.ceil((Math.max(a.y, a.y + dy) + radius) / cell - 0.5))
    const length2 = dx * dx + dy * dy
    let t = 1
    for(let j = j0; j <= j1; j++) {
      for(let i = i0; i <= i1; i++) {
        const rise = heights[i + nx * j] - threshold
        if(rise <= 0) continue
        // A ball only reaches as far as its curved tip allows
        const reach2 = !ball || rise >= radius ? radius * radius : rise * (2 * radius - rise)
        const cx = (i + 0.5) * cell - a.x
        const cy = (j + 0.5) * cell - a.y
        const b = dx * cx + dy * cy
        const discriminant = b * b - length2 * (cx * cx + cy * cy - reach2)
        if(discriminant < 0) continue
        const root = Math.sqrt(discriminant)
        if(b + root < 0) continue
        t = Math.min(t, Math.max(0, (b - root) / length2))
      }
    }
    // Stay clear of the chords between points on curved outlines
    t = Math.max(0, t - 0.01 / Math.sqrt(length2))
    return { x: a.x + dx * t, y: a.y + dy * t }
  }
  const next = new Map()
  const points = new Map()
  for(let J = 0; J < ey - 1; J++) {
    for(let I = 0; I < ex - 1; I++) {
      // Corners counterclockwise, each with the cell edge leaving it
      const corners = [[I, J], [I + 1, J], [I + 1, J + 1], [I, J + 1]]
      const edges = [2 * (I + ex * J), 2 * (I + 1 + ex * J) + 1, 2 * (I + ex * (J + 1)), 2 * (I + ex * J) + 1]
      const flags = corners.map(([i, j]) => high(i, j))
      const crossings = []
      for(let k = 0; k < 4; k++) {
        if(flags[k] == flags[(k + 1) % 4]) continue
        crossings.push({ edge: edges[k], entry: !flags[k] })
        if(!points.has(edges[k])) points.set(edges[k], crossing(...corners[k], ...corners[(k + 1) % 4]))
      }
      if(!crossings.length) continue
      // Saddles connect through the middle when it is high
      const joined = crossings.length == 4 &&
        corners.reduce((sum, [i, j]) => sum + values[i + ex * j], 0) / 4 > threshold
      crossings.forEach((entry, index) => {
        if(!entry.entry) return
        const exit = crossings.length == 2 ?
          crossings[1 - index] :
          crossings[(index + (joined ? 3 : 1)) % 4]
        next.set(entry.edge, exit.edge)
      })
    }
  }

  const loops = []
  for(const start of next.keys()) {
    if(!next.has(start)) continue
    const loop = []
    let edge = start
    do {
      loop.push(points.get(edge))
      const following = next.get(edge)
      next.delete(edge)
      edge = following
    } while(edge != start)
    loop.push(loop[0])
    loops.push(loop)
  }
  return loops
}

// Removes the closed loop closest to the position, starting it there
function takeClosest(loops, position) {
  let best = null
  loops.forEach((loop, index) => loop.forEach((point, start) => {
    const d = Math.hypot(point.x - position.x, point.y - position.y)
    if(!best || d < best.d) best = { d, index, start }
  }))
  const open = loops.splice(best.index, 1)[0].slice(0, -1)
  const rotated = [...open.slice(best.start), ...open.slice(0, best.start)]
  return [...rotated, rotated[0]]
}

// Peck drilling, backing out of the hole after each peck to clear chips
function drillMoves(operation, holes) {
  const remaining = [...holes]
  const moves = []
  let position = { x: 0, y: 0 }
  while(remaining.length) {
    const index = remaining.reduce((best, hole, index) =>
      distance(hole, position) < distance(remaining[best], position) ? index : best, 0)
    const { x, y, top, bottom, through, limit } = remaining.splice(index, 1)[0]
    // Through holes are drilled until the full diameter is out at the bottom
    const depth = through ? Math.max(bottom - operation.toolDiameter / 2 * drillTip - 0.5, limit) : bottom
    const retract = top + 1
    moves.push({ x, y, z: safeHeight, rapid: true }, { x, y, z: retract, rapid: true })
    for(let reached = top; reached > depth;) {
      const target = Math.max(reached - operation.peck, depth)
      if(reached < top) moves.push({ x, y, z: reached + 0.5, rapid: true })
      moves.push({ x, y, z: target, rapid: false }, { x, y, z: retract, rapid: true })
      reached = target
    }
    moves.push({ x, y, z: safeHeight, rapid: true })
    position = { x, y, z: 0 }
  }
  return moves
}

function layers(bottom, stepdown) {
  const levels = []
  for(let z = -stepdown; z > bottom; z -= stepdown) levels.push(z)
  levels.push(bottom)
  return levels
}

// Heights are only known at the samples. Moving straight between samples
// of very different height would cut through what lies between them, so
// steep changes climb before or descend after the move across.
function steps(points, cell) {
  const result = [points[0]]
  for(let i = 1; i < points.length; i++) {
    const a = points[i - 1]
    const b = points[i]
    if(b.z - a.z > cell) result.push({ x: a.x, y: a.y, z: b.z })
    if(a.z - b.z > cell) result.push({ x: b.x, y: b.y, z: a.z })
    result.push(b)
  }
  return result
}

// Drop points in the middle of straight runs
function simplify(points) {
  const result = [points[0]]
  for(let i = 1; i < points.length - 1; i++) {
    const a = result.at(-1)
    const b = points[i]
    const c = points[i + 1]
    const cross = new THREE.Vector3(b.x - a.x, b.y - a.y, b.z - a.z)
      .cross(new THREE.Vector3(c.x - b.x, c.y - b.y, c.z - b.z))
    if(cross.lengthSq() > 1e-10) result.push(b)
  }
  result.push(points.at(-1))
  return result
}

function distance(a, b) {
  return Math.hypot(b.x - a.x, b.y - a.y, (b.z || 0) - (a.z || 0))
}


// Material removal on a height field of the stock, sweeping the tool along
// the toolpath. Seeks to any machining time; going back starts over from
// the closest earlier snapshot of the stock.
export class CamSimulation {
  constructor(result) {
    this.result = result
    const { nx, ny } = result.grid
    this.heights = new Float32Array(nx * ny)
    this.snapshots = [{ time: 0, index: 0, heights: this.heights.slice() }]
    this.interval = result.minutes / 64
    this.index = 0
    this.offset = 0
    this.time = 0
    this.position = { ...result.moves[0] }
  }

  seek(time) {
    const { moves, times, minutes } = this.result
    time = THREE.MathUtils.clamp(time, 0, minutes)
    const snapshot = this.snapshots.findLast(snapshot => snapshot.time <= time)
    if(time < this.time || snapshot.index > this.index) {
      this.heights.set(snapshot.heights)
      this.index = snapshot.index
      this.offset = 0
    }
    while(this.index < moves.length - 1 && times[this.index + 1] <= time) {
      this.cutTo(distance(moves[this.index], moves[this.index + 1]))
      this.index++
      this.offset = 0
      if(times[this.index] >= this.snapshots.at(-1).time + this.interval) {
        this.snapshots.push({ time: times[this.index], index: this.index, heights: this.heights.slice() })
      }
    }
    const a = moves[this.index]
    const b = moves[Math.min(this.index + 1, moves.length - 1)]
    const duration = times[Math.min(this.index + 1, moves.length - 1)] - times[this.index]
    const t = duration > 0 ? (time - times[this.index]) / duration : 0
    if(a !== b) this.cutTo(t * distance(a, b))
    this.time = time
    this.position = {
      x: a.x + (b.x - a.x) * t,
      y: a.y + (b.y - a.y) * t,
      z: a.z + (b.z - a.z) * t,
      op: b.op,
    }
  }

  // Sweeps the tool along the current move, up to the given length into it
  cutTo(length) {
    const a = this.result.moves[this.index]
    const b = this.result.moves[this.index + 1]
    const segment = distance(a, b)
    const advance = length - this.offset
    if(advance <= 0 || segment == 0) return
    const tool = this.result.operations[b.op].tool
    const stamps = Math.ceil(advance / (this.result.grid.cell / 2))
    for(let k = 1; k <= stamps; k++) {
      const t = (this.offset + advance * k / stamps) / segment
      this.cut(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t, tool)
    }
    this.offset = length
  }

  cut(x, y, z, { radius, ball, drill }) {
    if(z >= 0) return
    const { cell, nx, ny } = this.result.grid
    const i0 = Math.max(0, Math.ceil((x - radius) / cell - 0.5))
    const i1 = Math.min(nx - 1, Math.floor((x + radius) / cell - 0.5))
    const j0 = Math.max(0, Math.ceil((y - radius) / cell - 0.5))
    const j1 = Math.min(ny - 1, Math.floor((y + radius) / cell - 0.5))
    for(let j = j0; j <= j1; j++) {
      const dy = (j + 0.5) * cell - y
      for(let i = i0; i <= i1; i++) {
        const dx = (i + 0.5) * cell - x
        const d2 = dx * dx + dy * dy
        if(d2 > radius * radius) continue
        const bottom = ball ? z + radius - Math.sqrt(radius * radius - d2) : drill ? z + Math.sqrt(d2) * drillTip : z
        const index = i + nx * j
        if(bottom < this.heights[index]) this.heights[index] = bottom
      }
    }
  }
}


function toolDescription(operation) {
  const shape = operation.type == 'drill' ? 'drill' : toolShapes[operation.shape || 'flat'].toLowerCase()
  return `${operation.toolDiameter} mm ${shape}`
}

// G-code for the setup's controller
function postProcess(setup, result) {
  const post = postProcessors[setup.postProcessor]
  const format = value => Number(value.toFixed(3)).toString()
  const lines = []
  if(post.percent) lines.push('%')
  lines.push(`(${setup.title})`)
  if(result) {
    const { width, depth, height } = result.grid
    lines.push(
      `(Stock ${format(width)} x ${format(depth)} x ${format(height)} mm, ${setup.clampHeight} mm held in the vise)`,
      `(Zero at the ${origins[setup.origin].toLowerCase()})`,
    )
  }
  lines.push(...post.header)

  // Operations using the same tool share its number
  const tools = []
  let previousTool = null
  result?.operations.forEach(({ operation, moves }) => {
    if(!moves.length) return
    const description = toolDescription(operation)
    let number = tools.indexOf(description) + 1
    if(!number) number = tools.push(description)
    lines.push(`(${operationTypes[operation.type].label} with T${number}: ${description})`)
    if(description != previousTool) {
      // GRBL starts with the tool its operator put in for the first operation
      if(previousTool || setup.postProcessor != 'grbl') lines.push(...post.toolChange(number, description))
      // Give the spindle time to reach its speed
      lines.push(`S${operation.spindleSpeed} M3`, 'G4 P3')
    }
    previousTool = description
    lines.push(`G0 Z${safeHeight}`)

    const current = { z: format(safeHeight) }
    let mode = 'G0'
    let feed = null
    moves.forEach((move, index) => {
      const position = { x: move.x - result.origin.x, y: move.y - result.origin.y, z: move.z }
      const words = ['x', 'y', 'z']
        .filter(axis => format(position[axis]) !== current[axis])
        .map(axis => {
          current[axis] = format(position[axis])
          return axis.toUpperCase() + current[axis]
        })
      if(!words.length) return
      const code = move.rapid ? 'G0' : 'G1'
      if(code == 'G1') {
        const previous = moves[index - 1]
        const rate = previous && isPlunge(previous, move) ? operation.plungeFeed : operation.feed
        if(rate != feed || mode != 'G1') words.push(`F${rate}`)
        feed = rate
      }
      lines.push([code, ...words].join(' '))
      mode = code
    })
  })
  lines.push(...post.footer)
  if(post.percent) lines.push('%')
  lines.push('')
  return lines.join('\n')
}

export async function exportGcode(setup, component) {
  const result = setup.result || generateToolpaths(setup, component)
  return await saveFile(postProcess(setup, result), 'nc', setup.path, setup.title)
}
