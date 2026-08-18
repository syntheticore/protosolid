import * as THREE from 'three'
import { markRaw } from 'vue'

import { Component, ComponentDefinition, createComponentInstance } from './component.js'
import { Sketch } from './sketch.js'
import { Compound } from './geom3d.js'
import { Reference, PatternInputReference } from './references.js'
import { PlaneHelper } from './helpers.js'
import { normalFromMatrix, ocCatch, rad } from './utils.js'
import Serialize from './serialize.js'
import { LengthGizmo, AngleGizmo } from '../three/gizmos.js'
import { makeID } from './id.js'
import {
  alignJointWorld,
  baselineWorldTransform,
  jointFrame,
  referenceComponentId,
  relativeComponentTransform,
  setBaselineWorldTransform,
  worldTransform,
} from './assembly.js'


export class Feature {
  constructor(document, booleanOutput, title, settings) {
    this.document = document
    this.title = title
    this.settings = settings
    this.error = null
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
          icon: 'magnet',
        },
        cut: {
          title: 'Cut',
          icon: 'clone',
        },
        intersect: {
          title: 'Intersect',
          icon: 'box',
        },
        create: {
          title: 'Create',
          icon: 'edit',
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

    this.updateFeature(tree, references)

    try {
      const comp = tree.findChild(this.componentId)
      comp.compound = comp.compound.repair()
    } catch(err) { this.error = (this.error && this.error.type == 'error') ? this.error : err }
  }

  updateReferences(tree) {
    const results = {}
    const errors = []

    Object.keys(this.settings).forEach(key => {
      if(!this[key] || !this.needsPicker(key, true)) return

      const setting = this.settings[key]
      const refs = setting.multi ? this[key]() : [this[key]()]

      let settingError

      const items = refs.map(ref => {
        const clone = ref.clone()
        const error = clone.update(tree)
        const severe = (error && error.type == 'error')
        settingError = (severe ? error : (settingError || error))
        if(severe) return
        if(!error) ref.update(tree) // Update intact references without user consent, such that their center etc. can be calculated
        return clone.getItem()
      }).filter(Boolean)

      if(items.length && settingError) settingError.type = 'warning'

      if(settingError) errors.push(settingError)
      results[key] = setting.multi ? items : items[0]
    })

    const severe = errors.filter(error => error.type == 'error' )
    this.error = severe.length ? severe[0] : errors[0]

    return results
  }

  needsPicker(setting, includeOptionals) {
    setting = this.settings[setting]
    return this.isSettingVisible(setting) && ['profile', 'curve', 'axis', 'plane', 'face', 'edge', 'solid', 'point', 'componentRef', 'patternInput'].some(type =>
      type == setting.type && (!setting.optional || includeOptionals)
    )
  }

  isPickerSetting(setting) {
    return ['profile', 'curve', 'axis', 'plane', 'face', 'edge', 'solid', 'point', 'componentRef', 'patternInput'].includes(setting.type)
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
    const sketches = Object.entries(this.settings)
      .filter(([key, setting]) => this[key] && ['profile', 'curve'].some(type => type == setting.type ) )
      .forEach(([key, _]) =>
        [this[key]()].flat().forEach(elem => comp.creator.itemsHidden[elem.getItem().sketch.id] = true )
      )
  }

  updateFeature(tree, references) {}

  getGizmo() {}

  updateGizmos() {
    if(this.isComplete()) {
      let gizmo = this.getGizmo()
      if(!gizmo) return
      gizmo = gizmo.distance
      if(!gizmo) return
      if(this.distanceGizmo) {
        this.distanceGizmo().set(gizmo.distance, gizmo.side)
      } else {
        const component = this.document.top().findChild(this.componentOccurrenceId) ||
          this.document.top().findChild(this.componentId)
        const transform = component && worldTransform(component)
        const center = transform ? gizmo.center.clone().applyMatrix4(transform) : gizmo.center
        const direction = transform ? gizmo.direction.clone().transformDirection(transform) : gizmo.direction
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

  repair() {}

  involvedSketches() {
    return Object.keys(this.settings).flatMap(key => {
      if(this.settings[key].type != 'profile') return []
      if(!this[key]) return []
      const refs = this[key]()
      return (Array.isArray(refs) ? refs : [refs]).map(ref => ref.getItem().sketch )
    })
  }

  getValues() {
    const values = {}
    Object.keys(this.settings).forEach(key => {
      values[key] = (this[key] && this.isPickerSetting(this.settings[key]) ? this[key]() : this[key])
    })
    return values
  }

  setValues(values) {
    Object.keys(values).forEach(key => {
      const value = values[key]
      this[key] = (value && this.isPickerSetting(this.settings[key]) ? () => value : value)
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
    }
  }

  static undump(dump, context) {
    const feature = new this(context.document)
    feature.id = dump.id
    feature.componentId = dump.componentId
    feature.componentOccurrenceId = dump.componentOccurrenceId || dump.componentId
    feature.setValues(dump.values)
    return feature
  }
}


export class PatternFeature extends Feature {
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
          grid: 'Grid',
          radial: 'Radial',
        },
      },
      center: {
        title: 'Center',
        type: 'point',
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
        when: feature => feature.patternType == 'grid',
      },
      vOffset: {
        title: 'V Step',
        type: 'length',
        autoFocus: false,
        when: feature => feature.patternType == 'grid',
      },
      radialCount: {
        title: 'Count',
        type: 'integer',
        min: 1,
        when: feature => feature.patternType == 'radial',
      },
      radialStep: {
        title: 'Angle Step',
        type: 'number',
        step: 1,
        when: feature => feature.patternType == 'radial',
      },
    })

    this.patternType = 'grid'
    this.center = null
    this.uOffset = 10
    this.uCount = 2
    this.vOffset = 10
    this.vCount = 1
    this.radialStep = 45
    this.radialCount = 8

    const selected = doc.selection.items.map(item => PatternInputReference.fromItem(item)).filter(Boolean)
    this.inputs = selected.length ? () => selected : null
  }

  isComplete() {
    if(!super.isComplete()) return false
    if(this.patternType == 'radial') {
      return this.radialCount >= 1 && Number.isFinite(this.radialStep)
    }
    return this.uCount >= 1 && this.vCount >= 1 &&
      Number.isFinite(this.uOffset) && Number.isFinite(this.vOffset)
  }

  instanceCount() {
    return this.patternType == 'radial' ?
      Math.max(0, Math.floor(this.radialCount) - 1) :
      Math.max(0, Math.floor(this.uCount) * Math.floor(this.vCount) - 1)
  }

  acceptsInput() { return true }

  updateFeature(tree, references) {
    const ownIndex = this.document.timeline.features.indexOf(this)
    const hasFutureInput = this.inputs().some(input => input.featureId &&
      this.document.timeline.features.findIndex(feature => feature.id == input.featureId) >= ownIndex
    )
    if(hasFutureInput) {
      this.error = { type: 'error', msg: 'Pattern inputs must precede the pattern feature' }
      return
    }

    const comp = tree.findChild(this.componentId)
    const transforms = this.patternType == 'radial' ?
      this.radialTransforms(references.center) : this.gridTransforms()

    const componentSources = references.inputs.filter(input => input?.typename?.() == 'Component')
    componentSources.forEach((source, sourceIndex) => {
      const canonical = tree.findChild(source.sourceId())
      const ownerWorld = baselineWorldTransform(comp)
      const sourceInOwner = ownerWorld.clone().invert().multiply(baselineWorldTransform(source))
      transforms.forEach((transform, transformIndex) => {
        const id = `${this.id}/instance/${transformIndex}/${sourceIndex}`
        const placement = transform.clone().multiply(sourceInOwner)
        createComponentInstance(comp, canonical, id, placement)
      })
    })

    const sources = references.inputs
      .filter(input => input?.typename?.() != 'Component')
      .map(input => input.toCompound ? input.toCompound() : input)
    if(!sources.length) {
      this.previewBody = null
      return
    }

    try {
      const copies = transforms.flatMap((transform, transformIndex) =>
        sources.map((source, sourceIndex) => {
          const copy = source.transform(transform)
          this.identifyCopy(copy, transformIndex, sourceIndex)
          return copy
        })
      )
      if(!copies.length) {
        this.previewBody = null
        return
      }
      const tool = copies.slice(1).reduce(
        (combined, copy) => combined.boolean(copy, 'join'),
        copies[0],
      )
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = err || this.error }
  }

  gridTransforms() {
    const transforms = []
    for(let u = 0; u < Math.floor(this.uCount); u++) {
      for(let v = 0; v < Math.floor(this.vCount); v++) {
        if(u == 0 && v == 0) continue
        transforms.push(new THREE.Matrix4().makeTranslation(u * this.uOffset, v * this.vOffset, 0))
      }
    }
    return transforms
  }

  radialTransforms(centerItem) {
    const center = centerItem instanceof THREE.Matrix4 ?
      new THREE.Vector3().setFromMatrixPosition(centerItem) : centerItem
    const transforms = []
    for(let i = 1; i < Math.floor(this.radialCount); i++) {
      const angle = rad(this.radialStep * i)
      transforms.push(
        new THREE.Matrix4().makeTranslation(center.clone().negate())
          .premultiply(new THREE.Matrix4().makeRotationZ(angle))
          .premultiply(new THREE.Matrix4().makeTranslation(center))
      )
    }
    return transforms
  }

  identifyCopy(compound, transformIndex, sourceIndex) {
    compound.solids().forEach((solid, solidIndex) => {
      solid.faces().forEach((face, faceIndex) => {
        face.id = `/${this.id}/pattern/${transformIndex}/${sourceIndex}/${solidIndex}/face/${faceIndex}`
      })
      solid.edges().forEach((edge, edgeIndex) => {
        edge.id = `/${this.id}/pattern/${transformIndex}/${sourceIndex}/${solidIndex}/edge/${edgeIndex}`
      })
    })
  }
}

Serialize.register(PatternFeature, 'PatternFeature')


export class PoseFeature extends Feature {
  static icon = 'street-view'

  constructor(doc, transforms) {
    super(doc, false, 'Pose', {})
    this.transforms = transforms || doc.top().getChildren()
      .filter(component => component.parent && component.transform)
      .map(component => ({ id: component.id, transform: component.localTransform() }))
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
  dump() { return { ...super.dump(), transforms: this.transforms } }

  static undump(dump, context) {
    const feature = new PoseFeature(context.document, dump.transforms)
    feature.id = dump.id
    feature.componentId = dump.componentId
    feature.componentOccurrenceId = dump.componentOccurrenceId || dump.componentId
    return feature
  }
}
Serialize.register(PoseFeature, 'PoseFeature')


export class JointFeature extends Feature {
  static icon = 'code-branch'

  constructor(doc) {
    const is = type => feature => feature.jointType == type
    super(doc, false, 'Joint', {
      jointType: {
        title: 'Type',
        type: 'enum',
        options: {
          axis: 'Axis',
          coplanar: 'Coplanar',
          ball: 'Ball',
          fix: 'Fix',
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

    const frameA = jointFrame(this.jointType, references[keys[0]])
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
    } catch(err) { this.error = err || this.error }
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
    tree.findChild(this.componentId).sketches.push(this.sketch)
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


export class PlaneFeature extends Feature {
  static icon = 'map'
  constructor(doc) {
    super(doc, false, 'Plane', {
      base: {
        title: 'Base',
        type: 'plane',
      },
      offset: {
        title: 'Offset',
        type: 'length',
      },
    })

    this.base = null
    this.offset = 1.0

    this.planeId = makeID()
  }

  updateFeature(tree, references) {
    const vec = normalFromMatrix(references.base).multiplyScalar(this.offset)
    const pos = new THREE.Vector3().setFromMatrixPosition(references.base)
    const plane = references.base.clone().setPosition(pos.add(vec))
    const helper = new PlaneHelper(this.componentId, plane, this.planeId)
    tree.findChild(this.componentId).helpers.push(helper)
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


export class ExtrudeFeature extends Feature {
  static icon = 'layer-group'
  constructor(doc) {
    super(doc, true, 'Extrusion', {
      profiles: {
        title: 'Profiles',
        type: 'profile',
        multi: true,
      },
      axis: {
        title: '(Axis)',
        type: 'axis',
        optional: true,
      },
      distance: {
        title: 'Distance',
        type: 'length',
      },
      // limit: {
      //   type: 'OR',
      //   settings: [
      //     { title: 'Distance', type: 'length' },
      //     { title: '(Up to)', type: 'surface|point' },
      //   ]
      // },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-up', 'caret-down']
      },
    })

    this.profiles = null
    this.axis = null
    this.distance = 1.0
    this.side = true
  }

  updateFeature(tree, references) {
    const distance = this.distance * (this.side ? 1 : -1)
    const comp = tree.findChild(this.componentId)

    let tool = new Compound(this.componentId)
    references.profiles.forEach(profile => {
      try {
        const extrusion = profile.extrude(this.componentId, distance, this.id)
        tool = tool.boolean(extrusion, 'join')
      } catch(err) { this.error = err || this.error }
    })
    try {
      comp.compound = comp.compound.boolean(tool, this.operation)
    } catch(err) { this.error = err || this.error }

    this.previewBody = tool
  }

  getGizmo() {
    const profile = this.profiles()[0].getItem()
    return {
      distance: {
        center: profile.center(),
        direction: (this.axis && this.axis()) || profile.normal(),
        distance: this.distance,
        side: this.side,
        cb: (dist, side) => {
          this.distance = dist
          this.side = side
        },
      }
    }
  }

  repair() {
    const newProfiles = this.profiles().filter(profileRef => {
      const error = profileRef.update(this.document.top())
      return !error || error.type == 'warning'
    })
    this.profiles = () => newProfiles
    this.error = null
  }
}

Serialize.register(ExtrudeFeature, 'ExtrudeFeature')


export class RevolveFeature extends Feature {
  static icon = 'wave-square'
  constructor(document) {
    super(document, true, 'Revolution', {
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
        type: 'length',
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-right', 'caret-left'],
      },
    })

    this.profiles = null
    this.axis = null
    this.angle = 1.0
    this.side = true
  }

  updateFeature(tree, references) {
    const angle = this.angle * (this.side ? 1 : -1)

    let tool = new Compound(this.componentId)
    references.profiles.forEach(profile => {
      try {
        const revolution = profile.revolve(this.componentId, references.axis, angle, this.id)
        tool = tool.boolean(revolution, 'join')
      } catch(err) { this.error = err || this.error }
    })

    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.boolean(tool, this.operation)
    } catch(err) { this.error = err || this.error }

    this.previewBody = tool
  }
}

Serialize.register(RevolveFeature, 'RevolveFeature')


export class LoftFeature extends Feature {
  static icon = 'layer-group'
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
    })

    this.profiles = null
    this.untwist = true
  }

  isComplete() {
    return super.isComplete() && this.profiles().length >= 2
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      const first = references.profiles[0]
      const tool = first.loft(this.componentId, references.profiles.slice(1), this.untwist)
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = err || this.error }
  }

  repair() {
    const newProfiles = this.profiles().filter(profileRef => {
      const error = profileRef.update(this.document.top())
      return !error || error.type == 'warning'
    })
    this.profiles = () => newProfiles
    this.error = null
  }
}

Serialize.register(LoftFeature, 'LoftFeature')


export class SplitFeature extends Feature {
  static icon = 'divide'
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
          both: 'Both',
          left: 'Left',
          right: 'Right',
        },
      },
    })

    this.plane = null
    this.keep = 'both'
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.split(references.plane, this.keep, this.id)
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(SplitFeature, 'SplitFeature')


export class DraftFeature extends Feature {
  static icon = 'clone'
  constructor(document) {
    super(document, true, 'Draft', {
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
    } catch(err) { this.error = err || this.error }
  }

  updateGizmos() {
    if(this.isComplete()) {
      if(this.angleGizmo) {
        this.angleGizmo.set(this.angle)
      } else {
        const center = new THREE.Vector3()
        this.angleGizmo = markRaw(new AngleGizmo(center, new THREE.Euler(), this.angle, (angle) => {
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


export class SweepFeature extends Feature {
  static icon = 'route'
  constructor(document) {
    super(document, true, 'Sweep', {
      profile: {
        title: 'Profile',
        type: 'profile',
      },
      rail: {
        title: 'Rail',
        type: 'curve',
      },
      bounds: {
        title: 'Bounds',
        type: 'bounds',
      },
    })

    this.profile = null
    this.rail = null
    this.bounds = [0.0, 1.0]
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      const tool = references.profile.sweep(this.componentId, references.rail, this.bounds)
      comp.compound = comp.compound.boolean(tool, this.operation)
      this.previewBody = tool
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(SweepFeature, 'SweepFeature')


export class FilletFeature extends Feature {
  static icon = 'scroll'
  constructor(doc) {
    super(doc, false, 'Fillet', {
      edges: {
        title: 'Edges',
        type: 'edge',
        multi: true,
      },
      radius: {
        title: 'Radius',
        type: 'length',
        gizmo: true,
      },
    })

    this.edges = null
    this.radius = 1.0
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.fillet(references.edges, this.radius)
    } catch(err) { this.error = err || this.error }
  }

  getGizmo() {
    return {
      distance: {
        center: this.edges()[0].getItem().center(),
        direction: new THREE.Vector3(0,1,0),
        distance: this.radius,
        side: true,
        cb: (dist, _side) => { this.radius = dist },
      }
    }
  }

  repair() {
    const tree = this.document.top(this.document.timeline.previousFeature(this))
    const remainingEdges = this.edges().filter(edgeRef => !edgeRef.update(tree) )
    this.edges = () => remainingEdges
    this.error = null
  }
}

Serialize.register(FilletFeature, 'FilletFeature')


export class ChamferFeature extends Feature {
  static icon = 'screwdriver'
  constructor(doc) {
    super(doc, false, 'Chamfer', {
      edges: {
        title: 'Edges',
        type: 'edge',
        multi: true,
      },
      distance: {
        title: 'Distance',
        type: 'length',
        gizmo: true,
      },
    })

    this.edges = null
    this.distance = 1.0
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.chamfer(references.edges, this.distance)
    } catch(err) { this.error = err || this.error }
  }

  getGizmo() {
    return {
      distance: {
        center: this.edges()[0].getItem().center(),
        direction: new THREE.Vector3(0,1,0),
        distance: this.distance,
        side: true,
        cb: (dist, _side) => { this.distance = dist },
      }
    }
  }

  repair() {
    const tree = this.document.top(this.document.timeline.previousFeature(this))
    const remainingEdges = this.edges().filter(edgeRef => !edgeRef.update(tree) )
    this.edges = () => remainingEdges
    this.error = null
  }
}

Serialize.register(ChamferFeature, 'ChamferFeature')


export class OffsetFeature extends Feature {
  static icon = 'dot-circle'
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
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-up', 'caret-down']
      },
    })

    this.faces = null
    this.thickness = 1.0
    this.side = false
  }

  updateFeature(tree, references) {
    const thickness = this.thickness * (this.side ? 1 : -1)
    const comp = tree.findChild(this.componentId)

    try {
      comp.compound = comp.compound.offset(references.faces, thickness)
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(OffsetFeature, 'OffsetFeature')


export class DissolveFeature extends Feature {
  static icon = 'virus-slash'
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
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(DissolveFeature, 'DissolveFeature')


export class BooleanFeature extends Feature {
  static icon = 'boxes'
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
      comp.compound = comp.compound
        .removeShapes(localTools.map(input => input.tool))
        .boolean(tool, this.operation)
      if(this.keepTools && localTools.length) {
        comp.compound = comp.compound.merge(localTools.map(input => input.compound))
      }
      this.previewBody = tool
    } catch(err) { this.error = err || this.error }
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
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(RemoveSolidsFeature, 'RemoveSolidsFeature')


export class MoveFeature extends Feature {
  static icon = 'arrows-alt'
  constructor(doc) {
    super(doc, false, 'Move', {
      faces: {
        title: 'Faces',
        type: 'face',
        multi: true,
      },
      // transform: {
      //   title: 'Transform',
      //   type: 'transform',
      // },
    })

    this.faces = null
    this.transform = new THREE.Matrix4().makeTranslation(10, 0, 0)
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.moveFaces(references.faces, this.transform)
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(MoveFeature, 'MoveFeature')


export class ReplaceFeature extends Feature {
  static icon = 'exchange-alt'
  constructor(doc) {
    super(doc, false, 'Move', {
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
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.replaceShapes([references.face], [references.replacement])
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(ReplaceFeature, 'ReplaceFeature')
