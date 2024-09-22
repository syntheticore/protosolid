import * as THREE from 'three'

import { Timeline } from './kernel.js'
import { saveFile, loadFile } from '../utils.js'
import { deserialize, Feature, CreateComponentFeature, CreateSketchFeature } from './features.js'
import { Selection } from '../selection.js'
import Component from './component.js'
import Emitter from '../emitter.js'

export default class Document extends Emitter {
  constructor() {
    super()

    this.colors = []
    this.filePath = null
    this.timeline = new Timeline()

    this.lastId = 1

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

  top() {
    return this.timeline.tree()
  }

  createComponent(parent) {
    const feature = new CreateComponentFeature(this, parent)
    this.addFeature(feature)
    const newComp = this.top().findChild(feature.component.id)
    this.activateComponent(newComp)
  }

  activateComponent(comp) {
    if(comp) comp.creator.hidden = false
    this.activeComponent = comp //XXX activeComponent should never be null. Search for parent that exists
    this.selection = this.selection.clear()
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
    this.selection = new Selection()
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

      // Activate sketch for sketch features
      if(feature.constructor === CreateSketchFeature) {
        this.activeSketch = feature.sketch

        // Store camera view
        this.previousActiveView = this.activeView
        this.previousDirtyView = this.dirtyView

        // Activate matching component
        this.activeComponent = this.getComponent(feature.componentId)

        // Make sketch visible & store previous visibility
        this.previousSketchVisibility = this.activeComponent.creator.itemsHidden[this.activeSketch.id]
        this.activeComponent.creator.itemsHidden[this.activeSketch.id] = false
      }

      // Make affected components visible
      compIds.forEach(id => this.getComponent(id).creator.hidden = false )
      this.activeFeature = feature

    } else if(this.activeFeature) {

      // Cancel FeatureBox & restore old feature state
      this.emit('deactivate-feature', this.activeFeature)
      this.activeFeature = null

      // Restore previous state
      if(doReset && this.previousComponent) {

        // Restore sketch visiblity
        if(this.previousSketchVisibility !== null && this.activeSketch) {
          this.activeComponent.creator.itemsHidden[this.activeSketch.id] = this.previousSketchVisibility
          this.activeView = this.previousActiveView
          this.dirtyView = this.previousDirtyView
        }
        this.previousSketchVisibility = null

        // Restore component visiblity
        Object.keys(this.oldVisibility).forEach(id => this.getComponent(id).creator.hidden = this.oldVisibility[id] )

        // Restore marker
        if(resetMarker && this.previousMarker != this.timeline.marker) this.regenerate(this.previousMarker)

        // Restore active component
        if(this.previousComponent) this.activateComponent(this.getComponent(this.previousComponent.id))
        this.previousComponent = null
      }
      this.activeSketch = null
    }
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

  // deleteSolid(solid) {
  //   solid.remove()
  //   this.selection = this.selection.delete(solid)
  //   this.emit('component-changed', solid.component)
  // }

  createView(title) {
    const newView = {
      id: this.lastId++,
      title: title || 'Custom ' + this.lastId,
      position: this.dirtyView.position.clone(),
      target: this.dirtyView.target.clone(),
    }
    this.views.push(newView)
    this.dirtyView = null
    this.activateView(newView)
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
  }

  makeColor() {
    const testColors = [...Array(100)].map(() => {
      const color = {
        h: Math.random() * 360,
        s: 45 + Math.random() * 20,
        l: 55 + Math.random() * 10,
      }
      const diffs = this.colors.map(c => this.colorDiff(c, color) )
      const worstDiff = Math.min(...diffs)
      return { color, diff: worstDiff }
    })
    testColors.sort((a, b) => Math.sign(b.diff - a.diff) )
    const color = testColors[0].color
    this.colors.push(color)
    return `hsl(${color.h}, ${color.s}%, ${color.l}%)`
  }

  colorDiff(c1, c2) {
    let hue = Math.abs(c1.h - c2.h)
    hue = hue > 180 ? 360 - hue : hue
    return hue + Math.abs(c1.s - c2.s) + Math.abs(c1.l - c2.l)
  }

  async save(as) {
    const json = JSON.stringify({
      // componentData: this.componentData(),
      features: this.features.map(feature => feature.serialize() ),
      real: this.real.serialize(),
    })
    try {
      this.filePath = await saveFile(json, 'alc', as ? null : this.filePath)
      this.hasChanges = false
    } catch(error) {
      if(error != 'canceled') alert(error)
      throw error
    }
  }

  async load(path) {
    let file
    try {
      file = await loadFile('alc', path)
    } catch(error) {
      if(error != 'canceled') alert(error)
      throw error
    }
    this.filePath = file.path
    this.isFresh = false
    const data = JSON.parse(file.data)
    // this.componentData = () => data.componentData
    this.real.deserialize(data.real)
    this.tree = new Component(this.real.tree(), null, this)
    this.features = this.real.features().map((feature, i) => deserialize(this, feature, data.features[i]) )
    this.activeComponent = this.tree
  }
}
