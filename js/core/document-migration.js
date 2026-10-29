import { topologyID, topologyDescription, selectionSource } from './topological-naming.js'

// Upgrades plain documents saved with topologyNames version 1, which numbered
// topology IDs by allocation and stored a binding table from construction
// description to ID. Version 2 derives each ID from its description, so every
// ID is rehashed from its description after rehashing the IDs it refers to,
// and every occurrence of an old ID in the document is replaced. Recipes now
// store incidence paths as arrays instead of JSON strings, and references no
// longer copy their recipe's context.
export function migrateDocument(dump) {
  const features = dump.timeline.features.filter(feature => feature.topologyNames?.version == 1)
  if(!features.length) return dump

  const bindings = new Map(), recipes = new Map()
  for(const feature of features) {
    for(const binding of feature.topologyNames.bindings) {
      bindings.set(binding.id, { feature, description: JSON.parse(binding.description) })
    }
    for(const selection of feature.topologyNames.selections || []) {
      arrayPaths(selection.recipe)
      recipes.set(selection.identity, selection.recipe)
    }
    for(const branch of feature.topologyNames.branches || []) arrayPaths(branch.recipe)
  }
  dropReferenceContexts(dump)

  const ids = new Map(), descriptions = new Map()
  // Body branch identities were never bound to a description and keep their IDs
  const rename = text => text.replace(/[\w-]+\/topology\/id-\d+/g, id => bindings.has(id) ? newID(id) : id)
  // Embedded documents of placed components are migrated when they are parsed
  const rewrite = value => typeof value == 'string' ? rename(value) :
    Array.isArray(value) ? value.map(rewrite) :
    value && typeof value == 'object' ? Object.fromEntries(Object.entries(value).map(([key, entry]) => {
      const rewritten = key == 'cachedData' ? entry : rewrite(entry)
      // Support constraints compare anchor arrays in their canonical ID order.
      if(key == 'support' || key == 'crossSupport') {
        const anchors = Array.isArray(rewritten) ? rewritten : rewritten.anchors
        anchors.sort()
      }
      return [rename(key), rewritten]
    })) :
    value
  const newID = id => {
    if(!ids.has(id)) {
      const { feature, description: [kind, role, sources, owner] } = bindings.get(id)
      const renamed = role == 'selection' ? [selectionSource(rewrite(recipes.get(id)))] : sources.map(rename)
      const description = topologyDescription(kind, role, renamed, owner)
      const migrated = topologyID(feature.id, description)
      ids.set(id, migrated)
      descriptions.set(migrated, JSON.parse(description))
    }
    return ids.get(id)
  }

  for(const feature of features) {
    const { bindings: featureBindings, ...names } = feature.topologyNames
    feature.topologyNames = { ...names, version: 2 }
    if(featureBindings.length) feature.topologyDescriptions = Object.fromEntries(featureBindings.map(binding => {
      const id = newID(binding.id)
      return [id, descriptions.get(id)]
    }))
  }
  return rewrite(dump)
}

function arrayPaths(recipe) {
  const parse = paths => paths.map(path => JSON.parse(path))
  recipe.context = parse(recipe.context)
  recipe.branch = parse(recipe.branch)
  for(const side of recipe.sides || []) {
    for(const support of [side.support, side.crossSupport]) {
      if(support?.branch) support.branch = parse(support.branch)
    }
  }
}

function dropReferenceContexts(value) {
  if(Array.isArray(value)) return value.forEach(dropReferenceContexts)
  if(!value || typeof value != 'object') return
  if(value.$class?.endsWith('Reference') && value.selection) delete value.selection.context
  Object.values(value).forEach(dropReferenceContexts)
}
