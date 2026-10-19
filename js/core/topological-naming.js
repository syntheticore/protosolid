import { makeID } from './id.js'

// This module owns document identities. Shapex supplies correspondence
// and incidence, never array positions or geometric proximity.
let activeFeature = null

const unique = values => [...new Set(values)].sort()
const ancestry = sources => unique(sources.flatMap(source =>
  [source.topology.identity, ...source.topology.ancestors]))

export function withTopologyNaming(feature, evaluate) {
  const previous = activeFeature
  activeFeature = feature
  feature.topologyNames ||= { version: 1, bindings: [] }
  try { return evaluate() }
  finally { activeFeature = previous }
}

export function topologyIdentity(kind, role, sources = [], owner) {
  const description = JSON.stringify([kind, role, unique(sources), owner || activeFeature?.id])
  if(!activeFeature) return JSON.stringify([owner || 'transient', description])
  const bindings = activeFeature.topologyNames.bindings
  let binding = bindings.find(binding => binding.description == description)
  if(!binding) {
    binding = { description, id: `${activeFeature.id}/topology/${makeID()}` }
    bindings.push(binding)
  }
  return binding.id
}

export function nameTopology(item, kind, role, sources = [], owner) {
  const identity = topologyIdentity(kind, role, sources, owner)
  const anchors = sources.length ? unique(sources.map(source => topologyIdentity(kind, role, [source], owner))) : [identity]
  item.topology = Object.freeze({ kind, identity, origin: { feature: activeFeature },
    anchors: Object.freeze(anchors), names: unique([identity, ...anchors]),
    ancestors: Object.freeze(unique(sources)),
  })
}

function inherit(item, sources, oneToOne) {
  const identities = unique(sources.map(source => source.topology.identity))
  const identity = identities.length == 1 ? identities[0] :
    topologyIdentity(item.topologyKind, 'merged', identities)
  item.topology = Object.freeze({
    kind: item.topologyKind,
    identity,
    // A bijective history mapping preserves an earlier feature's producer.
    // Within one feature, recapture recipes on its completed geometry: a
    // cutter's support sides can change when the Boolean trims its faces.
    origin: oneToOne && sources[0].topology.origin.feature != activeFeature ? sources[0].topology.origin :
      { feature: activeFeature || sources[0].topology.origin.feature },
    anchors: Object.freeze(unique(sources.flatMap(source => source.topology.anchors))),
    ancestors: Object.freeze(ancestry(sources)),
    names: oneToOne ? sources[0].topology.names :
      unique([identity, ...sources.flatMap(source => source.topology.names)]),
  })
}

// A semantic branch is a new logical cell even when the other branch is empty.
// Preserve continuity names for selections made before the operation, but use
// only the branch's own anchors for selections made on this result.
function nameBranch(item, role, sources) {
  const inherited = item.topology
  nameTopology(item, item.topologyKind, role,
    sources.flatMap(source => source.topology.anchors))
  item.topology = Object.freeze({ ...item.topology,
    names: unique([...item.topology.names, ...inherited.names]),
    ancestors: inherited.ancestors,
  })
}

// A correspondence is { source, relation, outputs }. Modified establishes
// logical continuity; generated establishes a new cell from named inputs;
// contributed records geometric ancestry without continuing the source cell.
// Modified includes exact retention. An empty output set means deletion.
// An operation can supply { name, branch }, where branch is a semantic role
// supplied by the operation, never an output position or a geometric guess.
export function propagateTopology(items, correspondence, operation, neighbors) {
  const modified = new Map(items.map(item => [item, []]))
  const generated = new Map(items.map(item => [item, []]))
  const contributed = new Map(items.map(item => [item, []]))
  const descendants = new Map()
  for(const { source, relation, outputs } of correspondence) {
    if(!source.topology) continue
    for(const output of outputs) {
      contributed.get(output).push(source)
      if(relation == 'modified') modified.get(output).push(source)
      if(relation == 'generated') generated.get(output).push(source)
    }
    if(relation == 'modified') {
      const cells = descendants.get(source) || new Set()
      outputs.forEach(output => cells.add(output))
      descendants.set(source, cells)
    }
  }
  // Faces precede edges, and edges precede bodies. This is a dependency order,
  // not an identity: reversing the items within a kind has no effect.
  for(const kind of ['face', 'edge', 'body']) {
    for(const item of items.filter(item => item.topologyKind == kind)) {
      const contract = typeof operation == 'function' ? operation(item) : operation
      const role = typeof contract == 'string' ? contract : contract.name
      const branch = typeof contract == 'string' ? null : contract.branch
      const constructionRole = branch ? `${role}/branch/${branch}` : role
      if(modified.get(item).length) {
        const sources = [...new Set(modified.get(item))]
        inherit(item, sources, sources.length == 1 && descendants.get(sources[0]).size == 1)
        if(branch) nameBranch(item, constructionRole, sources)
      } else if(generated.get(item).length) {
        nameTopology(item, kind, `${constructionRole}/generated`,
          generated.get(item).flatMap(source => source.topology.anchors))
      } else {
        nameTopology(item, kind, `${constructionRole}/boundary`,
          neighbors(item).flatMap(neighbor => neighbor.topology ? neighbor.topology.anchors : []))
      }
      item.topology = Object.freeze({ ...item.topology,
        ancestors: Object.freeze(unique([...item.topology.ancestors, ...ancestry(contributed.get(item))])),
      })
    }
  }
}

function incidencePaths(item, neighbors, depth = 2, visited = new Set()) {
  const paths = []
  const nextVisited = new Set([...visited, item])
  for(const neighbor of neighbors(item).filter(neighbor => !nextVisited.has(neighbor))) {
    const names = neighbor.topology.anchors
    const descendants = depth > 1 && names.length ?
      incidencePaths(neighbor, neighbors, depth - 1, nextVisited).map(path => JSON.parse(path)) : []
    for(const name of names) {
      paths.push([name])
      for(const path of descendants) paths.push([name, ...path])
    }
  }
  return unique(paths.map(path => JSON.stringify(path)))
}

// Resolution needs membership in named paths, not every combination of a
// merged cell's aliases. Follow only the requested names through incidence.
function incidenceContexts(neighbors) {
  const contexts = new Map(), paths = new Map(), adjacent = new Map(), namesByCell = new Map()
  const cells = cell => {
    if(!adjacent.has(cell)) adjacent.set(cell, neighbors(cell))
    return adjacent.get(cell)
  }
  const named = (cell, name) => {
    if(!namesByCell.has(cell)) namesByCell.set(cell, new Set(cell.topology.names))
    return namesByCell.get(cell).has(name)
  }
  const contains = (cell, names, visited = new Set()) => {
    const nextVisited = new Set([...visited, cell])
    return cells(cell).some(neighbor => !nextVisited.has(neighbor) &&
      named(neighbor, names[0]) &&
      (names.length == 1 || contains(neighbor, names.slice(1), nextVisited)))
  }
  return item => {
    if(!contexts.has(item)) {
      const results = new Map()
      contexts.set(item, {
        has(path) {
          if(!paths.has(path)) paths.set(path, JSON.parse(path))
          if(!results.has(path)) results.set(path, contains(item, paths.get(path)))
          return results.get(path)
        },
      })
    }
    return contexts.get(item)
  }
}

export function topologySelection(item, neighbors, policy = 'single') {
  if(!item.topology) throw new Error('This entity has no construction identity')
  return {
    version: 1,
    kind: item.topology.kind,
    identity: item.topology.identity,
    anchors: item.topology.anchors,
    policy,
    context: incidencePaths(item, neighbors),
  }
}

export function resolveTopology(items, selection, neighbors) {
  return resolveTopologyWithContexts(items, selection, incidenceContexts(neighbors))
}

function resolveTopologyWithContexts(items, selection, contextFor) {
  let candidates = items.filter(item => matchesSelection(item, selection))
  if(selection.policy == 'all-descendants') return { status: candidates.length ? 'resolved' : 'deleted', items: candidates }
  if(candidates.length > 1) {
    // An inserted feature can remove an old boundary. Only surviving named
    // constraints discriminate descendants; never pick the first or nearest.
    const surviving = selection.context.filter(name => candidates.some(item => contextFor(item).has(name)))
    candidates = candidates.filter(item => surviving.every(name => contextFor(item).has(name)))
  }
  return {
    status: candidates.length == 1 ? 'resolved' : candidates.length ? 'ambiguous' : 'deleted',
    items: candidates.length == 1 ? candidates : [],
  }
}

function matchesSelection(item, selection) {
  return item.topology?.kind == selection.kind &&
    (selection.anchors ? selection.anchors.every(name => item.topology.names.includes(name)) :
      item.topology.names.includes(selection.identity))
}

function addName(item, identity) {
  // Cached copies of the same cell share its metadata. Annotating a preview
  // must also bind the upstream cache that feature regeneration consumes.
  // Bijective kernel history also shares annotations with the producing cell;
  // branching history creates independent annotation lists for each branch.
  const names = item.topology.names
  if(!names.includes(identity)) names.push(identity)
  names.sort()
}

// Record the producer's incidence before subsequent operations change it.
// A one-to-one descendant retains this origin; it must not acquire a
// dependency on an unrelated feature merely because it was selected later.
export function initializeTopologyOrigins(items, neighbors, sides = () => []) {
  const branches = []
  // Names and incidence stay fixed until the branch annotation pass below.
  // Share lookup tables across recipes only within this resolution phase.
  const contextFor = incidenceContexts(neighbors)
  for(const item of items) {
    const origin = item.topology.origin
    if(origin.recipe) continue
    const recipe = topologySelection(item, neighbors)
    const siblings = items.filter(candidate => matchesSelection(candidate, recipe))
    const contexts = siblings.length > 1 ? siblings.map(contextFor) : []
    recipe.branch = siblings.length > 1 ? recipe.context.filter(path =>
      contexts.some(context => !context.has(path))) : []
    const resolution = resolveRecipe(items, recipe, neighbors, sides, contextFor)
    origin.recipe = recipe
    origin.selectable = !!origin.feature && resolution.status == 'resolved' && resolution.items[0] === item
    // Retain the producer's geometry, not a later modifier's preview. A pick
    // can add oriented support constraints when named incidence is symmetric.
    origin.pickRecipe = () => {
      const constraints = sides(item)
      const captured = { ...recipe, sides: [...constraints] }
      const resolved = () => {
        const result = resolveRecipe(items, captured, neighbors, sides)
        return result.status == 'resolved' && result.items[0] === item
      }
      if(!origin.feature || !constraints.length || !resolved()) {
        throw new Error('The selected topology cannot be distinguished by its construction and boundaries')
      }
      // Keep only the evidence needed to identify this cell. In particular,
      // a bore rim needs its axial side, not unrelated supporting boundaries.
      for(let i = captured.sides.length - 1; i >= 0; i--) {
        const [constraint] = captured.sides.splice(i, 1)
        if(!resolved()) captured.sides.splice(i, 0, constraint)
      }
      return captured
    }
    if(item.topology.kind == 'body' && siblings.length > 1 && origin.selectable) {
      branches.push(item)
    }
  }
  // Resolve every saved branch before annotating any result. Changing boundary
  // context must not allocate new body IDs, and two old branches must never
  // claim the same result. Visibility and copy namespaces depend on these IDs.
  const features = new Set(branches.map(item => item.topology.origin.feature))
  if(activeFeature?.topologyNames.branches?.length) features.add(activeFeature)
  for(const feature of features) {
    const branchContextFor = incidenceContexts(neighbors)
    const bindings = feature.topologyNames.branches ||= []
    const matches = new Map(items.filter(item => item.topology.kind == 'body')
      .map(item => [item, []]))
    for(const binding of bindings) {
      const result = resolveRecipe(items, binding.recipe, neighbors, sides, branchContextFor)
      if(result.status == 'resolved' && matches.has(result.items[0])) {
        matches.get(result.items[0]).push(binding)
      }
    }
    for(const [item, matched] of matches) {
      if(matched.length > 1) {
        item.topology = Object.freeze({ ...item.topology,
          origin: { ...item.topology.origin, selectable: false },
        })
        continue
      }
      let binding = matched[0]
      if(!binding) {
        if(!branches.includes(item) || item.topology.origin.feature != feature) continue
        binding = { identity: `${feature.id}/topology/${makeID()}`, recipe: item.topology.origin.recipe }
        bindings.push(binding)
      }
      const identity = binding.identity
      item.topology = Object.freeze({
        ...item.topology, identity,
        anchors: Object.freeze(unique([...item.topology.anchors, identity])),
        names: unique([...item.topology.names, identity]),
        origin: { feature, selectable: true },
      })
      // A selected branch requires its restored logical identity. Its broad
      // construction ancestry must never bypass an unresolved branch binding.
      item.topology.origin.recipe = { ...topologySelection(item, neighbors), branch: [] }
    }
  }
}

// Capture a selected cell at its producer, not a changing current array slot.
// The feature persists this recipe and rebinds it on each independent rebuild.
export function captureTopologySelection(item, policy = 'single') {
  const origin = item.topology.origin
  const { feature } = origin
  if(!origin.selectable && !origin.pickRecipe) {
    throw new Error('The selected topology cannot be distinguished by its construction and boundaries')
  }
  const recipe = origin.selectable ? origin.recipe : origin.pickRecipe()
  if(!feature) throw new Error('Display-only topology cannot be referenced')
  return withTopologyNaming(feature, () => {
    const identity = topologyIdentity(recipe.kind, 'selection', [JSON.stringify(recipe)])
    feature.topologyNames.selections ||= []
    if(!feature.topologyNames.selections.some(binding => binding.identity == identity)) {
      feature.topologyNames.selections.push({ identity, recipe })
    }
    addName(item, identity)
    return { version: 1, kind: recipe.kind, identity, policy, context: recipe.context }
  })
}

function resolveRecipe(items, recipe, neighbors, sides = () => [], contextFor = incidenceContexts(neighbors)) {
  const constrained = items.filter(item => {
    if(!matchesSelection(item, recipe)) return false
    if(recipe.branch.length) {
      const context = contextFor(item)
      if(!recipe.branch.every(path => context.has(path))) return false
    }
    return !recipe.sides || recipe.sides.every(constraint =>
      sides(item).some(side => JSON.stringify(side) == JSON.stringify(constraint)))
  })
  return resolveTopologyWithContexts(constrained, recipe, contextFor)
}

export function restoreTopologySelections(feature, items, neighbors, sides) {
  const failures = new Map()
  for(const binding of feature.topologyNames.selections || []) {
    const result = resolveRecipe(items, binding.recipe, neighbors, sides)
    if(result.status == 'resolved') addName(result.items[0], binding.identity)
    else failures.set(binding.identity, result.status)
  }
  return failures
}

export function namespaceTopology(items, role) {
  for(const item of items) {
    const ancestors = ancestry([item])
    nameTopology(item, item.topology.kind, role, item.topology.anchors)
    item.topology = Object.freeze({ ...item.topology, ancestors: Object.freeze(ancestors) })
  }
}
