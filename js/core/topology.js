import { nameTopology, propagateTopology, initializeTopologyOrigins, resolveTopology, restoreTopologySelections, supportName } from './topological-naming.js'
import * as THREE from 'three'
import { EPSILON } from './utils.js'

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

// Geometric branch evidence belongs to the application reference recipe.
// The frame is a named support face, evaluated anew at the producer. Store
// only its oriented side, never coordinates, output ranks or kernel IDs.
export function topologySides(item) {
  if(item.cachedTopologySides) return item.cachedTopologySides
  // A cut can split both faces adjacent to an edge into symmetric branches.
  // Other named faces on the same solid still provide stable support frames.
  const faces = item.topologyKind == 'body' ? item.faces() : item.solid.faces()
  const supportFaces = (item.topologyKind == 'body' ? item.compound : item.solid.compound)
    .solids().flatMap(solid => solid.faces())
  const points = item.topologyKind == 'vertex' ? [item.point] :
    item.topologyKind == 'edge' ? item.tesselate().map(point =>
    Array.isArray(point) ? new THREE.Vector3(...point) : point) :
    (item.topologyKind == 'body' ? item.faces() : [item]).flatMap(face => {
      const { positions } = face.tesselate()
      return Array.from({ length: positions.length / 3 }, (_, i) =>
        new THREE.Vector3(...positions.slice(i * 3, i * 3 + 3)))
    })
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
    const frame = plane || face.getAxis()
    if(!frame) continue
    // A frame must itself have a unique named support in this producer.
    const support = topologySupport(face, supportFaces)
    if(!support) continue
    const center = face.center()
    if(!plane) axes.push({ support, center, direction: new THREE.Vector3().setFromMatrixColumn(frame, 2).normalize() })
    for(const axis of plane ? [2, 0, 1] : [2]) {
      const direction = new THREE.Vector3().setFromMatrixColumn(frame, axis).normalize()
      recordSide(center, direction, { support, frame: plane ? 'plane' : 'axis', axis })
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
  item.cachedTopologySides = constraints.sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)))
  return item.cachedTopologySides
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
