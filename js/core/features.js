import * as THREE from 'three'
import { markRaw } from 'vue'

import { Component, ComponentDefinition, createComponentInstance } from './component.js'
import { Sketch } from './sketch.js'
import { Compound } from './geom3d.js'
import { PatternInputReference } from './references.js'
import { PlaneHelper } from './helpers.js'
import { normalFromMatrix, ocCatch, rad } from './utils.js'
import { loadFile } from '../utils.js'
import Serialize from './serialize.js'
import { LengthGizmo, AngleGizmo, TransformGizmo, FeatureGizmos, edgeGizmoFrame, faceGizmoFrame } from '../three/gizmos.js'
import { lastId, makeID, setLastId } from './id.js'
import Expression from './expression.js'
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


export class Feature {
  constructor(document, booleanOutput, title, settings) {
    this.document = document
    this.title = title
    this.settings = settings
    this.error = null
    this.expressions = {}
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

    const component = tree.findChild(this.componentId)
    try {
      Object.entries(this.expressions).forEach(([key, expression]) => {
        this[key] = new Expression(expression, component.getParameters(), this.settings[key].type).getBase()
      })
    } catch(err) {
      this.error = { type: 'error', msg: 'Invalid parameter expression' }
      return
    }

    this.updateGeometry(tree, references)
  }

  updateGeometry(tree, references) {
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
    return this.isSettingVisible(setting) && ['profile', 'curve', 'axis', 'plane', 'face', 'edge', 'solid', 'point', 'componentRef', 'patternInput', 'joint'].some(type =>
      type == setting.type && (!setting.optional || includeOptionals)
    )
  }

  isPickerSetting(setting) {
    return ['profile', 'curve', 'axis', 'plane', 'face', 'edge', 'solid', 'point', 'componentRef', 'patternInput', 'joint'].includes(setting.type)
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
    Object.entries(this.settings)
      .filter(([key, setting]) => this[key] && ['profile', 'curve'].some(type => type == setting.type ) )
      .forEach(([key, _]) =>
        [this[key]()].flat().forEach(elem => comp.creator.itemsHidden[elem.getItem().sketch.id] = true )
      )
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
      visibilities: this.visibilities,
    }
  }

  static undump(dump, context) {
    const feature = new this(context.document)
    feature.id = dump.id
    feature.componentId = dump.componentId
    feature.componentOccurrenceId = dump.componentOccurrenceId || dump.componentId
    feature.visibilities = dump.visibilities
    feature.setValues(dump.values)
    feature.expressions = Object.fromEntries(
      Object.entries(dump.expressions || {}).filter(([key]) => feature.settings[key])
    )
    return feature
  }
}


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
    for(const key of ['faces', 'edges', 'face', 'tools']) {
      if(!references[key]) continue
      const items = [references[key]].flat().filter(item => this.isSolidVisible(tree, item.solid || item))
      scopedReferences[key] = this.settings[key].multi ? items : items[0]
      if(!this.settings[key].optional && !items.length) return
    }

    component.compound = original.subset(visible)
    super.updateGeometry(tree, scopedReferences)
    component.compound = component.compound.restoreSolids(solids, hidden)
  }
}


export class PatternFeature extends SolidFeature {
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
          radial: { title: 'Radial', icon: 'sun' },
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
      radialCount: {
        title: 'Count',
        type: 'integer',
        min: 1,
        when: feature => feature.patternType == 'radial',
      },
      radialStep: {
        title: 'Angle Step',
        type: 'angle',
        step: 1,
        allowNegative: true,
        gizmo: true,
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

  getGizmo() {
    if(this.patternType == 'radial') {
      const item = this.center().getItem()
      const center = item instanceof THREE.Matrix4 ?
        new THREE.Vector3().setFromMatrixPosition(item) : item.clone()
      return {
        radialStep: {
          type: 'angle', center, rotation: new THREE.Matrix4(), value: this.radialStep,
          cb: angle => {
            this.setExpression('radialStep', null)
            this.radialStep = angle
          },
        },
      }
    }

    const source = this.inputs().at(-1).getItem()
    const center = source.center()
    if(source instanceof Component) {
      const owner = this.document.top().findChild(this.componentId)
      center.applyMatrix4(baselineWorldTransform(owner).invert().multiply(baselineWorldTransform(source)))
    }
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
  static icon = 'code-branch'

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
    if(this.suppressUpdate || !this.isComplete()) return
    this.error = null
    const references = this.updateReferences(tree)
    if(this.error && this.error.type == 'error') return
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

    const selected = doc.selection.items
      .filter(item => item?.typename?.() == 'Component' && this.acceptsInput(item))
      .filter((item, index, items) => items.findIndex(other => other.id == item.id) == index)
      .map(component => component.componentReference())
    this.components = selected.length ? () => selected : null
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
        allowNegative: true,
      },
      rotationX: {
        title: 'Rotation X',
        type: 'angle',
        allowNegative: true,
      },
      rotationY: {
        title: 'Rotation Y',
        type: 'angle',
        allowNegative: true,
      },
    })

    this.base = null
    this.offset = 1.0
    this.rotationX = 0
    this.rotationY = 0
    this.rotationTransform = new THREE.Matrix4()
    this.rotationAppliedX = 0
    this.rotationAppliedY = 0

    this.planeId = makeID()
  }

  updateFeature(tree, references) {
    const plane = references.base.clone()
      .multiply(new THREE.Matrix4().makeTranslation(0, 0, this.offset))
      .multiply(this.getRotation())
    const helper = new PlaneHelper(this.componentId, plane, this.planeId)
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
    const base = this.base().getItem()
    return {
      distance: {
        center: new THREE.Vector3().setFromMatrixPosition(base),
        direction: normalFromMatrix(base),
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
    if(!this.isComplete()) {
      this.clearRotationGizmo()
      return
    }
    const component = this.document.top().findChild(this.componentOccurrenceId) ||
      this.document.top().findChild(this.componentId)
    const transform = worldTransform(component).multiply(this.base().getItem())
      .multiply(new THREE.Matrix4().makeTranslation(0, 0, this.offset))
      .multiply(this.getRotation())
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


export class PlaceFeature extends Feature {
  static icon = 'link'

  constructor(doc) {
    super(doc, false, 'Place', {
      filePath: {
        title: 'Document',
        type: 'file',
        filetype: '.cad',
        autoActivate: true,
      },
      variantOption: {
        title: 'Variant',
        type: 'enum',
        options: {},
        when: feature => !!feature.sourceVariant()?.options?.length,
      },
    })

    this.filePath = null
    this.cachedData = null
    this.variantOption = null
    this.importTransform = new THREE.Matrix4()
    this.sourceTree = null
    this.pendingData = null
    this.pendingTree = null
    this.activationError = null
    this.sourceRevision = null
    this.linkCheck = 0
    this.importGizmo = null
  }

  isComplete() {
    return !!this.filePath && !!this.cachedData && !!this.sourceTree
  }

  async chooseFile() {
    const linkCheck = ++this.linkCheck
    const file = await loadFile('.cad', 'text')
    const sourceTree = this.parseSource(file.data)
    if(linkCheck != this.linkCheck) return
    this.filePath = file.path
    this.cachedData = file.data
    this.sourceTree = sourceTree
    this.variantOption = this.normalizedVariantOption(sourceTree)
    this.sourceRevision = this.snapshotRevision(file.data)
    this.pendingData = null
    this.pendingTree = null
    this.activationError = null
  }

  fileLabel() {
    return this.filePath ? this.filePath.split(/[\\/]/).pop() : 'Choose file…'
  }

  async activate() {
    const linkCheck = ++this.linkCheck
    this.pendingData = null
    this.pendingTree = null
    this.activationError = null
    if(!this.filePath || !this.cachedData || !window.ipc) return

    try {
      const file = await loadFile('.cad', 'text', this.filePath)
      if(linkCheck != this.linkCheck) return
      if(file.data == this.cachedData) return
      this.pendingTree = this.parseSource(file.data, this.variantOption)
      this.pendingData = file.data
      this.activationError = {
        type: 'warning',
        msg: 'The imported document has changed. Confirm to update it.',
      }
    } catch(error) {
      if(linkCheck != this.linkCheck) return
      this.pendingData = null
      this.pendingTree = null
      this.activationError = {
        type: 'error',
        msg: 'The imported document could not be read. Choose a replacement file or cancel.',
      }
    }
  }

  commit() {
    if(!this.pendingData || !this.pendingTree) return
    this.cachedData = this.pendingData
    this.sourceTree = this.parseSource(this.cachedData, this.variantOption)
    this.variantOption = this.normalizedVariantOption(this.sourceTree, this.variantOption)
    this.sourceRevision = this.snapshotRevision(this.cachedData)
    this.pendingData = null
    this.pendingTree = null
    this.activationError = null
    this.document.timeline.invalidateFeature(this)
    this.document.regenerate()
  }

  execute(tree) {
    if(this.suppressUpdate || !this.isComplete()) return
    this.error = null
    this.updateSourceVariant()
    this.updateFeature(tree)
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
      this.error = error || { type: 'error', msg: 'The imported geometry could not be generated' }
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
    const geometryRevision = `${this.sourceRevision}/variant-${this.variantOption ?? 'default'}`
    const sourceSolids = sourceCompound.solids()
    compound.solids().forEach((solid, solidIndex) => {
      const sourceSolid = sourceSolids[solidIndex]
      const sourceFaces = sourceSolid?.faces() || []
      solid.faces().forEach((face, faceIndex) => {
        const sourceFace = sourceFaces[faceIndex]
        face.id = `${this.id}/import/face/${sourceFace?.id || faceIndex}`
        face.tessId = `${this.id}/${geometryRevision}/face/${sourceFace?.tessId || faceIndex}`
      })
      const sourceEdges = sourceSolid?.edges() || []
      solid.edges().forEach((edge, edgeIndex) => {
        const sourceEdge = sourceEdges[edgeIndex]
        edge.id = `${this.id}/import/edge/${sourceEdge?.id || edgeIndex}`
        edge.tessId = `${this.id}/${geometryRevision}/edge/${sourceEdge?.tessId || edgeIndex}`
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

  parseSource(data, variantOption=null) {
    const previousLastId = lastId
    try {
      const dump = Serialize.parse(data, { document: this.document, sketches: {} })
      if(!dump.timeline?.evaluate) throw new Error('Not a ProtoSolid document')

      const selected = this.normalizedVariantOption(dump.timeline.baseCompDef, variantOption)
      if(selected !== null) dump.timeline.baseCompDef.selectVariantOption(Number(selected), false)

      const sourceDocument = {
        timeline: dump.timeline,
        selection: { items: [] },
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

  getValues() {
    return {
      filePath: this.filePath,
      cachedData: this.cachedData,
      variantOption: this.variantOption,
      importTransform: this.importTransform,
    }
  }

  setValues(values) {
    const cachedDataChanged = values.cachedData !== this.cachedData
    const variantChanged = values.variantOption !== this.variantOption
    this.filePath = values.filePath || null
    this.cachedData = values.cachedData || null
    this.variantOption = values.variantOption ?? null
    this.sourceRevision = this.cachedData ? this.snapshotRevision(this.cachedData) : null
    this.importTransform = values.importTransform || new THREE.Matrix4()
    if(cachedDataChanged || variantChanged) {
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
    this.pendingData = null
    this.pendingTree = null
    this.activationError = null
    this.clearImportGizmo()
    super.dispose()
  }
}

Serialize.register(PlaceFeature, 'PlaceFeature')
// Load documents saved while this feature was temporarily named ImportFeature.
Serialize.register(PlaceFeature, 'ImportFeature')


export class ExtrudeFeature extends SolidFeature {
  static icon = 'layer-group'
  constructor(doc) {
    super(doc, true, 'Extrusion', {
      extent: {
        title: 'Extent',
        type: 'select',
        options: {
          distance: { title: 'Distance', icon: 'ruler-vertical' },
          face: { title: 'Up to face', icon: 'sign-in-alt' },
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
  }

  settingChanged(key, value) {
    if(key == 'extent' && value == 'through') this.operation = 'cut'
  }

  setValues(values) {
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
    } catch(err) {
      this.error = err?.msg ? err : { type: 'error', msg: 'Extrusion could not be created' }
    }
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
          if(this.side != 'both') this.side = side ? 'forward' : 'reverse'
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
  static icon = 'wave-square'
  constructor(document) {
    super(document, true, 'Revolution', {
      extent: {
        title: 'Extent',
        type: 'select',
        options: {
          angle: { title: 'Angle', icon: 'drafting-compass' },
          face: { title: 'Up to face', icon: 'sign-in-alt' },
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
    this.angle = 180.0
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
    } catch(err) {
      this.error = err?.msg ? err : { type: 'error', msg: 'Revolution could not be created' }
    }
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


export class SplitFeature extends SolidFeature {
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
      comp.compound = comp.compound.split(references.plane, this.keep, this.id)
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(SplitFeature, 'SplitFeature')


export class DraftFeature extends SolidFeature {
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
    } catch(err) { this.error = err || this.error }
  }

  updateGizmos() {
    if(this.isComplete()) {
      const center = this.faces().at(-1).getItem().center().clone()
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


export class FilletFeature extends SolidFeature {
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
        center: this.edges().at(-1).getItem().center(),
        direction: new THREE.Vector3(0,1,0),
        distance: this.radius,
        side: true,
        cb: (dist, _side) => {
          this.setExpression('radius', null)
          this.radius = dist
        },
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


export class ChamferFeature extends SolidFeature {
  static icon = 'screwdriver'
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
    })

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
      })
    } catch(err) { this.error = err || this.error }
  }

  getGizmo() {
    const edge = this.edges().at(-1).getItem()
    const faces = edge.connectedFaces().sort((a, b) => a.id.localeCompare(b.id))
    if(this.chamferType != 'equal' && this.flipSides) faces.reverse()
    const { center, directions: [first, second] } = edgeGizmoFrame(edge.tesselate(), faces.map(face => face.tesselate()))
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

  repair() {
    const tree = this.document.top(this.document.timeline.previousFeature(this))
    const remainingEdges = this.edges().filter(edgeRef => !edgeRef.update(tree) )
    this.edges = () => remainingEdges
    this.error = null
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
    } catch(err) { this.error = err || this.error }
  }

  getGizmo() {
    const tree = this.document.top(this.document.timeline.features.indexOf(this))
    const compound = tree.findChild(this.componentId).compound
    const openings = this.faces ? this.faces().map(ref => ref.getItem()) : []
    const retained = face => !openings.some(opening => opening.isSame(face))
    const visible = compound.solids().filter(solid => this.isSolidVisible(tree, solid))
    const face = openings.filter(face => visible.some(solid => solid.id == face.solid.id)).at(-1)?.connectedFaces().find(retained) ||
      visible.flatMap(solid => solid.faces()).find(retained)
    if(!face) return
    const { center, direction } = faceGizmoFrame(face.tesselate(), face.center())
    if(face.geom().Orientation_1() == window.oc.oc.TopAbs_Orientation.TopAbs_REVERSED) direction.negate()
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
  static icon = 'arrows-alt'
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
      comp.compound = comp.compound.offsetFaces(references.faces, distance, this.id)
    } catch(err) { this.error = err || this.error }
  }

  static undump(dump, context) {
    // Older documents stored the hollowing operation as OffsetFeature.
    if(Object.hasOwn(dump.values || {}, 'thickness')) return ShellFeature.undump(dump, context)
    return super.undump(dump, context)
  }
}

Serialize.register(OffsetFeature, 'OffsetFeature')


export class DissolveFeature extends SolidFeature {
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


export class BooleanFeature extends SolidFeature {
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
      if(localTools.length) comp.compound = comp.compound.removeShapes(localTools.map(input => input.tool))
      comp.compound = comp.compound.boolean(tool, this.operation)
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


export class MoveFeature extends SolidFeature {
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


export class ReplaceFeature extends SolidFeature {
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
