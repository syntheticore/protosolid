import * as THREE from 'three'
import { nextTick } from 'vue'

import { Timeline } from './timeline.js'
import { saveFile, loadFile } from '../utils.js'
import { Feature, SolidFeature, ConvertBodyToComponentFeature, CreateComponentFeature, CreateComponentInstanceFeature, CreateSketchFeature, PoseFeature } from './features.js'
import { Selection } from '../selection.js'
import { Component, SectionView } from './component.js'
import Emitter from '../emitter.js'
import Serialize from './serialize.js'
import { KernelMismatchError, parseDocument } from './document-format.js'
import { makeID, lastId, setLastId } from './id.js'
import Expression from './expression.js'
import preferences from '../preferences.js'
import { autoExportComponents } from './export.js'

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

let nextCalculationId = 0

export default class Document extends Emitter {
  constructor(bus) {
    super()

    this.bus = bus
    this.regenerationQueue = Promise.resolve()
    this.pendingRegenerations = 0
    this.filePath = null
    this.timeline = new Timeline()
    this.materials = []
    this.treeletsExpandedByDefault = true
    this.treeletExpansion = new Map()

    this.activeComponent = this.top()
    this.activeSketch = null
    this.activeFeature = null
    this.creatingFeature = null
    this.activeInteractivePanel = null
    this.activeSimulation = null
    this.activeSimulationPicker = false
    this.activeExportConfig = null
    this.activeExportComponentId = null
    this.selection = new Selection()

    this.hasChanges = false
    this.isFresh = true

    this.views = [
      {
        id: makeID(),
        title: 'Top',
        position: new THREE.Vector3(0.0, 90.0, 0.0),
        target: new THREE.Vector3(),
        system: true,
      },
      {
        id: makeID(),
        title: 'Front',
        position: new THREE.Vector3(0.0, 0.0, 90.0),
        target: new THREE.Vector3(),
        system: true,
      },
      {
        id: makeID(),
        title: 'Side',
        position: new THREE.Vector3(90.0, 0.0, 0.0),
        target: new THREE.Vector3(),
        system: true,
      },
      {
        id: makeID(),
        title: 'Home',
        position: new THREE.Vector3(90.0, 90.0, 90.0),
        target: new THREE.Vector3(),
        system: true,
      },
    ]
    this.activeView = null
    this.previewView = null
    this.dirtyView = null
    this.currentCameraView = cloneCameraView(this.views[3])
    this.viewPreviewSnapshot = null

    // this.on('component-changed', () => {
    //   this.hasChanges = true
    //   this.isFresh = false
    // })
  }

  top(at) {
    return this.timeline.tree(at)
  }

  get isRegenerating() {
    return this.pendingRegenerations > 0
  }

  isTreeletExpanded(key, defaultValue = this.treeletsExpandedByDefault) {
    if(!this.treeletExpansion.has(key)) {
      this.treeletExpansion.set(key, defaultValue)
    }
    return this.treeletExpansion.get(key)
  }

  setTreeletExpanded(key, expanded) {
    this.treeletExpansion.set(key, expanded)
  }

  markChanged() {
    this.hasChanges = true
    this.isFresh = false
  }

  async createComponent(parent) {
    if(parent.readOnly) return
    const feature = new CreateComponentFeature(this, parent.sourceId())
    await this.addFeature(feature)
    const newComp = this.top().findChild(feature.id)
    this.activateComponent(newComp)
  }

  async createComponentInstance(component) {
    if(!component.parent || component.readOnly) return
    const feature = new CreateComponentInstanceFeature(this, component.sourceId(), component.parent.id)
    await this.addFeature(feature)
  }

  async convertBodyToComponent(component, body) {
    if(component.readOnly) return
    const feature = new ConvertBodyToComponentFeature(this)
    feature.componentId = component.sourceId()
    feature.body = () => body.reference()
    await this.addFeature(feature)
  }

  keepPose() {
    if(!this.top().hasPoseChange()) return
    const feature = new PoseFeature(this)
    // The cache entry before this feature must retain the old design pose.
    this.top().resetPose()
    this.addFeature(feature)
  }

  activateComponent(comp, closeInteractivePanel = true) {
    if(comp?.readOnly) return
    if(closeInteractivePanel) this.activateInteractivePanel(null)
    if(comp) comp.creator.hidden = false
    this.activeComponent = comp
    this.selection.clear()
    this.activeSketch = null
  }

  getComponent(id) {
    return this.top().findChild(id)
  }

  async addFeature(feature) {
    this.activateInteractivePanel(null)
    await this.activateFeature(null, true, false)
    if(!(feature instanceof PoseFeature)) this.top().resetPose()
    feature.captureVisibility()
    this.timeline.insertFeature(feature, false)
    await this.regenerate()
  }

  async moveMarker(i) {
    const creatingIndex = this.timeline.features.indexOf(this.creatingFeature)
    if(creatingIndex >= 0 && creatingIndex < i) i--
    await this.cancelFeature(false)
    return this.regenerate(i)
  }

  async cancelFeature(resetMarker = true) {
    this.bus.emit('close-feature', this)
    // Closing the creation box removes its temporary feature during unmount.
    await nextTick()
    await this.regenerationQueue
    await this.activateFeature(null, true, resetMarker)
    return this.regenerationQueue
  }

  reorder(feature, other, after) {
    this.timeline.reorder(feature, other, after)
    return this.regenerate()
  }

  regenerate(at) {
    // Runtime poses never belong to cached timeline states. A pose only
    // crosses a marker after keepPose() has turned it into a PoseFeature.
    this.top().resetPose()
    if(at !== undefined) {
      const previousMarker = this.timeline.marker
      if(at instanceof Feature) {
        this.timeline.moveMarkerToFeature(at)
      } else {
        this.timeline.marker = at
      }
      if(this.timeline.marker !== previousMarker) this.timeline.change_version++
    }
    // The running pass retries invalidated work and reads the latest values
    // after yielding. Share it instead of queuing a pass for every drag event.
    if(this.isRegenerating) return this.regenerationQueue
    this.pendingRegenerations++
    const run = async () => {
      const calculationId = ++nextCalculationId
      try {
        const onFeature = (feature, index, total) => {
          if(feature.id === this.activeFeature?.id || feature.id === this.creatingFeature?.id) {
            this.bus.emit('calculation-finished', calculationId)
            return
          }
          this.bus.emit('calculation', {
            id: calculationId,
            title: 'Calculating',
            detail: feature.title,
            current: index + 1,
            total,
          })
        }
        // Superseded passes must finish the latest edit before their callers
        // refresh errors, activate components, or confirm a picked feature.
        while(await this.timeline.evaluateAsync(onFeature) === null) {}
        this.reactivateActiveComponent()
        this.selection.clear()
        this.emit('regenerated')
      } finally {
        this.pendingRegenerations--
        this.bus.emit('calculation-finished', calculationId)
      }
    }
    this.regenerationQueue = this.regenerationQueue.catch(() => {}).then(run)
    return this.regenerationQueue
  }

  activateInteractivePanel(key) {
    // Remove the previous panel's viewport controls before opening the next.
    this.activeInteractivePanel = null
    this.activeSimulation = null
    this.activeSimulationPicker = false
    this.activeExportConfig = null
    this.activeExportComponentId = null
    if(key) this.selection.clear()
    this.activeInteractivePanel = key
  }

  activateSimulation(simulation) {
    this.activateInteractivePanel(simulation ? 'simulation' : null)
    this.activeSimulation = simulation
  }

  activateExport(config, component) {
    if(config || this.activeExportConfig) {
      this.activateInteractivePanel(config ? 'export' : null)
    }
    this.activeExportConfig = config
    this.activeExportComponentId = config && component ? component.id : null
  }

  // Parameter values live on component definitions, which are shared by the
  // cached timeline trees. Invalidate the timeline before regenerating so
  // every feature that may consume an expression referencing a parameter is
  // evaluated again.
  async regenerateParameters() {
    const activeFeature = this.activeFeature
    const activeSketch = this.activeSketch
    const firstFeature = this.timeline.features[0]
    if(firstFeature) this.timeline.invalidateFeature(firstFeature)
    await this.regenerate()

    // regenerate() reactivates the active component, which intentionally
    // clears sketch-editing state. Parameter edits must not close an active
    // sketch or switch the toolbar back to non-sketch tools.
    if(activeFeature) {
      this.activeFeature = activeFeature
      this.activeComponent = this.getComponent(activeFeature.componentOccurrenceId) ||
        this.getComponent(activeFeature.componentId) || this.activeComponent
      this.activeSketch = activeFeature.sketch || activeSketch
    }

    // Profiles are cached by the sketch proxies. Mark all current sketches
    // dirty so closed regions are rebuilt from the regenerated geometry.
    this.top().getChildren().flatMap(component => component.sketches)
      .forEach(sketch => { sketch.profileUpdateNeeded = true })
    this.hasChanges = true
    this.isFresh = false
  }

  renameParameterReferences(oldName, newName) {
    if(oldName == newName) return
    this.rewriteParameterReferences(oldName, newName)
  }

  removeParameterReferences(parameter, component) {
    let replacement = parameter.value
    try {
      replacement = new Expression(parameter.value, component.getParameters())
        .format(preferences.preferredUnit)
    } catch(error) {}
    this.rewriteParameterReferences(parameter.name, replacement)
  }

  rewriteParameterReferences(oldName, replacement) {
    if(!oldName || oldName == replacement) return
    const identifier = new RegExp(`\\b${escapeRegExp(oldName)}\\b`, 'g')
    const rewrite = expression => typeof expression == 'string' ?
      expression.replace(identifier, replacement) : expression

    const definitions = [this.timeline.baseCompDef]
    this.timeline.features.forEach(feature => {
      if(feature.definition) definitions.push(feature.definition)
    })
    this.top().getChildren().forEach(component => definitions.push(component.creator))

    definitions
      .filter((definition, index, all) => definition && all.indexOf(definition) == index)
      .forEach(definition => {
        definition.parameters?.forEach(parameter => {
          parameter.value = rewrite(parameter.value)
        })
        definition.variant?.options?.forEach(option => {
          Object.keys(option.values || {}).forEach(key => {
            option.values[key] = rewrite(option.values[key])
          })
        })
      })

    this.timeline.features.forEach(feature => {
      Object.keys(feature.expressions || {}).forEach(key => {
        feature.expressions[key] = rewrite(feature.expressions[key])
      })
    })

    const sketches = new Set()
    this.timeline.features.forEach(feature => {
      if(feature.sketch) sketches.add(feature.sketch)
    })
    this.top().getChildren().forEach(component =>
      component.sketches.forEach(sketch => sketches.add(sketch))
    )
    sketches.forEach(sketch => sketch.constraints.forEach(constraint => {
      constraint.expression = rewrite(constraint.expression)
    }))
  }

  reactivateActiveComponent(comp = this.activeComponent) {
    const updated = this.getComponent(comp.id)
    if(updated) {
      const hidden = updated.creator.hidden
      this.activateComponent(updated, false)
      updated.creator.hidden = hidden
    } else {
      this.reactivateActiveComponent(comp.parent)
    }
  }

  removeFeature(feature) {
    this.timeline.removeFeature(feature)
    return this.regenerate()
  }

  async activateFeature(feature, doReset, resetMarker) {
    if(feature) {
      this.activateInteractivePanel(null)

      await this.cancelFeature(false)

      // Store marker, visibility and active component
      this.previousMarker = this.timeline.marker
      this.previousComponent = this.activeComponent
      const compIds = feature.modifiedComponents()
      this.oldVisibility = this.top().getChildren().map(comp => ({
        id: comp.id,
        definition: comp.creator,
        hidden: comp.creator.hidden,
        itemsHidden: { ...comp.creator.itemsHidden },
        ownsItemsHidden: Object.hasOwn(comp.creator, 'itemsHidden'),
      }))

      const applyVisibilities = () => {
        this.applyPartVisibilities(feature.visibilities)
        // Reordering can bring sketches into a feature's history that were
        // absent from its saved visibility snapshot. Preserve sketch visibility
        // from before editing, then reveal only this feature's input sketches.
        this.oldVisibility.forEach(({ id, itemsHidden }) => {
          const component = this.getComponent(id)
          if(!component) return
          component.sketches.forEach(sketch => {
            component.creator.itemsHidden[sketch.id] = itemsHidden[sketch.id]
          })
        })
      }
      applyVisibilities()

      // Regenerate at feature position
      if(!this.timeline.isCurrentFeature(feature)) await this.regenerate(feature)
      applyVisibilities()

      // Activate matching component
      this.activeComponent = this.getComponent(feature.componentId)

      // Activate sketch for sketch features
      if(feature.constructor === CreateSketchFeature) {
        this.activeSketch = feature.sketch
        this.activeSketch.startHistory()

        // Store camera view
        this.previousActiveView = this.activeView
        this.previousDirtyView = this.dirtyView
        this.emit('look-at', this.activeSketch.workplane)
      }

      // Show involved sketches while editing.
      feature.involvedSketches().forEach(sketch => {
        this.activeComponent.creator.itemsHidden[sketch.id] = false
      })

      // Legacy and non-solid features do not have a saved visibility state.
      if(!feature.visibilities) compIds.forEach(id => this.getComponent(id).creator.hidden = false )
      this.activeFeature = feature

    } else if(this.activeFeature) {

      // Cancel FeatureBox & restore old feature state
      this.emit('deactivate-feature', this.activeFeature)

      // Restore previous state
      if(doReset && this.previousComponent) {

        // Restore the camera after sketch editing.
        if(this.activeSketch) {
          this.activeView = this.previousActiveView
          this.dirtyView = this.previousDirtyView
          const view = this.dirtyView || this.previewView || this.activeView
          this.emit('force-view', view)
        }

        // Definitions are shared by cached trees, including future components.
        const restoreVisibility = () => this.oldVisibility.forEach(({ id, definition, hidden, itemsHidden, ownsItemsHidden }) => {
          const restoreDefinition = definition => {
            definition.hidden = hidden
            // Instances must resume inheriting their source's visibility map.
            if(ownsItemsHidden) definition.itemsHidden = { ...itemsHidden }
            else delete definition.itemsHidden
          }
          restoreDefinition(definition)
          const current = this.getComponent(id)
          if(current && current.creator !== definition) {
            restoreDefinition(current.creator)
          }
        })
        restoreVisibility()

        // Regeneration yields to rendering. Leave sketch mode before exposing
        // the restored timeline, so it cannot section the finished model using
        // the sketch that has just been closed.
        this.activeSketch = null
        this.activeFeature = null

        // Restore marker
        if(resetMarker && this.previousMarker != this.timeline.marker) await this.regenerate(this.previousMarker)

        // Restore active component
        if(this.previousComponent) this.activateComponent(this.getComponent(this.previousComponent.id))
        restoreVisibility()
        this.previousComponent = null
      }
      this.activeFeature = null
      this.activeSketch = null
    }
    this.selection.clear()
  }

  async updateFeatureVisibility() {
    this.markChanged()
    const feature = this.activeFeature || this.creatingFeature
    if(!(feature instanceof SolidFeature)) return
    feature.captureVisibility()
    this.timeline.invalidateFeature(feature)
    await this.regenerate()
    this.emit('feature-visibility-changed', feature)
  }

  activateSketch(sketch) {
    const feature = this.timeline.features.find(f => f.sketch == sketch )
    this.activateFeature(feature)
  }

  addView(view) {
    this.views.push(view)
    this.dirtyView = null
    this.markChanged()
    this.activateView(view)
  }

  removeView(index) {
    this.clearViewPreview()
    const [view] = this.views.splice(index, 1)
    if(!view) return
    if(this.activeView?.id == view.id) {
      this.activeView = null
      this.dirtyView = cloneCameraView(this.currentCameraView)
    }
    if(this.previewView?.id == view.id) this.previewView = null
    this.markChanged()
  }

  // User changed camera from viewport
  viewChanged(position, target) {
    this.dirtyView = { position: position.clone(), target: target.clone() }
    this.currentCameraView = cloneCameraView(this.dirtyView)
    this.activeView = null
  }

  cameraViewApplied(position, target) {
    this.currentCameraView = {
      position: position.clone(),
      target: target.clone(),
    }
  }

  previewSavedView(view) {
    this.restoreViewPreview()
    this.viewPreviewSnapshot = this.captureVisualViewState()
    this.previewView = view
    this.applyPartVisibilities(view.visibilities)
    this.applySectionVisibilities(view.sections)
  }

  clearViewPreview() {
    this.restoreViewPreview()
    this.previewView = this.dirtyView
  }

  restoreViewPreview() {
    if(!this.viewPreviewSnapshot) return
    this.applyPartVisibilities(this.viewPreviewSnapshot.visibilities)
    this.applySectionVisibilities(this.viewPreviewSnapshot.sections)
    this.viewPreviewSnapshot = null
  }

  captureVisualViewState() {
    const visibilities = {}
    const sections = {}
    this.top().getChildren().forEach(comp => {
      visibilities[comp.id] = {
        compHidden: comp.creator.hidden,
        itemsHidden: JSON.parse(JSON.stringify(comp.creator.itemsHidden)),
      }
      sections[comp.id] = comp.creator.sectionViews.map(section => ({
        id: section.id,
        hidden: section.hidden ?? false,
      }))
    })
    return { visibilities, sections }
  }

  applyPartVisibilities(visibilities) {
    if(!visibilities) return
    Object.entries(visibilities).forEach(([compId, { compHidden, itemsHidden }]) => {
      const comp = this.getComponent(compId)
      if(!comp) return
      comp.creator.hidden = compHidden
      comp.creator.itemsHidden = JSON.parse(JSON.stringify(itemsHidden))
    })
  }

  applySectionVisibilities(sections) {
    if(!sections) return
    Object.entries(sections).forEach(([compId, snapshots]) => {
      const comp = this.getComponent(compId)
      if(!comp) return
      const hiddenById = new Map(snapshots.map(section => [section.id, section.hidden ?? false]))
      comp.creator.sectionViews.forEach(section => {
        if(hiddenById.has(section.id)) section.hidden = hiddenById.get(section.id)
      })
    })
  }

  async activateView(view) {
    this.restoreViewPreview()
    this.activeView = view
    this.previewView = null
    this.dirtyView = null
    if(view.marker !== undefined && view.marker !== null) await this.moveMarker(view.marker)
    if(view.activeComponentId) {
      const component = this.getComponent(view.activeComponentId)
      if(component) this.activateComponent(component)
    }
    this.applyPartVisibilities(view.visibilities)
    if(view.sections) {
      Object.entries(view.sections).forEach(([compId, snapshots]) => {
        const comp = this.getComponent(compId)
        if(!comp) return
        const existing = new Map(comp.creator.sectionViews.map(section => [section.id, section]))
        comp.creator.sectionViews = snapshots.map(snapshot => {
          const section = existing.get(snapshot.id) || new SectionView(snapshot.id)
          section.orientation = { ...snapshot.orientation }
          section.position = { ...snapshot.position }
          section.side = snapshot.side ?? false
          section.hidden = snapshot.hidden ?? false
          return section
        })
      })
    }
    if(view.expansions) {
      this.treeletsExpandedByDefault = view.expansionDefault ?? this.treeletsExpandedByDefault
      this.treeletExpansion.clear()
      Object.entries(view.expansions).forEach(([key, expanded]) => {
        this.treeletExpansion.set(key, expanded)
      })
    }
  }

  async save(as) {
    await this.cancelFeature()
    const dump = {
      kernel: window.kernel.modeling.id,
      lastId,
      timeline: this.timeline,
      views: this.views.map(view => {
        const { editing, ...storedView } = view
        return storedView
      }),
    }
    const json = Serialize.stringify(dump)
    try {
      this.filePath = await saveFile(json, 'cad', as ? null : this.filePath)
      this.hasChanges = false
      await autoExportComponents(this.top())
    } catch(error) {
      if(error != 'canceled') alert(error)
      throw error
    }
  }

  async load(path) {
    let file
    let dump
    const materials = []
    try {
      file = await loadFile('.cad', 'text', path)
      dump = parseDocument(file.data, { document: this, sketches: {}, materials })
    } catch(error) {
      if(error != 'canceled') {
        console.error(error)
        alert(error instanceof KernelMismatchError ? error.message : 'The document could not be opened.')
      }
      throw error
    }
    const calculationId = ++nextCalculationId
    this.bus.emit('calculation', {
      id: calculationId,
      title: 'Opening document',
      detail: file.path?.split(/[\\/]/).pop() || 'Document',
    })
    await yieldToBrowser()
    try {
      this.filePath = file.path
      this.isFresh = false
      this.treeletsExpandedByDefault = false
      setLastId(bigIntMax(BigInt(dump.lastId || 0), lastId))
      this.timeline = dump.timeline
      this.materials = materials
      await this.timeline.evaluateAsync((feature, index, total) => {
        this.bus.emit('calculation', {
          id: calculationId,
          title: 'Calculating',
          detail: feature.title,
          current: index + 1,
          total,
        })
      })
      this.activeComponent = this.top()
      if(Array.isArray(dump.views)) this.views = dump.views
      this.activeView = null
      this.previewView = null
      this.dirtyView = null
      this.viewPreviewSnapshot = null
      const homeView = this.views.find(view => view.system && view.title == 'Home') ||
        this.views.find(view => view.position && view.target)
      if(homeView) this.currentCameraView = cloneCameraView(homeView)
    } finally {
      this.bus.emit('calculation-finished', calculationId)
    }
  }
}

const bigIntMax = (...args) => args.reduce((m, e) => e > m ? e : m)

function cloneCameraView(view) {
  if(!view?.position || !view?.target) return null
  return {
    position: view.position.clone(),
    target: view.target.clone(),
  }
}

function yieldToBrowser() {
  return new Promise(resolve => {
    requestAnimationFrame(() => setTimeout(resolve, 0))
  })
}
