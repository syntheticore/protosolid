import { nameTopology, propagateTopology } from './topological-naming.js'
import { topologyItems, topologyNeighbors, identifyTopology, seedRoles } from './topology.js'

// Native IDs are operation-local correspondence tokens. Only sketch generators,
// semantic roles and named incidence reach the document identity layer.
export function seedShapexTopology(compound, profiles, owner, extraSources = []) {
  const kernel = window.kernel.modeling
  const constructionRoles = kernel.topologyHistory(compound.geom()).constructionRoles
  const sources = [...profiles.flatMap(profile => profile.rings.flatMap(ring =>
    ring.segments.map(segment => profile.getBaseId(segment)))), ...extraSources]
  for(const face of compound.faces()) {
    const roles = constructionRoles[face.kernelId].map(role => {
      if(role.kind.endsWith('-start')) return { role: 'profile-start', sources }
      if(role.kind.endsWith('-end')) return { role: 'profile-end', sources }
      const generators = role.generators?.length ? role.generators : [[0,
        role.boundary == 'outer' ? 0 : role.holeIndex + 1, role.segment]]
      return { role: role.chart ? `profile-side/${role.chart}` : 'profile-side', sources: [...generators.map(generator => {
        const [section, ring, segment] = generator
        const profile = profiles[section]
        return profile.getBaseId(profile.rings[ring].segments[segment])
      }), ...extraSources] }
    })
    seedRoles(face, 'face', roles, 'profile/unclassified', sources, owner)
  }
  for(const solid of compound.solids()) {
    for(const edge of solid.edges()) {
      nameTopology(edge, 'edge', 'profile/boundary',
        edge.connectedFaces().flatMap(face => face.topology.anchors), owner)
    }
    nameTopology(solid, 'body', 'profile/body', sources, owner)
  }
  identifyTopology(compound)
  return compound
}

export function trackShapexTopology(out, parents, operation, bodyParents = parents) {
  return trackShapexOperations(out, parents, [{ compound: out, operation, bodyParents }])
}

export function trackShapexOperations(out, parents, operations) {
  const kernel = window.kernel.modeling
  const inputs = parents.flatMap(topologyItems)
  const outputs = topologyItems(out)
  out.topologyFailures = new Map(parents.flatMap(parent => [...(parent.topologyFailures || [])]))
  const roles = new Map()
  const correspondence = operations.flatMap(({ compound, operation, branch, bodyParents = parents }) => {
    const solidIds = new Set(compound.solids().map(solid => solid.kernelId))
    const cells = outputs.filter(item => solidIds.has((item.topologyKind == 'body' ? item : item.solid).kernelId))
    cells.forEach(item => roles.set(item, { name: operation, branch }))
    const history = kernel.topologyHistory(compound.geom())
    // Document naming policy belongs here; native shell roles describe geometry.
    const shellLabels = { 'shell-source': 'source-skin', 'shell-offset': 'offset-skin', 'shell-rim': 'opening-rim' }
    const shellRole = face => history.constructionRoles[face.kernelId].filter(role => role.kind in shellLabels)
    const sourceSkin = face => shellRole(face).some(role => role.kind == 'shell-source')
    for(const item of cells) if(operation == 'shell' && item.topologyKind == 'face') {
      const labels = shellRole(item).map(role => shellLabels[role.kind]).sort()
      if(labels.length) roles.set(item, { name: `${operation}/${labels.join('+')}`, branch })
    }
    const changes = new Map(history.changes.map(change => [change.source, change]))
    const origins = new Map(history.origins.map(origin => [origin.id, origin]))
    const generatedFrom = (id, source, visited = new Set()) => {
      if(visited.has(id)) return false
      const origin = origins.get(id)
      return !!origin && origin.sources.some(parent => parent == source ||
        generatedFrom(parent, source, new Set([...visited, id])))
    }
    const records = []
    for(const source of inputs.filter(item => item.topologyKind != 'body')) {
      const resolution = kernel.resolveEntity(compound.geom(), source.geom())
      const descendants = cells.filter(item => item.topologyKind == source.topologyKind && resolution.entityIds.includes(item.kernelId) &&
        (operation != 'shell' || (item.topologyKind == 'face' ? sourceSkin(item) :
          item.connectedFaces().some(sourceSkin))))
      records.push({ source, relation: 'modified', outputs: descendants })
      records.push({ source, relation: 'generated', outputs: cells.filter(item =>
        !descendants.includes(item) && (generatedFrom(item.kernelId, source.kernelId) ||
          (operation == 'shell' && item.topologyKind == source.topologyKind && resolution.entityIds.includes(item.kernelId)))) })
      if(resolution.kind == 'preserved') for(const item of descendants) {
        item.tessId = source.tessId
        item.cachedTesselation = source.cachedTesselation
      }
    }
    const bodySources = bodyParents.flatMap(parent => parent.solids())
    for(const source of inputs.filter(item => item.topologyKind == 'body')) {
      const ids = changes.get(source.kernelId)?.outputs || []
      const descendants = cells.filter(item => item.topologyKind == 'body' &&
        (item.kernelId == source.kernelId || ids.includes(item.kernelId)))
      const faces = new Set(records.filter(record => record.relation == 'modified' &&
        source.faces().includes(record.source)).flatMap(record => record.outputs))
      records.push({ source, relation: bodySources.includes(source) ? 'modified' : 'contributed',
        outputs: descendants.length ? descendants : cells.filter(item =>
          item.topologyKind == 'body' && item.faces().some(face => faces.has(face))) })
    }
    return records
  })
  // Rounded shells previously omitted their skin roles. Keep those construction
  // names as continuity aliases so saved selections still resolve, while new
  // selections use the distinct source, offset and rim identities.
  const shellNames = new Map()
  if(operations.some(({ operation }) => operation == 'shell')) {
    propagateTopology(outputs, correspondence.map(record => ({ ...record,
      relation: record.source.topologyKind != 'body' && record.relation == 'modified' ? 'generated' : record.relation,
    })), 'shell', topologyNeighbors)
    for(const item of outputs) shellNames.set(item, [...item.topology.names])
  }
  propagateTopology(outputs, correspondence, item => roles.get(item), topologyNeighbors)
  for(const [item, names] of shellNames) {
    for(const name of names) if(!item.topology.names.includes(name)) item.topology.names.push(name)
    item.topology.names.sort()
  }
  identifyTopology(out)
  return out
}
