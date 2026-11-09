import { consumeEntityIds } from './shapex.js'
import { nameTopology, propagateTopology } from './topological-naming.js'
import { topologyItems, topologyNeighbors, identifyTopology, seedRoles } from './topology.js'

// Native IDs are operation-local correspondence tokens. Only sketch generators,
// semantic roles and named incidence reach the document identity layer.
export function seedProfileTopology(compound, profiles, owner, extraSources = []) {
  const constructionRoles = compound.geom().topologyHistory().constructionRoles
  const turnAliases = new Map()
  const sources = [...profiles.flatMap(profile => profile.rings.flatMap(ring =>
    ring.segments.map(segment => profile.getBaseId(segment)))), ...extraSources]
  const generatorSource = ([section, ring, segment]) => {
    const profile = profiles[section]
    return profile.getBaseId(profile.rings[ring].segments[segment])
  }
  const nameMeridian = item => {
    const tags = (constructionRoles[item.kernelId] || [])
      .filter(role => role.kind == 'revolution-meridian')
      .map(({ kind, chart, part, generators }) =>
        JSON.stringify({ role: kind, chart, part, sources: generators.map(generatorSource) }))
    if(tags.length) setConstruction(item, tags)
  }
  for(const face of compound.faces()) {
    const roles = constructionRoles[face.kernelId].map(role => {
      if(role.kind.endsWith('-start')) return { role: 'profile-start', sources }
      if(role.kind.endsWith('-end')) return { role: 'profile-end', sources }
      const generators = role.generators?.length ? role.generators : [[0,
        role.boundary == 'outer' ? 0 : role.holeIndex + 1, role.segment]]
      const generatorSources = [...generators.map(generatorSource), ...extraSources]
      if(role.chart == 'full-turn') {
        turnAliases.set(face, ['turn-start', 'turn-end'].map(chart => {
          const half = {}
          nameTopology(half, 'face', `profile-side/${chart}`, generatorSources, owner)
          return half.topology
        }))
        return { role: 'profile-side', sources: generatorSources }
      }
      return { role: role.chart ? `profile-side/${role.chart}` : 'profile-side', sources: generatorSources }
    })
    seedRoles(face, 'face', roles, 'profile/unclassified', sources, owner)
    if(turnAliases.has(face)) {
      face.topology.names.push(...turnAliases.get(face).flatMap(half => half.names))
      face.topology.names.sort()
    }
  }
  for(const solid of compound.solids()) {
    for(const edge of solid.edges()) {
      const faces = edge.connectedFaces()
      nameTopology(edge, 'edge', 'profile/boundary', faces.flatMap(face => face.topology.anchors), owner)
      // The old half-turn rims used the incidence of their corresponding
      // face halves. Both saved references now resolve to the closed rim.
      if(faces.length == 2 && faces.every(face => turnAliases.has(face))) {
        for(const half of [0, 1]) {
          const arc = {}
          nameTopology(arc, 'edge', 'profile/boundary',
            faces.flatMap(face => turnAliases.get(face)[half].anchors), owner)
          edge.topology.names.push(...arc.topology.names)
        }
        edge.topology.names.sort()
      }
      nameMeridian(edge)
    }
    for(const vertex of solid.vertices()) {
      nameTopology(vertex, 'vertex', 'profile/boundary',
        vertex.edges().flatMap(edge => edge.topology.anchors), owner)
      nameMeridian(vertex)
    }
    nameTopology(solid, 'body', 'profile/body', sources, owner)
  }
  identifyTopology(compound)
  return compound
}

function setConstruction(item, tags) {
  item.topology = Object.freeze({ ...item.topology,
    construction: [...new Set([...(item.topology.construction || []), ...tags])].sort(),
  })
}

function constructionSource(item) {
  const origin = item.topology.origin
  const recipe = origin.incidenceResolves ? origin.recipe : origin.branchRecipe?.()
  return {
    anchors: item.topology.anchors,
    ...(recipe?.branch.length ? { branch: recipe.branch } : {}),
    ...(recipe?.sides?.length ? { sides: recipe.sides } : {}),
  }
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
  const inputById = new Map(inputs.map(item => [item.kernelId, item]))
  const construction = new Map()
  const correspondence = operations.flatMap(({ compound, operation, branch, bodyParents = parents }) => {
    const solidIds = new Set(compound.solids().map(solid => solid.kernelId))
    const cells = outputs.filter(item => solidIds.has((item.topologyKind == 'body' ? item : item.solid).kernelId))
    const cellsById = new Map()
    for(const item of cells) {
      if(!cellsById.has(item.kernelId)) cellsById.set(item.kernelId, [])
      cellsById.get(item.kernelId).push(item)
    }
    const cellOrder = new Map(cells.map((item, index) => [item, index]))
    const inOutputOrder = items => [...items].sort((a, b) => cellOrder.get(a) - cellOrder.get(b))
    const history = compound.geom().topologyHistory()
    const policy = (namingPolicies[operation] || defaultPolicy)(operation, history)
    cells.forEach(item => roles.set(item, { name: policy.role(item), branch }))
    const changes = new Map(history.changes.map(change => [change.source, change]))
    const origins = new Map(history.origins.map(origin => [origin.id, origin]))
    const tagSources = (id, visited = new Set()) => {
      const input = inputById.get(id)
      if(input) return [JSON.stringify(constructionSource(input))]
      if(visited.has(id)) return []
      visited.add(id)
      return origins.get(id)?.sources.flatMap(source => tagSources(source, visited)) || []
    }
    for(const item of cells) {
      const tags = history.constructionTags[item.kernelId]?.flatMap(({ source, ...tag }) => {
        const inputs = [...new Set(tagSources(source))].sort()
        return inputs.length ? [JSON.stringify({ ...tag, inputs })] : []
      })
      if(tags?.length) construction.set(item, tags)
    }
    const childrenBySource = new Map()
    for(const origin of origins.values()) {
      for(const source of origin.sources) {
        if(!childrenBySource.has(source)) childrenBySource.set(source, [])
        childrenBySource.get(source).push(origin.id)
      }
    }
    const records = []
    const modifiedBodies = new Map()
    for(const source of inputs.filter(item => item.topologyKind != 'body')) {
      const resolution = compound.geom().resolve(source.geom())
      const entityIds = consumeEntityIds(resolution.entities)
      const resolved = inOutputOrder(new Set(entityIds.flatMap(id => cellsById.get(id) || [])
        .filter(item => item.topologyKind == source.topologyKind)))
      const descendants = resolved.filter(policy.continues)
      const generated = new Set(resolved)
      const visited = new Set()
      const pending = [...(childrenBySource.get(source.kernelId) || [])]
      while(pending.length) {
        const id = pending.pop()
        if(visited.has(id)) continue
        visited.add(id)
        for(const item of cellsById.get(id) || []) generated.add(item)
        for(const child of childrenBySource.get(id) || []) pending.push(child)
      }
      for(const item of descendants) generated.delete(item)
      records.push({ source, relation: 'modified', outputs: descendants })
      records.push({ source, relation: 'generated', outputs: inOutputOrder(generated) })
      if(source.topologyKind == 'face' && descendants.length) {
        if(!modifiedBodies.has(source.solid)) modifiedBodies.set(source.solid, new Set())
        const bodies = modifiedBodies.get(source.solid)
        for(const face of descendants) bodies.add(face.solid)
      }
      if(resolution.kind == 'preserved') for(const item of descendants) {
        item.tessId = source.tessId
        item.cachedTesselation = source.cachedTesselation
      }
    }
    const bodySources = new Set(bodyParents.flatMap(parent => parent.solids()))
    for(const source of inputs.filter(item => item.topologyKind == 'body')) {
      const ids = changes.get(source.kernelId)?.outputs || []
      const descendants = new Set([source.kernelId, ...ids].flatMap(id => cellsById.get(id) || [])
        .filter(item => item.topologyKind == 'body'))
      records.push({ source, relation: bodySources.has(source) ? 'modified' : 'contributed',
        outputs: inOutputOrder(descendants.size ? descendants : modifiedBodies.get(source) || []) })
    }
    return records
  })
  propagateTopology(outputs, correspondence, item => roles.get(item), topologyNeighbors)
  for(const [item, tags] of construction) setConstruction(item, tags)
  identifyTopology(out)
  return out
}
