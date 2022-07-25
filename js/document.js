import * as THREE from 'three'

import { saveFile, loadFile } from './utils.js'
import Component from './component.js'

export default class Document {
  constructor() {
    this.lastId = 1

    const componentData = {}
    this.componentData = () => { return componentData }

    this.real = new window.alcWasm.JsDocument()
    this.tree = new Component(this.real.get_tree(), null, this.componentData())

    this.activeComponent = this.tree
    this.activeSketch = null
    this.activeView = null
    this.activePose = null
    this.isPoseDirty = false
    this.isSetDirty = true
    this.filePath = null
    this.hasChanges = false
    this.isFresh = true

    this.features = this.real.get_features().map(feature => new Feature(this.document, feature) )

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

    this.poses = [
      { title: 'Base', id: this.lastId++ },
    ]

    this.sets = [
      { title: 'Filet 14', id: this.lastId++ },
      { title: 'Extrude 2', id: this.lastId++ },
    ]
  }

  insertFeature(feature, index) {
    this.features.splice(index, 0, feature);
  }

  removeFeature(feature) {
    this.features = this.features.filter(f => f != feature )
    feature.real.remove()
    feature.real.free()
  }

  async save(as) {
    const json = JSON.stringify({
      tree: this.tree.serialize(),
    })
    console.log(json)
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
    const doc = JSON.parse(file.data)
    console.log(doc)
    this.tree.unserialize(doc.tree)
  }

  dispose() {
    this.tree.free()
  }
}
