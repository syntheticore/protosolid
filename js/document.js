import * as THREE from 'three'

import { saveFile, loadFile } from './utils.js'
import { deserialize } from './features.js'
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
    this.filePath = null
    this.hasChanges = false
    this.isFresh = true

    this.features = []

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
  }

  insertFeature(feature, index) {
    this.features.splice(index, 0, feature);
  }

  removeFeature(feature) {
    this.features = this.features.filter(f => f != feature )
    feature.real.remove()
    feature.real.free()
  }

  getChildIds(comp) {
    let ids = [comp.id]
    for(const child of comp.children) {
      ids = ids.concat(this.getChildIds(child))
    }
    return ids
  }

  getFutureComp(id, tree) {
    if(id == tree.id) return tree
    for(const child of tree.children) {
      const self = this.getFutureComp(id, child)
      if(self) return self
    }
  }

  getFutureChildIds(compId) {
    const tree = this.real.get_final_tree()
    const comp = this.getFutureComp(compId, tree)
    return this.getChildIds(comp)
  }

  async save(as) {
    const json = JSON.stringify({
      componentData: this.componentData(),
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
    this.componentData = () => data.componentData
    this.real.deserialize(data.real)
    this.tree = new Component(this.real.get_tree(), null, this.componentData())
    this.features = this.real.get_features().map((feature, i) => deserialize(this, feature, data.features[i]) )
    this.activeComponent = this.tree
  }

  dispose() {
    this.tree.free()
  }
}
