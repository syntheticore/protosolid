import * as THREE from 'three'

import { Timeline } from './timeline.js'
import { saveFile, loadFile } from '../utils.js'
import { Feature, ConvertBodyToComponentFeature, CreateComponentFeature, CreateComponentInstanceFeature, CreateSketchFeature, PoseFeature } from './features.js'
import { Selection } from '../selection.js'
import { Component } from './component.js'
import Emitter from '../emitter.js'
import Serialize from './serialize.js'
import { makeID, lastId, setLastId } from './id.js'
import Expression from './expression.js'
import preferences from '../preferences.js'
import { autoExportComponents } from './export.js'

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

export default class Document extends Emitter {
  constructor() {
    super()

    this.filePath = null
    this.timeline = new Timeline()
    this.materials = []

    this.activeComponent = this.top()
    this.activeSketch = null
    this.activeFeature = null
    this.activeSimulation = null
    this.activeSimulationPicker = false
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

    // this.on('component-changed', () => {
    //   this.hasChanges = true
    //   this.isFresh = false
    // })
  }

  top(at) {
    return this.timeline.tree(at)
  }

  createComponent(parent) {
    const feature = new CreateComponentFeature(this, parent.sourceId())
    this.addFeature(feature)
    const newComp = this.top().findChild(feature.id)
    this.activateComponent(newComp)
  }

  createComponentInstance(component) {
    if(!component.parent) return
    const feature = new CreateComponentInstanceFeature(this, component.sourceId(), component.parent.id)
    this.addFeature(feature)
  }

  convertBodyToComponent(component, body) {
    const feature = new ConvertBodyToComponentFeature(this)
    feature.componentId = component.sourceId()
    feature.body = () => body.reference()
    this.addFeature(feature)
  }

  keepPose() {
    if(!this.top().hasPoseChange()) return
    const feature = new PoseFeature(this)
    // The cache entry before this feature must retain the old design pose.
    this.top().resetPose()
    this.addFeature(feature)
  }

  activateComponent(comp) {
    if(comp) comp.creator.hidden = false
    this.activeComponent = comp
    this.selection.clear()
    this.activeSketch = null
  }

  getComponent(id) {
    return this.top().findChild(id)
  }

  addFeature(feature) {
    this.activateSimulation(null)
    this.activateFeature(null, true, false)
    if(!(feature instanceof PoseFeature)) this.top().resetPose()
    this.timeline.insertFeature(feature)
    this.reactivateActiveComponent()
  }

  moveMarker(i) {
    this.activateFeature(null, true, false)
    this.regenerate(i)
  }

  reorder(feature, other, after) {
    this.timeline.reorder(feature, other, after)
    this.regenerate()
  }

  regenerate(at) {
    // Runtime poses never belong to cached timeline states. A pose only
    // crosses a marker after keepPose() has turned it into a PoseFeature.
    this.top().resetPose()
    const oldTop = this.top()
    if(at !== undefined) {
      if(at instanceof Feature) {
        this.timeline.moveMarkerToFeature(at)
      } else {
        this.timeline.marker = at
      }
    }
    const compIds = this.timeline.evaluate()
    this.reactivateActiveComponent()
    this.selection.clear()
    this.emit('regenerated')
  }

  activateSimulation(simulation) {
    this.activeSimulation = simulation
    this.activeSimulationPicker = false
    if(simulation) this.selection.clear()
  }

  // Parameter values live on component definitions, which are shared by the
  // cached timeline trees. Invalidate the timeline before regenerating so
  // every feature that may consume an expression referencing a parameter is
  // evaluated again.
  regenerateParameters() {
    const activeFeature = this.activeFeature
    const activeSketch = this.activeSketch
    const firstFeature = this.timeline.features[0]
    if(firstFeature) this.timeline.invalidateFeature(firstFeature)
    this.regenerate()

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
        definition.variants?.forEach(variant => variant.options?.forEach(option => {
          option.parameters?.forEach(parameter => {
            parameter.value = rewrite(parameter.value)
          })
        }))
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
      this.activateComponent(updated)
    } else {
      this.reactivateActiveComponent(comp.parent)
    }
  }

  removeFeature(feature) {
    this.timeline.removeFeature(feature)
    this.regenerate()
  }

  activateFeature(feature, doReset, resetMarker) {
    if(feature) {
      this.activateSimulation(null)

      // Deactivate old active feature
      if(this.activeFeature) this.activateFeature(null, true, false)

      // Store marker, visibility and active component
      this.previousMarker = this.timeline.marker
      this.previousComponent = this.activeComponent
      const compIds = feature.modifiedComponents()
      const comps = compIds.map(id => this.getComponent(id) ).filter(Boolean)
      this.oldVisibility = {}
      comps.forEach(comp => this.oldVisibility[comp.id] = comp.creator.hidden )

      // Regenerate at feature position
      if(!this.timeline.isCurrentFeature(feature)) this.regenerate(feature)

      // Activate matching component
      this.activeComponent = this.getComponent(feature.componentId)

      // Activate sketch for sketch features
      if(feature.constructor === CreateSketchFeature) {
        this.activeSketch = feature.sketch

        // Store camera view
        this.previousActiveView = this.activeView
        this.previousDirtyView = this.dirtyView
        this.emit('look-at', this.activeSketch.workplane)
      }

      // Show involved sketches & store previous visibility
      feature.involvedSketches().forEach(sketch => {
        this.oldVisibility[sketch.id] = this.activeComponent.creator.itemsHidden[sketch.id]
        this.activeComponent.creator.itemsHidden[sketch.id] = false
      })

      // Make affected components visible
      compIds.forEach(id => this.getComponent(id).creator.hidden = false )
      this.activeFeature = feature

    } else if(this.activeFeature) {

      // Cancel FeatureBox & restore old feature state
      this.emit('deactivate-feature', this.activeFeature)

      // Restore previous state
      if(doReset && this.previousComponent) {

        // Restore sketch visiblities
        if(this.activeSketch) {
          this.activeComponent.creator.itemsHidden[this.activeSketch.id] = this.oldVisibility[this.activeSketch.id]
          this.activeView = this.previousActiveView
          this.dirtyView = this.previousDirtyView
          const view = this.dirtyView || this.previewView || this.activeView
          this.emit('force-view', view)
        } else {
          this.activeFeature.involvedSketches().forEach(sketch => {
            this.activeComponent.creator.itemsHidden[sketch.id] = this.oldVisibility[sketch.id]
          })
        }

        // Restore component visiblities
        this.activeFeature.modifiedComponents().forEach(id =>
          this.getComponent(id).creator.hidden = this.oldVisibility[id]
        )

        // Restore marker
        if(resetMarker && this.previousMarker != this.timeline.marker) this.regenerate(this.previousMarker)

        // Restore active component
        if(this.previousComponent) this.activateComponent(this.getComponent(this.previousComponent.id))
        this.previousComponent = null
      }
      this.activeFeature = null
      this.activeSketch = null
    }
    this.selection.clear()
  }

  activateSketch(sketch) {
    const feature = this.timeline.features.find(f => f.sketch == sketch )
    this.activateFeature(feature)
  }

  addView(view) {
    this.views.push(view)
    this.dirtyView = null
    this.activateView(view)
  }

  // User changed camera from viewport
  viewChanged(position, target) {
    this.dirtyView = { position: position.clone(), target: target.clone() }
    this.activeView = null
  }

  activateView(view) {
    this.activeView = view
    this.previewView = null
    this.dirtyView = null
    if(view.marker) this.moveMarker(view.marker)
    if(view.activeComponentId) this.activateComponent(this.getComponent(view.activeComponentId))
    if(view.visibilities) {
      Object.entries(view.visibilities).forEach(([compId, { compHidden, itemsHidden }]) => {
        const comp = this.getComponent(compId)
        comp.creator.hidden = compHidden
        comp.creator.itemsHidden = JSON.parse(JSON.stringify(itemsHidden))
      })
    }
  }

  async save(as) {
    const dump = {
      lastId,
      timeline: this.timeline,
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
    try {
      file = await loadFile('.cad', 'text', path)
    } catch(error) {
      if(error != 'canceled') alert(error)
      throw error
    }
    this.filePath = file.path
    this.isFresh = false
    const dump = Serialize.parse(file.data, { document: this, sketches: {} })
    setLastId(bigIntMax(BigInt(dump.lastId || 0), lastId))
    this.timeline = dump.timeline
    this.timeline.evaluate()
    this.activeComponent = this.top()
  }
}

const bigIntMax = (...args) => args.reduce((m, e) => e > m ? e : m)
