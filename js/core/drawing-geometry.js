import * as THREE from 'three'
import { MeshBVH } from 'three-mesh-bvh'

import { relativeComponentTransform } from './assembly.js'
import { tessellationOptions } from './shapex.js'


// Triangles with vertex normals and semantic edges of all visible bodies,
// in the component's frame
export function drawingModel(component) {
  const positions = []
  const normals = []
  const edges = []
  const occurrences = []
  const point = new THREE.Vector3()
  const visit = occurrence => {
    const transform = relativeComponentTransform(occurrence, component)
    const normalMatrix = new THREE.Matrix3().getNormalMatrix(transform)
    const solids = occurrence.compound.solids().filter(solid => !occurrence.isItemHidden(solid.id))
    if(solids.length) occurrences.push(occurrence)
    solids.forEach(solid => {
      solid.faces().forEach(face => {
        const mesh = face.tesselate()
        for(let i = 0; i < mesh.positions.length; i += 3) {
          positions.push(...point.fromArray(mesh.positions, i).applyMatrix4(transform).toArray())
          normals.push(...point.fromArray(mesh.normals, i).applyMatrix3(normalMatrix).normalize().toArray())
        }
      })
      solid.edges()
        .filter(edge => edge.isSemantic())
        .forEach(edge => edges.push({
          points: edge.tesselate().map(point => point.clone().applyMatrix4(transform)),
          edge,
          occurrence,
        }))
    })
    occurrence.children.filter(child => !child.creator.hidden).forEach(visit)
  }
  visit(component)

  const array = new Float32Array(positions)
  const diagonal = positions.length ? new THREE.Box3().setFromArray(positions).getSize(point).length() : 1
  return {
    component,
    positions: array,
    normals,
    edges,
    occurrences,
    bvh: positions.length ? new MeshBVH(meshGeometry(array)) : null,
    // Sample spacing of visibility tests
    step: diagonal / 200,
    // Edges lie on the exact surface, up to a chord off the tessellation
    near: Math.max(3 * tessellationOptions.chordTolerance, diagonal * 1e-4),
    geometries: new Map(),
  }
}

function meshGeometry(positions) {
  const geometry = new THREE.BufferGeometry()
  geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3))
  return geometry
}

function frameKey(frame) {
  return [frame.dir, frame.u, frame.v].map(vector => vector.toArray().map(value => value.toFixed(6)).join(',')).join('|')
}

// Lines of a view in view coordinates [u, v], split into visible and hidden
// runs. Section views keep the material behind the cut and add its outline.
export function viewGeometry(model, frame, section) {
  const key = `${frameKey(frame)}|${section ? section.toArray().join(',') : ''}`
  if(!model.geometries.has(key)) model.geometries.set(key, computeViewGeometry(model, frame, section))
  return model.geometries.get(key)
}

function computeViewGeometry(model, frame, section) {
  const project = point => [point.dot(frame.u), point.dot(frame.v)]
  if(!model.bvh) return { lines: [], cut: [] }

  const depth = point => point.clone().sub(section).dot(frame.dir)
  let bvh = model.bvh
  if(section) {
    const kept = clipTriangles(model.positions, depth)
    bvh = kept.length ? new MeshBVH(meshGeometry(kept)) : null
  }

  const ray = new THREE.Ray(new THREE.Vector3(), frame.dir.clone().negate())
  // Tilted slightly, so rays don't graze faces along the view direction
  const probe = new THREE.Ray(new THREE.Vector3(), frame.dir.clone().negate()
    .add(frame.u.clone().multiplyScalar(1.3e-3)).add(frame.v.clone().multiplyScalar(0.7e-3)).normalize())
  const isHidden = point => {
    ray.origin.copy(point)
    if(bvh?.raycastFirst(ray, THREE.DoubleSide, model.near)) return true
    if(!section) return false
    // Behind the cut face, where the section plane passes through material
    probe.origin.copy(point).addScaledVector(frame.dir, -depth(point))
    return model.bvh.raycast(probe, THREE.DoubleSide).length % 2 == 1
  }

  const sources = [
    ...model.edges.map(edge => ({ points: edge.points, source: edge })),
    ...silhouettes(model, frame.dir).map(points => ({ points, source: null })),
  ]
  const lines = []
  const start = new THREE.Vector3()
  const end = new THREE.Vector3()
  const middle = new THREE.Vector3()
  for(const { points, source } of sources) {
    const pieces = section ? clipPolyline(points, depth) : [points]
    for(const polyline of pieces) {
      let line = null
      for(let i = 1; i < polyline.length; i++) {
        const a = polyline[i - 1]
        const b = polyline[i]
        const count = Math.max(1, Math.ceil(a.distanceTo(b) / model.step))
        for(let j = 0; j < count; j++) {
          start.lerpVectors(a, b, j / count)
          end.lerpVectors(a, b, (j + 1) / count)
          const hidden = isHidden(middle.lerpVectors(start, end, 0.5))
          if(line?.hidden !== hidden) {
            line = { hidden, source, points: [project(start)] }
            lines.push(line)
          }
          line.points.push(project(end))
        }
      }
    }
  }
  // Hidden lines are not shown in sections
  return {
    lines: section ? lines.filter(line => !line.hidden) : lines,
    cut: section ? cutSegments(model.positions, depth).map(segment => segment.map(project)) : [],
  }
}

// Contours where interpolated vertex normals turn perpendicular to the view
function silhouettes(model, dir) {
  const { positions, normals } = model
  const segments = []
  // Snapping noise to zero keeps contours on seams, whose duplicated
  // vertices would otherwise fall on either side, and skips planes seen edge on
  const facing = index => {
    const value = normals[index] * dir.x + normals[index + 1] * dir.y + normals[index + 2] * dir.z
    return Math.abs(value) < 1e-9 ? 0 : value
  }
  for(let triangle = 0; triangle < positions.length; triangle += 9) {
    const corners = [triangle, triangle + 3, triangle + 6]
    const crossings = crossTriangle(positions, corners, corners.map(facing))
    if(crossings.length == 2) segments.push(crossings)
  }
  return chain(segments)
}

// Points where a field over the triangle corners changes sign
function crossTriangle(positions, corners, values) {
  const compare = (i, j) => positions[i] - positions[j] ||
    positions[i + 1] - positions[j + 1] ||
    positions[i + 2] - positions[j + 2]
  const crossings = []
  for(let i = 0; i < 3; i++) {
    // Order shared mesh edges alike, so neighbors cross at the same point
    let [a, b] = [i, (i + 1) % 3]
    if(compare(corners[a], corners[b]) > 0) [a, b] = [b, a]
    if(values[a] > 0 == values[b] > 0) continue
    const t = values[a] / (values[a] - values[b])
    crossings.push(new THREE.Vector3().fromArray(positions, corners[a])
      .lerp(new THREE.Vector3().fromArray(positions, corners[b]), t))
  }
  return crossings
}

function cutSegments(positions, depth) {
  const segments = []
  const point = new THREE.Vector3()
  for(let triangle = 0; triangle < positions.length; triangle += 9) {
    const corners = [triangle, triangle + 3, triangle + 6]
    const values = corners.map(corner => depth(point.fromArray(positions, corner)))
    const crossings = crossTriangle(positions, corners, values)
    if(crossings.length == 2) segments.push(crossings)
  }
  return segments
}

// Triangles cut down to the far side of a section plane
function clipTriangles(positions, depth) {
  const kept = []
  for(let triangle = 0; triangle < positions.length; triangle += 9) {
    const corners = [0, 3, 6].map(offset => new THREE.Vector3().fromArray(positions, triangle + offset))
    const polygon = []
    corners.forEach((corner, i) => {
      const next = corners[(i + 1) % 3]
      const a = depth(corner)
      const b = depth(next)
      if(a >= 0) polygon.push(corner)
      if(a >= 0 != b >= 0) polygon.push(corner.clone().lerp(next, a / (a - b)))
    })
    for(let i = 2; i < polygon.length; i++) {
      kept.push(...polygon[0].toArray(), ...polygon[i - 1].toArray(), ...polygon[i].toArray())
    }
  }
  return new Float32Array(kept)
}

// Runs of a 3D polyline on the far side of a section plane
function clipPolyline(points, depth) {
  const runs = []
  let run = null
  points.forEach((point, i) => {
    const value = depth(point)
    if(i > 0) {
      const previous = depth(points[i - 1])
      if(previous >= 0 != value >= 0) {
        const crossing = points[i - 1].clone().lerp(point, previous / (previous - value))
        if(run) run.push(crossing)
        else runs.push(run = [crossing])
        if(value < 0) run = null
      }
    }
    if(value >= 0) {
      if(!run) runs.push(run = [])
      run.push(point)
    }
  })
  return runs.filter(run => run.length > 1)
}

// Join segments sharing end points into polylines, so dashes run on
function chain(segments) {
  const key = point => `${point.x.toFixed(5)},${point.y.toFixed(5)},${point.z.toFixed(5)}`
  const ends = new Map()
  segments.forEach((segment, index) => segment.forEach(point => {
    const id = key(point)
    if(!ends.has(id)) ends.set(id, [])
    ends.get(id).push(index)
  }))
  const used = new Set()
  const next = point => {
    const index = ends.get(key(point)).find(index => !used.has(index))
    if(index === undefined) return null
    used.add(index)
    const [a, b] = segments[index]
    return key(a) == key(point) ? b : a
  }
  const polylines = []
  segments.forEach((segment, index) => {
    if(used.has(index)) return
    used.add(index)
    const polyline = [...segment]
    for(let point = next(polyline.at(-1)); point; point = next(point)) polyline.push(point)
    for(let point = next(polyline[0]); point; point = next(point)) polyline.unshift(point)
    polylines.push(polyline)
  })
  return polylines
}


// 2D clipping of view coordinate polylines

function clipRuns(points, inside, crossing) {
  const runs = []
  let run = null
  points.forEach((point, i) => {
    if(i > 0) {
      for(const cut of crossing(points[i - 1], point)) {
        if(run) {
          run.push(cut)
          run = null
        } else {
          runs.push(run = [cut])
        }
      }
    }
    if(inside(point)) {
      if(!run) runs.push(run = [])
      run.push(point)
    } else {
      run = null
    }
  })
  return runs.filter(run => run.length > 1)
}

const lerp2 = (a, b, t) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]

// Removes the bands of breaks and closes the gaps, leaving gap wide seams
export function applyBreaks(points, breaks, gap) {
  let runs = [points]
  for(const { axis, from, to } of sortedBreaks(breaks)) {
    const k = axis == 'u' ? 0 : 1
    const outside = point => point[k] <= from || point[k] >= to
    const crossing = (a, b) => [from, to]
      .map(value => (value - a[k]) / (b[k] - a[k]))
      .filter(t => t > 0 && t < 1)
      .sort((s, t) => b[k] > a[k] ? s - t : t - s)
      .map(t => lerp2(a, b, t))
    runs = runs.flatMap(run => clipRuns(run, outside, crossing))
  }
  return runs.map(run => run.map(point => breakPoint(point, breaks, gap)))
}

export function breakPoint(point, breaks, gap) {
  const result = [...point]
  for(const { axis, from, to } of sortedBreaks(breaks)) {
    const k = axis == 'u' ? 0 : 1
    if(result[k] >= to) result[k] -= to - from - gap
    else if(result[k] > from) result[k] = from + gap / 2
  }
  return result
}

export function unbreakPoint(point, breaks, gap) {
  const result = [...point]
  for(const { axis, from, to } of sortedBreaks(breaks).reverse()) {
    const k = axis == 'u' ? 0 : 1
    if(result[k] >= from + gap) result[k] += to - from - gap
  }
  return result
}

// Highest bands first, so shifting one leaves the coordinates of the next
function sortedBreaks(breaks) {
  return [...breaks].sort((a, b) => b.from - a.from)
}

export function clipToCircle(points, center, radius) {
  const inside = point => Math.hypot(point[0] - center[0], point[1] - center[1]) <= radius
  const crossing = (a, b) => {
    const d = [b[0] - a[0], b[1] - a[1]]
    const f = [a[0] - center[0], a[1] - center[1]]
    const qa = d[0] * d[0] + d[1] * d[1]
    const qb = 2 * (f[0] * d[0] + f[1] * d[1])
    const qc = f[0] * f[0] + f[1] * f[1] - radius * radius
    const discriminant = qb * qb - 4 * qa * qc
    if(discriminant <= 0 || !qa) return []
    const root = Math.sqrt(discriminant)
    return [(-qb - root) / (2 * qa), (-qb + root) / (2 * qa)]
      .filter(t => t > 0 && t < 1)
      .map(t => lerp2(a, b, t))
  }
  return clipRuns(points, inside, crossing)
}

// Section hatching at 45°, clipped to the cut outlines by the even-odd rule
export function hatch(segments, spacing) {
  const rotate = ([x, y], sign) => [(x + sign * y) * Math.SQRT1_2, (y - sign * x) * Math.SQRT1_2]
  const rotated = segments.map(segment => segment.map(point => rotate(point, 1)))
  const ys = rotated.flat().map(point => point[1])
  const lines = []
  for(let y = Math.ceil(Math.min(...ys) / spacing) * spacing; y < Math.max(...ys); y += spacing) {
    const xs = rotated
      .filter(([a, b]) => a[1] > y != b[1] > y)
      .map(([a, b]) => a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]))
      .sort((a, b) => a - b)
    for(let i = 1; i < xs.length; i += 2) {
      lines.push([rotate([xs[i - 1], y], -1), rotate([xs[i], y], -1)])
    }
  }
  return lines
}

// Odd hit counts put a point inside the bodies
export function isInside(model, point) {
  const ray = new THREE.Ray(point, new THREE.Vector3(0.577, 0.5774, 0.5772).normalize())
  return model.bvh.raycast(ray, THREE.DoubleSide).length % 2 == 1
}
