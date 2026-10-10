import { nameTopology, propagateTopology, initializeTopologyOrigins, resolveTopology, restoreTopologySelections } from './topological-naming.js'

// The only OCC-specific part of persistent naming: translate operation history
// into kernel-independent correspondence, and expose topological incidence.
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

export function readShapeList(list) {
  const copy = new window.oc.oc.TopTools_ListOfShape_1()
  copy.Assign(list)
  list.delete()
  const shapes = []
  while(copy.Size()) {
    shapes.push(copy.First_1())
    copy.RemoveFirst()
  }
  copy.delete()
  return shapes
}

export function operationHistory(algorithm) {
  if(algorithm.Modified) return algorithm
  if(algorithm.History) return algorithm.History().get()
  if(algorithm.History_1) return algorithm.History_1().get()
  return algorithm.Context().get().History().get()
}

export function trackOccTopology(out, parents, algorithm, operation, bodyParents = parents) {
  return trackOccOperations(out, parents, [{ algorithm, operation, compound: out, bodyParents }])
}

// Compound operations must count descendants across every result before
// deciding whether an upstream cell's origin can be retained.
export function trackOccOperations(out, parents, operations) {
  out.topologyFailures = new Map(parents.flatMap(parent => [...(parent.topologyFailures || [])]))
  const inputs = parents.flatMap(topologyItems)
  const outputs = topologyItems(out)
  const roles = new Map()
  const correspondence = operations.flatMap(({ algorithm, operation, compound, branch, bodyParents = parents }) => {
    const solids = compound.solids()
    const cells = outputs.filter(item => solids.some(solid =>
      solid.geom().IsSame((item.topologyKind == 'body' ? item : item.solid).geom())))
    for(const item of cells) roles.set(item, { name: operation, branch })
    // The operation declares which operands can continue as bodies. All
    // operands still contribute face and edge history independently.
    const bodySources = bodyParents.flatMap(parent => parent.solids())
    return occCorrespondence(inputs, cells, operationHistory(algorithm), bodySources)
  })
  propagateTopology(outputs, correspondence, item => roles.get(item), topologyNeighbors)
  identifyTopology(out)
  return out
}

function occCorrespondence(inputs, outputs, history, bodySources) {
  const correspondence = []
  for(const source of inputs.filter(item => item.topologyKind != 'body')) {
    const retained = outputs.filter(output => output.topologyKind == source.topologyKind && output.geom().IsSame(source.geom()))
    const modified = readShapeList(history.Modified(source.geom()))
    const generated = readShapeList(history.Generated(source.geom()))
    const descendants = outputs.filter(output => output.topologyKind == source.topologyKind &&
      modified.some(shape => shape.IsSame(output.geom())))
    correspondence.push({ source, relation: 'modified', outputs: [...new Set([...retained, ...descendants])] })
    correspondence.push({ source, relation: 'generated', outputs: outputs.filter(output =>
      generated.some(shape => shape.IsSame(output.geom()))) })
    for(const output of retained) {
      output.tessId = source.tessId
      output.cachedTesselation = source.cachedTesselation
    }
    modified.forEach(shape => shape.delete())
    generated.forEach(shape => shape.delete())
  }
  // Prefer direct body history: distinct bodies can share a boundary face.
  // Operations without body history follow actual face correspondence instead.
  const bodies = outputs.filter(item => item.topologyKind == 'body')
  for(const source of inputs.filter(item => item.topologyKind == 'body')) {
    const modified = readShapeList(history.Modified(source.geom()))
    const descendants = bodies.filter(solid => solid.geom().IsSame(source.geom()) ||
      modified.some(shape => shape.IsSame(solid.geom())))
    modified.forEach(shape => shape.delete())
    const faces = new Set(correspondence.filter(record =>
      source.faces().includes(record.source)).flatMap(record => record.outputs))
    correspondence.push({ source, relation: bodySources.includes(source) ? 'modified' : 'contributed', outputs: descendants.length ? descendants :
      bodies.filter(solid => solid.faces().some(face => faces.has(face))) })
  }
  return correspondence
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

function seedRoles(item, kind, origins, role, sources, owner) {
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

export function seedOccTopology(compound, faceRoles, role, sources, owner, edgeRoles = []) {
  for(const face of compound.solids().flatMap(solid => solid.faces())) {
    const origins = faceRoles.filter(origin => origin.shapes.some(shape => shape.IsSame(face.geom())))
    seedRoles(face, 'face', origins, `${role}/unclassified`, sources, owner)
  }
  for(const solid of compound.solids()) {
    for(const edge of solid.edges()) {
      const origins = edgeRoles.filter(origin => origin.shapes.some(shape => shape.IsSame(edge.geom())))
      seedRoles(edge, 'edge', origins, `${role}/boundary`,
        edge.connectedFaces().flatMap(face => face.topology.anchors), owner)
    }
    nameTopology(solid, 'body', `${role}/body`, sources, owner)
  }
  identifyTopology(compound)
  return compound
}

export function restoreOccSelections(compound, feature) {
  const failures = new Map(compound.topologyFailures)
  const current = restoreTopologySelections(feature, topologyItems(compound), topologyNeighbors)
  for(const binding of feature.topologyNames.selections || []) failures.delete(binding.identity)
  for(const [identity, status] of current) failures.set(identity, status)
  compound.topologyFailures = failures
}

export function resolveOccSelection(compound, selection) {
  const result = resolveTopology(topologyItems(compound), selection, topologyNeighbors)
  if(result.status == 'deleted' && compound.topologyFailures?.has(selection.identity)) {
    result.status = compound.topologyFailures.get(selection.identity)
  }
  return result
}
