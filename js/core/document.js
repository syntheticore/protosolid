import * as THREE from 'three'

import { Timeline } from './timeline.js'
import { saveFile, loadFile } from '../utils.js'
import { Feature, CreateComponentFeature, CreateSketchFeature } from './features.js'
import { Selection } from '../selection.js'
import { Component } from './component.js'
import Emitter from '../emitter.js'
import Serialize from './serialize.js'

export default class Document extends Emitter {
  constructor() {
    super()

    this.filePath = null
    this.timeline = new Timeline()
    this.lastId = 1
    this.materials = []

    this.activeComponent = this.top()
    this.activeSketch = null
    this.activeFeature = null
    this.selection = new Selection()

    this.hasChanges = false
    this.isFresh = true

    this.views = [
      {
        id: this.lastId++,
        title: 'Top',
        position: new THREE.Vector3(0.0, 0.0, 90.0),
        target: new THREE.Vector3(),
      },
      {
        id: this.lastId++,
        title: 'Front',
        position: new THREE.Vector3(0.0, 90.0, 0.0),
        target: new THREE.Vector3(),
      },
      {
        id: this.lastId++,
        title: 'Side',
        position: new THREE.Vector3(90.0, 0.0, 0.0),
        target: new THREE.Vector3(),
      },
      {
        id: this.lastId++,
        title: 'Home',
        position: new THREE.Vector3(90.0, 90.0, 90.0),
        target: new THREE.Vector3(),
      },
    ]
    this.activeView = null
    this.previewView = null
    this.dirtyView = null

    this.on('component-changed', () => {
      this.hasChanges = true
      this.isFresh = false
    })
  }

  top(at) {
    return this.timeline.tree(at)
  }

  createComponent(parent) {
    const feature = new CreateComponentFeature(this, parent.id)
    this.addFeature(feature)
    const newComp = this.top().findChild(feature.id)
    this.activateComponent(newComp)
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
    this.activateFeature(null, true, false)
    this.timeline.insertFeature(feature)
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
    const oldTop = this.top()
    if(at !== undefined) {
      if(at instanceof Feature) {
        this.timeline.moveMarkerToFeature(at)
      } else {
        this.timeline.marker = at
      }
    }
    const compIds = this.timeline.evaluate()
    const top = this.top()
    console.log('TOP', top.children.length)
    compIds.forEach(id => {
      const oldComp = oldTop.findChild(id)
      if(oldComp) this.emit('component-deleted', oldComp)
      const newComp = this.getComponent(id)
      if(newComp) this.emit('component-changed', newComp)
    })
    this.reactivateActiveComponent()
    this.selection.clear()
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
        this.emit('sketch-changed', sketch)
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
            this.emit('sketch-changed', sketch)
          })
        }

        // Restore component visiblities
        this.activeFeature.modifiedComponents().forEach(id => this.getComponent(id).creator.hidden = this.oldVisibility[id] )

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

  deleteComponent(comp) {
    this.emit('component-deleted', comp)
    comp.parent.deleteComponent(comp)
    if(this.document.activeComponent.hasAncestor(comp)) {
      this.document.activeComponent = comp.parent
    }
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
        this.emit('component-changed', comp)
      })
    }
  }

  async save(as) {
    const dump = {
      lastId: this.lastId,
      timeline: this.timeline,
    }
    const json = Serialize.stringify(dump)
    try {
      this.filePath = await saveFile(json, 'cad', as ? null : this.filePath)
      this.hasChanges = false
    } catch(error) {
      if(error != 'canceled') alert(error)
      throw error
    }
  }

  async load(path) {
    let file
    try {
      file = await loadFile('cad', path)
    } catch(error) {
      if(error != 'canceled') alert(error)
      throw error
    }
    this.filePath = file.path
    this.isFresh = false
    const dump = Serialize.parse(file.data, { document: this, sketches: {} })
    Object.assign(this, dump)
    this.timeline.evaluate()
    this.activeComponent = this.top()
  }
}
