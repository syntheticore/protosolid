import * as THREE from 'three'
import Component from './component.js'

export default class Document {
  constructor(wasm) {
    this.lastId = 1

    const tree = new wasm.JsComponent()
    this.tree = new Component(tree, null, 'Main Assembly')

    this.title = 'Untitled Document'
    this.activeComponent = null
    this.activeView = null
    this.activePose = null
    this.isPoseDirty = false
    this.isSetDirty = true

    this.views = [
      {
        id: this.lastId++,
        title: 'Top',
        position: new THREE.Vector3(0.0, 0.0, 90.0),
        target: new THREE.Vector3(0.0, 0.0, 0.0),
      },
      {
        id: this.lastId++,
        title: 'Front',
        position: new THREE.Vector3(0.0, 90.0, 0.0),
        target: new THREE.Vector3(0.0, 0.0, 0.0),
      },
      {
        id: this.lastId++,
        title: 'Side',
        position: new THREE.Vector3(90.0, 0.0, 0.0),
        target: new THREE.Vector3(0.0, 0.0, 0.0),
      },
      {
        id: this.lastId++,
        title: 'Home',
        position: new THREE.Vector3(90.0, 90.0, 90.0),
        target: new THREE.Vector3(0.0, 0.0, 0.0),
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
}
