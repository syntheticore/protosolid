import { nextTick, toRaw } from 'vue'

import Expression from '../core/expression.js'
import { ConstructionHelper } from '../core/helpers.js'
import { lastId, setLastId } from '../core/id.js'
import { inputReference } from '../core/references.js'
import { decodeFile } from '../core/document-format.js'
import { loadFile } from '../utils.js'
import { featureTypes, featureName } from './catalog.js'
import { editSketch, prepareSketchOperations, operationTypes, constraintTypeNames } from './sketch-ops.js'
import { settle, editable, closeEditing, activateComponent, beforeMarker, failingFeatures } from './common.js'
import { propertyTools } from './properties.js'
import { analysisTools } from './analysis.js'
import { outputTools } from './output.js'
import { historyTools } from './history.js'
import {
  ToolError,
  itemId,
  resolveItem,
  findComponent,
  findFeature,
  findSketch,
  sketchComponent,
  describeDocument,
  describeComponent,
  describeBody,
  describeTopology,
  describeFeature,
  describeSettings,
  describeTimeline,
  describeSketch,
} from './describe.js'

// MCP tools. Agents work on the document structure: they add features and
// sketches at the timeline marker and update existing ones, which regenerates
// the timeline like the user's edits do. Every change lands in the undo history.

const pickerTypes = {
  profile: 'closed sketch region (profile:…)',
  curve: 'sketch curve (curve:…)',
  path: 'sketch curve or model edge',
  axis: 'straight edge, cylindrical face, axis (helper:…) or sketch line',
  plane: 'planar face or plane (helper:…)',
  face: 'face (face:…)',
  edge: 'edge (edge:…)',
  solid: 'body (body:…)',
  point: 'vertex, sketch point or point (helper:…)',
  componentRef: 'component (component:…)',
  patternInput: 'feature, body or component',
  joint: 'joint (joint:…)',
}

function setValue(feature, key, value, component) {
  const setting = feature.settings[key]
  const invalid = expected => new ToolError(`Setting '${key}' expects ${expected}, got ${JSON.stringify(value)}`)
  switch(setting.type) {
    case 'length':
    case 'angle': {
      if(typeof value == 'string') {
        let expression
        try {
          expression = new Expression(value, component.getParameters(), setting.type)
          feature[key] = expression.getBase()
        } catch(error) {
          throw new ToolError(`Invalid expression for '${key}': ${error.message}`)
        }
        feature.setExpression(key, expression.expression)
      } else if(Number.isFinite(value)) {
        feature.setExpression(key, null)
        feature[key] = value
      } else {
        throw invalid(setting.type == 'length' ? 'a length in mm or an expression like "5mm" or "width / 2"' : 'an angle in degrees or an expression like "30deg"')
      }
      if(feature[key] < 0 && !setting.allowNegative) throw new ToolError(`Setting '${key}' must not be negative`)
      return
    }
    case 'integer':
    case 'number':
      if(!(setting.type == 'integer' ? Number.isInteger(value) : Number.isFinite(value)) ||
        value < (setting.min ?? -Infinity) || value > (setting.max ?? Infinity)) throw invalid(`a ${setting.type} within its limits`)
      feature[key] = value
      return
    case 'bool':
      if(typeof value != 'boolean') throw invalid('true or false')
      feature[key] = value
      return
    case 'text':
      if(typeof value != 'string') throw invalid('a string')
      feature[key] = value
      return
    case 'select':
    case 'enum': {
      const options = Object.keys(feature.settingOptions?.(key) || setting.options)
      if(!options.includes(value)) throw invalid(`one of ${options.join(', ')}`)
      feature[key] = value
      feature.settingChanged?.(key, value)
      return
    }
  }
  throw new ToolError(`Setting '${key}' of type ${setting.type} cannot be set by agents`)
}

// Picked inputs reference items of the model before the feature
function setPicker(doc, feature, key, value, tree) {
  const setting = feature.settings[key]
  const ids = value === null ? [] : [value].flat()
  if(ids.length > 1 && !setting.multi) throw new ToolError(`Setting '${key}' takes a single item`)
  const picked = ids.map(id => {
    const item = resolveItem(doc, id, tree)
    if(!feature.acceptsInput(item)) throw new ToolError(`${id} cannot be used for '${key}' of this feature`)
    try {
      return { item, reference: inputReference(setting.type, item) }
    } catch(error) {
      throw new ToolError(`${id} cannot be used for '${key}', which expects a ${pickerTypes[setting.type]}: ${error.message}`)
    }
  })
  const references = picked.map(({ reference }) => reference)
  feature[key] = !references.length ? null : setting.multi ? () => references : () => references[0]
  picked.forEach(({ item, reference }) => feature.pickedInput?.(key, reference, item))
}

// Options go first, as they decide which inputs exist
function applySettings(doc, feature, settings, tree) {
  const component = tree.findChild(feature.componentId)
  const entries = Object.entries(settings || {})
  entries.forEach(([key]) => {
    if(!feature.settings[key]) {
      throw new ToolError(`${featureName(feature)} has no setting '${key}'. Its settings are ${Object.keys(feature.settings).join(', ')}.`)
    }
  })
  const isPicker = ([key]) => feature.isPickerSetting(feature.settings[key])
  entries.filter(entry => !isPicker(entry)).forEach(([key, value]) => setValue(feature, key, value, component))
  entries.filter(isPicker).forEach(([key, value]) => setPicker(doc, feature, key, value, tree))
  const missing = Object.keys(feature.settings).filter(key => feature.needsPicker(key) &&
    !(feature[key] && (!feature.settings[key].multi || feature[key]().length)))
  if(missing.length) {
    throw new ToolError(`Required inputs are missing: ${missing.map(key => `'${key}' (${pickerTypes[feature.settings[key].type]})`).join(', ')}.`)
  }
}

const featureFailure = feature => feature.error?.type == 'error' && feature.error.msg

// Selections repaired after upstream changes are accepted, like confirming
async function acceptRepairs(doc, feature) {
  if(!feature.error) return
  const warning = feature.error.msg
  feature.repair()
  doc.timeline.invalidateFeature(toRaw(feature))
  await doc.regenerate()
  await settle(doc)
  return warning
}

async function addFeature(doc, feature) {
  await doc.addFeature(feature)
  await settle(doc)
  const failure = featureFailure(feature)
  if(failure) {
    await doc.removeFeature(feature)
    await settle(doc)
    throw new ToolError(`The feature failed: ${failure}. No feature was created.`)
  }
  return acceptRepairs(doc, feature)
}

// Changes settings of an existing feature, restoring them when it fails
async function updateFeature(doc, feature, change) {
  const values = feature.getValues()
  const expressions = { ...feature.expressions }
  const restore = () => {
    feature.setValues(values)
    feature.expressions = expressions
  }
  try {
    change()
  } catch(error) {
    restore()
    throw error
  }
  doc.timeline.invalidateFeature(toRaw(feature))
  await doc.regenerate()
  await settle(doc)
  const failure = featureFailure(feature)
  if(failure) {
    restore()
    doc.timeline.invalidateFeature(toRaw(feature))
    await doc.regenerate()
    await settle(doc)
    throw new ToolError(`The feature failed: ${failure}. The feature was left unchanged.`)
  }
  const warning = await acceptRepairs(doc, feature)
  doc.markChanged()
  return warning
}

function featureResult(doc, feature, warning) {
  const tree = doc.top()
  // Bodies are named after the feature that created them
  const created = solid => solid.id.includes(`${feature.id}/topology/`)
  return {
    feature: describeFeature(doc, feature, { schema: false }),
    ...(warning && { warning }),
    ...(feature.planeId && { plane: `helper:${feature.componentId}:${feature.planeId}` }),
    marker: doc.timeline.marker,
    modifiedComponents: feature.modifiedComponents().map(id => tree.findChild(id)).filter(Boolean).map(component => ({
      id: itemId(component),
      name: component.creator.title,
      bodies: component.compound.solids().map(solid => ({ ...describeBody(component, solid), ...(created(solid) && { created: true }) })),
    })),
    failingFeatures: failingFeatures(doc, itemId(feature)),
  }
}

// The change is done at this point, so a failing description must not report it as failed
function appliedResult(doc, feature, warning) {
  try {
    return featureResult(doc, feature, warning)
  } catch(error) {
    console.error(error)
    return {
      feature: itemId(feature),
      warning: `The change was applied, but its result could not be described: ${error.message}. Use get_document to inspect it.`,
      marker: doc.timeline.marker,
    }
  }
}

// Sketch edits regenerate the sketch feature and everything after it
async function updateSketch(doc, sketch, operations) {
  beforeMarker(doc, sketch.creator)
  const result = editSketch(doc, sketch, await prepareSketchOperations(operations))
  doc.timeline.invalidateFeature(toRaw(sketch.creator))
  doc.markChanged()
  await doc.regenerate()
  await settle(doc)
  return { ...result, sketch: describeSketch(doc, sketch), failingFeatures: failingFeatures(doc) }
}

// Temporary features describe the settings of a type
function featureTypeSchema(doc, type) {
  const Type = featureTypes[type]
  if(!Type) throw new ToolError(`Unknown feature type '${type}'. Types are ${Object.keys(featureTypes).join(', ')}.`)
  const id = lastId
  const feature = new Type(doc)
  setLastId(id)
  return { type, title: feature.title, settings: describeSettings(feature) }
}

const itemIdHelp = 'Item ids come from the query tools, like face:<component>:<id>, edge:…, body:…, profile:<sketch>:<index>, ' +
  'curve:<sketch>:<element>, helper:<component>:<id> for origin planes, axes and points, component:… and feature:…. ' +
  'They are valid for the model state they were read from.'

const settingsSchema = {
  type: 'object',
  description: 'Feature settings by key, see get_feature with a type. Picker settings take item ids (an array for multiple inputs, null to clear). ' +
    'Lengths are numbers in mm and angles in degrees, or expression strings with units and parameters like "12.5mm", "1inch", "30deg" or "width / 2". ' +
    'Options take their value. Unset settings keep their defaults.',
  additionalProperties: true,
}

const sketchPoint = { type: 'array', items: { type: 'number' }, minItems: 2, maxItems: 2 }

const sketchOperations = {
  type: 'array',
  description: 'Drawing and constraint operations, applied in order. ' +
    'Points are [x, y] in mm on the sketch workplane. Points placed exactly onto existing points are joined to them, like snapping. ' +
    'Drawing ops accept "name" to refer to their elements in later ops of the same call (polylines and rectangles name their lines name.0, name.1, …, rectangles also name.bottom/right/top/left) ' +
    'and "construction": true for construction geometry. Ops: ' +
    'point {at}; line {from, to}; polyline {points, closed}; rectangle {from, to} or {center, width, height}; circle {center, radius}; ' +
    'arc {center, radius, startAngle, endAngle} counterclockwise in degrees, or {start, through, end}; spline {points}; ' +
    'text {at, content, size?, fontFamily?, fontStyle?, explode?} with the baseline starting at "at", size in mm, a system font, ' +
    'and explode to turn it into editable curves; ' +
    'fillet {items, radius} rounding the corner of two curves, or of one corner point {element, point}, with a dimensioned tangent arc; ' +
    'offset {items, through, distance?} offsetting one curve with the chain it connects to, or a profile, towards the point "through", ' +
    'optionally at an exact distance, kept parallel by an offset constraint; ' +
    `constraint {type, items} with type ${constraintTypeNames.join(', ')}; ` +
    'dimension {items, value, orientation?, position?} with a value in mm/degrees or an expression, orientation horizontal or vertical for point distances; ' +
    'delete {items} for elements and constraints; construction {items, construction}; ' +
    'project {items, intersect?} to project model edges, vertices, axes or curves of other sketches by item id. ' +
    'Constraint and dimension items are element ids or names, "origin" for the sketch origin, or {element, point} with point start, end or center. ' +
    'Fully constrain sketches with dimensions, so later edits stay predictable.',
  items: {
    type: 'object',
    properties: {
      op: { type: 'string', enum: operationTypes },
      name: { type: 'string' },
      construction: { type: 'boolean' },
      at: sketchPoint,
      from: sketchPoint,
      to: sketchPoint,
      center: sketchPoint,
      start: sketchPoint,
      through: sketchPoint,
      end: sketchPoint,
      position: sketchPoint,
      points: { type: 'array', items: sketchPoint },
      closed: { type: 'boolean' },
      radius: { type: 'number' },
      width: { type: 'number' },
      height: { type: 'number' },
      startAngle: { type: 'number' },
      endAngle: { type: 'number' },
      type: { type: 'string', enum: constraintTypeNames },
      items: { type: 'array', items: {} },
      value: { type: ['number', 'string'] },
      orientation: { type: 'string', enum: ['horizontal', 'vertical'] },
      intersect: { type: 'boolean' },
      content: { type: 'string' },
      size: { type: 'number' },
      fontFamily: { type: 'string' },
      fontStyle: { type: 'string' },
      explode: { type: 'boolean' },
      distance: { type: 'number' },
    },
    required: ['op'],
  },
}

export const tools = [
  {
    name: 'get_document',
    title: 'Get document',
    readOnly: true,
    description: 'Overview of the active document: its component tree with bodies, sketches, construction geometry (origin planes and axes) and parameters, ' +
      'and the feature timeline with errors and the marker position. Start here. Geometry reflects the model at the marker. ' +
      'Coordinates are in mm, in the space of the owning component.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'Only describe this component and its children' },
      },
    },
    run: ({ component }, { doc }) => component ?
      describeComponent(doc, findComponent(doc, component)) :
      describeDocument(doc),
  },
  {
    name: 'get_body',
    title: 'Get body topology',
    readOnly: true,
    description: 'Faces, edges and corner vertices of a body with their geometry (surface or curve type, area, length, radius, center, normals, axes, adjacent faces). ' +
      'Use it to find faces and edges for features like fillets, chamfers, shells or sketches on faces.',
    inputSchema: {
      type: 'object',
      properties: { body: { type: 'string', description: 'Body id, like body:id-0:12' } },
      required: ['body'],
    },
    run: ({ body }, { doc }) => {
      const solid = resolveItem(doc, body)
      if(solid.typename() != 'Solid') throw new ToolError(`${body} is no body`)
      return describeTopology(doc.top().findChild(solid.compound.componentId), solid)
    },
  },
  {
    name: 'get_sketch',
    title: 'Get sketch',
    readOnly: true,
    description: 'Elements, constraints, projections and closed profiles of a sketch, with its workplane. ' +
      'Element coordinates are 2D on the workplane, whose origin and axes are given in component space.',
    inputSchema: {
      type: 'object',
      properties: { sketch: { type: 'string', description: 'Sketch id, like sketch:id-7' } },
      required: ['sketch'],
    },
    run: ({ sketch }, { doc }) => describeSketch(doc, findSketch(doc, sketch)),
  },
  {
    name: 'get_feature',
    title: 'Get feature',
    readOnly: true,
    description: 'With a feature id: its settings and current values, picked inputs as item ids, and its error. ' +
      'With a type: the settings of that feature type, their options and defaults. With neither: all feature types. ' +
      `Picker setting types accept: ${Object.entries(pickerTypes).map(([type, accepts]) => `${type}: ${accepts}`).join('; ')}.`,
    inputSchema: {
      type: 'object',
      properties: {
        feature: { type: 'string', description: 'Feature id, like feature:id-42' },
        type: { type: 'string', enum: Object.keys(featureTypes) },
      },
    },
    run: ({ feature, type }, { doc }) => {
      if(feature) return describeFeature(doc, findFeature(doc, feature))
      if(type) return featureTypeSchema(doc, type)
      return Object.keys(featureTypes).map(type => {
        const { title, settings } = featureTypeSchema(doc, type)
        return { type, title, settings: settings.map(setting => `${setting.key}: ${setting.type}`).join(', ') }
      })
    },
  },
  {
    name: 'capture_view',
    title: 'Capture view',
    readOnly: true,
    description: 'Screenshot of the 3D viewport as the user sees it, optionally from a standard view fitted to the model, or from a saved view.',
    inputSchema: {
      type: 'object',
      properties: {
        view: { type: 'string', description: 'current (the default), home, top, front, side, or the title of a saved view' },
        width: { type: 'integer', description: 'Maximum image width in pixels, defaults to 1024' },
      },
    },
    run: async ({ view = 'current', width = 1024 }, { app, doc }) => {
      const renderer = window.alcRenderer
      if(view != 'current') {
        const saved = doc.views.find(saved => saved.title.toLowerCase() == view.toLowerCase())
        if(!saved) throw new ToolError(`No view '${view}'. Views are current, ${doc.views.map(saved => saved.title).join(', ')}.`)
        await doc.activateView(saved)
        await nextTick()
        if(saved.system || saved.zoomToFit) app.bus.emit('zoom-all')
        while(renderer.isAnimating || renderer.cameraTarget || renderer.viewControlsTarget) {
          await new Promise(resolve => setTimeout(resolve, 20))
        }
      }
      renderer.render()
      const source = renderer.renderer.domElement
      const scale = Math.min(1, width / source.width)
      const canvas = document.createElement('canvas')
      canvas.width = Math.round(source.width * scale)
      canvas.height = Math.round(source.height * scale)
      canvas.getContext('2d').drawImage(source, 0, 0, canvas.width, canvas.height)
      return { image: canvas.toDataURL('image/png').split(',')[1], mimeType: 'image/png' }
    },
  },
  {
    name: 'create_feature',
    title: 'Create feature',
    description: 'Adds a feature at the timeline marker with the given settings and regenerates the model. ' +
      'Fails without changes when inputs are missing or the feature cannot be built. ' +
      'Solid features join, cut, intersect or create bodies in their component depending on "operation". ' +
      'Sketches take a plane or planar face as "plane" and can be drawn right away with "operations". ' +
      itemIdHelp,
    inputSchema: {
      type: 'object',
      properties: {
        type: { type: 'string', enum: Object.keys(featureTypes) },
        settings: settingsSchema,
        component: { type: 'string', description: 'Component to add the feature to, defaults to the active component' },
        operations: { ...sketchOperations, description: 'Sketches only, see edit_sketch' },
      },
      required: ['type', 'settings'],
    },
    run: async ({ type, settings, component, operations }, { doc }) => {
      editable(doc)
      const Type = featureTypes[type]
      if(!Type) throw new ToolError(`Unknown feature type '${type}'`)
      await closeEditing(doc)
      // Features belong to the active component. Naming another one only
      // targets it, the user's active component stays.
      const previous = doc.activeComponent
      activateComponent(doc, component)
      try {
        const feature = new Type(doc)
        applySettings(doc, feature, settings, doc.top())
        const warning = await addFeature(doc, feature)
        if(feature.sketch) {
          if(operations?.length) return await updateSketch(doc, feature.sketch, operations)
          return { feature: itemId(feature), sketch: describeSketch(doc, feature.sketch) }
        }
        // Hides consumed sketches, as confirming a feature does
        feature.confirm()
        feature.commit?.()
        doc.markChanged()
        return appliedResult(doc, feature, warning)
      } finally {
        if(component) doc.reactivateActiveComponent(previous)
      }
    },
  },
  {
    name: 'edit_feature',
    title: 'Edit feature',
    description: 'Changes settings of an existing feature and regenerates the timeline after it. Settings that are not given keep their values. ' +
      'Picked inputs refer to the model before the feature, so move the marker behind it to query their ids. ' +
      'Fails without changes when the feature cannot be built. Use edit_sketch to change sketch geometry.',
    inputSchema: {
      type: 'object',
      properties: {
        feature: { type: 'string' },
        settings: settingsSchema,
      },
      required: ['feature', 'settings'],
    },
    run: async ({ feature: id, settings }, { doc }) => {
      editable(doc)
      const feature = findFeature(doc, id)
      await closeEditing(doc)
      beforeMarker(doc, feature)
      const tree = doc.top(doc.timeline.features.indexOf(toRaw(feature)))
      const warning = await updateFeature(doc, feature, () => applySettings(doc, feature, settings, tree))
      return appliedResult(doc, feature, warning)
    },
  },
  {
    name: 'edit_sketch',
    title: 'Edit sketch',
    description: 'Draws into, constrains or deletes from an existing sketch, solves it and regenerates the features that depend on it. ' +
      'If any operation fails or the sketch becomes unsolvable, nothing changes.',
    inputSchema: {
      type: 'object',
      properties: {
        sketch: { type: 'string', description: 'Sketch id' },
        operations: sketchOperations,
      },
      required: ['sketch', 'operations'],
    },
    run: async ({ sketch: id, operations }, { doc }) => {
      editable(doc)
      const sketch = findSketch(doc, id)
      await closeEditing(doc)
      return updateSketch(doc, sketch, operations)
    },
  },
  {
    name: 'delete_feature',
    title: 'Delete feature',
    description: 'Removes a feature from the timeline. Later features that depend on it may fail.',
    inputSchema: {
      type: 'object',
      properties: { feature: { type: 'string' } },
      required: ['feature'],
    },
    run: async ({ feature: id }, { doc }) => {
      editable(doc)
      const feature = findFeature(doc, id)
      await closeEditing(doc)
      doc.markChanged()
      await doc.removeFeature(toRaw(feature))
      await settle(doc)
      return { marker: doc.timeline.marker, failingFeatures: failingFeatures(doc) }
    },
  },
  {
    name: 'move_marker',
    title: 'Move timeline marker',
    description: 'Moves the timeline marker, rolling the model back or forward. New features are inserted at the marker, ' +
      'features after it are not applied. Move it back to the end when done.',
    inputSchema: {
      type: 'object',
      properties: {
        position: { type: 'integer', description: 'Number of features before the marker, from 0 to the timeline length' },
        after: { type: 'string', description: 'Place the marker after this feature instead' },
      },
    },
    run: async ({ position, after }, { doc }) => {
      editable(doc)
      const features = doc.timeline.features
      if(after) position = features.indexOf(toRaw(findFeature(doc, after))) + 1
      if(!Number.isInteger(position) || position < 0 || position > features.length) {
        throw new ToolError(`Position must be between 0 and ${features.length}`)
      }
      await doc.moveMarker(position)
      await settle(doc)
      return { marker: doc.timeline.marker, timeline: describeTimeline(doc) }
    },
  },
  {
    name: 'undo',
    title: 'Undo or redo',
    description: 'Undoes or redoes changes to the document, like Ctrl+Z and Ctrl+Y.',
    inputSchema: {
      type: 'object',
      properties: {
        steps: { type: 'integer', description: 'Number of steps, defaults to 1' },
        redo: { type: 'boolean' },
      },
    },
    run: async ({ steps = 1, redo = false }, { doc }) => {
      editable(doc)
      await closeEditing(doc)
      for(let i = 0; i < steps; i++) {
        await doc.restoreHistory(redo ? 1 : -1)
        await settle(doc)
      }
      return { marker: doc.timeline.marker, timeline: describeTimeline(doc) }
    },
  },
  {
    name: 'create_component',
    title: 'Create component',
    description: 'Adds a component as a child of the given or the active component. Without further options it is empty ' +
      'and becomes active, so following features go into it. Use activate_component to switch back. ' +
      'With instanceOf it adds another instance of a component next to it, which shares its definition and geometry. ' +
      'With fromBody it turns a body into a child component of the body\'s component. ' +
      'With file it places a .cad or STEP file as a linked component into the parent. Move components into place with pose_component.',
    inputSchema: {
      type: 'object',
      properties: {
        parent: { type: 'string' },
        name: { type: 'string' },
        instanceOf: { type: 'string', description: 'Component to instantiate' },
        fromBody: { type: 'string', description: 'Body to convert, like body:id-0:12' },
        file: { type: 'string', description: 'Absolute path of a .cad, .step or .stp file to place. Placed files are read only and keep their own names.' },
      },
    },
    run: async ({ parent, name, instanceOf, fromBody, file }, { doc }) => {
      editable(doc)
      const parentComponent = findComponent(doc, parent)
      if(parentComponent.readOnly) throw new ToolError('The parent component is read-only')
      await closeEditing(doc)
      let component
      if(file && name) throw new ToolError('Placed files are read only and keep their own names')
      if(instanceOf || fromBody || file) {
        const previous = doc.activeComponent
        if(instanceOf) {
          const source = findComponent(doc, instanceOf)
          if(!source.parent || source.readOnly) throw new ToolError('Only editable child components can be instantiated')
          await doc.createComponentInstance(source)
        } else if(fromBody) {
          const body = resolveItem(doc, fromBody)
          if(body.typename() != 'Solid') throw new ToolError(`${fromBody} is no body`)
          await doc.convertBodyToComponent(doc.top().findChild(body.compound.componentId), body)
        } else {
          doc.activateComponent(parentComponent)
          const loaded = await loadFile('', 'binary', file)
          loaded.data = await decodeFile(loaded.data)
          try {
            await doc.placeFile(loaded)
          } finally {
            doc.reactivateActiveComponent(previous)
          }
        }
        await settle(doc)
        const feature = doc.timeline.features[doc.timeline.marker - 1]
        const failure = feature.error?.type == 'error' && feature.error.msg
        if(failure) {
          await doc.removeFeature(feature)
          await settle(doc)
          throw new ToolError(`The component could not be created: ${failure}`)
        }
        // Placed files nest their components under the feature id
        component = doc.top().getChildren().find(child => child.id == feature.id || child.id.startsWith(`${feature.id}/`))
      } else {
        await doc.createComponent(parentComponent)
        await settle(doc)
        component = doc.activeComponent
      }
      if(name) doc.renameComponent(component, name)
      return describeComponent(doc, component, { recursive: false })
    },
  },
  {
    name: 'activate_component',
    title: 'Activate component',
    description: 'Makes a component active, like the user does in the tree. New features go into the active component ' +
      'unless they name one, and the viewport fades the other components. Activate the top component to see the whole model.',
    inputSchema: {
      type: 'object',
      properties: { component: { type: 'string', description: 'Component id, like component:id-0' } },
      required: ['component'],
    },
    run: async ({ component }, { doc }) => {
      editable(doc)
      await closeEditing(doc)
      activateComponent(doc, component)
      return describeComponent(doc, doc.activeComponent, { recursive: false })
    },
  },
  {
    name: 'set_visibility',
    title: 'Set visibility',
    description: 'Shows or hides a component, body, sketch or construction plane, axis or point, like the eye icons in the tree. ' +
      'New solid features only affect bodies visible when they are created.',
    inputSchema: {
      type: 'object',
      properties: {
        item: { type: 'string', description: 'component:…, body:…, sketch:… or helper:… id' },
        hidden: { type: 'boolean' },
      },
      required: ['item', 'hidden'],
    },
    run: async ({ item: id, hidden }, { doc }) => {
      editable(doc)
      await closeEditing(doc)
      const tree = doc.top()
      if(id.startsWith('sketch:')) {
        const sketch = findSketch(doc, id)
        sketchComponent(tree, sketch.id).creator.itemsHidden[sketch.id] = hidden
        doc.markChanged()
        return { item: id, hidden }
      }
      const item = resolveItem(doc, id)
      if(item.typename() == 'Component') {
        item.creator.hidden = hidden
        await doc.updateFeatureVisibility(item)
      } else if(item.typename() == 'Solid') {
        const component = tree.findChild(item.compound.componentId)
        component.creator.itemsHidden[component.sourceItemId(item.id)] = hidden
        await doc.updateFeatureVisibility(component, item.id)
      } else if(item instanceof ConstructionHelper) {
        const component = tree.findChild(item.componentId)
        component.creator.itemsHidden[component.sourceItemId(item.id)] = hidden
        doc.markChanged()
      } else {
        throw new ToolError(`The visibility of ${id} cannot be changed`)
      }
      return { item: id, hidden }
    },
  },
  {
    name: 'save_document',
    title: 'Save document',
    description: 'Saves the active document to its file, like Ctrl+S. Documents that were never saved, or with as_new_file, ' +
      'ask the user for a location, and versioned documents ask for a version message. The call waits for the user.',
    inputSchema: {
      type: 'object',
      properties: { as_new_file: { type: 'boolean', description: 'Save under a new name, like Save As' } },
    },
    run: async ({ as_new_file = false }, { doc }) => {
      editable(doc)
      await doc.save(as_new_file)
      return { saved: !doc.hasUnsavedChanges, file: doc.filePath || null }
    },
  },
  {
    name: 'set_parameter',
    title: 'Set parameter',
    description: 'Creates, changes, renames or deletes a named parameter of a component, then regenerates the model. ' +
      'Parameters can be used in feature settings and sketch dimensions, like "width / 2", and are inherited by child components.',
    inputSchema: {
      type: 'object',
      properties: {
        component: { type: 'string', description: 'Defaults to the active component' },
        name: { type: 'string' },
        value: { type: 'string', description: 'Expression with units, like "40mm" or "height * 2"' },
        newName: { type: 'string', description: 'Renames the parameter and its references' },
        delete: { type: 'boolean', description: 'Removes the parameter, replacing references with its value' },
      },
      required: ['name'],
    },
    run: async ({ component: id, name, value, newName, delete: remove }, { doc }) => {
      editable(doc)
      await closeEditing(doc)
      const component = findComponent(doc, id)
      const definition = component.creator
      let parameter = definition.parameters.find(parameter => parameter.name == name)
      if(remove) {
        if(!parameter) throw new ToolError(`No parameter '${name}' in this component`)
        doc.removeParameterReferences(parameter, component)
        definition.removeParameter(parameter)
      } else {
        if(!/^[A-Za-z_]\w*$/.test(newName || name)) throw new ToolError('Parameter names must be identifiers')
        if(value !== undefined) {
          try {
            new Expression(value, component.getParameters().filter(other => other.name != name)).getBase()
          } catch(error) {
            throw new ToolError(`Invalid parameter value: ${error.message}`)
          }
        }
        if(!parameter) {
          if(value === undefined) throw new ToolError('A new parameter needs a value')
          definition.addParameter({ name, value })
          parameter = definition.parameters.at(-1)
        }
        if(value !== undefined) parameter.value = value
        if(newName && newName != name) {
          doc.renameParameterReferences(name, newName)
          parameter.name = newName
        }
        definition.syncActiveVariantOption()
      }
      await doc.regenerateParameters()
      await settle(doc)
      return {
        parameters: definition.parameters.map(({ name, value }) => ({ name, value })),
        failingFeatures: failingFeatures(doc),
      }
    },
  },
  ...propertyTools,
  ...analysisTools,
  ...outputTools,
  ...historyTools,
]
