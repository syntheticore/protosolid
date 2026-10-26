import { consumeEntityIds } from './shapex.js'
import { nameTopology, propagateTopology } from './topological-naming.js'
import { topologyItems, topologyNeighbors, identifyTopology, seedRoles } from './topology.js'

// Native IDs are operation-local correspondence tokens. Only sketch generators,
// semantic roles and named incidence reach the document identity layer.
export function seedProfileTopology(compound, profiles, owner, extraSources = []) {
  const constructionRoles = compound.geom().topologyHistory().constructionRoles
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
    for(const vertex of solid.vertices()) {
      nameTopology(vertex, 'vertex', 'profile/boundary',
        vertex.edges().flatMap(edge => edge.topology.anchors), owner)
    }
    nameTopology(solid, 'body', 'profile/body', sources, owner)
  }
  identifyTopology(compound)
  return compound
}

// Imported bodies carry no construction semantics. Their order in the stored
// file is fixed, so it identifies faces across regenerations.
export function seedImportTopology(compound, owner) {
  compound.solids().forEach((solid, body) => {
    solid.faces().forEach((face, index) => {
      nameTopology(face, 'face', 'import/face', [`${body}/${index}`], owner)
    })
    for(const edge of solid.edges()) {
      nameTopology(edge, 'edge', 'import/boundary',
        edge.connectedFaces().flatMap(face => face.topology.anchors), owner)
    }
    for(const vertex of solid.vertices()) {
      nameTopology(vertex, 'vertex', 'import/boundary',
        vertex.edges().flatMap(edge => edge.topology.anchors), owner)
    }
    nameTopology(solid, 'body', 'import/body', [`${body}`], owner)
  })
  identifyTopology(compound)
  return compound
}

// Document naming policy per operation; native construction roles only
// describe geometry. `role` names an output cell, and `continues` decides
// whether a cell the kernel resolves a source to keeps that source's
// identity. Resolved cells that do not continue it are generated from it.
const defaultPolicy = operation => ({ role: () => operation, continues: () => true })

const shellLabels = { 'shell-source': 'source-skin', 'shell-offset': 'offset-skin', 'shell-rim': 'opening-rim' }
const namingPolicies = {
  shell(operation, history) {
    const labels = face => history.constructionRoles[face.kernelId]
      .filter(role => role.kind in shellLabels).map(role => shellLabels[role.kind]).sort()
    const sourceSkin = face => labels(face).includes('source-skin')
    return {
      role: item => item.topologyKind == 'face' && labels(item).length ? `${operation}/${labels(item).join('+')}` : operation,
      // The offset skin resolves to the same source face, but only the
      // source skin continues it.
      continues: item => item.topologyKind == 'face' ? sourceSkin(item) : item.connectedFaces().some(sourceSkin),
    }
  },
}

export function trackTopology(out, parents, operation, bodyParents = parents) {
  return trackOperations(out, parents, [{ compound: out, operation, bodyParents }])
}

export function trackOperations(out, parents, operations) {
  const inputs = parents.flatMap(topologyItems)
  const outputs = topologyItems(out)
  out.topologyFailures = new Map(parents.flatMap(parent => [...(parent.topologyFailures || [])]))
  const roles = new Map()
  const correspondence = operations.flatMap(({ compound, operation, branch, bodyParents = parents }) => {
    const solidIds = new Set(compound.solids().map(solid => solid.kernelId))
    const cells = outputs.filter(item => solidIds.has((item.topologyKind == 'body' ? item : item.solid).kernelId))
    const history = compound.geom().topologyHistory()
    const policy = (namingPolicies[operation] || defaultPolicy)(operation, history)
    cells.forEach(item => roles.set(item, { name: policy.role(item), branch }))
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
      const resolution = compound.geom().resolve(source.geom())
      const entityIds = consumeEntityIds(resolution.entities)
      const resolved = cells.filter(item => item.topologyKind == source.topologyKind && entityIds.includes(item.kernelId))
      const descendants = resolved.filter(policy.continues)
      records.push({ source, relation: 'modified', outputs: descendants })
      records.push({ source, relation: 'generated', outputs: cells.filter(item =>
        !descendants.includes(item) && (resolved.includes(item) || generatedFrom(item.kernelId, source.kernelId))) })
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
  propagateTopology(outputs, correspondence, item => roles.get(item), topologyNeighbors)
  identifyTopology(out)
  return out
}
