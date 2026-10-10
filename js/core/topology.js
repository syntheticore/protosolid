import { nameTopology, propagateTopology, initializeTopologyOrigins, resolveTopology, restoreTopologySelections } from './topological-naming.js'

export function topologyItems(compound) {
  return compound.solids().flatMap(solid => [
    ...solid.faces().map(face => Object.assign(face, { topologyKind: 'face' })),
    ...solid.edges().map(edge => Object.assign(edge, { topologyKind: 'edge' })),
    Object.assign(solid, { topologyKind: 'body' }),
  ])
}

export function topologyNeighbors(item) {
  if(item.cachedTopologyNeighbors) return item.cachedTopologyNeighbors
  if(item.topologyKind == 'body') return item.faces()
  item.cachedTopologyNeighbors = item.topologyKind == 'edge' ? [...new Set(item.connectedFaces())] :
    item.solid.edges().filter(edge => topologyNeighbors(edge).includes(item))
  return item.cachedTopologyNeighbors
}

export function identifyTopology(compound) {
  const items = topologyItems(compound)
  initializeTopologyOrigins(items, topologyNeighbors)
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
  const current = restoreTopologySelections(feature, topologyItems(compound), topologyNeighbors)
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
