import * as THREE from 'three'
import { markRaw, toRaw } from 'vue'

import { Component, ComponentDefinition, createComponentInstance } from './component.js'
import { Sketch } from './sketch.js'
import { Compound, importedCompound } from './geom3d.js'
import { shapex, modelTolerance, matrixFromTransform } from './shapex.js'
import { PatternInputReference } from './references.js'
import { PlaneHelper } from './helpers.js'
import { normalFromMatrix, parseHsl, rad, rotationFromNormal } from './utils.js'
import { loadFile } from '../utils.js'
import Serialize from './serialize.js'
import { checkedOutData, parseDocument, decodeFile } from './document-format.js'
import { LengthGizmo, AngleGizmo, TransformGizmo, FeatureGizmos, edgeGizmoFrame, faceGizmoFrame } from '../three/gizmos.js'
import { lastId, makeID, setLastId } from './id.js'
import Expression from './expression.js'
import { measureIds, measureValue } from './measures.js'
import { topologyID, withTopologyNaming } from './topological-naming.js'
import { fail, featureError } from './errors.js'
import {
  alignJointWorld,
  baselineWorldTransform,
  isRotationalMotion,
  jointFrame,
  jointMotionOptions,
  referenceComponentId,
  relativeComponentTransform,
  solveAssembly,
  setBaselineWorldTransform,
  worldTransform,
} from './assembly.js'

// Enable to eagerly tessellate each feature and report tessellation failures as feature errors.
const validateFeatureTessellation = false


export class Feature {
  constructor(document, booleanOutput, title, settings) {
    this.document = document
    this.title = title
    this.settings = settings
    this.error = null
    this.expressions = {}
    // Picked geometry measured in expressions, as { [id]: { reference, property } }
    this.measures = {}
    this.componentId = this.document.activeComponent.sourceId()
    this.componentOccurrenceId = this.document.activeComponent.id
    this.id = makeID()

    if(!booleanOutput) return
    this.operation = 'join'
    this.settings.operation = {
      title: 'Operation',
      type: 'select',
      options: {
        join: {
          title: 'Join',
          icon: 'boolean-union',
        },
        cut: {
          title: 'Cut',
          icon: 'boolean-subtract',
        },
        intersect: {
          title: 'Intersect',
          icon: 'boolean-intersect',
        },
        create: {
          title: 'Create',
          icon: 'body-new',
        },
      },
    }
  }

  typename() {
    return this.title
  }

  patternReference() {
    return PatternInputReference.fromFeature(this)
  }

  execute(tree) {
    if(this.suppressUpdate || !this.isComplete()) return

    this.error = null
    const references = this.updateReferences(tree)

    if(this.error && this.error.type == 'error') return

    this.updateExpressions(tree)
    if(this.error?.type == 'error') return

    withTopologyNaming(this, () => {
      this.updateGeometry(tree, references)
      if(this.error?.type == 'error') return
      const components = new Set(this.modifiedComponents().flatMap(id => tree.findChild(id).getChildren()))
      for(const component of components) component.compound.restoreTopologySelections(this)
    })
  }

  updateExpressions(tree) {
    const component = tree.findChild(this.componentId)
    const { values: measures, error } = this.measureValues(tree)
    if(error) {
      this.error = error
      return
    }
    try {
      Object.entries(this.expressions).forEach(([key, expression]) => {
        const value = new Expression(expression, component.getParameters(), this.settings[key].type, measures).getBase()
        if(!Number.isFinite(value)) fail('Parameter expression must be finite')
        this[key] = value
      })
    } catch(err) {
      this.error = { type: 'error', msg: 'Invalid parameter expression' }
    }
  }

  updateGeometry(tree, references) {
    this.updateFeature(tree, references)

    if(!validateFeatureTessellation) return

    try {
      // Prepare every face before the timeline publishes the feature result.
      // A display failure rejects the feature just like a modeling failure.
      for(const id of this.modifiedComponents()) {
        for(const face of tree.findChild(id).compound.faces()) face.tesselate()
      }
    } catch(err) {
      if(this.error?.type != 'error') this.error = featureError(err, 'Geometry could not be displayed')
    }
  }

  updateReferences(tree) {
    const results = {}
    const errors = []

    Object.keys(this.settings).forEach(key => {
      if(!this[key] || !this.needsPicker(key, true)) return

      const setting = this.settings[key]
      const { references, error } = this.resolveInputReferences(key, tree)
      let items = references.flatMap(ref => ref.getItems())
      if(error) errors.push(error)
      // Saved selections of separate patches may resolve to the same cell.
      if(setting.multi && (setting.type == 'face' || setting.type == 'edge')) items = [...new Set(items)]
      results[key] = setting.multi ? items : items[0]
    })

    this.error = errors.find(error => error.type == 'error') || errors[0]

    return results
  }

  selectInput(key, reference, item) {
    if(this.settings[key].multi) {
      const { references } = this.resolveInputReferences(key)
      const previous = references.find(ref => ref.matches ? ref.matches(item) :
        ref.getItems().some(current => current.id == item.id))
      if(previous) references.splice(references.indexOf(previous), 1)
      else references.push(reference)
      this[key] = () => references
    } else {
      this[key] = () => reference
    }
    this.pickedInput?.(key, reference, item)
  }

  needsPicker(setting, includeOptionals) {
    setting = this.settings[setting]
    return this.isSettingVisible(setting) && ['profile', 'curve', 'path', 'axis', 'plane', 'face', 'edge', 'solid', 'point', 'componentRef', 'patternInput', 'joint'].some(type =>
      type == setting.type && (!setting.optional || includeOptionals)
    )
  }

  isPickerSetting(setting) {
    return ['profile', 'curve', 'path', 'axis', 'plane', 'face', 'edge', 'solid', 'point', 'componentRef', 'patternInput', 'joint'].includes(setting.type)
  }

  isSettingVisible(setting) {
    return !setting.when || setting.when(this)
  }

  isComplete() {
    return Object.keys(this.settings)
      .filter(key => this.needsPicker(key) )
      .every(key => this[key] && (!this.settings[key].multi || this[key]().length) )
  }

  preview() { return this.previewBody }

  confirm() {
    const comp = this.document.top().findChild(this.componentId)
    // Hide sketches whose geometry was consumed by the feature
    this.involvedSketches(setting => ['profile', 'curve'].includes(setting.type) || setting.consumesSketch)
      .forEach(sketch => comp.creator.itemsHidden[sketch.id] = true )
  }

  updateFeature(tree, references) {}

  captureVisibility() {}

  getGizmo() {}

  updateGizmos() {
    const gizmo = this.isComplete() && this.getGizmo()?.distance
    if(gizmo) {
      const component = this.document.top().findChild(this.componentOccurrenceId) ||
        this.document.top().findChild(this.componentId)
      const transform = component && worldTransform(component)
      const center = transform ? gizmo.center.clone().applyMatrix4(transform) : gizmo.center
      const direction = transform ? gizmo.direction.clone().transformDirection(transform) : gizmo.direction
      if(this.distanceGizmo) {
        this.distanceGizmo().startPosition.copy(center)
        this.distanceGizmo().translationSnapOrigin.copy(center)
        this.distanceGizmo().direction.copy(direction)
        this.distanceGizmo().set(gizmo.distance, gizmo.side)
      } else {
        const distanceGizmo = markRaw(new LengthGizmo(center, direction, gizmo.distance, gizmo.side, gizmo.cb))
        this.distanceGizmo = () => distanceGizmo
        window.alcRenderer.addGizmo(distanceGizmo)
      }
    } else {
      if(this.distanceGizmo) window.alcRenderer.removeGizmo(this.distanceGizmo())
      this.distanceGizmo = null
    }
  }

  modifiedComponents() { return [this.componentId] }

  acceptsInput(item) {
    const componentId = inputComponentId(item)
    return !componentId || componentId == this.componentId
  }

  resolveInputReferences(key, tree = this.document.top(this.document.timeline.features.indexOf(this))) {
    let error
    const references = (this[key] ? [this[key]()].flat() : []).flatMap(ref => {
      // Preserve failed bindings for cancel and upstream recovery. Warnings
      // carry repaired geometry on the clone until the selection is accepted.
      const clone = ref.clone()
      const result = clone.update(tree)
      if(result?.type == 'error' || !error) error = result
      if(result?.type == 'error') return []
      if(result) return [clone]
      ref.update(tree)
      return [ref]
    })
    const setting = this.settings[key]
    if(error?.type == 'error' && references.some(ref => ref.getItems().length) &&
      setting.multi && ['edge', 'face', 'solid'].includes(setting.type)) {
      error = { type: 'warning', msg: 'Selection has been repaired' }
    }
    return { references, error }
  }

  inputItems(key) {
    return this.resolveInputReferences(key).references.flatMap(ref => ref.getItems())
  }

  repair() {
    const tree = this.document.top(this.document.timeline.features.indexOf(this))
    for(const key of Object.keys(this.settings)) {
      if(!this[key] || !this.needsPicker(key, true)) continue
      const { references } = this.resolveInputReferences(key, tree)
      this[key] = this.settings[key].multi ? () => references : () => references[0]
    }
    this.error = null
  }

  involvedSketches(filter = setting => ['profile', 'curve', 'path'].includes(setting.type)) {
    return Object.keys(this.settings).flatMap(key => {
      if(!filter(this.settings[key])) return []
      if(!this[key]) return []
      const refs = this[key]()
      // Paths may also be model edges, which have no sketch
      return (Array.isArray(refs) ? refs : [refs]).map(ref => ref.getItem().sketch ).filter(Boolean)
    })
  }

  referencedItems() {
    return Object.keys(this.settings)
      .filter(key => this[key] && this.needsPicker(key, true))
      .flatMap(key => [this[key]()].flat().flatMap(ref => ref.referencedItems()))
      .concat(this.usedMeasureIds().flatMap(id => this.resolveMeasure(id).reference.referencedItems()))
  }

  usedMeasureIds() {
    return [...new Set(Object.values(this.expressions).flatMap(measureIds))]
      .filter(id => this.measures[id])
  }

  nextMeasureId() {
    return String(Math.max(0, ...Object.keys(this.measures).map(Number)) + 1)
  }

  addMeasure(reference, property) {
    const id = this.nextMeasureId()
    this.measures[id] = { reference, property }
    return id
  }

  // Measures are replaced rather than modified, so snapshots stay intact
  setMeasureProperty(id, property) {
    this.measures[id] = { ...this.measures[id], property }
  }

  // Resolves a measure against the model before this feature, like an input.
  // Returns { reference, item, value, error }.
  resolveMeasure(id, tree = this.document.top(this.document.timeline.features.indexOf(this))) {
    const { reference, property } = this.measures[id]
    const clone = reference.clone()
    const result = clone.update(tree)
    const item = clone.getItem()
    if(result?.type == 'error' || !item) {
      return { reference: clone, error: { type: 'error', msg: result?.msg || 'Measured geometry was lost' } }
    }
    const value = measureValue(item, property)
    if(!Number.isFinite(value)) {
      return { reference: clone, item, error: { type: 'error', msg: 'Measurement is not available for this geometry' } }
    }
    return { reference: clone, item, value, error: result }
  }

  measureValues(tree) {
    const values = {}
    for(const id of this.usedMeasureIds()) {
      const result = this.resolveMeasure(id, tree)
      if(result.error?.type == 'error') return { values, error: result.error }
      values[id] = result.value
    }
    return { values }
  }

  getValues() {
    const values = {}
    Object.keys(this.settings).forEach(key => {
      values[key] = (this[key] && this.isPickerSetting(this.settings[key]) ? this[key]() : this[key])
    })
    return values
  }

  setExpression(key, expression) {
    if(expression) this.expressions[key] = expression
    else delete this.expressions[key]
  }

  setValues(values) {
    Object.keys(values).forEach(key => {
      const setting = this.settings[key]
      // Ignore settings that are not supported by this version. This lets
      // documents survive feature options being removed or renamed.
      if(!setting) return

      const value = values[key]
      this[key] = (value && this.isPickerSetting(setting) ? () => value : value)
    })
  }

  dispose() {
    if(this.distanceGizmo) window.alcRenderer.removeGizmo(this.distanceGizmo())
    this.distanceGizmo = null
  }

  dump() {
    return {
      id: this.id,
      componentId: this.componentId,
      componentOccurrenceId: this.componentOccurrenceId,
      values: this.getValues(),
      expressions: this.expressions,
      measures: Object.fromEntries(this.usedMeasureIds().map(id => [id, this.measures[id]])),
      visibilities: this.visibilities,
      topologyNames: this.topologyNames,
    }
  }

  static undump(dump, context) {
    const feature = new this(context.document)
    feature.id = dump.id
    feature.componentId = dump.componentId
    feature.componentOccurrenceId = dump.componentOccurrenceId || dump.componentId
    feature.visibilities = dump.visibilities
    feature.topologyNames = dump.topologyNames
    feature.setValues(dump.values)
    feature.expressions = Object.fromEntries(
      Object.entries(dump.expressions || {}).filter(([key]) => feature.settings[key])
    )
    feature.measures = dump.measures || {}
    return feature
  }
}


// Inputs naming the geometry a feature changes, as opposed to geometry it reads.
const modifiedInputKeys = ['faces', 'edges', 'face', 'tools']

export class SolidFeature extends Feature {
  captureVisibility() {
    this.visibilities = this.document.captureVisualViewState().visibilities
  }

  isSolidVisible(tree, solid) {
    if(!this.visibilities) return true // Preserve the scope of older documents.
    let component = tree.findChild(solid.compound.componentId)
    if(component.id == this.componentId) {
      component = tree.findChild(this.componentOccurrenceId) || component
    }
    if(this.visibilities[component.id]?.itemsHidden[component.sourceItemId(solid.id)]) return false
    for(; component; component = component.parent) {
      if(this.visibilities[component.id]?.compHidden) return false
    }
    return true
  }

  updateGeometry(tree, references) {
    if(!this.visibilities) return super.updateGeometry(tree, references)

    const component = tree.findChild(this.componentId)
    const original = component.compound
    const solids = original.solids()
    const visible = solids.filter(solid => this.isSolidVisible(tree, solid))
    const hidden = solids.filter(solid => !visible.includes(solid))

    this.previewBody = null
    if(!visible.length && !this.operation) return

    // Inputs such as limit faces can remain references to hidden geometry;
    // selected faces and edges to modify must belong to visible solids.
    const scopedReferences = { ...references }
    for(const key of modifiedInputKeys) {
      if(!references[key]) continue
      const items = [references[key]].flat().filter(item => this.isSolidVisible(tree, item.solid || item))
      scopedReferences[key] = this.settings[key].multi ? items : items[0]
      if(!this.settings[key].optional && !items.length) return
    }

    component.compound = original.subset(visible)
    // The scoped compound has its own native topology handles. Rebind local
    // selections before operations that require the current body revision.
    const scopedSolids = component.compound.solids()
    for(const key of modifiedInputKeys) {
      if(!scopedReferences[key]) continue
      const items = [scopedReferences[key]].flat().map(item => {
        if(inputComponentId(item) != this.componentId) return item
        const solid = scopedSolids.find(solid => solid.id == (item.solid || item).id)
        return item.solid ? solid[key == 'edges' ? 'edges' : 'faces']().find(shape => shape.id == item.id) : solid
      })
      scopedReferences[key] = this.settings[key].multi ? items : items[0]
    }
    super.updateGeometry(tree, scopedReferences)
    component.compound = component.compound.subset([...component.compound.solids(), ...hidden]).cloneCached()
  }
}


const inputItemKind = item => item instanceof Feature ? 'feature' :
  ['Component', 'Solid'].find(type => type == item?.typename?.())
const inputReferenceKind = reference => reference.componentReference ? 'Component' :
  reference.solidReference ? 'Solid' : 'feature'

// Repeats solids, components or timeline features at other places. A feature
// takes one kind of input; multiple features combine different kinds.
class RepeatingFeature extends SolidFeature {
  constructor(...args) {
    super(...args)
    // Features repeat with their own operations, components as instances.
    this.settings.operation.when = feature => feature.inputs?.().some(input => input.solidReference)
  }

  acceptsInput(item) {
    const kind = inputItemKind(item)
    if(!kind) return true
    const current = this.inputs?.()[0]
    if(current && inputReferenceKind(current) != kind) return false
    return kind != 'feature' || this.canReplay(item)
  }

  // Replaying a feature needs a solid modeling feature of the same component.
  // Its component instances could not be repeated under the same IDs.
  canReplay(feature) {
    return feature instanceof SolidFeature && feature.componentId == this.componentId &&
      !feature.inputs?.().some(input => input.componentReference)
  }

  // Runs the input features again at every instance, in timeline order. Moving
  // the model into the features' own frame keeps their sketches, planes and
  // construction identities valid without any per-feature handling. Holding the
  // names of the original results lets references resolve to the new geometry only.
  // An instance is { key, enter, leave }, with the latter two moving a compound
  // into the features' frame and back.
  replayInputFeatures(tree, comp, instances) {
    const inputIds = new Set(this.inputs().map(input => input.featureId))
    const timeline = this.document.pendingTimeline || this.document.timeline
    const features = timeline.features.filter(feature => inputIds.has(feature.id))
    if(!features.length) return

    const unsupported = features.find(feature => !this.canReplay(feature))
    if(unsupported) fail(`${unsupported.title} cannot be repeated`)

    const isInputName = name => features.some(feature => name.startsWith(`${feature.id}/topology/`))
    const heldPrefix = `${this.id}/held/`
    const hold = name => isInputName(name) ? heldPrefix + name : name

    // Modifying pre-existing geometry would change it again at its original place.
    for(const feature of features) {
      for(const key of modifiedInputKeys) {
        if(!feature.settings[key] || !feature[key]) continue
        const references = [feature[key]()].flat().filter(reference => reference.componentId == this.componentId)
        if(!references.every(reference => isInputName(reference.selection.identity))) {
          fail(`Also select the feature that created the geometry modified by ${feature.title}`)
        }
      }
    }

    for(const { key, enter, leave } of instances) {
      const failures = comp.compound.topologyFailures
      comp.compound = enter(comp.compound).renameTopology(hold)

      const replicas = features.map(feature => {
        const replica = replayReplica(feature)
        replica.execute(tree)
        if(replica.error?.type == 'error') fail(`${feature.title}: ${replica.error.msg}`)
        if(replica.error) this.error = { ...replica.error, msg: `${feature.title}: ${replica.error.msg}` }
        return replica
      })

      const release = name => name.startsWith(heldPrefix) ? name.slice(heldPrefix.length) :
        isInputName(name) ? topologyID(this.id, JSON.stringify([key, name])) : name
      comp.compound = leave(comp.compound).renameTopology(release, topology =>
        replicas.includes(topology.origin.feature) ? { feature: this } : topology.origin)
      comp.compound.topologyFailures = failures
    }
  }
}


export class PatternFeature extends RepeatingFeature {
  static icon = 'th'

  constructor(doc) {
    super(doc, true, 'Pattern', {
      inputs: {
        title: 'Features',
        type: 'patternInput',
        multi: true,
      },
      patternType: {
        title: 'Type',
        type: 'enum',
        options: {
          grid: { title: 'Grid', icon: 'th' },
          radial: { title: 'Radial', icon: 'pattern-radial' },
        },
      },
      axis: {
        title: 'Axis',
        type: 'axis',
        when: feature => feature.patternType == 'radial',
      },
      uCount: {
        title: 'U Count',
        type: 'integer',
        min: 1,
        when: feature => feature.patternType == 'grid',
      },
      vCount: {
        title: 'V Count',
        type: 'integer',
        min: 1,
        when: feature => feature.patternType == 'grid',
      },
      uOffset: {
        title: 'U Step',
        type: 'length',
        autoFocus: false,
        allowNegative: true,
        gizmo: true,
        when: feature => feature.patternType == 'grid',
      },
      vOffset: {
        title: 'V Step',
        type: 'length',
        autoFocus: false,
        allowNegative: true,
        gizmo: true,
        when: feature => feature.patternType == 'grid',
      },
      radialMode: {
        title: 'Spacing',
        type: 'enum',
        options: {
          total: { title: 'Total angle', icon: 'circle-notch' },
          step: { title: 'Angle step', icon: 'ellipsis-h' },
        },
        when: feature => feature.patternType == 'radial',
      },
      radialCount: {
        title: 'Count',
        type: 'integer',
        min: 1,
        when: feature => feature.patternType == 'radial',
      },
      radialAngle: {
        title: 'Total Angle',
        type: 'angle',
        step: 1,
        allowNegative: true,
        gizmo: true,
        when: feature => feature.patternType == 'radial' && feature.radialMode == 'total',
      },
      radialStep: {
        title: 'Angle Step',
        type: 'angle',
        step: 1,
        allowNegative: true,
        gizmo: true,
        when: feature => feature.patternType == 'radial' && feature.radialMode == 'step',
      },
    })

    this.patternType = 'grid'
    this.axis = null
    this.uOffset = 10
    this.uCount = 2
    this.vOffset = 10
    this.vCount = 1
    this.radialMode = 'total'
    this.radialAngle = 360
    this.radialStep = 45
    this.radialCount = 8
    this.inputs = null
  }

  static undump(dump, context) {
    const feature = super.undump(dump, context)
    // Radial patterns saved before total angles existed were spaced by steps
    if(!dump.values.radialMode) feature.radialMode = 'step'
    return feature
  }

  isComplete() {
    if(!super.isComplete()) return false
    if(this.patternType == 'radial') {
      return this.radialCount >= 1 && Number.isFinite(this.radialMode == 'step' ? this.radialStep : this.radialAngle)
    }
    return this.uCount >= 1 && this.vCount >= 1 &&
      Number.isFinite(this.uOffset) && Number.isFinite(this.vOffset)
  }

  instanceCount() {
    return this.patternType == 'radial' ?
      Math.max(0, Math.floor(this.radialCount) - 1) :
      Math.max(0, Math.floor(this.uCount) * Math.floor(this.vCount) - 1)
  }

  instanceKeys() {
    return this.patternType == 'radial' ?
      Array.from({ length: this.instanceCount() }, (_, i) => `radial/${i + 1}`) :
      this.gridTransforms().map(transform => transform.topologyKey)
  }

  updateFeature(tree, references) {
    if(hasFutureInput(this)) {
      this.error = { type: 'error', msg: 'Pattern inputs must precede the pattern feature' }
      return
    }

    const comp = tree.findChild(this.componentId)
    const transforms = this.patternType == 'radial' ?
      this.radialTransforms(references.axis) : this.gridTransforms()

    const componentSources = references.inputs.filter(input => input?.typename?.() == 'Component')
    componentSources.forEach(source => {
      const canonical = tree.findChild(source.sourceId())
      const ownerWorld = baselineWorldTransform(comp)
      const sourceInOwner = ownerWorld.clone().invert().multiply(baselineWorldTransform(source))
      transforms.forEach(transform => {
        const id = `${this.id}/instance/${transform.topologyKey}/${source.id}`
        const placement = transform.clone().multiply(sourceInOwner)
        createComponentInstance(comp, canonical, id, placement)
      })
    })

    this.previewBody = null
    const sources = references.inputs
      .filter(input => input?.typename?.() == 'Solid')
      .map(input => input.toCompound())

    try {
      this.replayInputFeatures(tree, comp, transforms.map(transform => ({
        key: transform.topologyKey,
        enter: compound => compound.transform(transform.clone().invert()),
        leave: compound => compound.transform(transform),
      })))

      const copies = transforms.flatMap(transform =>
        sources.map(source => {
          const copy = source.transform(transform)
          copy.namespaceTopology(`pattern/${transform.topologyKey}/${source.componentId}`)
          return copy
        })
      )
      if(!copies.length) return
      const tool = copies.slice(1).reduce(
        (combined, copy) => combined.boolean(copy, 'join'),
        copies[0],
      )
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = featureError(err, 'Pattern could not be created') }
  }

  gridTransforms() {
    const transforms = []
    for(let u = 0; u < Math.floor(this.uCount); u++) {
      for(let v = 0; v < Math.floor(this.vCount); v++) {
        if(u == 0 && v == 0) continue
        const transform = new THREE.Matrix4().makeTranslation(u * this.uOffset, v * this.vOffset, 0)
        transform.topologyKey = `grid/${u}/${v}`
        transforms.push(transform)
      }
    }
    return transforms
  }

  // A full turn spaces instances evenly, as the last would coincide with the
  // first. A partial total angle places the last instance at its end.
  radialAngleStep() {
    if(this.radialMode == 'step') return this.radialStep
    const fullTurn = Math.abs(Math.abs(this.radialAngle) - 360) < 1e-9
    return this.radialAngle / (fullTurn ? Math.floor(this.radialCount) : Math.floor(this.radialCount) - 1)
  }

  radialTransforms(axis) {
    const origin = new THREE.Vector3().setFromMatrixPosition(axis)
    const direction = normalFromMatrix(axis)
    const transforms = []
    const step = this.radialAngleStep()
    for(let i = 1; i < Math.floor(this.radialCount); i++) {
      const angle = rad(step * i)
      const transform = new THREE.Matrix4().makeTranslation(origin.clone().negate())
          .premultiply(new THREE.Matrix4().makeRotationAxis(direction, angle))
          .premultiply(new THREE.Matrix4().makeTranslation(origin))
      transform.topologyKey = `radial/${i}`
      transforms.push(transform)
    }
    return transforms
  }

  sourceCenter() {
    const source = this.inputs().at(-1).getItem()
    const center = source.center()
    if(source instanceof Component) {
      const owner = this.document.top().findChild(this.componentId)
      center.applyMatrix4(baselineWorldTransform(owner).invert().multiply(baselineWorldTransform(source)))
    }
    return center
  }

  getGizmo() {
    if(this.patternType == 'radial') {
      const axis = this.axis().getItem()
      const sourceCenter = this.sourceCenter()
      const direction = normalFromMatrix(axis)
      const origin = new THREE.Vector3().setFromMatrixPosition(axis)
      const center = origin.clone().addScaledVector(direction, sourceCenter.clone().sub(origin).dot(direction))
      const radial = sourceCenter.sub(center)
      if(radial.lengthSq() < 1e-12) radial.setFromMatrixColumn(axis, 0)
      radial.normalize()
      const rotation = new THREE.Matrix4().makeBasis(radial, direction.clone().cross(radial), direction)
      const key = this.radialMode == 'step' ? 'radialStep' : 'radialAngle'
      return {
        [key]: {
          type: 'angle', center, rotation, value: this[key],
          cb: angle => {
            this.setExpression(key, null)
            this[key] = angle
          },
        },
      }
    }

    const center = this.sourceCenter()
    const step = (key, direction) => ({
      type: 'length', center, direction,
      value: Math.abs(this[key]), side: this[key] >= 0,
      cb: (value, side) => {
        this.setExpression(key, null)
        this[key] = value * (side ? 1 : -1)
      },
    })
    return {
      uOffset: step('uOffset', new THREE.Vector3(1, 0, 0)),
      vOffset: step('vOffset', new THREE.Vector3(0, 1, 0)),
    }
  }

  updateGizmos() {
    this.patternGizmos ||= markRaw(new FeatureGizmos())
    const component = this.document.top().findChild(this.componentOccurrenceId) ||
      this.document.top().findChild(this.componentId)
    this.patternGizmos.update(this.isComplete() && this.error?.type != 'error' ? this.getGizmo() : {}, worldTransform(component))
  }

  dispose() {
    super.dispose()
    this.patternGizmos?.dispose()
    this.patternGizmos = null
  }

}

Serialize.register(PatternFeature, 'PatternFeature')


// A stand-in for evaluating a feature again without touching its published
// state. It shares identities and values, but keeps its own inputs and results.
function replayReplica(feature) {
  const replica = Object.create(toRaw(feature))
  replica.expressions = {}
  replica.suppressUpdate = false
  replica.error = null
  for(const [key, setting] of Object.entries(feature.settings)) {
    if(!feature[key] || !feature.isPickerSetting(setting)) continue
    const input = feature[key]()
    const clone = Array.isArray(input) ? input.map(reference => reference.clone()) : input.clone()
    replica[key] = () => clone
  }
  return replica
}


// Whether any input feature of a pattern or mirror comes after it in the timeline.
function hasFutureInput(feature) {
  const timeline = feature.document.pendingTimeline || feature.document.timeline
  // By ID, since a replayed pattern or mirror is not itself in the timeline
  const ownIndex = timeline.features.findIndex(other => other.id == feature.id)
  return feature.inputs().some(input => input.featureId &&
    timeline.features.findIndex(other => other.id == input.featureId) >= ownIndex
  )
}


export class MirrorFeature extends RepeatingFeature {
  static icon = 'mirror'

  constructor(doc) {
    super(doc, true, 'Mirror', {
      inputs: {
        title: 'Features',
        type: 'patternInput',
        multi: true,
      },
      plane: {
        title: 'Plane',
        type: 'plane',
      },
    })

    this.inputs = null
    this.plane = null
  }

  // Component placements are rigid, so mirrored instances cannot be expressed
  acceptsInput(item) { return item?.typename?.() != 'Component' && super.acceptsInput(item) }

  updateFeature(tree, references) {
    if(hasFutureInput(this)) {
      this.error = { type: 'error', msg: 'Mirror inputs must precede the mirror feature' }
      return
    }

    const comp = tree.findChild(this.componentId)
    let plane = references.plane
    const planeSourceId = referenceComponentId(this.plane())
    const planeSource = planeSourceId && tree.findChild(planeSourceId)
    const target = tree.findChild(this.componentOccurrenceId) || comp
    if(planeSource && planeSource != target) {
      plane = relativeComponentTransform(planeSource, target).multiply(plane)
    }

    this.previewBody = null
    try {
      const reflect = compound => compound.mirror(plane)
      this.replayInputFeatures(tree, comp, [{ key: 'mirror', enter: reflect, leave: reflect }])

      const copies = references.inputs
        .filter(input => input?.typename?.() == 'Solid')
        .map(input => input.toCompound())
        .map(source => source.mirror(plane).namespaceTopology(`mirror/${source.componentId}`))
      if(!copies.length) return
      const tool = copies.reduce((combined, copy) => combined.boolean(copy, 'join'))
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = featureError(err, 'Mirror could not be created') }
  }
}

Serialize.register(MirrorFeature, 'MirrorFeature')


export class PoseFeature extends Feature {
  static icon = 'street-view'

  constructor(doc, transforms, transformSpace='component') {
    super(doc, false, 'Pose', {
      transformSpace: {
        title: 'Gizmo Space',
        type: 'select',
        options: {
          world: {
            title: 'World',
            icon: 'globe',
          },
          component: {
            title: 'Component',
            icon: 'cube',
          },
        },
      },
    })
    this.transformSpace = transformSpace
    this.transforms = transforms || doc.top().getChildren()
      .filter(component => component.parent && component.transform)
      .map(component => ({ id: component.id, transform: component.localTransform() }))
    this.poseGizmos = []
  }

  execute(tree) {
    this.error = null
    for(const entry of this.transforms) {
      const component = tree.findChild(entry.id)
      if(!component) {
        this.error = { type: 'error', msg: 'Positioned component was lost' }
        return
      }
      component.designTransform = entry.transform.clone()
      component.transform = null
    }
  }

  modifiedComponents() { return this.transforms.map(entry => entry.id) }
  isComplete() { return this.transforms.length > 0 }
  getValues() { return { transforms: this.transforms, transformSpace: this.transformSpace } }
  setValues(values) {
    this.transforms = values.transforms || []
    this.transformSpace = values.transformSpace || 'component'
  }

  updateGizmos() {
    this.clearPoseGizmos()
    const tree = this.document.top()

    this.transforms.forEach(entry => {
      const component = tree.findChild(entry.id)
      if(!component?.parent) return

      // A point away from every local gizmo axis lets the point-based
      // assembly solver observe rotations around X, Y, and Z.
      const target = new THREE.Vector3(1, 2, 3).normalize().multiplyScalar(10)

      let gizmo
      gizmo = markRaw(new TransformGizmo(worldTransform(component), desiredWorld => {
        const currentTree = this.document.top()
        const current = currentTree.findChild(entry.id)
        if(!current) return

        solveAssembly(currentTree, current, desiredWorld, 64, target)
        this.captureTransforms(currentTree)
        this.document.timeline.invalidateFeature(this)
        this.poseGizmos.forEach(({ id, gizmo: other }) => {
          const moved = currentTree.findChild(id)
          if(moved) other.setTransform(worldTransform(moved))
        })
      }, this.transformSpace == 'world' ? 'world' : 'local'))
      this.poseGizmos.push({ id: entry.id, gizmo })
      window.alcRenderer.addGizmo(gizmo)
    })
  }

  captureTransforms(tree) {
    const featureIndex = this.document.timeline.features.indexOf(this)
    const before = this.document.timeline.tree(featureIndex)
    const retainedIds = new Set(this.poseGizmos.map(({ id }) => id))
    const locals = tree.getChildren()
      .filter(component => component.parent)
      .map(component => ({ component, transform: component.localTransform() }))

    // Turn the transient assembly-solver result into the new design pose.
    // The tree before this feature remains untouched and is the comparison
    // point used to decide which component transforms the pose must store.
    locals.forEach(({ component, transform }) => {
      if(!component.transform) return
      component.designTransform = transform.clone()
      component.transform = null
    })

    this.transforms = locals.flatMap(({ component, transform }) => {
      const previous = before?.findChild(component.id)
      if(
        !retainedIds.has(component.id) &&
        previous &&
        matricesAlmostEqual(transform, previous.localTransform())
      ) return []
      return [{ id: component.id, transform: transform.clone() }]
    })
  }

  clearPoseGizmos() {
    this.poseGizmos.forEach(({ gizmo }) => window.alcRenderer?.removeGizmo(gizmo))
    this.poseGizmos = []
  }

  dispose() {
    this.clearPoseGizmos()
    super.dispose()
  }

  dump() {
    return {
      ...super.dump(),
      values: { transformSpace: this.transformSpace },
      transforms: this.transforms,
    }
  }

  static undump(dump, context) {
    const feature = new PoseFeature(context.document, dump.transforms, dump.values?.transformSpace)
    feature.id = dump.id
    feature.componentId = dump.componentId
    feature.componentOccurrenceId = dump.componentOccurrenceId || dump.componentId
    return feature
  }
}
Serialize.register(PoseFeature, 'PoseFeature')


function matricesAlmostEqual(left, right, epsilon=1e-8) {
  return left.elements.every((value, index) => Math.abs(value - right.elements[index]) < epsilon)
}


export class JointFeature extends Feature {
  static icon = 'kinematic-joint'

  constructor(doc) {
    const is = type => feature => feature.jointType == type
    super(doc, false, 'Joint', {
      jointType: {
        title: 'Type',
        type: 'enum',
        options: {
          axis: { title: 'Axis', icon: 'arrows-alt-v' },
          coplanar: { title: 'Coplanar', icon: 'layer-group' },
          ball: { title: 'Ball', icon: 'globe' },
          fix: { title: 'Fix', icon: 'lock' },
        },
      },
      axisA: { title: '1', type: 'face', when: is('axis') },
      axisB: { title: '2', type: 'face', when: is('axis') },
      lockSlide: {
        title: 'Lock Slide',
        type: 'bool',
        icons: ['lock', 'lock-open'],
        when: is('axis'),
      },
      offset: {
        title: 'Offset',
        type: 'length',
        autoFocus: false,
        allowNegative: true,
        when: feature => feature.jointType == 'axis' && feature.lockSlide,
      },
      planeA: { title: '1', type: 'face', when: is('coplanar') },
      planeB: { title: '2', type: 'face', when: is('coplanar') },
      pointA: { title: '1', type: 'point', when: is('ball') },
      pointB: { title: '2', type: 'point', when: is('ball') },
      fixedComponent: { title: 'Component', type: 'componentRef', when: is('fix') },
    })
    // Assembly joints belong to an occurrence, not to its shared definition.
    this.componentId = doc.activeComponent.id
    this.jointType = 'axis'
    this.lockSlide = false
    this.offset = 0
  }

  inputKeys() {
    return {
      axis: ['axisA', 'axisB'],
      coplanar: ['planeA', 'planeB'],
      ball: ['pointA', 'pointB'],
      fix: ['fixedComponent'],
    }[this.jointType]
  }

  execute(tree) {
    if(this.suppressUpdate || !this.isComplete()) return
    this.error = null
    const references = this.updateReferences(tree)
    if(this.error && this.error.type == 'error') return
    this.updateExpressions(tree)
    if(this.error?.type == 'error') return
    this.updateFeature(tree, references)
  }

  updateFeature(tree, references) {
    const keys = this.inputKeys()
    const refs = keys.map(key => this[key]())
    const componentIds = refs.map(referenceComponentId)
    if(componentIds.some(id => !id || !tree.findChild(id))) {
      this.error = { type: 'error', msg: 'Joint input component was lost' }
      return
    }
    if(componentIds.length == 2 && componentIds[0] == componentIds[1]) {
      this.error = { type: 'error', msg: 'Joint inputs must belong to different components' }
      return
    }
    const owner = tree.findChild(this.componentId)
    const components = componentIds.map(id => tree.findChild(id))
    if(!owner || components.some(component => component == owner || !component.hasAncestor(owner))) {
      this.error = { type: 'error', msg: 'Joint inputs must be strict descendants of the joint component' }
      return
    }

    let frameA = jointFrame(this.jointType, references[keys[0]])
    const frameB = keys[1] && jointFrame(this.jointType, references[keys[1]])
    if(!frameA || (keys[1] && !frameB)) {
      this.error = {
        type: 'error',
        msg: this.jointType == 'axis' ? 'Axis joints require cylindrical faces' :
          this.jointType == 'coplanar' ? 'Coplanar joints require planar faces' :
          'This joint requires points',
      }
      return
    }

    // A locked axis normally makes the two attachment origins coincide. Move
    // that target along axis A so the same solver behavior locks at the
    // requested axial position instead.
    if(this.jointType == 'axis' && this.lockSlide && this.offset) {
      frameA = frameA.clone().multiply(new THREE.Matrix4().makeTranslation(0, 0, this.offset))
    }

    const componentA = tree.findChild(componentIds[0])
    if(this.jointType == 'fix') {
      tree.assemblyJoints.push({
        id: this.id,
        type: 'fix',
        componentA: componentA.id,
        frameA: frameA.clone(),
        fixedWorld: baselineWorldTransform(componentA),
      })
      return
    }

    const componentB = tree.findChild(componentIds[1])
    const worldA = baselineWorldTransform(componentA)
    const worldB = baselineWorldTransform(componentB)
    const alignedB = alignJointWorld(this.jointType, worldA, frameA, worldB, frameB, this.lockSlide)
    setBaselineWorldTransform(componentB, alignedB)
    tree.assemblyJoints.push({
      id: this.id,
      type: this.jointType,
      componentA: componentA.id,
      componentB: componentB.id,
      frameA: frameA.clone(),
      frameB: frameB.clone(),
      lockSlide: this.jointType == 'axis' && this.lockSlide,
    })
  }

  modifiedComponents() {
    return (this.inputKeys() || []).map(key => this[key] && referenceComponentId(this[key]()))
      .filter(Boolean)
  }

  acceptsInput(item) {
    const component = item && (
      item.typename && item.typename() == 'Component' ? item :
      item.component || item.solid?.component || item.compound?.component
    )
    const owner = this.document.top().findChild(this.componentId)
    return !!component && !!owner && component != owner && component.hasAncestor(owner)
  }
}
Serialize.register(JointFeature, 'JointFeature')


export class MotionLinkFeature extends Feature {
  static icon = 'link'

  constructor(doc) {
    const rotational = side => feature => isRotationalMotion(feature[`motion${side}`])
    const linear = side => feature => !!feature[`motion${side}`] && !rotational(side)(feature)
    super(doc, false, 'Motion Link', {
      jointA: { title: 'Joint 1', type: 'joint' },
      jointB: { title: 'Joint 2', type: 'joint' },
      motionA: { title: 'Motion 1', type: 'enum', options: {}, when: feature => !!feature.motionA },
      angleA: { title: 'Travel 1', type: 'angle', autoFocus: false, when: rotational('A') },
      distanceA: { title: 'Travel 1', type: 'length', autoFocus: false, when: linear('A') },
      motionB: { title: 'Motion 2', type: 'enum', options: {}, when: feature => !!feature.motionB },
      angleB: { title: 'Travel 2', type: 'angle', autoFocus: false, when: rotational('B') },
      distanceB: { title: 'Travel 2', type: 'length', autoFocus: false, when: linear('B') },
      reverse: { title: 'Reverse', type: 'bool', icons: ['exchange-alt', 'arrows-alt-h'] },
    })
    this.componentId = doc.activeComponent.id
    this.angleA = 360
    this.angleB = 360
    this.distanceA = 10
    this.distanceB = 10
    this.motionA = null
    this.motionB = null
    this.reverse = false
  }

  settingOptions(key) {
    if(key != 'motionA' && key != 'motionB') return this.settings[key].options
    const side = key.at(-1)
    const reference = this[`joint${side}`]?.()
    const joint = reference?.getItem() || this.document.top()?.assemblyJoints?.find(joint =>
      joint.id == reference?.jointId
    )
    const icons = {
      rotateX: 'undo', rotateY: 'redo', rotateZ: 'sync-alt',
      slideX: 'arrows-alt-h', slideY: 'arrows-alt-v', slideZ: 'expand-alt',
    }
    return Object.fromEntries(Object.entries(jointMotionOptions(joint)).map(([value, title]) =>
      [value, { title, icon: icons[value] }]
    ))
  }

  pickedInput(key, _reference, item) {
    if(key != 'jointA' && key != 'jointB') return
    const side = key.at(-1)
    const options = jointMotionOptions(item)
    const motionKey = `motion${side}`
    if(!options[this[motionKey]]) this[motionKey] = Object.keys(options)[0] || null
  }

  isComplete() {
    if(!super.isComplete()) return false
    const refs = [this.jointA(), this.jointB()]
    if(refs[0].jointId == refs[1].jointId) return false
    return ['A', 'B'].every(side => {
      const motion = this[`motion${side}`]
      const travel = isRotationalMotion(motion) ? this[`angle${side}`] : this[`distance${side}`]
      return !!motion && Number.isFinite(travel) && travel != 0
    })
  }

  execute(tree) {
    if(this.suppressUpdate || !super.isComplete() || !this.motionA || !this.motionB) return
    this.error = null
    const references = this.updateReferences(tree)
    if(this.error && this.error.type == 'error') return
    this.updateExpressions(tree)
    if(this.error?.type == 'error') return
    const jointA = references.jointA
    const jointB = references.jointB
    if(!jointA || !jointB || jointA.id == jointB.id) {
      this.error = { type: 'error', msg: 'Select two different joints' }
      return
    }
    if(!jointMotionOptions(jointA)[this.motionA] || !jointMotionOptions(jointB)[this.motionB]) {
      this.error = { type: 'error', msg: 'The selected motion is not available on this joint' }
      return
    }
    const travel = side => isRotationalMotion(this[`motion${side}`])
      ? rad(this[`angle${side}`])
      : this[`distance${side}`]
    const travelA = travel('A')
    const travelB = travel('B')
    if(!Number.isFinite(travelA) || !Number.isFinite(travelB) || !travelA || !travelB) {
      this.error = { type: 'error', msg: 'Motion link travel must be non-zero' }
      return
    }
    tree.motionLinks.push({
      id: this.id,
      jointA: jointA.id,
      motionA: this.motionA,
      travelA,
      jointB: jointB.id,
      motionB: this.motionB,
      travelB,
      reverse: this.reverse,
    })
  }

  acceptsInput(item) { return !!Object.keys(jointMotionOptions(item)).length }
}
Serialize.register(MotionLinkFeature, 'MotionLinkFeature')


export class GroupFeature extends Feature {
  static icon = 'object-group'

  constructor(doc) {
    super(doc, false, 'Group', {
      components: {
        title: 'Components',
        type: 'componentRef',
        multi: true,
      },
    })
    // Assembly relationships belong to an occurrence, not to its shared definition.
    this.componentId = doc.activeComponent.id
    this.components = null
  }

  isComplete() {
    if(!super.isComplete()) return false
    const componentIds = this.components().map(reference => reference.componentId)
    return new Set(componentIds).size >= 2
  }

  execute(tree) {
    if(this.suppressUpdate || !this.isComplete()) return
    this.error = null
    const references = this.updateReferences(tree)
    if(this.error && this.error.type == 'error') return
    this.updateFeature(tree, references)
  }

  updateFeature(tree, references) {
    const components = references.components || []
    const componentIds = components.map(component => component.id)
    if(new Set(componentIds).size != componentIds.length) {
      this.error = { type: 'error', msg: 'Group components must be different' }
      return
    }

    const owner = tree.findChild(this.componentId)
    if(!owner || components.some(component => component == owner || !component.hasAncestor(owner))) {
      this.error = { type: 'error', msg: 'Select components inside the active assembly' }
      return
    }

    const anchor = components[0]
    const anchorWorld = baselineWorldTransform(anchor)
    components.slice(1).forEach(component => {
      const componentWorld = baselineWorldTransform(component)
      tree.assemblyJoints.push({
        id: this.id,
        type: 'group',
        componentA: anchor.id,
        componentB: component.id,
        frameA: new THREE.Matrix4(),
        // Both attachment frames coincide at creation, preserving the full
        // relative transform without repositioning either component.
        frameB: componentWorld.clone().invert().multiply(anchorWorld),
      })
    })
  }

  modifiedComponents() {
    return (this.components?.() || []).map(reference => reference.componentId)
  }

  acceptsInput(item) {
    const component = item?.typename?.() == 'Component' ? item : null
    const owner = this.document.top().findChild(this.componentId)
    return !!component && !!owner && component != owner && component.hasAncestor(owner)
  }
}
Serialize.register(GroupFeature, 'GroupFeature')


function inputComponentId(item) {
  if(!item) return
  if(item.componentId) return item.componentId
  if(item.compound?.componentId) return item.compound.componentId
  if(item.solid?.compound?.componentId) return item.solid.compound.componentId

  const component = item.sketch?.component || item.component
  return component?.sourceId?.() || component?.id
}


export class CreateComponentFeature extends Feature {
  static icon = 'box'
  static editable = false
  constructor(doc, parentId) {
    super(doc, false, 'New Component', {
      parent: {
        title: 'Parent Component',
        type: 'component',
      },
      title: {
        title: 'Title',
        type: 'string',
      },
    })

    this.parent = parentId
    this.title = "New Component"

    this.definition = new ComponentDefinition(this.title, doc.timeline.makeColor())
  }

  updateFeature(tree) {
    let parent = tree.findChild(this.parent)
    const comp = new Component(parent, this.id)
    this.definition.parent = this.parent
    this.definition.title = this.title
    comp.creator = this.definition
    parent.children.push(comp)
  }

  modifiedComponents() {
    return [this.parent]
  }

  dump() {
    return {
      ...super.dump(),
      definition: this.definition,
    }
  }

  static undump(dump, context) {
    const feature = super.undump(dump, context)
    Object.assign(feature, dump)
    delete feature.values
    return feature
  }
}

Serialize.register(CreateComponentFeature, 'CreateComponentFeature')


export class ConvertBodyToComponentFeature extends Feature {
  static icon = 'box'
  static editable = false

  constructor(doc) {
    super(doc, false, 'Body to Component', {
      body: {
        title: 'Body',
        type: 'solid',
      },
    })

    this.body = null
    this.definition = new ComponentDefinition('New Component', doc.timeline.makeColor())
  }

  updateFeature(tree, references) {
    const parent = tree.findChild(this.componentId)
    if(!parent || !references.body) {
      this.error = { type: 'error', msg: 'Body or parent component was lost' }
      return
    }

    try {
      const component = new Component(parent, this.id)
      this.definition.parent = this.componentId
      component.creator = this.definition
      component.compound = references.body.toCompound().cloneForComponent(component.id)

      const remaining = parent.compound.removeShapes([references.body])
      parent.compound = remaining.solids().length ? remaining : new Compound(parent.id)
      parent.children.push(component)
    } catch(err) { this.error = featureError(err, 'Body could not be converted to a component') }
  }

  dump() {
    return {
      ...super.dump(),
      definition: this.definition,
    }
  }

  static undump(dump, context) {
    const feature = super.undump(dump, context)
    feature.definition = dump.definition
    return feature
  }
}

Serialize.register(ConvertBodyToComponentFeature, 'ConvertBodyToComponentFeature')


export class CreateComponentInstanceFeature extends Feature {
  static icon = 'clone'
  static editable = false

  constructor(doc, sourceId, parentId) {
    super(doc, false, 'Component Instance', {})
    this.sourceId = sourceId
    this.parentId = parentId
  }

  updateFeature(tree) {
    const parent = tree.findChild(this.parentId)
    const source = tree.findChild(this.sourceId)
    if(!source || !parent) {
      this.error = { type: 'error', msg: 'Instance source or parent was lost' }
      return
    }
    createComponentInstance(parent, source, this.id)
  }

  modifiedComponents() { return [this.parentId, this.sourceId].filter(Boolean) }
  isComplete() { return !!this.sourceId && !!this.parentId }
  dump() { return { ...super.dump(), sourceId: this.sourceId, parentId: this.parentId } }

  static undump(dump, context) {
    const feature = super.undump(dump, context)
    feature.sourceId = dump.sourceId
    feature.parentId = dump.parentId
    return feature
  }
}
Serialize.register(CreateComponentInstanceFeature, 'CreateComponentInstanceFeature')


export class CreateSketchFeature extends Feature {
  static icon = 'solar-panel'
  constructor(doc) {
    super(doc, false, 'Sketch', {
      plane: {
        title: 'Plane',
        type: 'plane',
        autoConfirm: true,
      },
    })

    this.plane = null

    this.sketch = new Sketch()
    this.sketch.creator = this
  }

  // A sketch may use planar geometry from any visible component as its support.
  // The plane picker already limits candidates to planar faces and helpers.
  acceptsInput() { return true }

  updateFeature(tree, references) {
    let plane = references.plane
    const sourceId = referenceComponentId(this.plane())
    const source = sourceId && tree.findChild(sourceId)
    const target = tree.findChild(this.componentOccurrenceId) || tree.findChild(this.componentId)
    if(source && target && source != target) {
      plane = relativeComponentTransform(source, target).multiply(plane)
    }
    this.sketch.workplane = plane
    // Solving can create curve references before the sketch's UI is mounted.
    this.sketch.component = tree.findChild(this.componentId)
    this.sketch.component.sketches.push(this.sketch)
    this.sketch.solve(tree)
  }

  involvedSketches() {
    return [this.sketch]
  }

  confirm() {
    this.document.activateFeature(this)
  }

  dump() {
    return {
      ...super.dump(),
      sketch: this.sketch,
    }
  }

  static undump(dump, context) {
    const feature = super.undump(dump, context)
    feature.sketch = dump.sketch
    feature.sketch.creator = feature
    return feature
  }
}

Serialize.register(CreateSketchFeature, 'CreateSketchFeature')


// Builds a plane matrix whose X axis follows xHint as closely as possible.
function planeFrame(origin, normal, xHint) {
  if(normal.lengthSq() < 1e-12) return null
  normal = normal.clone().normalize()
  const xAxis = xHint.clone().projectOnPlane(normal)
  if(xAxis.lengthSq() < 1e-12) return rotationFromNormal(normal).setPosition(origin)
  xAxis.normalize()
  return new THREE.Matrix4().makeBasis(xAxis, normal.clone().cross(xAxis), normal).setPosition(origin)
}

// Planes halfway between parallel planes, or bisecting the acute angle
// between intersecting ones. The origin lies central to both base origins.
function midplaneFrame(first, second) {
  const normalA = normalFromMatrix(first)
  const normalB = normalFromMatrix(second)
  const originA = new THREE.Vector3().setFromMatrixPosition(first)
  const originB = new THREE.Vector3().setFromMatrixPosition(second)
  const line = normalA.clone().cross(normalB)
  let normal, support
  if(line.lengthSq() < 1e-12) {
    normal = normalA
    support = originA.clone().addScaledVector(normalA, originB.clone().sub(originA).dot(normalA) / 2)
  } else {
    normal = normalA.dot(normalB) >= 0 ? normalA.clone().add(normalB) : normalA.clone().sub(normalB)
    // Point on the intersection line of both planes
    support = normalB.clone().cross(line).multiplyScalar(normalA.dot(originA))
      .add(line.clone().cross(normalA).multiplyScalar(normalB.dot(originB)))
      .divideScalar(line.lengthSq())
  }
  normal = normal.clone().normalize()
  const center = originA.clone().add(originB).multiplyScalar(0.5)
  const origin = center.clone().addScaledVector(normal, -center.clone().sub(support).dot(normal))
  return planeFrame(origin, normal, new THREE.Vector3().setFromMatrixColumn(first, 0))
}

// Plane normal to a path at a fraction of its length. Tangents are blended
// between polyline vertices to stay accurate on tessellated curves.
function curveFrame(path, fraction) {
  const points = path.getPath()
  const chords = points.slice(1).map((point, i) => point.clone().sub(points[i]))
  const lengths = chords.reduce((sums, chord) => [...sums, sums.at(-1) + chord.length()], [0])
  const tangents = points.map((_, i) =>
    (chords[i - 1] || chords[i]).clone().normalize().add((chords[i] || chords[i - 1]).clone().normalize())
  )
  const target = lengths.at(-1) * THREE.MathUtils.clamp(fraction, 0, 1)
  const index = THREE.MathUtils.clamp(lengths.findIndex(length => length >= target), 1, points.length - 1)
  const t = (target - lengths[index - 1]) / ((lengths[index] - lengths[index - 1]) || 1)
  const origin = points[index - 1].clone().lerp(points[index], t)
  const tangent = tangents[index - 1].clone().lerp(tangents[index], t).normalize()
  return rotationFromNormal(tangent).setPosition(origin)
}

export class PlaneFeature extends Feature {
  static icon = 'construction-plane'
  constructor(doc) {
    const modes = modes => feature => modes.includes(feature.mode)
    super(doc, false, 'Plane', {
      mode: {
        title: 'Mode',
        type: 'select',
        options: {
          offset: { title: 'Offset & angle', icon: 'layer-group' },
          parallel: { title: 'Parallel through point', icon: 'grip-lines' },
          midplane: { title: 'Midplane', icon: 'grip-lines-vertical' },
          threePoints: { title: 'Three points', icon: 'draw-polygon' },
          axisAngle: { title: 'Through axis at angle', icon: 'sync-alt' },
          edgePoint: { title: 'Through axis and point', icon: 'project-diagram' },
          curve: { title: 'Normal to curve', icon: 'bezier-curve' },
        },
      },
      axis: {
        title: 'Axis',
        type: 'axis',
        when: modes(['axisAngle', 'edgePoint']),
      },
      path: {
        title: 'Curve',
        type: 'path',
        when: modes(['curve']),
      },
      base: {
        title: 'Base',
        type: 'plane',
        when: modes(['offset', 'parallel', 'midplane', 'axisAngle']),
      },
      base2: {
        title: 'Second plane',
        type: 'plane',
        when: modes(['midplane']),
      },
      point: {
        title: 'Point',
        type: 'point',
        when: modes(['parallel', 'threePoints', 'edgePoint']),
      },
      point2: {
        title: 'Second point',
        type: 'point',
        when: modes(['threePoints']),
      },
      point3: {
        title: 'Third point',
        type: 'point',
        when: modes(['threePoints']),
      },
      angle: {
        title: 'Angle',
        type: 'angle',
        allowNegative: true,
        when: modes(['axisAngle']),
      },
      position: {
        title: 'Position %',
        type: 'number',
        min: 0,
        max: 100,
        when: modes(['curve']),
      },
      offset: {
        title: 'Offset',
        type: 'length',
        allowNegative: true,
      },
      rotationX: {
        title: 'Rotation X',
        type: 'angle',
        allowNegative: true,
        when: modes(['offset']),
      },
      rotationY: {
        title: 'Rotation Y',
        type: 'angle',
        allowNegative: true,
        when: modes(['offset']),
      },
    })

    this.mode = 'offset'
    this.base = null
    this.base2 = null
    this.axis = null
    this.path = null
    this.point = null
    this.point2 = null
    this.point3 = null
    this.angle = 0
    this.position = 0
    this.offset = 1.0
    this.rotationX = 0
    this.rotationY = 0
    this.rotationTransform = new THREE.Matrix4()
    this.rotationAppliedX = 0
    this.rotationAppliedY = 0

    this.planeId = makeID()
  }

  settingChanged(key, value) {
    if(key != 'mode') return
    // Only the free placement mode starts away from its inputs
    this.setExpression('offset', null)
    this.offset = value == 'offset' ? 1.0 : 0
  }

  // Plane before offset and rotation are applied, or null if the
  // inputs do not define one. Items are looked up by setting key.
  placement(item) {
    const point = key => new THREE.Vector3().setFromMatrixPosition(item(key))
    switch(this.mode) {
      case 'parallel':
        return item('base').clone().setPosition(point('point'))
      case 'midplane':
        return midplaneFrame(item('base'), item('base2'))
      case 'threePoints': {
        const [first, second, third] = ['point', 'point2', 'point3'].map(point)
        const xAxis = second.clone().sub(first)
        return planeFrame(first, xAxis.clone().cross(third.clone().sub(first)), xAxis)
      }
      case 'axisAngle': {
        const direction = normalFromMatrix(item('axis'))
        const normal = normalFromMatrix(item('base')).projectOnPlane(direction)
          .applyAxisAngle(direction, rad(this.angle))
        return planeFrame(point('axis'), normal, direction)
      }
      case 'edgePoint': {
        const direction = normalFromMatrix(item('axis'))
        const origin = point('axis')
        const toPoint = point('point').sub(origin)
        const foot = origin.clone().addScaledVector(direction, toPoint.dot(direction))
        return planeFrame(foot, direction.clone().cross(toPoint), direction)
      }
      case 'curve':
        return curveFrame(item('path'), this.position / 100)
      default:
        return item('base').clone()
    }
  }

  placementError() {
    return {
      threePoints: 'Points must not be collinear',
      axisAngle: 'Axis must not be perpendicular to the base plane',
      edgePoint: 'Point must not lie on the axis',
    }[this.mode]
  }

  currentPlacement() {
    return this.placement(key => this[key]().getItem())
  }

  // Full plane in component space, including offset and rotation
  finishPlacement(placement) {
    const plane = placement.multiply(new THREE.Matrix4().makeTranslation(0, 0, this.offset))
    return this.mode == 'offset' ? plane.multiply(this.getRotation()) : plane
  }

  updateFeature(tree, references) {
    const placement = this.placement(key => references[key])
    if(!placement) {
      this.error = { type: 'error', msg: this.placementError() }
      return
    }
    const helper = new PlaneHelper(this.componentId, this.finishPlacement(placement), this.planeId)
    tree.findChild(this.componentId).helpers.push(helper)
  }

  getRotation() {
    // Numeric edits apply a turn about the plane's current local axis.
    if(this.rotationX != this.rotationAppliedX || this.rotationY != this.rotationAppliedY) {
      this.rotationTransform = this.rotationTransform.clone()
        .multiply(new THREE.Matrix4().makeRotationX(rad(this.rotationX - this.rotationAppliedX)))
        .multiply(new THREE.Matrix4().makeRotationY(rad(this.rotationY - this.rotationAppliedY)))
      this.rotationAppliedX = this.rotationX
      this.rotationAppliedY = this.rotationY
    }
    return this.rotationTransform.clone()
  }

  getGizmo() {
    const placement = this.currentPlacement()
    if(!placement) return
    return {
      distance: {
        center: new THREE.Vector3().setFromMatrixPosition(placement),
        direction: normalFromMatrix(placement),
        distance: Math.abs(this.offset),
        side: this.offset >= 0,
        cb: (distance, side) => {
          this.setExpression('offset', null)
          this.offset = distance * (side ? 1 : -1)
        },
      },
    }
  }

  updateGizmos() {
    super.updateGizmos()
    if(!this.isComplete() || this.mode != 'offset') {
      this.clearRotationGizmo()
      return
    }
    const component = this.document.top().findChild(this.componentOccurrenceId) ||
      this.document.top().findChild(this.componentId)
    const transform = worldTransform(component).multiply(this.finishPlacement(this.currentPlacement()))
    if(this.rotationGizmo) {
      if(!this.rotationGizmo.dragging) this.rotationGizmo.setTransform(transform)
      return
    }
    const gizmo = markRaw(new TransformGizmo(transform, desiredWorld => {
      const key = `rotation${gizmo.rotationControl.axis}`
      this.setExpression(key, null)
      this[key] = this.rotationDragStartValue + THREE.MathUtils.radToDeg(gizmo.rotationControl.rotationAngle)
      const current = this.document.top().findChild(this.componentOccurrenceId) ||
        this.document.top().findChild(this.componentId)
      const reference = worldTransform(current).multiply(this.base().getItem())
      this.rotationTransform = new THREE.Matrix4().extractRotation(reference.invert().multiply(desiredWorld))
      this.rotationAppliedX = this.rotationX
      this.rotationAppliedY = this.rotationY
      this.document.timeline.invalidateFeature(this)
      this.document.regenerate()
    }))
    gizmo.translationControl.showX = false
    gizmo.translationControl.showY = false
    gizmo.translationControl.showZ = false
    gizmo.rotationControl.showZ = false
    gizmo.addEventListener('mouseDown', () => {
      this.rotationDragStartValue = this[`rotation${gizmo.rotationControl.axis}`]
    })
    gizmo.addEventListener('dragging-changed', event => { this.distanceGizmo().enabled = !event.value })
    this.distanceGizmo().addEventListener('dragging-changed', event => { gizmo.enabled = !event.value })
    this.rotationGizmo = gizmo
    window.alcRenderer.addGizmo(gizmo)
  }

  clearRotationGizmo() {
    if(this.rotationGizmo) window.alcRenderer.removeGizmo(this.rotationGizmo)
    this.rotationGizmo = null
  }

  dispose() {
    super.dispose()
    this.clearRotationGizmo()
  }

  getValues() {
    return {
      ...super.getValues(),
      rotationTransform: this.rotationTransform,
      rotationAppliedX: this.rotationAppliedX,
      rotationAppliedY: this.rotationAppliedY,
    }
  }

  setValues(values) {
    super.setValues(values)
    this.rotationTransform = values.rotationTransform || new THREE.Matrix4().makeRotationFromEuler(
      new THREE.Euler(rad(this.rotationX), rad(this.rotationY))
    )
    this.rotationAppliedX = values.rotationAppliedX ?? this.rotationX
    this.rotationAppliedY = values.rotationAppliedY ?? this.rotationY
  }

  dump() {
    return {
      ...super.dump(),
      planeId: this.planeId,
    }
  }

  static undump(dump, context) {
    return Object.assign(super.undump(dump, context), {
      planeId: dump.planeId,
    })
  }
}

Serialize.register(PlaneFeature, 'PlaneFeature')


// Features that store a referenced file's contents regenerate without it.
// The file is compared again when the document is opened; a newer revision
// is offered for update but never applied silently.
async function readNewerRevision(feature, filetype, normalize = data => data) {
  if(!feature.filePath || !feature.cachedData || !window.ipc) return null
  try {
    const file = await loadFile(filetype, 'binary', feature.filePath)
    const data = normalize(await decodeFile(file.data))
    return data == feature.cachedData ? null : data
  } catch(error) {
    console.warn(`Referenced file ${feature.filePath} could not be read`, error)
    return null
  }
}

export const stepFiletype = '.step,.stp'
const placeFiletype = `.cad,${stepFiletype}`

// STEP files open with their exchange structure keyword. Only documents
// are normalized to their checked-out state.
const isStepData = data => data.trimStart().startsWith('ISO-10303-21')
const sourceData = data => isStepData(data) ? data : checkedOutData(data)


// Products take their first solid's sRGB colour. Uncoloured products fan out
// from the feature's color by the golden angle.
function stepColor(product, color, index) {
  const rgb = product.colors.find(Boolean)
  if(rgb) {
    const { h, s, l } = new THREE.Color().setRGB(...rgb, THREE.SRGBColorSpace).getHSL({}, THREE.SRGBColorSpace)
    const round = value => Number(value.toFixed(2))
    return `hsl(${round(h * 360)}, ${round(s * 100)}%, ${round(l * 100)}%)`
  }
  const { h, s, l } = parseHsl(color)
  return `hsl(${(h + index * 137.508) % 360}, ${s}%, ${l}%)`
}


// Places a ProtoSolid document's components, or a STEP file's product
// structure, read-only in the active component.
export class PlaceFeature extends Feature {
  static icon = 'link'

  constructor(doc) {
    super(doc, false, 'Place', {
      filePath: {
        title: 'File',
        type: 'file',
        filetype: placeFiletype,
        autoActivate: true,
      },
      variantOption: {
        title: 'Variant',
        type: 'enum',
        options: {},
        when: feature => !!feature.sourceVariant()?.options?.length,
      },
      flatten: {
        title: 'Flatten',
        type: 'bool',
        icons: ['layer-group', 'sitemap'],
        when: feature => isStepData(feature.cachedData || ''),
      },
    })

    this.filePath = null
    this.flatten = false
    this.cachedData = null
    this.variantOption = null
    this.importTransform = new THREE.Matrix4()
    this.sourceTree = null
    this.activationError = null
    this.sourceRevision = null
    this.newerData = null
    this.linkCheck = 0
    this.importGizmo = null
    this.color = null
    this.stepSource = null
  }

  isComplete() {
    return !!this.filePath && !!this.cachedData && !!this.sourceTree
  }

  async chooseFile() {
    const linkCheck = ++this.linkCheck
    const file = await loadFile(placeFiletype, 'binary')
    file.data = await decodeFile(file.data)
    if(linkCheck == this.linkCheck) this.useFile(file)
  }

  useFile(file) {
    file.data = sourceData(file.data)
    if(isStepData(file.data)) this.color ||= this.document.timeline.makeColor()
    const sourceTree = this.parseSource(file.data, null, file.path)
    this.filePath = file.path
    this.cachedData = file.data
    this.sourceTree = sourceTree
    this.variantOption = this.normalizedVariantOption(sourceTree)
    this.sourceRevision = this.snapshotRevision(file.data)
    this.activationError = null
  }

  fileLabel() {
    return this.filePath ? this.filePath.split(/[\\/]/).pop() : 'Choose file…'
  }

  async checkSource() {
    this.newerData = await readNewerRevision(this, placeFiletype, sourceData)
  }

  updateSource() {
    this.cachedData = this.newerData
    this.sourceTree = this.parseSource(this.cachedData, this.variantOption)
    this.variantOption = this.normalizedVariantOption(this.sourceTree, this.variantOption)
    this.sourceRevision = this.snapshotRevision(this.cachedData)
    this.newerData = null
    this.document.timeline.invalidateFeature(this)
    this.document.regenerate()
    this.document.markChanged()
  }

  execute(tree) {
    if(this.suppressUpdate || !this.isComplete()) return
    this.error = null
    this.updateSourceVariant()
    if(isStepData(this.cachedData)) this.sourceTree = this.parseSource(this.cachedData)
    withTopologyNaming(this, () => {
      this.updateFeature(tree)
      if(this.error?.type == 'error') return
      for(const component of tree.findChild(this.componentId).getChildren()) {
        component.compound.restoreTopologySelections(this)
      }
    })
  }

  sourceVariant() {
    return this.sourceTree?.creator.variant || null
  }

  settingOptions(key) {
    if(key != 'variantOption') return this.settings[key].options
    return Object.fromEntries(
      (this.sourceVariant()?.options || []).map((option, index) => [index, {
        title: option.title,
        icon: ['dice-one', 'dice-two', 'dice-three', 'dice-four', 'dice-five', 'dice-six'][index % 6],
      }])
    )
  }

  normalizedVariantOption(source, requested=this.variantOption) {
    const variant = source?.creator?.variant || source?.variant
    if(!variant?.options?.length) return null
    let index = requested === null || requested === undefined ?
      Number(variant.activeOption) : Number(requested)
    if(!Number.isInteger(index) || !variant.options[index]) index = 0
    return String(index)
  }

  updateSourceVariant() {
    const selected = this.normalizedVariantOption(this.sourceTree)
    this.variantOption = selected
    if(selected === null || Number(selected) == this.sourceVariant().activeOption) return
    this.sourceTree = this.parseSource(this.cachedData, selected)
  }

  updateFeature(tree) {
    const target = tree.findChild(this.componentId)
    if(!target || !this.sourceTree) return

    try {
      const { componentIds } = this.createImportedComponents(target, true)
      this.importJoints(tree, componentIds)
    } catch(error) {
      this.error = featureError(error, 'The imported geometry could not be generated')
    }
  }

  addStructuralComponents(tree) {
    const target = tree.findChild(this.componentId)
    if(target && this.sourceTree) this.createImportedComponents(target, false)
  }

  createImportedComponents(target, includeGeometry) {
    const componentIds = new Map(this.sourceTree.getChildren().map(component => [
      component.id,
      `${this.id}/import/component/${component.id}`,
    ]))
    const definitions = new Map()
    this.cloneImportedComponent(
      this.sourceTree,
      target,
      componentIds,
      definitions,
      includeGeometry,
      true,
    )
    return { componentIds }
  }

  cloneImportedComponent(source, parent, componentIds, definitions, includeGeometry, isRoot=false) {
    const componentId = componentIds.get(source.id)
    const component = new Component(parent, componentId)
    component.readOnly = true
    const sourceDefinitionId = source.sourceId()
    let definition = definitions.get(sourceDefinitionId)
    if(!definition) {
      const canonical = this.sourceTree.findChild(sourceDefinitionId) || source
      definition = new ComponentDefinition(canonical.creator.title, canonical.creator.color)
      definition.hidden = !!canonical.creator.hidden
      definition.material = canonical.creator.material
      definitions.set(sourceDefinitionId, definition)
    }
    component.creator = source.instanceOf ? Object.assign(Object.create(definition), {
      hidden: !!source.creator.hidden,
    }) : definition
    component.designTransform = source.localTransform()
    if(isRoot) {
      component.designTransform = this.importTransform.clone().multiply(component.designTransform)
    }
    if(includeGeometry) {
      component.compound = source.compound.cloneForComponent(componentId)
      this.identifyImportedShapes(component.compound, source.compound)
    }
    component.instanceOf = source.instanceOf && componentIds.get(source.instanceOf)
    component.sourceOccurrence = source.sourceOccurrence && componentIds.get(source.sourceOccurrence)
    parent.children.push(component)
    source.children.forEach(child => this.cloneImportedComponent(
      child,
      component,
      componentIds,
      definitions,
      includeGeometry,
    ))
    return component
  }

  identifyImportedShapes(compound, sourceCompound) {
    const geometryRevision = `${this.sourceRevision}/variant-${this.variantOption ?? 'default'}${this.flatten ? '/flat' : ''}`
    compound.namespaceTopology(`import/${sourceCompound.componentId}`)
    compound.solids().forEach(solid => {
      solid.faces().forEach(face => {
        face.tessId = `${this.id}/${geometryRevision}/face/${face.tessId}`
      })
      solid.edges().forEach(edge => {
        edge.tessId = `${this.id}/${geometryRevision}/edge/${edge.tessId}`
      })
    })
  }

  importJoints(tree, componentIds) {
    this.sourceTree.assemblyJoints
      .filter(joint => joint.type != 'fix')
      .forEach(joint => {
        const componentA = componentIds.get(joint.componentA)
        const componentB = componentIds.get(joint.componentB)
        if(!componentA || !componentB) return
        tree.assemblyJoints.push({
          ...joint,
          id: `${this.id}/import/joint/${joint.id}`,
          componentA,
          componentB,
          frameA: joint.frameA && joint.frameA.clone(),
          frameB: joint.frameB && joint.frameB.clone(),
          fixedWorld: undefined,
        })
      })
  }

  snapshotRevision(data) {
    let hash = 2166136261
    for(let index = 0; index < data.length; index++) {
      hash = Math.imul(hash ^ data.charCodeAt(index), 16777619)
    }
    return `${data.length}-${(hash >>> 0).toString(36)}`
  }

  parseSource(data, variantOption=null, path=this.filePath) {
    if(isStepData(data)) return this.parseStepSource(data, path)
    const previousLastId = lastId
    try {
      const dump = parseDocument(data, { document: this.document, sketches: {} })
      if(!dump.timeline?.evaluate) throw new Error('Not a ProtoSolid document')

      const selected = this.normalizedVariantOption(dump.timeline.baseCompDef, variantOption)
      if(selected !== null) dump.timeline.baseCompDef.selectVariantOption(Number(selected), false)

      const sourceDocument = {
        timeline: dump.timeline,
        top(at) { return this.timeline.tree(at) },
      }
      sourceDocument.activeComponent = dump.timeline.tree(0)
      dump.timeline.features.forEach(feature => { feature.document = sourceDocument })
      dump.timeline.evaluate()
      return markRaw(dump.timeline.tree())
    } finally {
      setLastId(previousLastId)
    }
  }

  // Reading and sewing a STEP file is costly; its source tree changes only
  // with the file's contents and the flatten option.
  parseStepSource(data, path) {
    if(this.stepSource?.data !== data || this.stepSource.flatten !== this.flatten) {
      const title = path ? path.split(/[\\/]/).pop().replace(/\.[^.]*$/, '') : 'STEP Import'
      const tree = this.flatten ?
        this.stepComponent(null, 'step', title, this.color, shapex.Compound.fromStep(data, modelTolerance)) :
        this.stepAssembly(shapex.StepAssembly.fromStep(data, modelTolerance), title)
      this.stepSource = markRaw({ data, flatten: this.flatten, tree: markRaw(tree) })
    }
    return this.stepSource.tree
  }

  stepComponent(parent, id, title, color, bodies) {
    const component = new Component(parent, id)
    component.readOnly = true
    component.creator = new ComponentDefinition(title, color)
    if(bodies) component.compound = importedCompound(id, bodies, this.id)
    parent?.children.push(component)
    return component
  }

  // Each product placed more than once becomes an instance of its first
  // occurrence. Several root products share a root named after the file.
  stepAssembly(assembly, title) {
    const canonicals = new Map()
    const place = (parent, id, index, transform, occurrenceSource) => {
      const product = assembly.products[index]
      const canonical = canonicals.get(index)
      let component
      if(canonical) {
        occurrenceSource ||= canonical
        component = new Component(parent, id)
        component.readOnly = true
        component.instanceOf = canonical.id
        component.sourceOccurrence = occurrenceSource.id
        component.creator = Object.assign(Object.create(canonical.creator), { hidden: false })
        component.compound = canonical.compound.cloneForComponent(id)
        parent?.children.push(component)
      } else {
        component = this.stepComponent(parent, id, product.name || title,
          stepColor(product, this.color, index), product.bodies)
        canonicals.set(index, component)
      }
      component.designTransform = transform
      product.occurrences.forEach((occurrence, child) => place(component, `${id}/${child}`,
        occurrence.product, matrixFromTransform(occurrence.placement), occurrenceSource?.children[child]))
      return component
    }
    if(assembly.roots.length == 1) return place(null, 'step', assembly.roots[0], null)
    const root = this.stepComponent(null, 'step', title, this.color, null)
    assembly.roots.forEach((index, child) => place(root, `step/${child}`, index, null))
    return root
  }

  getValues() {
    return {
      filePath: this.filePath,
      cachedData: this.cachedData,
      variantOption: this.variantOption,
      importTransform: this.importTransform,
      color: this.color,
      flatten: this.flatten,
    }
  }

  setValues(values) {
    const cachedDataChanged = values.cachedData !== this.cachedData
    const variantChanged = values.variantOption !== this.variantOption
    const flattenChanged = !!values.flatten !== this.flatten
    this.color = values.color || null
    this.flatten = !!values.flatten
    this.filePath = values.filePath || null
    this.cachedData = values.cachedData || null
    this.variantOption = values.variantOption ?? null
    this.sourceRevision = this.cachedData ? this.snapshotRevision(this.cachedData) : null
    this.importTransform = values.importTransform || new THREE.Matrix4()
    if(cachedDataChanged || variantChanged || flattenChanged) {
      this.sourceTree = this.cachedData ? this.parseSource(this.cachedData, this.variantOption) : null
      this.variantOption = this.normalizedVariantOption(this.sourceTree, this.variantOption)
    }
  }

  updateGizmos() {
    this.clearImportGizmo()
    if(!this.isComplete()) return
    const component = this.document.top().findChild(this.componentOccurrenceId) ||
      this.document.top().findChild(this.componentId)
    if(!component) return

    const sourceTransform = this.sourceTree.localTransform()
    const componentWorld = worldTransform(component)
    const gizmo = markRaw(new TransformGizmo(
      componentWorld.clone().multiply(this.importTransform).multiply(sourceTransform),
      desiredWorld => {
        const current = this.document.top().findChild(this.componentOccurrenceId) ||
          this.document.top().findChild(this.componentId)
        if(!current) return
        this.importTransform = worldTransform(current).invert()
          .multiply(desiredWorld)
          .multiply(this.sourceTree.localTransform().invert())
        this.document.timeline.invalidateFeature(this)
        this.document.regenerate()
      },
    ))
    this.importGizmo = () => gizmo
    window.alcRenderer.addGizmo(gizmo)
  }

  clearImportGizmo() {
    if(this.importGizmo) window.alcRenderer?.removeGizmo(this.importGizmo())
    this.importGizmo = null
  }

  dispose() {
    this.linkCheck++
    this.activationError = null
    this.clearImportGizmo()
    super.dispose()
  }
}

Serialize.register(PlaceFeature, 'PlaceFeature')
// Load documents saved while this feature was temporarily named ImportFeature.
Serialize.register(PlaceFeature, 'ImportFeature')


export class ExtrudeFeature extends SolidFeature {
  static icon = 'extrude'
  constructor(doc) {
    super(doc, true, 'Extrusion', {
      extent: {
        title: 'Extent',
        type: 'select',
        options: {
          distance: { title: 'Distance', icon: 'ruler-vertical' },
          face: { title: 'Up to face', icon: 'extent-to-face' },
          through: { title: 'Through all', icon: 'infinity' },
        },
      },
      side: {
        title: 'Side',
        type: 'select',
        when: feature => feature.extent != 'face',
        options: {
          forward: { title: 'Forward', icon: 'caret-up' },
          reverse: { title: 'Reverse', icon: 'caret-down' },
          both: { title: 'Both directions', icon: 'arrows-alt-v' },
        },
      },
      profiles: {
        title: 'Profiles',
        type: 'profile',
        multi: true,
      },
      axis: {
        title: '(Axis)',
        type: 'axis',
        optional: true,
        when: feature => feature.extent == 'distance',
      },
      distance: {
        title: 'Distance',
        type: 'length',
        when: feature => feature.extent == 'distance',
      },
      limitFace: {
        title: 'Up to face',
        type: 'face',
        when: feature => feature.extent == 'face',
      },
      offset: {
        title: 'Start offset',
        type: 'length',
        allowNegative: true,
        when: feature => feature.extent != 'through',
      },
      endOffset: {
        title: 'End offset',
        type: 'length',
        allowNegative: true,
        when: feature => feature.extent == 'face',
      },
    })

    this.extent = 'distance'
    this.limitFace = null
    this.profiles = null
    this.axis = null
    this.distance = 1.0
    this.offset = 0
    this.endOffset = 0
    this.side = 'forward'
    this.sideChangedByUser = false
  }

  settingChanged(key, value) {
    if(key == 'side') this.sideChangedByUser = true
    if(key == 'extent' && value == 'through') {
      this.operation = 'cut'
      this.settingChanged('operation', 'cut')
    }
    if(key == 'operation' && value == 'cut' && !this.sideChangedByUser) this.side = 'reverse'
  }

  getValues() {
    return { ...super.getValues(), sideChangedByUser: this.sideChangedByUser }
  }

  setValues(values) {
    this.sideChangedByUser = values.sideChangedByUser ?? true
    // Preserve the direction of documents saved with the old boolean toggle.
    super.setValues({
      ...values,
      extent: values.extent || 'distance',
      side: typeof values.side == 'boolean' ? (values.side ? 'forward' : 'reverse') : (values.side || 'forward'),
    })
  }

  updateFeature(tree, references) {
    const distance = this.distance * (this.side == 'reverse' ? -1 : this.side == 'both' ? 2 : 1)
    const offset = this.offset * (this.side == 'reverse' ? -1 : 1) - (this.side == 'both' ? this.distance : 0)
    const comp = tree.findChild(this.componentId)

    let tool = new Compound(this.componentId)
    this.previewBody = null
    try {
      for(const profile of references.profiles) {
        const extrusion = this.extent == 'face' ?
          profile.extrudeUpToFace(this.componentId, references.limitFace, this.id, this.offset, this.endOffset) :
          this.extent == 'through' ?
            profile.extrudeThroughAll(this.componentId, comp.compound, this.id, this.side) :
            profile.extrude(this.componentId, distance, this.id, offset)
        tool = tool.boolean(extrusion, 'join')
      }
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = featureError(err, 'Extrusion could not be created') }
  }

  getGizmo() {
    if(this.extent != 'distance') return
    const profile = this.profiles().at(-1).getItem()
    const center = profile.center().clone().addScaledVector(profile.normal(), this.offset * (this.side == 'reverse' ? -1 : 1))
    return {
      distance: {
        center,
        direction: (this.axis && this.axis()) || profile.normal(),
        distance: this.distance,
        side: this.side != 'reverse',
        cb: (dist, side) => {
          this.setExpression('distance', null)
          this.distance = dist
          if(this.side != 'both') {
            const direction = side ? 'forward' : 'reverse'
            if(this.side != direction) this.sideChangedByUser = true
            this.side = direction
          }
        },
      }
    }
  }

  updateGizmos() {
    if(this.extent != 'distance') {
      if(this.distanceGizmo) window.alcRenderer.removeGizmo(this.distanceGizmo())
      this.distanceGizmo = null
      return
    }
    super.updateGizmos()
  }

  preview() {
    return this.isComplete() && this.error?.type != 'error' ? this.previewBody : null
  }
}

Serialize.register(ExtrudeFeature, 'ExtrudeFeature')


export class RevolveFeature extends SolidFeature {
  static icon = 'revolve'
  constructor(document) {
    super(document, true, 'Revolution', {
      extent: {
        title: 'Extent',
        type: 'select',
        options: {
          angle: { title: 'Angle', icon: 'drafting-compass' },
          face: { title: 'Up to face', icon: 'extent-to-face' },
          between: { title: 'Between faces', icon: 'exchange-alt' },
        },
      },
      profiles: {
        title: 'Profiles',
        type: 'profile',
        multi: true,
      },
      axis: {
        title: 'Axis',
        type: 'axis',
      },
      angle: {
        title: 'Angle',
        type: 'angle',
        when: feature => feature.extent == 'angle',
        gizmo: true,
      },
      offset: {
        title: 'Offset',
        type: 'angle',
        allowNegative: true,
        when: feature => feature.extent == 'angle',
        gizmo: true,
      },
      fromFace: {
        title: 'From face',
        type: 'face',
        when: feature => feature.extent == 'between',
      },
      limitFace: {
        title: 'Up to face',
        type: 'face',
        when: feature => feature.extent != 'angle',
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-right', 'caret-left'],
      },
    })

    this.profiles = null
    this.axis = null
    this.extent = 'angle'
    this.fromFace = null
    this.limitFace = null
    this.angle = 360.0
    this.offset = 0
    this.side = true
  }

  updateFeature(tree, references) {
    const angle = this.angle * (this.side ? 1 : -1)
    const offset = this.offset * (this.side ? 1 : -1)

    let tool = new Compound(this.componentId)
    this.previewBody = null
    const comp = tree.findChild(this.componentId)
    try {
      for(const profile of references.profiles) {
        const revolution = this.extent == 'between' ?
          profile.revolveBetweenFaces(this.componentId, references.axis, references.fromFace, references.limitFace, this.id, this.side) :
          this.extent == 'face' ?
          profile.revolveUpToFace(this.componentId, references.axis, references.limitFace, this.id, this.side) :
          profile.revolve(this.componentId, references.axis, rad(angle), this.id, rad(offset))
        tool = tool.boolean(revolution, 'join')
      }
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = featureError(err, 'Revolution could not be created') }
  }

  getGizmo() {
    const axis = this.axis().getItem()
    const profileCenter = this.profiles().at(-1).getItem().center()
    const direction = normalFromMatrix(axis)
    const origin = new THREE.Vector3().setFromMatrixPosition(axis)
    const center = origin.clone().addScaledVector(direction, profileCenter.clone().sub(origin).dot(direction))
    const radial = profileCenter.clone().sub(center)
    if(radial.lengthSq() < 1e-12) radial.setFromMatrixColumn(axis, 0)
    radial.normalize()
    const rotation = new THREE.Matrix4().makeBasis(radial, direction.clone().cross(radial), direction)
    const sign = this.side ? 1 : -1
    return {
      angle: {
        type: 'angle', center,
        rotation: rotation.clone().multiply(new THREE.Matrix4().makeRotationZ(rad(this.offset * sign))),
        value: this.angle * sign,
        cb: angle => {
          this.setExpression('angle', null)
          this.angle = Math.min(360, Math.abs(angle))
          if(angle) this.side = angle > 0
        },
      },
      offset: {
        type: 'angle', center, rotation, size: 0.65,
        value: this.offset * sign,
        cb: angle => {
          this.setExpression('offset', null)
          this.offset = angle * (this.side ? 1 : -1)
        },
      },
    }
  }

  updateGizmos() {
    this.revolveGizmos ||= markRaw(new FeatureGizmos())
    const component = this.document.top().findChild(this.componentOccurrenceId) ||
      this.document.top().findChild(this.componentId)
    this.revolveGizmos.update(this.isComplete() && this.extent == 'angle' ? this.getGizmo() : {}, worldTransform(component))
  }

  dispose() {
    super.dispose()
    this.revolveGizmos?.dispose()
    this.revolveGizmos = null
  }

  preview() {
    return this.isComplete() && this.error?.type != 'error' ? this.previewBody : null
  }
}

Serialize.register(RevolveFeature, 'RevolveFeature')


export class LoftFeature extends SolidFeature {
  static icon = 'loft'
  constructor(doc) {
    super(doc, true, 'Loft', {
      profiles: {
        title: 'Profiles',
        type: 'profile',
        multi: true,
      },
      untwist: {
        title: 'Untwist',
        type: 'bool',
        icons: ['caret-up', 'caret-down']
      },
      guides: {
        title: '(Guides)',
        type: 'path',
        multi: true,
        optional: true,
        consumesSketch: true,
      },
    })

    this.profiles = null
    this.untwist = true
    this.guides = null
  }

  isComplete() {
    return super.isComplete() && this.profiles().length >= 2
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      const first = references.profiles[0]
      const tool = first.loft(
        this.componentId,
        references.profiles.slice(1),
        this.untwist,
        references.guides || [],
        this.id,
      )
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = featureError(err, 'Loft could not be built') }
  }
}

Serialize.register(LoftFeature, 'LoftFeature')


export class SplitFeature extends SolidFeature {
  static icon = 'split'
  constructor(document) {
    super(document, false, 'Split', {
      plane: {
        title: 'Tool',
        type: 'plane',
      },
      // bodies: {
      //   title: 'Bodies',
      //   type: 'solid',
      //   multi: true,
      // },
      keep: {
        title: 'Keep',
        type: 'enum',
        options: {
          both: { title: 'Both', icon: 'arrows-alt-h' },
          left: { title: 'Left', icon: 'arrow-left' },
          right: { title: 'Right', icon: 'arrow-right' },
        },
      },
    })

    this.plane = null
    this.keep = 'both'
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.split(references.plane, this.keep)
    } catch(err) { this.error = featureError(err, 'Split could not be performed') }
  }
}

Serialize.register(SplitFeature, 'SplitFeature')


export class DraftFeature extends SolidFeature {
  static icon = 'draft'
  static modifiedFaceInputs = ['faces']
  constructor(document) {
    super(document, false, 'Draft', {
      ref_plane: {
        title: 'Reference',
        type: 'plane',
      },
      faces: {
        title: 'Faces',
        type: 'face',
        multi: true,
      },
      angle: {
        title: 'Angle',
        type: 'angle',
        allowNegative: true,
      },
    })

    this.ref_plane = null
    this.faces = null
    this.angle = 0.0
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.draft(
        references.faces,
        references.ref_plane,
        rad(this.angle),
      )
    } catch(err) { this.error = featureError(err, 'Draft could not be built') }
  }

  updateGizmos() {
    const face = this.isComplete() && this.inputItems('faces').at(-1)
    if(face) {
      const center = face.center().clone()
      const component = this.document.top().findChild(this.componentOccurrenceId) ||
        this.document.top().findChild(this.componentId)
      if(component) center.applyMatrix4(worldTransform(component))
      if(this.angleGizmo) {
        this.angleGizmo.setFrame(center, new THREE.Euler())
        this.angleGizmo.set(this.angle)
      } else {
        this.angleGizmo = markRaw(new AngleGizmo(center, new THREE.Euler(), this.angle, (angle) => {
          this.setExpression('angle', null)
          this.angle = angle
        }))
        window.alcRenderer.addGizmo(this.angleGizmo)
      }
    } else {
      window.alcRenderer.removeGizmo(this.angleGizmo)
      this.angleGizmo = null
    }
  }

  // confirm() {
  //   // Refetch faces in case they've been repaired
  //   this.faces().forEach(faceRef => faceRef.free())
  //   const faces = this.real.face_refs()
  //   this.faces = () => faces
  // }

  dispose() {
    super.dispose()
    window.alcRenderer.removeGizmo(this.angleGizmo)
    this.angleGizmo = null
  }
}

Serialize.register(DraftFeature, 'DraftFeature')


export class SweepFeature extends SolidFeature {
  static icon = 'sweep'
  static undump(dump, context) {
    const feature = super.undump(dump, context)
    // Existing sweeps predate multi-segment rails and followed only their selection.
    if(feature.rail && !Array.isArray(feature.rail())) feature.rail = () => [dump.values.rail]
    if(dump.values?.propagation == null) feature.propagation = 'selected'
    return feature
  }

  constructor(document) {
    super(document, true, 'Sweep', {
      profile: {
        title: 'Profile',
        type: 'profile',
      },
      rail: {
        title: 'Rail',
        type: 'path',
        multi: true,
        consumesSketch: true,
      },
      propagation: edgePropagationSetting(),
    })

    this.profile = null
    this.rail = null
    this.propagation = 'tangent'
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      const tool = references.profile.sweep(this.componentId, references.rail,
        this.propagation === 'tangent', this.id)
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = featureError(err, 'Sweep could not be built') }
  }
}

Serialize.register(SweepFeature, 'SweepFeature')


export class CoilFeature extends SolidFeature {
  static icon = 'coil'
  constructor(document) {
    super(document, true, 'Coil', {
      size: {
        title: 'Size',
        type: 'select',
        options: {
          turnsPitch: { title: 'Revolutions and pitch', icon: 'grip-lines' },
          turnsHeight: { title: 'Revolutions and height', icon: 'ruler-vertical' },
          heightPitch: { title: 'Height and pitch', icon: 'text-height' },
        },
      },
      profiles: {
        title: 'Profiles',
        type: 'profile',
        multi: true,
      },
      axis: {
        title: 'Axis',
        type: 'axis',
      },
      revolutions: {
        title: 'Revolutions',
        type: 'number',
        min: 0,
        step: 0.25,
        when: feature => feature.size != 'heightPitch',
      },
      height: {
        title: 'Height',
        type: 'length',
        when: feature => feature.size != 'turnsPitch',
      },
      pitch: {
        title: 'Pitch',
        type: 'length',
        when: feature => feature.size != 'turnsHeight',
      },
      handedness: {
        title: 'Handedness',
        type: 'select',
        options: {
          right: { title: 'Right-handed', icon: 'redo' },
          left: { title: 'Left-handed', icon: 'undo' },
        },
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-up', 'caret-down'],
      },
    })

    this.size = 'turnsPitch'
    this.profiles = null
    this.axis = null
    this.revolutions = 3
    this.height = 60.0
    this.pitch = 20.0
    this.handedness = 'right'
    this.side = true
  }

  // Revolutions and pitch, whichever two quantities define the coil
  coilSize() {
    if(this.size == 'turnsHeight') return { revolutions: this.revolutions, pitch: this.height / this.revolutions }
    if(this.size == 'heightPitch') return { revolutions: this.height / this.pitch, pitch: this.pitch }
    return { revolutions: this.revolutions, pitch: this.pitch }
  }

  updateFeature(tree, references) {
    const { revolutions, pitch } = this.coilSize()
    const sign = this.side ? 1 : -1
    const angle = 2 * Math.PI * revolutions * sign * (this.handedness == 'right' ? 1 : -1)

    let tool = new Compound(this.componentId)
    this.previewBody = null
    const comp = tree.findChild(this.componentId)
    try {
      for(const profile of references.profiles) {
        tool = tool.boolean(profile.coil(this.componentId, references.axis, angle, pitch * sign, this.id), 'join')
      }
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = featureError(err, 'Coil could not be created') }
  }

  getGizmo() {
    const axis = this.axis().getItem()
    const direction = normalFromMatrix(axis)
    const origin = new THREE.Vector3().setFromMatrixPosition(axis)
    const profileCenter = this.profiles().at(-1).getItem().center()
    const center = origin.clone().addScaledVector(direction, profileCenter.clone().sub(origin).dot(direction))
    const { revolutions, pitch } = this.coilSize()
    return {
      distance: {
        center,
        direction,
        distance: revolutions * pitch,
        side: this.side,
        cb: (height, side) => {
          if(this.size == 'turnsPitch') {
            this.revolutions = height / this.pitch
          } else {
            this.setExpression('height', null)
            this.height = height
          }
          this.side = side
        },
      },
    }
  }

  preview() {
    return this.isComplete() && this.error?.type != 'error' ? this.previewBody : null
  }
}

Serialize.register(CoilFeature, 'CoilFeature')


function edgePropagationSetting() {
  return {
    title: 'Propagation',
    type: 'select',
    options: {
      tangent: { title: 'Follow tangent edges', icon: 'link' },
      selected: { title: 'Selected edges only', icon: 'mouse-pointer' },
    },
  }
}


export class FilletFeature extends SolidFeature {
  static icon = 'fillet'
  constructor(doc) {
    super(doc, false, 'Fillet', {
      edges: {
        title: 'Edges',
        type: 'edge',
        multi: true,
      },
      filletType: {
        title: 'Mode',
        type: 'select',
        options: {
          radius: { title: 'Constant radius', icon: 'fillet-constant-radius' },
          width: { title: 'Constant width', icon: 'fillet-constant-width' },
        },
      },
      radius: {
        title: 'Radius',
        type: 'length',
        gizmo: true,
        when: feature => feature.filletType == 'radius',
      },
      width: {
        title: 'Width',
        type: 'length',
        gizmo: true,
        when: feature => feature.filletType == 'width',
      },
      continuity: {
        title: 'Continuity',
        type: 'select',
        options: {
          g1: { title: 'G1 — tangent continuity', label: 'G1' },
          g2: { title: 'G2 — curvature continuity', label: 'G2' },
        },
      },
      propagation: edgePropagationSetting(),
    })

    this.edges = null
    this.radius = 1.0
    this.width = 1.0
    this.filletType = 'radius'
    this.continuity = 'g1'
    this.propagation = 'tangent'
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.fillet(
        references.edges,
        this.filletType == 'width' ? this.width : this.radius,
        this.propagation === 'tangent',
        { constantWidth: this.filletType == 'width', curvatureContinuous: this.continuity == 'g2' },
      )
    } catch(err) { this.error = featureError(err, 'Fillet could not be built') }
  }

  getGizmo() {
    const edge = this.inputItems('edges').at(-1)
    if(!edge) return
    const points = edge.tesselate().map(point => point.toArray())
    const { center, directions } = edgeGizmoFrame(points, edge.connectedFaces().map(face => face.tesselate()))
    // Either face measures the radius; the more vertical one is easier to drag.
    const component = this.document.top().findChild(this.componentOccurrenceId) ||
      this.document.top().findChild(this.componentId)
    const transform = worldTransform(component)
    const verticality = direction => Math.abs(direction.clone().transformDirection(transform).y)
    const direction = directions.reduce((best, other) => verticality(other) > verticality(best) ? other : best)
    return {
      distance: {
        center,
        direction,
        distance: this.filletType == 'width' ? this.width : this.radius,
        side: true,
        cb: (dist, _side) => {
          const key = this.filletType == 'width' ? 'width' : 'radius'
          this.setExpression(key, null)
          this[key] = dist
        },
      }
    }
  }
}

Serialize.register(FilletFeature, 'FilletFeature')


export class ChamferFeature extends SolidFeature {
  static icon = 'chamfer'
  static undump(dump, context) {
    const feature = super.undump(dump, context)
    // Existing chamfers predate propagation and used only their selection.
    if(dump.values?.propagation == null) feature.propagation = 'selected'
    return feature
  }

  constructor(doc) {
    super(doc, false, 'Chamfer', {
      edges: {
        title: 'Edges',
        type: 'edge',
        multi: true,
      },
      chamferType: {
        title: 'Mode',
        type: 'select',
        options: {
          equal: { title: 'Equal distances', icon: 'equals' },
          'two-distances': { title: 'Two distances', icon: 'not-equal' },
          'distance-angle': { title: 'Distance + angle', icon: 'drafting-compass' },
        },
      },
      distance: {
        title: 'Distance',
        type: 'length',
        gizmo: true,
      },
      distance2: {
        title: 'Distance 2',
        type: 'length',
        gizmo: true,
        when: feature => feature.chamferType == 'two-distances',
      },
      angle: {
        title: 'Angle',
        type: 'angle',
        when: feature => feature.chamferType == 'distance-angle',
        gizmo: true,
      },
      flipSides: {
        title: 'Flip sides',
        type: 'bool',
        icons: ['exchange-alt', 'exchange-alt'],
        when: feature => feature.chamferType != 'equal',
      },
      propagation: edgePropagationSetting(),
    })

    this.propagation = 'tangent'
    this.edges = null
    this.distance = 1.0
    this.chamferType = 'equal'
    this.distance2 = 1.0
    this.angle = 45
    this.flipSides = false
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.chamfer(references.edges, this.distance, {
        type: this.chamferType,
        distance2: this.distance2,
        angle: rad(this.angle),
        flipSides: this.flipSides,
        followTangentEdges: this.propagation === 'tangent',
      })
    } catch(err) { this.error = featureError(err, 'Chamfer could not be built') }
  }

  getGizmo() {
    const edge = this.inputItems('edges').at(-1)
    if(!edge) return {}
    const faces = edge.connectedFaces().sort((a, b) => a.id.localeCompare(b.id))
    if(this.chamferType != 'equal' && this.flipSides) faces.reverse()
    const points = edge.tesselate().map(point => point.toArray())
    const { center, directions: [first, second] } = edgeGizmoFrame(points, faces.map(face => face.tesselate()))
    const distance = (key, direction) => ({
      type: 'length', center, direction, value: this[key],
      cb: value => {
        this.setExpression(key, null)
        this[key] = value
      },
    })
    const gizmos = { distance: distance('distance', first) }
    if(this.chamferType == 'two-distances') gizmos.distance2 = distance('distance2', second)
    if(this.chamferType == 'distance-angle') {
      // Measure at the first setback, from the reference face toward the
      // chamfer, in the cross-section perpendicular to the selected edge.
      const x = first.clone().negate()
      const z = x.clone().cross(second).normalize()
      const y = z.clone().cross(x)
      gizmos.angle = {
        type: 'angle',
        center: center.clone().addScaledVector(first, this.distance),
        rotation: new THREE.Matrix4().makeBasis(x, y, z),
        value: this.angle,
        cb: angle => {
          this.setExpression('angle', null)
          this.angle = angle
        },
      }
    }
    return gizmos
  }

  updateGizmos() {
    this.chamferGizmos ||= markRaw(new FeatureGizmos())
    const component = this.document.top().findChild(this.componentOccurrenceId) ||
      this.document.top().findChild(this.componentId)
    this.chamferGizmos.update(this.isComplete() ? this.getGizmo() : {}, worldTransform(component))
  }

  dispose() {
    super.dispose()
    this.chamferGizmos?.dispose()
    this.chamferGizmos = null
  }
}

Serialize.register(ChamferFeature, 'ChamferFeature')


export class ShellFeature extends SolidFeature {
  static icon = 'shell'
  constructor(doc) {
    super(doc, false, 'Shell', {
      faces: {
        title: '(Openings)',
        type: 'face',
        multi: true,
        optional: true,
      },
      thickness: {
        title: 'Thickness',
        type: 'length',
        gizmo: true,
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-up', 'caret-down']
      },
      joinType: {
        title: 'Corners',
        type: 'select',
        options: {
          sharp: { title: 'Sharp', icon: 'square' },
          rounded: { title: 'Rounded', icon: 'circle' },
        },
      },
    })

    this.faces = null
    this.thickness = 1.0
    this.side = false
    this.joinType = 'rounded'
  }

  updateFeature(tree, references) {
    const thickness = this.thickness * (this.side ? 1 : -1)
    const comp = tree.findChild(this.componentId)

    try {
      comp.compound = comp.compound.shell(references.faces, thickness, this.joinType)
    } catch(err) { this.error = featureError(err, 'Shell could not be built') }
  }

  getGizmo() {
    const tree = this.document.top(this.document.timeline.features.indexOf(this))
    const compound = tree.findChild(this.componentId).compound
    const openings = this.inputItems('faces')
    const retained = face => !openings.some(opening => opening.kernelId == face.kernelId)
    const visible = compound.solids().filter(solid => this.isSolidVisible(tree, solid))
    const face = openings.filter(face => visible.some(solid => solid.id == face.solid.id)).at(-1)?.connectedFaces().find(retained) ||
      visible.flatMap(solid => solid.faces()).find(retained)
    if(!face) return
    const { center, direction } = faceGizmoFrame(face.tesselate(), face.center())
    return {
      distance: {
        center, direction, distance: this.thickness, side: this.side,
        cb: (thickness, side) => {
          this.setExpression('thickness', null)
          this.thickness = thickness
          this.side = side
        },
      },
    }
  }
}

Serialize.register(ShellFeature, 'ShellFeature')


export class OffsetFeature extends SolidFeature {
  static icon = 'offset-faces'
  static modifiedFaceInputs = ['faces']
  constructor(doc) {
    super(doc, false, 'Offset', {
      faces: {
        title: 'Faces',
        type: 'face',
        multi: true,
      },
      distance: {
        title: 'Distance',
        type: 'length',
        allowNegative: true,
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-up', 'caret-down'],
      },
    })

    this.faces = null
    this.distance = 1.0
    this.side = true
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    const distance = this.distance * (this.side ? 1 : -1)
    try {
      comp.compound = comp.compound.offsetFaces(references.faces, distance)
    } catch(err) { this.error = featureError(err, 'Faces could not be offset') }
  }

  static undump(dump, context) {
    // Older documents stored the hollowing operation as OffsetFeature.
    if(Object.hasOwn(dump.values || {}, 'thickness')) return ShellFeature.undump(dump, context)
    return super.undump(dump, context)
  }
}

Serialize.register(OffsetFeature, 'OffsetFeature')


export class DissolveFeature extends SolidFeature {
  static icon = 'dissolve'
  constructor(doc) {
    super(doc, false, 'Dissolve', {
      faces: {
        title: 'Faces',
        type: 'face',
        multi: true,
      },
    })

    this.faces = null
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.dissolveFaces(references.faces)
    } catch(err) { this.error = featureError(err, 'Faces could not be dissolved') }
  }
}

Serialize.register(DissolveFeature, 'DissolveFeature')


export class BooleanFeature extends SolidFeature {
  static icon = 'boolean-union'
  constructor(doc) {
    super(doc, true, 'Boolean', {
      tools: {
        title: 'Tools',
        type: 'solid',
        multi: true,
      },
      keepTools: {
        title: 'Keep',
        type: 'bool',
        icons: ['hand-paper', 'trash'],
      },
    })

    this.tools = null
    this.keepTools = false
  }

  acceptsInput(item) {
    return item?.typename?.() == 'Solid' || super.acceptsInput(item)
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      const target = tree.findChild(this.componentOccurrenceId) || comp
      const inputs = references.tools.map(tool => {
        const source = tree.findChild(inputComponentId(tool))
        const external = !!source && source != target
        let compound = tool.toCompound()
        if(external) {
          compound = compound.transform(relativeComponentTransform(source, target))
        }
        return { tool, compound, external }
      })
      const tools = inputs.map(input => input.compound)
      const tool = tools.reduce((acc, tool) => acc.boolean(tool, 'join') )
      const localTools = inputs.filter(input => !input.external)
      if(localTools.length) comp.compound = comp.compound.removeShapes(localTools.map(input => input.tool))
      comp.compound = comp.compound.boolean(tool, this.operation)
      if(this.keepTools && localTools.length) {
        comp.compound = comp.compound.merge(localTools.map(input => input.compound))
      }
      this.previewBody = tool
    } catch(err) { this.error = featureError(err, 'Boolean operation could not be performed') }
  }

}

Serialize.register(BooleanFeature, 'BooleanFeature')


export class RemoveSolidsFeature extends Feature {
  static icon = 'trash'
  constructor(doc) {
    super(doc, false, 'Remove', {
      solids: {
        title: 'Solids',
        type: 'solid',
        multi: true,
      },
    })

    this.solids = null
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.removeShapes(references.solids)
    } catch(err) { this.error = featureError(err, 'Solids could not be removed') }
  }
}

Serialize.register(RemoveSolidsFeature, 'RemoveSolidsFeature')


export class MoveFeature extends SolidFeature {
  static icon = 'arrows-alt'
  static modifiedFaceInputs = ['faces']
  constructor(doc) {
    super(doc, false, 'Move', {
      faces: {
        title: 'Faces',
        type: 'face',
        multi: true,
      },
      transformSpace: {
        title: 'Gizmo Space',
        type: 'select',
        options: {
          world: {
            title: 'World',
            icon: 'globe',
          },
          component: {
            title: 'Component',
            icon: 'cube',
          },
        },
      },
      x: { title: 'Translate X', type: 'length', allowNegative: true },
      y: { title: 'Translate Y', type: 'length', allowNegative: true },
      z: { title: 'Translate Z', type: 'length', allowNegative: true },
      rx: { title: 'Rotate X', type: 'angle', allowNegative: true },
      ry: { title: 'Rotate Y', type: 'angle', allowNegative: true },
      rz: { title: 'Rotate Z', type: 'angle', allowNegative: true },
    })

    this.faces = null
    this.x = this.y = this.z = this.rx = this.ry = this.rz = 0
    this.transformSpace = 'component'
    this.moveCenter = new THREE.Vector3()
  }

  moveFrame() {
    return new THREE.Matrix4().compose(
      this.moveCenter.clone().add(new THREE.Vector3(this.x, this.y, this.z)),
      new THREE.Quaternion().setFromEuler(new THREE.Euler(rad(this.rx), rad(this.ry), rad(this.rz))),
      new THREE.Vector3(1, 1, 1),
    )
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      // Use the input revision so repeated previews rotate about a fixed point.
      this.moveCenter = references.faces.reduce((center, face) => center.add(face.center()), new THREE.Vector3())
        .divideScalar(references.faces.length)
      const transform = this.moveFrame().multiply(new THREE.Matrix4().makeTranslation(
        -this.moveCenter.x, -this.moveCenter.y, -this.moveCenter.z,
      ))
      comp.compound = comp.compound.moveFaces(references.faces, transform)
    } catch(err) { this.error = featureError(err, 'Faces could not be moved') }
  }

  updateGizmos() {
    if(!this.isComplete()) {
      this.clearMoveGizmo()
      return
    }
    const component = this.document.top().findChild(this.componentOccurrenceId) ||
      this.document.top().findChild(this.componentId)
    const frame = worldTransform(component).multiply(this.moveFrame())
    const space = this.transformSpace == 'world' ? 'world' : 'local'
    if(this.moveGizmo) {
      if(!this.moveGizmo().dragging) {
        this.moveGizmo().setTransform(frame)
        this.moveGizmo().setSpace(space)
      }
      return
    }
    const gizmo = markRaw(new TransformGizmo(frame, desiredWorld => {
      const current = this.document.top().findChild(this.componentOccurrenceId) ||
        this.document.top().findChild(this.componentId)
      const local = worldTransform(current).invert().multiply(desiredWorld)
      const position = new THREE.Vector3().setFromMatrixPosition(local).sub(this.moveCenter)
      const rotation = new THREE.Euler().setFromRotationMatrix(local)
      const values = { x: position.x, y: position.y, z: position.z,
        rx: THREE.MathUtils.radToDeg(rotation.x), ry: THREE.MathUtils.radToDeg(rotation.y),
        rz: THREE.MathUtils.radToDeg(rotation.z) }
      for(const [key, value] of Object.entries(values)) {
        this.setExpression(key, null)
        this[key] = value
      }
      this.document.timeline.invalidateFeature(this)
      this.document.regenerate()
    }, space))
    this.moveGizmo = () => gizmo
    window.alcRenderer.addGizmo(gizmo)
  }

  clearMoveGizmo() {
    if(this.moveGizmo) window.alcRenderer.removeGizmo(this.moveGizmo())
    this.moveGizmo = null
  }

  dispose() {
    this.clearMoveGizmo()
    super.dispose()
  }
}

Serialize.register(MoveFeature, 'MoveFeature')


export class ReplaceFeature extends SolidFeature {
  static icon = 'exchange-alt'
  static modifiedFaceInputs = ['face']
  constructor(doc) {
    super(doc, false, 'Replace', {
      face: {
        title: 'Replace',
        type: 'face',
      },
      replacement: {
        title: 'With',
        type: 'face',
      },
    })

    this.face = null
    this.replacement = null
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.replaceShapes([references.face], [references.replacement])
    } catch(err) { this.error = featureError(err, 'Face could not be replaced') }
  }
}

Serialize.register(ReplaceFeature, 'ReplaceFeature')
