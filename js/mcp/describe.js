import * as THREE from 'three'
import { toRaw } from 'vue'

import Serialize from '../core/serialize.js'
import { Face, Edge, Vertex, Solid, Profile, selectionType } from '../core/geom3d.js'
import { SketchElement, SketchPoint, Line, Circle, Arc, Spline, SketchText } from '../core/geom2d.js'
import { SketchOrigin, ProjectedPoint, HorVertConstraint, Dimension } from '../core/sketch.js'
import { ConstructionHelper } from '../core/helpers.js'
import { Feature } from '../core/features.js'
import { PlanarReference, AxialReference, PointReference, ProfileReference, PatternInputReference } from '../core/references.js'
import { measureOptions, measureValue } from '../core/measures.js'
import { worldTransform } from '../core/assembly.js'
import { featureName, settingNotes } from './catalog.js'

// Agents address geometry with item ids like `face:<component id>:<face id>`,
// valid for the document state they were read from. Coordinates are in the
// space of the owning component, in millimeters and degrees.

export class ToolError extends Error {}

const round = value => Number(value.toFixed(4))
const vector = v => [round(v.x), round(v.y), round(v.z)]
const vector2 = v => [round(v.x), round(v.y)]

export function itemId(item, component) {
  if(item instanceof Feature) return `feature:${item.id}`
  if(item?.typename?.() == 'Component') return `component:${item.id}`
  if(item instanceof Solid) return `body:${item.compound.componentId}:${item.id}`
  if(item instanceof Face) return `face:${item.solid.compound.componentId}:${item.id}`
  if(item instanceof Edge) return `edge:${item.solid.compound.componentId}:${item.id}`
  if(item instanceof Vertex) return `vertex:${item.solid.compound.componentId}:${item.id}`
  if(item instanceof ConstructionHelper) return `helper:${item.componentId}:${item.id}`
  if(item instanceof Profile) return `profile:${item.sketch.id}:${profileIndex(item, component)}`
  if(item instanceof SketchElement || item instanceof ProjectedPoint) return `curve:${item.sketch.id}:${item.id}`
  if(item?.componentA) return `joint:${item.id}`
}

// The tree a sketch lives in decides the component its profiles belong to
export function sketchComponent(tree, sketchId) {
  return tree.getChildren().find(component => component.sketches.some(sketch => sketch.id == sketchId))
}

function sketchProfiles(sketch, component) {
  return sketch.profiles(component)
}

function profileIndex(profile, component) {
  const reference = new ProfileReference(profile)
  return sketchProfiles(profile.sketch, component).findIndex(other => reference.matches(other))
}

export function findSketch(doc, sketchId) {
  const id = String(sketchId).replace(/^sketch:/, '')
  const feature = doc.timeline.features.find(feature => feature.sketch?.id == id)
  if(!feature) throw new ToolError(`No sketch with id ${sketchId}`)
  return feature.sketch
}

export function findComponent(doc, id, tree = doc.top()) {
  if(!id) return doc.activeComponent
  const component = tree.findChild(id.replace(/^component:/, ''))
  if(!component) throw new ToolError(`No component with id ${id} at the current timeline position`)
  return component
}

export function findFeature(doc, id) {
  const feature = doc.timeline.features.find(feature => feature.id == id.replace(/^feature:/, ''))
  if(!feature) throw new ToolError(`No feature with id ${id}`)
  return feature
}

export function resolveItem(doc, id, tree = doc.top()) {
  const [kind, ...parts] = String(id).split(':')
  const missing = () => new ToolError(`Item ${id} does not exist at the current timeline position. Query the document again to get current ids.`)
  const component = () => {
    const found = tree.findChild(parts[0])
    if(!found) throw missing()
    return found
  }
  const find = (items, itemId) => {
    const item = items.find(item => String(item.id) == itemId)
    if(!item) throw missing()
    return item
  }
  switch(kind) {
    case 'component': return component()
    case 'body': return find(component().compound.solids(), parts[1])
    case 'face': return find(component().compound.faces(), parts[1])
    case 'edge': return find(component().compound.edges(), parts[1])
    case 'vertex': return find(component().compound.solids().flatMap(solid => solid.vertices()), parts[1])
    case 'helper': return find(component().helpers, parts[1])
    case 'feature': return findFeature(doc, parts[0])
    case 'joint': return find(tree.assemblyJoints || [], parts[0])
    case 'curve': {
      const sketchComp = sketchComponent(tree, parts[0])
      if(!sketchComp) throw missing()
      const sketch = sketchComp.sketches.find(sketch => sketch.id == parts[0])
      return find([...sketch.elements, ...sketch.projectedElements()], parts[1])
    }
    case 'profile': {
      const sketchComp = sketchComponent(tree, parts[0])
      if(!sketchComp) throw missing()
      const sketch = sketchComp.sketches.find(sketch => sketch.id == parts[0])
      const profile = sketchProfiles(sketch, sketchComp)[Number(parts[1])]
      if(!profile) throw missing()
      return profile
    }
  }
  throw new ToolError(`Unknown item id ${id}`)
}

// Items a reference points at, looking through planar, axial and point wrappers
function referenceItems(reference) {
  if(reference instanceof PlanarReference || reference instanceof AxialReference || reference instanceof PointReference) {
    return referenceItems(reference.item)
  }
  // Feature inputs of patterns and mirrors only locate their result through a body
  if(reference instanceof PatternInputReference && reference.featureId) {
    return reference.document.timeline.features.filter(feature => feature.id == reference.featureId)
  }
  return reference.getItems()
}

function measurements(item) {
  return Object.fromEntries(measureOptions(item)
    .filter(({ group }) => !group)
    .map(({ key }) => [key, round(measureValue(item, key))]))
}

function boundingBox(solids) {
  const box = new THREE.Box3()
  solids.forEach(solid => solid.faces().forEach(face => {
    const positions = face.tesselate().positions
    for(let i = 0; i < positions.length; i += 3) {
      box.expandByPoint(new THREE.Vector3(positions[i], positions[i + 1], positions[i + 2]))
    }
  }))
  if(box.isEmpty()) return null
  return { min: vector(box.min), max: vector(box.max), size: vector(box.getSize(new THREE.Vector3())) }
}

function isIdentity(matrix) {
  return matrix.equals(new THREE.Matrix4())
}

export function describeBody(component, solid) {
  return {
    id: itemId(solid),
    name: component.itemTitle(solid.id, 'Body'),
    hidden: component.isItemHidden(solid.id),
    volume: round(solid.volume()),
    area: round(solid.area()),
    center: vector(solid.center()),
    bounds: boundingBox([solid]),
    faces: solid.faces().length,
    edges: solid.edges().filter(edge => edge.isSemantic()).length,
  }
}

function describeFace(face) {
  const surface = face.geom().surface()
  const plane = face.getPlane()
  const axis = face.getAxis()
  return {
    id: itemId(face),
    type: selectionType(face),
    surface: surface.kind,
    ...measurements(face),
    center: vector(face.center()),
    ...(plane && { normal: vector(new THREE.Vector3().setFromMatrixColumn(plane, 2)) }),
    ...(axis && {
      axisOrigin: vector(new THREE.Vector3().setFromMatrixPosition(axis)),
      axisDirection: vector(new THREE.Vector3().setFromMatrixColumn(axis, 2)),
    }),
  }
}

function describeEdge(edge) {
  const [start, end] = edge.vertices()
  return {
    id: itemId(edge),
    type: selectionType(edge),
    ...measurements(edge),
    ...(measureValue(edge, 'center-x') !== undefined && {
      center: [measureValue(edge, 'center-x'), measureValue(edge, 'center-y'), measureValue(edge, 'center-z')].map(round),
    }),
    ...(start && { start: vector(start) }),
    ...(end && !end.equals(start) && { end: vector(end) }),
    midpoint: vector(edge.center()),
    faces: edge.connectedFaces().map(face => itemId(face)),
  }
}

export function describeTopology(component, solid) {
  return {
    ...describeBody(component, solid),
    faces: solid.faces().map(describeFace),
    // Seams of closed surfaces are no edges to the user
    edges: solid.edges().filter(edge => edge.isSemantic()).map(describeEdge),
    vertices: solid.cornerVertices().map(vertex => ({ id: itemId(vertex), point: vector(vertex.point) })),
  }
}

function describeHelper(component, helper) {
  const transform = helper.transform
  return {
    id: itemId(helper),
    type: helper.typename().toLowerCase(),
    name: helper.isStandard() ? helper.id.split('/').pop() : undefined,
    ...(component.isItemHidden(helper.id) && { hidden: true }),
    origin: vector(new THREE.Vector3().setFromMatrixPosition(transform)),
    ...(helper.typename() != 'Point' && {
      [helper.typename() == 'Plane' ? 'normal' : 'direction']: vector(new THREE.Vector3().setFromMatrixColumn(transform, 2)),
    }),
  }
}

function sketchTitle(component, sketch) {
  return component.itemTitle(sketch.id, sketch.creator?.title || 'Sketch')
}

export function describeComponent(doc, component, { recursive = true, details = true } = {}) {
  const transform = worldTransform(component)
  return {
    id: itemId(component),
    name: component.creator.title,
    ...(component.instanceOf && { instanceOf: `component:${component.instanceOf}` }),
    ...(component.readOnly && { readOnly: true }),
    hidden: !!component.creator.hidden,
    active: toRaw(component) === toRaw(doc.activeComponent),
    ...(!isIdentity(transform) && { worldTransform: transform.elements.map(round) }),
    ...(details && {
      bodies: component.compound.solids().map(solid => describeBody(component, solid)),
      sketches: component.sketches.map(sketch => ({
        id: `sketch:${sketch.id}`,
        name: sketchTitle(component, sketch),
        feature: `feature:${sketch.creator?.id}`,
        hidden: component.isItemHidden(sketch.id),
        elements: sketch.elements.length,
      })),
      constructionGeometry: component.helpers.map(helper => describeHelper(component, helper)),
      parameters: component.creator.parameters.map(({ name, value }) => ({ name, value })),
      ...(component.creator.material && { material: component.creator.material.name || component.creator.material.title }),
    }),
    ...(recursive && {
      children: component.children.map(child => describeComponent(doc, child, { recursive, details })),
    }),
  }
}

function describeFeatureSummary(doc, feature, index) {
  const marker = doc.timeline.marker
  return {
    index,
    id: itemId(feature),
    type: featureName(feature),
    title: feature.title,
    component: `component:${feature.componentId}`,
    ...(index >= marker && { afterMarker: true }),
    ...(feature.error && { [feature.error.type]: feature.error.msg }),
    ...(feature.sketch && { sketch: `sketch:${feature.sketch.id}` }),
  }
}

export function describeTimeline(doc) {
  return doc.timeline.features.map((feature, index) => describeFeatureSummary(doc, feature, index))
}

// Settings of a feature, as a schema for agents
export function describeSettings(feature) {
  const notes = settingNotes[featureName(feature)] || {}
  return Object.entries(feature.settings).map(([key, setting]) => {
    const options = feature.settingOptions?.(key) || setting.options
    return {
      key,
      title: setting.title,
      ...(notes[key] && { description: notes[key] }),
      type: setting.type,
      ...(setting.multi && { multiple: true }),
      ...(setting.optional && { optional: true }),
      ...(options && { options: Object.entries(options).map(([value, option]) => ({ value, title: option.title })) }),
      ...(setting.min !== undefined && { min: setting.min }),
      ...(setting.max !== undefined && { max: setting.max }),
      ...(setting.allowNegative && { allowNegative: true }),
      ...(setting.when && { visible: feature.isSettingVisible(setting) }),
    }
  })
}

export function describeFeature(doc, feature, { schema = true } = {}) {
  const index = doc.timeline.features.indexOf(toRaw(feature))
  const tree = doc.top(index)
  const component = tree.findChild(feature.componentId)
  const values = Object.fromEntries(Object.entries(feature.settings).map(([key, setting]) => {
    if(feature.isPickerSetting(setting)) {
      if(!feature[key]) return [key, null]
      const items = feature.resolveInputReferences(key, tree).references
        .flatMap(referenceItems)
        .map(item => itemId(item, sketchComponent(tree, item.sketch?.id) || component) || selectionType(item))
      return [key, setting.multi ? items : items[0] || null]
    }
    if(feature.expressions[key]) return [key, { expression: feature.expressions[key], value: round(feature[key]) }]
    return [key, typeof feature[key] == 'number' ? round(feature[key]) : feature[key] ?? null]
  }))
  return {
    ...describeFeatureSummary(doc, feature, index),
    ...(feature.operation && { operation: feature.operation }),
    ...(schema && { settings: describeSettings(feature) }),
    values,
  }
}

const constraintNames = new Map()

function constraintName(constraint) {
  if(constraint instanceof HorVertConstraint) return constraint.isVertical ? 'vertical' : 'horizontal'
  const entry = Object.entries(Serialize.classes).find(([, klass]) => klass === constraint.constructor)
  if(!constraintNames.has(entry[0])) {
    constraintNames.set(entry[0], entry[0].replace(/Constraint$/, '').replace(/(?<=.)([A-Z])/g, '_$1').toLowerCase())
  }
  return constraintNames.get(entry[0])
}

const handleNames = elem =>
  elem instanceof Line ? ['start', 'end'] :
  elem instanceof Arc ? ['center', 'start', 'end'] :
  elem instanceof Circle ? ['center'] :
  elem instanceof Spline ? elem.handles().map((_, index, all) => index == 0 ? 'start' : index == all.length - 1 ? 'end' : String(index)) :
  ['point']

function describeElemRef(ref) {
  const curve = ref.curve()
  if(!curve) return 'lost'
  if(curve instanceof SketchOrigin) return 'origin'
  if(ref.index === undefined) return curve.id
  return { element: curve.id, point: handleNames(curve)[ref.index] ?? ref.index }
}

function describeElement(elem) {
  const common = {
    id: elem.id,
    ...(elem.projection && { projected: true }),
    ...(elem.isReference && { construction: true }),
    ...(elem.fullyConstrained && { fullyConstrained: true }),
  }
  if(elem instanceof Line) return { ...common, type: 'line', start: vector2(elem.points[0]), end: vector2(elem.points[1]), length: round(elem.length()) }
  if(elem instanceof Circle) return { ...common, type: 'circle', center: vector2(elem.center()), radius: round(elem.radius) }
  if(elem instanceof Arc) {
    const [start, end] = elem.endpoints()
    return {
      ...common,
      type: 'arc',
      center: vector2(elem.center()),
      radius: round(elem.radius),
      start: vector2(start),
      end: vector2(end),
      startAngle: round(THREE.MathUtils.radToDeg(elem.start)),
      sweepAngle: round(THREE.MathUtils.radToDeg(elem.sweep)),
    }
  }
  if(elem instanceof SketchPoint || elem instanceof ProjectedPoint) return { ...common, type: 'point', at: vector2(elem.point) }
  if(elem instanceof Spline) return { ...common, type: 'spline', controlPoints: elem.points.map(vector2) }
  if(elem instanceof SketchText) return { ...common, type: 'text', content: elem.content, at: vector2(elem.handles()[0]) }
  return { ...common, type: 'curve', length: round(elem.length()) }
}

function describeConstraint(constraint) {
  return {
    id: constraint.id,
    type: constraintName(constraint),
    items: constraint.items.map(describeElemRef),
    ...(constraint instanceof Dimension && {
      value: round(constraint.distance),
      unit: constraint.isAngular() ? 'deg' : 'mm',
      ...(constraint.expression && { expression: constraint.expression }),
      ...(constraint.orientation && { orientation: constraint.orientation }),
    }),
  }
}

function profileArea(profile) {
  const positions = profile.tesselate().positions
  let area = 0
  for(let i = 0; i < positions.length; i += 9) {
    const [ax, ay, , bx, by, , cx, cy] = positions.slice(i, i + 9)
    area += Math.abs((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)) / 2
  }
  return area
}

export function describeSketch(doc, sketch, tree = doc.top()) {
  const component = sketchComponent(tree, sketch.id)
  const workplane = sketch.workplane
  const axis = column => vector(new THREE.Vector3().setFromMatrixColumn(workplane, column))
  const profiles = component ? sketchProfiles(sketch, component) : []
  const elements = [...sketch.elements, ...sketch.projectedElements()]
  return {
    id: `sketch:${sketch.id}`,
    name: component ? sketchTitle(component, sketch) : 'Sketch',
    feature: `feature:${sketch.creator.id}`,
    component: component && itemId(component),
    workplane: {
      origin: vector(new THREE.Vector3().setFromMatrixPosition(workplane)),
      xAxis: axis(0),
      yAxis: axis(1),
      normal: axis(2),
    },
    fullyConstrained: sketch.elements.length > 0 && sketch.elements.every(elem => elem.isReference || elem.fullyConstrained),
    elements: elements.map(describeElement),
    constraints: sketch.constraints.map(describeConstraint),
    projections: sketch.projections.map(projection => ({
      id: projection.id,
      type: projection.typename().toLowerCase(),
      outputs: projection.geometries().map(output => output.id),
      ...(projection.error && { error: projection.error }),
    })),
    profiles: profiles.map((profile, index) => ({
      id: `profile:${sketch.id}:${index}`,
      area: round(profileArea(profile)),
      center: vector(profile.center()),
      holes: profile.rings.length - 1,
      elements: [...new Set(profile.rings.flatMap(ring => ring.segments.map(segment => profile.getBaseId(segment))))],
    })),
  }
}

export function describeDocument(doc) {
  return {
    name: doc.filePath?.split(/[\\/]/).pop() || 'Untitled Document',
    file: doc.filePath || null,
    unsavedChanges: doc.hasUnsavedChanges,
    units: { length: 'mm', angle: 'deg' },
    marker: doc.timeline.marker,
    activeComponent: itemId(doc.activeComponent),
    components: describeComponent(doc, doc.top()),
    timeline: describeTimeline(doc),
  }
}
