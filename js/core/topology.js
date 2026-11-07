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

// Branch evidence is evaluated anew at the producer. Named boundary uses
// come first so selection recipes prefer them over geometric sides.
export function topologySides(item) {
  if(item.cachedTopologySides) return item.cachedTopologySides
  // A cut can split both faces adjacent to an edge into symmetric branches.
  // Other named faces on the same solid still provide stable support frames.
  const faces = item.topologyKind == 'body' ? item.faces() : item.solid.faces()
  const supportFaces = (item.topologyKind == 'body' ? item.compound : item.solid.compound)
    .solids().flatMap(solid => solid.faces())
  const named = item.topologyKind == 'body' ? [] : boundaryEvidence(item, supportFaces)
  let points
  try {
    points = sidePoints(item)
  } catch(error) {
    if(!isModelingFailure(error)) throw error
    // Named boundary evidence remains available without a mesh.
    return item.cachedTopologySides = named
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
  item.cachedTopologySides = [...named, ...constraints.sort(compareEvidence)]
  return item.cachedTopologySides
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

function boundaryEvidence(item, supportFaces) {
  const { solid, topologyKind } = item
  const faces = new Map(solid.faces().map(face => [face.kernelId, face]))
  const edges = new Map(solid.edges().map(edge => [edge.kernelId, edge]))
  const evidence = []
  const body = topologySupport(solid, solid.compound.solids())
  if(body) evidence.push({ frame: 'body', support: body })
  const incident = topologyKind == 'edge' ? [item] :
    topologyKind == 'face' ? item.boundaryEdges() : item.edges()
  for(const edge of incident) {
    for(const [faceId, previousId, nextId, start] of edge.geom().boundaryUses()) {
      const face = faces.get(faceId)
      // Shell traversal reverses with the material side. Use the direction
      // of the supporting chart so flipping an extrusion keeps its corners.
      const reversed = face.geom().orientation() == 'reversed'
      const previous = edges.get(reversed ? nextId : previousId)
      const next = edges.get(reversed ? previousId : nextId)
      if(topologyKind == 'face') {
        if(faceId == item.kernelId) evidence.push({ frame: 'face-boundary', edge: edge.topology.anchors,
          previous: previous.topology.anchors, next: next.topology.anchors })
        continue
      }
      const support = topologySupport(face, supportFaces)
      if(!support) continue
      if(topologyKind == 'edge') {
        evidence.push({ frame: 'boundary', support, previous: previous.topology.anchors, next: next.topology.anchors })
      } else {
        const [first, last] = edge.vertexIds()
        const chartStart = reversed ? (start == first ? last : first) : start
        if(chartStart == item.kernelId) evidence.push({ frame: 'vertex-boundary', support,
          incoming: previous.topology.anchors, outgoing: edge.topology.anchors })
      }
    }
  }
  return evidence.sort(compareEvidence)
}

function topologySupport(face, supportFaces) {
  if(face.cachedTopologySupport === undefined) {
    face.cachedTopologySupport = supportName(face, supportFaces, topologyNeighbors)
  }
  return face.cachedTopologySupport
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
