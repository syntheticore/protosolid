import { makeID, hash } from './id.js'

// This module owns document identities. Shapex supplies correspondence
// and incidence, never array positions or geometric proximity.
let activeFeature = null

// Descriptions behind each feature's identities, kept only for debug output
const descriptions = new WeakMap()
// Incidence context of every selection identity. An identity is derived from
// its recipe, so this never goes stale, and references need not store it.
const selectionContexts = new Map()

const unique = values => [...new Set(values)].sort()
const shortHash = text => hash(text).slice(0, 12)
const ancestry = sources => unique(sources.flatMap(source =>
  [source.topology.identity, ...source.topology.ancestors]))

export class TopologySelectionError extends Error {}

const geometricFrames = new Set(['plane', 'axis', 'cylinder', 'axes'])

export function topologyIncludesFeature(topology, feature) {
  return topology.origin.feature?.id == feature.id ||
    [topology.identity, ...topology.ancestors].some(name => name.startsWith(`${feature.id}/topology/`)) ||
    (feature.constructor.modifiedFaceInputs || []).some(key => feature[key] &&
      [feature[key]()].flat().some(ref => topology.names.includes(ref.selection.identity)))
}

export function withTopologyNaming(feature, evaluate) {
  const previous = activeFeature
  activeFeature = feature
  feature.topologyNames ||= { version: 2 }
  try { return evaluate() }
  finally { activeFeature = previous }
}

// An identity is a pure function of its description, so every rebuild
// reproduces it without a stored lookup table.
export function topologyID(featureId, description) {
  return `${featureId}/topology/${shortHash(description)}`
}

export function topologyDescription(kind, role, sources, owner) {
  return JSON.stringify([kind, role, unique(sources), owner])
}

export function selectionSource(recipe) {
  return shortHash(JSON.stringify(recipe))
}

export function topologyIdentity(kind, role, sources = [], owner) {
  const description = topologyDescription(kind, role, sources, owner || activeFeature?.id)
  if(!activeFeature) return JSON.stringify([owner || 'transient', description])
  const id = topologyID(activeFeature.id, description)
  topologyDescriptions(activeFeature).set(id, description)
  return id
}

// Map of identity to description JSON for the identities a feature produced
export function topologyDescriptions(feature) {
  if(!descriptions.has(feature)) descriptions.set(feature, new Map())
  return descriptions.get(feature)
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
  const construction = unique(sources.flatMap(source => source.topology.construction || []))
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
    ...(construction.length ? { construction } : {}),
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
    ...(inherited.construction ? { construction: inherited.construction } : {}),
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
  // Faces precede edges, edges precede vertices, and vertices precede bodies.
  // This is a dependency order, not an identity: reversing the items within a
  // kind has no effect.
  for(const kind of ['face', 'edge', 'vertex', 'body']) {
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
      incidencePaths(neighbor, neighbors, depth - 1, nextVisited) : []
    for(const name of names) {
      paths.push([name])
      for(const path of descendants) paths.push([name, ...path])
    }
  }
  return unique(paths.map(path => JSON.stringify(path))).map(path => JSON.parse(path))
}

// Resolution needs membership in named paths, not every combination of a
// merged cell's aliases. Follow only the requested names through incidence.
function incidenceContexts(neighbors) {
  const contexts = new Map(), adjacent = new Map(), namesByCell = new Map()
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
          const key = JSON.stringify(path)
          if(!results.has(key)) results.set(key, contains(item, path))
          return results.get(key)
        },
      })
    }
    return contexts.get(item)
  }
}

// Name a support face or body. Its anchors suffice when unique. Split
// siblings also need distinguishing incidence paths.
export function supportName(item, candidates, neighbors) {
  const siblings = candidates.filter(candidate =>
    item.topology.anchors.every(name => candidate.topology.names.includes(name)))
  if(siblings.length == 1) return item.topology.anchors
  const contextFor = incidenceContexts(neighbors)
  const excludes = []
  const distinguishes = branch => siblings.filter(sibling =>
    branch.every(path => contextFor(sibling).has(path)) &&
    excludes.every(path => !contextFor(sibling).has(path))).length == 1
  const branch = incidencePaths(item, neighbors).filter(path =>
    siblings.some(sibling => !contextFor(sibling).has(path)))
  if(!distinguishes(branch) && item.topology.kind == 'body') {
    // A component can carry a subset of a sibling's construction paths.
    // Use named boundaries on the siblings as witnesses for that complement.
    const paths = siblings.flatMap(sibling => incidencePaths(sibling, neighbors))
    excludes.push(...unique(paths.map(path => JSON.stringify(path)))
      .map(path => JSON.parse(path)).filter(path => !contextFor(item).has(path)))
  }
  if(!distinguishes(branch)) return null
  for(let i = branch.length - 1; i >= 0; i--) {
    const [path] = branch.splice(i, 1)
    if(!distinguishes(branch)) branch.splice(i, 0, path)
  }
  for(let i = excludes.length - 1; i >= 0; i--) {
    const [path] = excludes.splice(i, 1)
    if(!distinguishes(branch)) excludes.splice(i, 0, path)
  }
  return { anchors: item.topology.anchors, branch, ...(excludes.length ? { excludes } : {}) }
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

// References store only the selection identity; its recipe holds the context
export function resolveTopology(items, selection, neighbors) {
  const context = selection.context || selectionContexts.get(selection.identity)
  return resolveTopologyWithContexts(items, { ...selection, context }, incidenceContexts(neighbors))
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
  const itemsByName = new Map()
  for(const item of items) {
    for(const name of item.topology.names) {
      if(!itemsByName.has(name)) itemsByName.set(name, [])
      itemsByName.get(name).push(item)
    }
  }
  for(const item of items) {
    const origin = item.topology.origin
    if(origin.recipe) continue
    const recipe = topologySelection(item, neighbors)
    const candidates = recipe.anchors.map(name => itemsByName.get(name) || [])
      .reduce((smallest, matches) => matches.length < smallest.length ? matches : smallest, items)
    const siblings = candidates.filter(candidate => matchesSelection(candidate, recipe))
    const contexts = siblings.length > 1 ? siblings.map(contextFor) : []
    recipe.branch = siblings.length > 1 ? recipe.context.filter(path =>
      contexts.some(context => !context.has(path))) : []
    const resolution = resolveRecipe(siblings, recipe, contextFor, sides)
    origin.recipe = recipe
    origin.incidenceResolves = !!origin.feature && resolution.status == 'resolved' && resolution.items[0] === item
    // Retain the producer's evidence for picks made after later modifiers.
    origin.branchRecipe = () => {
      if(!origin.feature) return null
      const constraints = sides(item)
      const captured = { ...recipe, sides: [...constraints] }
      const pickContext = incidenceContexts(neighbors)
      const resolved = () => {
        const result = resolveRecipe(items, captured, pickContext, sides)
        return result.status == 'resolved' && result.items[0] === item
      }
      if(!constraints.length || !resolved()) return null
      // Keep only the evidence needed to identify this cell.
      for(let i = captured.sides.length - 1; i >= 0; i--) {
        const [constraint] = captured.sides.splice(i, 1)
        if(!resolved()) captured.sides.splice(i, 0, constraint)
      }
      return captured
    }
    if(item.topology.kind == 'body' && siblings.length > 1 && origin.incidenceResolves) {
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
      const result = resolveRecipe(items, binding.recipe, branchContextFor, sides)
      if(result.status == 'resolved' && matches.has(result.items[0])) {
        matches.get(result.items[0]).push(binding)
      }
    }
    for(const [item, matched] of matches) {
      if(matched.length > 1) {
        item.topology = Object.freeze({ ...item.topology,
          origin: { ...item.topology.origin, incidenceResolves: false },
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
        origin: { feature, incidenceResolves: true },
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
  if(!feature) throw new TopologySelectionError('Display-only topology cannot be referenced')
  const recipe = origin.incidenceResolves ? origin.recipe : origin.branchRecipe?.()
  if(!recipe) {
    throw new TopologySelectionError('The selected topology cannot be distinguished by its construction and boundaries')
  }
  return withTopologyNaming(feature, () => {
    const identity = topologyIdentity(recipe.kind, 'selection', [selectionSource(recipe)])
    feature.topologyNames.selections ||= []
    if(!feature.topologyNames.selections.some(binding => binding.identity == identity)) {
      feature.topologyNames.selections.push({ identity, recipe })
    }
    selectionContexts.set(identity, recipe.context)
    addName(item, identity)
    return { version: 1, kind: recipe.kind, identity, policy }
  })
}

function resolveRecipe(items, recipe, contextFor, sides = () => []) {
  const legacyGeometry = recipe.sides?.some(constraint => geometricFrames.has(constraint.frame))
  const constrained = items.filter(item => {
    if(!matchesSelection(item, recipe)) return false
    if(recipe.branch.length) {
      const context = contextFor(item)
      if(!recipe.branch.every(path => context.has(path))) return false
    }
    return !recipe.sides || recipe.sides.every(constraint =>
      sides(item, legacyGeometry).some(side => JSON.stringify(side) == JSON.stringify(constraint)))
  })
  return resolveTopologyWithContexts(constrained, recipe, contextFor)
}

export function restoreTopologySelections(feature, items, neighbors, sides) {
  const failures = new Map()
  for(const binding of feature.topologyNames.selections || []) {
    selectionContexts.set(binding.identity, binding.recipe.context)
    const result = resolveRecipe(items, binding.recipe, incidenceContexts(neighbors), sides)
    if(result.status == 'resolved') addName(result.items[0], binding.identity)
    else failures.set(binding.identity, result.status)
  }
  return failures
}

// Rewrites a cell's names while keeping its structure. Unlike a namespace, this
// lets merged cells keep the continuity names of their other parents.
export function renameTopology(items, rename, origin = topology => topology.origin) {
  for(const item of items) {
    const { topology } = item
    item.topology = Object.freeze({ ...topology,
      identity: rename(topology.identity),
      origin: origin(topology),
      anchors: Object.freeze(unique(topology.anchors.map(rename))),
      names: unique(topology.names.map(rename)),
    })
  }
}

export function namespaceTopology(items, role) {
  for(const item of items) {
    const ancestors = ancestry([item])
    nameTopology(item, item.topology.kind, role, item.topology.anchors)
    item.topology = Object.freeze({ ...item.topology, ancestors: Object.freeze(ancestors) })
  }
}
