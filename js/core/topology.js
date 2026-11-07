import { nameTopology, propagateTopology, initializeTopologyOrigins, resolveTopology, restoreTopologySelections, supportName } from './topological-naming.js'
import * as THREE from 'three'
import { EPSILON } from './utils.js'
import { threeVector } from './shapex.js'
import { isModelingFailure } from './errors.js'

export function topologyItems(compound) {
  return compound.solids().flatMap(solid => [
    ...solid.faces().map(face => Object.assign(face, { topologyKind: 'face' })),
    ...solid.edges().map(edge => Object.assign(edge, { topologyKind: 'edge' })),
    ...solid.vertices().map(vertex => Object.assign(vertex, { topologyKind: 'vertex' })),
    Object.assign(solid, { topologyKind: 'body' }),
  ])
}

export function topologyNeighbors(item) {
  if(item.cachedTopologyNeighbors) return item.cachedTopologyNeighbors
  if(item.topologyKind == 'body') return item.faces()
  item.cachedTopologyNeighbors = item.topologyKind == 'edge' ? [...new Set(item.connectedFaces())] :
    item.topologyKind == 'vertex' ? item.edges() :
    item.solid.edges().filter(edge => topologyNeighbors(edge).includes(item))
  return item.cachedTopologyNeighbors
}

// Evaluate branch evidence at the producer. Geometric sides are read only
// for saved recipes that contain them.
export function topologySides(item, legacyGeometry = false) {
  if(!item.cachedTopologySides) {
    const named = item.topologyKind == 'body' ? bodyEvidence(item) : boundaryEvidence(item)
    if(item.topology.construction) named.unshift({ frame: 'construction', tags: item.topology.construction })
    item.cachedTopologySides = named
  }
  if(!legacyGeometry) return item.cachedTopologySides
  return item.cachedLegacyTopologySides ||= [...item.cachedTopologySides, ...geometricSides(item)]
}

function bodyEvidence(body) {
  const support = topologySupport(body, body.compound.solids())
  return support ? [{ frame: 'body', support }] : []
}

// Older saved recipes include signed geometric sides.
function geometricSides(item) {
  // A cut can split both faces adjacent to an edge into symmetric branches.
  // Other named faces on the same solid still provide stable support frames.
  const faces = item.topologyKind == 'body' ? item.faces() : item.solid.faces()
  const supportFaces = (item.topologyKind == 'body' ? item.compound : item.solid.compound)
    .solids().flatMap(solid => solid.faces())
  let points
  try {
    points = sidePoints(item)
  } catch(error) {
    if(!isModelingFailure(error)) throw error
    return []
  }
  const constraints = []
  const axes = []
  const recordSide = (center, direction, descriptor) => {
    let minimum = Infinity, maximum = -Infinity
    for(const point of points) {
      const value = point.clone().sub(center).dot(direction)
      minimum = Math.min(minimum, value)
      maximum = Math.max(maximum, value)
    }
    const side = minimum >= -EPSILON && maximum > EPSILON ? 1 :
      maximum <= EPSILON && minimum < -EPSILON ? -1 : 0
    if(side) constraints.push({ ...descriptor, side })
  }
  for(const face of faces.slice().sort((a, b) =>
    JSON.stringify(a.topology.anchors).localeCompare(JSON.stringify(b.topology.anchors)))) {
    const plane = face.getPlane()
    const surface = plane ? null : face.geom().surface()
    if(!plane && surface.kind != 'cylinder') continue
    // A frame must itself have a unique named support in this producer.
    const support = topologySupport(face, supportFaces)
    if(!support) continue
    const center = face.center()
    if(plane) {
      for(const axis of [2, 0, 1]) {
        const direction = new THREE.Vector3().setFromMatrixColumn(plane, axis).normalize()
        recordSide(center, direction, { support, frame: 'plane', axis })
      }
    } else {
      const direction = threeVector(surface.axis.direction())
      axes.push({ support, center, direction })
      recordSide(center, direction, { support, frame: 'axis', axis: 2 })
      // Extruded corners can share both faces and lie on the same side of
      // every cap center. The cylinder supplies its own radial frame.
      const origin = threeVector(surface.axis.origin())
      surface.radialAxes.forEach((radial, axis) =>
        recordSide(origin, threeVector(radial).normalize(), { support, frame: 'cylinder', axis }))
    }
  }
  // Two cylindrical supports also define an oriented transverse direction.
  // This distinguishes periodic rim segments without an arbitrary world axis.
  for(let i = 0; i < axes.length; i++) for(let j = i + 1; j < axes.length; j++) {
    const first = axes[i], second = axes[j]
    const direction = new THREE.Vector3().crossVectors(first.direction, second.direction)
    if(direction.length() <= EPSILON) continue
    recordSide(first.center.clone().add(second.center).multiplyScalar(0.5), direction.normalize(), {
      support: first.support, crossSupport: second.support, frame: 'axes',
    })
  }
  return constraints.sort(compareEvidence)
}

function sidePoints(item) {
  if(item.topologyKind == 'vertex') return [item.point]
  if(item.topologyKind == 'edge') return item.tesselate().map(point =>
    Array.isArray(point) ? new THREE.Vector3(...point) : point)
  return (item.topologyKind == 'body' ? item.faces() : [item]).flatMap(face => {
    const { positions } = face.tesselate()
    return Array.from({ length: positions.length / 3 }, (_, i) =>
      new THREE.Vector3(...positions.slice(i * 3, i * 3 + 3)))
  })
}

const compareEvidence = (a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))

function boundaryEvidence(item) {
  const { solid, topologyKind } = item
  const siblings = solid.compound.solids()
  const supportFaces = siblings.flatMap(body => body.faces())
  const faces = new Map(solid.faces().map(face => [face.kernelId, face]))
  const edges = new Map(solid.edges().map(edge => [edge.kernelId, edge]))
  const vertices = new Map(solid.vertices().map(vertex => [vertex.kernelId, vertex]))
  const evidence = bodyEvidence(solid)
  const incident = topologyKind == 'edge' ? [item] :
    topologyKind == 'face' ? item.boundaryEdges() : item.edges()
  for(const edge of incident) {
    const [first, last] = edge.vertexIds()
    if(topologyKind == 'edge') {
      evidence.push({ frame: 'edge-vertices', ends: [vertices.get(first).topology.anchors,
        vertices.get(last).topology.anchors].sort(compareEvidence) })
    } else if(topologyKind == 'vertex') {
      const opposite = vertices.get(first == item.kernelId ? last : first)
      evidence.push({ frame: 'vertex-edge', edge: edge.topology.anchors, opposite: opposite.topology.anchors })
    }
    for(const [faceId, previousId, nextId, start, winding] of boundaryUses(edge)) {
      const face = faces.get(faceId)
      // Shell traversal reverses with the material side. Use the direction
      // of the supporting chart so flipping an extrusion keeps its corners.
      const reversed = face.geom().orientation() == 'reversed'
      const previous = edges.get(reversed ? nextId : previousId)
      const next = edges.get(reversed ? previousId : nextId)
      const chartStart = reversed ? (start == first ? last : first) : start
      if(topologyKind == 'face') {
        if(faceId == item.kernelId) {
          evidence.push({ frame: 'face-boundary', edge: edge.topology.anchors,
            previous: previous.topology.anchors, next: next.topology.anchors })
          evidence.push({ frame: 'face-vertex', edge: edge.topology.anchors, vertex: vertices.get(chartStart).topology.anchors })
        }
        continue
      }
      const support = topologySupport(face, supportFaces) || face.topology.anchors
      const run = () => boundaryRun(edge, [faceId, previousId, nextId, start], edges, reversed)
      if(topologyKind == 'edge') {
        evidence.push({ frame: 'boundary', support, previous: previous.topology.anchors, next: next.topology.anchors })
        if(face.topology.construction) evidence.push({ frame: 'constructed-face', support, tags: face.topology.construction })
        if(winding.some(turns => turns)) evidence.push({ frame: 'boundary-winding', support,
          winding: reversed ? winding.map(turns => -turns) : winding })
        const span = run()
        if(span) evidence.push({ frame: 'boundary-run', support, ...span })
      } else {
        if(chartStart == item.kernelId) {
          evidence.push({ frame: 'vertex-boundary', support,
            incoming: previous.topology.anchors, outgoing: edge.topology.anchors })
          const span = run()
          if(span) evidence.push({ frame: 'vertex-run', support, ...span })
        }
      }
    }
  }
  return evidence.sort(compareEvidence)
}

function boundaryUses(edge) {
  return edge.cachedBoundaryUses ||= edge.geom().boundaryUses()
}

// A run of repeated arcs can be named by its offset between distinct coedges,
// such as the two uses of a periodic surface seam.
function boundaryRun(edge, use, edges, reversed) {
  const anchors = JSON.stringify(edge.topology.anchors)
  const endpoint = (current, start) => current.vertexIds().find(id => id != start) || start
  const walk = direction => {
    let current = edge, currentUse = use
    const seen = new Set([current])
    while(true) {
      const [faceId, previous, next, start] = currentUse
      const adjacent = edges.get(direction == 1 ? next : previous)
      if(JSON.stringify(adjacent.topology.anchors) != anchors) {
        return { edge: adjacent.topology.anchors, offset: seen.size - 1 }
      }
      if(seen.has(adjacent)) return
      const uses = boundaryUses(adjacent).filter(([face, before, after, vertex]) =>
        face == faceId && (direction == 1 ?
          before == current.kernelId && vertex == endpoint(current, start) :
          after == current.kernelId && endpoint(adjacent, vertex) == start))
      if(uses.length != 1) return
      seen.add(adjacent)
      current = adjacent
      currentUse = uses[0]
    }
  }
  const before = walk(reversed ? 1 : -1)
  const after = walk(reversed ? -1 : 1)
  if(before && after && (before.offset || after.offset)) {
    return { before, after, edge: edge.topology.anchors }
  }
}

function topologySupport(item, candidates) {
  if(item.cachedTopologySupport === undefined) {
    item.cachedTopologySupport = supportName(item, candidates, topologyNeighbors)
  }
  return item.cachedTopologySupport
}

export function identifyTopology(compound) {
  const items = topologyItems(compound)
  initializeTopologyOrigins(items, topologyNeighbors, topologySides)
  const counts = new Map()
  for(const item of items) counts.set(item.topology.identity, (counts.get(item.topology.identity) || 0) + 1)
  for(const item of items) {
    let id = item.topology.identity
    // UI object IDs need to be unique even for an unresolved split family.
    // References use ancestry and incidence, never these display IDs.
    if(counts.get(id) > 1) id = `${id}/display/${item.tessId || item.faces()[0].tessId}`
    item.id = item.topologyKind == 'body' ? `${compound.componentId}/${id}` : id
  }
}

export function seedRoles(item, kind, origins, role, sources, owner) {
  const roots = origins.map(origin => {
    const root = {}
    nameTopology(root, kind, origin.role, origin.sources, owner)
    return root
  })
  if(!roots.length) nameTopology(item, kind, role, sources, owner)
  else {
    item.topologyKind = kind
    propagateTopology([item], roots.map(source => ({ source, relation: 'modified', outputs: [item] })), role, () => [])
  }
}

export function restoreCompoundSelections(compound, feature) {
  const failures = new Map(compound.topologyFailures)
  const current = restoreTopologySelections(feature, topologyItems(compound), topologyNeighbors, topologySides)
  for(const binding of feature.topologyNames.selections || []) failures.delete(binding.identity)
  for(const [identity, status] of current) failures.set(identity, status)
  compound.topologyFailures = failures
}

export function resolveCompoundSelection(compound, selection) {
  const result = resolveTopology(topologyItems(compound), selection, topologyNeighbors)
  if(result.status == 'deleted' && compound.topologyFailures?.has(selection.identity)) {
    result.status = compound.topologyFailures.get(selection.identity)
  }
  return result
}
