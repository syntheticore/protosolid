import { makeID } from './id.js'

// This module owns document identities. Kernel adapters supply correspondence
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
    // A bijective history mapping preserves the cell's producer and its
    // shared selection annotations. Splits and merges establish new cells.
    origin: oneToOne ? sources[0].topology.origin :
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

function incidencePaths(item, neighbors, aliases = false, depth = 2, visited = new Set()) {
  const paths = []
  const nextVisited = new Set([...visited, item])
  for(const neighbor of neighbors(item).filter(neighbor => !nextVisited.has(neighbor))) {
    const names = aliases ? neighbor.topology.names : neighbor.topology.anchors
    for(const name of names) {
      paths.push([name])
      if(depth > 1) {
        for(const path of incidencePaths(neighbor, neighbors, aliases, depth - 1, nextVisited)) paths.push([name, ...path])
      }
    }
  }
  return unique(paths.map(path => JSON.stringify(path)))
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
  let candidates = items.filter(item => matchesSelection(item, selection))
  if(selection.policy == 'all-descendants') return { status: candidates.length ? 'resolved' : 'deleted', items: candidates }
  if(candidates.length > 1) {
    const contexts = new Map(candidates.map(item => [item, new Set(incidencePaths(item, neighbors, true))]))
    // An inserted feature can remove an old boundary. Only surviving named
    // constraints discriminate descendants; never pick the first or nearest.
    const surviving = selection.context.filter(name => candidates.some(item => contexts.get(item).has(name)))
    candidates = candidates.filter(item => surviving.every(name => contexts.get(item).has(name)))
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
export function initializeTopologyOrigins(items, neighbors) {
  const branches = []
  for(const item of items) {
    const origin = item.topology.origin
    if(origin.recipe) continue
    const recipe = topologySelection(item, neighbors)
    const siblings = items.filter(candidate => matchesSelection(candidate, recipe))
    recipe.branch = siblings.length > 1 ? recipe.context.filter(path =>
      siblings.some(sibling => !incidencePaths(sibling, neighbors, true).includes(path))) : []
    const resolution = resolveRecipe(items, recipe, neighbors)
    origin.recipe = recipe
    origin.selectable = !!origin.feature && resolution.status == 'resolved' && resolution.items[0] === item
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
    const bindings = feature.topologyNames.branches ||= []
    const matches = new Map(items.filter(item => item.topology.kind == 'body')
      .map(item => [item, []]))
    for(const binding of bindings) {
      const result = resolveRecipe(items, binding.recipe, neighbors)
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
  const { feature, recipe, selectable } = item.topology.origin
  if(!selectable) {
    throw new Error('The selected topology cannot be distinguished by its construction and boundaries')
  }
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

function resolveRecipe(items, recipe, neighbors) {
  const constrained = items.filter(item => matchesSelection(item, recipe) &&
    recipe.branch.every(path => incidencePaths(item, neighbors, true).includes(path)))
  return resolveTopology(constrained, recipe, neighbors)
}

export function restoreTopologySelections(feature, items, neighbors) {
  const failures = new Map()
  for(const binding of feature.topologyNames.selections || []) {
    const result = resolveRecipe(items, binding.recipe, neighbors)
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
