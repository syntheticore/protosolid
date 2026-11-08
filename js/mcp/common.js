import { nextTick, toRaw } from 'vue'

import { ToolError, itemId, findComponent, describeTimeline } from './describe.js'

// Helpers shared by the tool modules

export function settle(doc) {
  return nextTick().then(() => doc.regenerationQueue.catch(() => {}))
}

export function editable(doc) {
  if(doc.activeMerge) throw new ToolError('The document is showing a merge preview, which cannot be edited. Ask the user to finish the merge.')
}

// Closes feature boxes and sketches the user has open, keeping sketch edits
export async function closeEditing(doc) {
  await doc.cancelFeature()
  await settle(doc)
}

// Edits of a component go to the component it is an instance of
export function editableComponent(doc, id) {
  const component = findComponent(doc, id)
  if(component.readOnly) throw new ToolError(`Component ${itemId(component)} is read-only`)
  return doc.getComponent(component.sourceId()) || component
}

export function activateComponent(doc, id) {
  if(!id) return
  doc.activateComponent(editableComponent(doc, id))
}

// Features after the marker are not evaluated, so changes could not be checked
export function beforeMarker(doc, feature) {
  if(doc.timeline.features.indexOf(toRaw(feature)) >= doc.timeline.marker) {
    throw new ToolError(`${itemId(feature)} is after the timeline marker. Use move_marker to move the marker behind it first.`)
  }
}

export const failingFeatures = (doc, except) => describeTimeline(doc).filter(feature => feature.error && feature.id != except)

// Simulations, drawings, CAM setups, sections and canvases live on component
// definitions and are addressed as `<kind>:<id>`
export const storedKinds = {
  simulation: 'simulations',
  drawing: 'drawings',
  cam: 'camSetups',
  section: 'sectionViews',
  canvas: 'canvases',
}

export function findStored(doc, id, kind) {
  const [prefix, key] = String(id).split(':')
  if(prefix != kind || !key) throw new ToolError(`Expected an id like ${kind}:<id>, got ${id}`)
  for(const component of doc.top().getChildren()) {
    const item = component.creator[storedKinds[kind]].find(item => item.id == key)
    if(item) return { component, item }
  }
  throw new ToolError(`No ${kind} with id ${id}`)
}

// Options are validated against what the app offers
export function option(value, options, label) {
  const keys = Array.isArray(options) ? options : Object.keys(options)
  if(!keys.includes(value)) throw new ToolError(`${label} must be one of ${keys.join(', ')}`)
  return value
}

export function number(value, label, { min = -Infinity, max = Infinity, integer = false } = {}) {
  if(!Number.isFinite(value) || value < min || value > max || (integer && !Number.isInteger(value))) {
    const range = [min > -Infinity && `at least ${min}`, max < Infinity && `at most ${max}`].filter(Boolean).join(' and ')
    throw new ToolError(`${label} must be ${integer ? 'an integer' : 'a number'}${range ? ' ' + range : ''}`)
  }
  return value
}

export function bool(value, label) {
  if(typeof value != 'boolean') throw new ToolError(`${label} must be true or false`)
  return value
}

export function string(value, label) {
  if(typeof value != 'string') throw new ToolError(`${label} must be a string`)
  return value
}

export function vector3(value, label) {
  if(!Array.isArray(value) || value.length != 3 || !value.every(Number.isFinite)) {
    throw new ToolError(`${label} must be given as [x, y, z]`)
  }
  return value
}

// Assigns known keys of a settings object, checking each with its validator
export function assign(target, values, validators, label) {
  Object.entries(values || {}).forEach(([key, value]) => {
    if(!validators[key]) throw new ToolError(`${label} has no setting '${key}'. Its settings are ${Object.keys(validators).join(', ')}.`)
    target[key] = validators[key](value, key)
  })
}
