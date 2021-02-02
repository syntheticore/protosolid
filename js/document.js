import * as THREE from 'three'

import { saveFile, loadFile } from './utils.js'
import Component from './component.js'

export default class Document {
  constructor(wasm) {
    this.lastId = 1

    const tree = new wasm.JsComponent()
    this.tree = new Component(tree, null, 'Main Assembly')

    this.title = 'Untitled Document'
    this.activeComponent = this.tree
    this.activeView = null
    this.activePose = null
    this.isPoseDirty = false
    this.isSetDirty = true
    this.filePath = null

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

  async save(as) {
    const json = JSON.stringify({
      tree: this.tree.serialize(),
    })
    console.log(json)
    try {
      this.filePath = await saveFile(json, 'alc', as ? null : this.filePath, this.title)
    } catch(error) {
      if(error != 'canceled') alert(error)
    }
  }

  async load(path) {
    let file
    try {
      file = await loadFile('alc', path)
    } catch(error) {
      if(error != 'canceled') alert(error)
      return
    }
    this.filePath = file.path
    const doc = JSON.parse(file.data)
    console.log(doc)
    this.tree.unserialize(doc.tree)
  }
}
