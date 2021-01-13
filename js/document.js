import * as THREE from 'three'

import Component from './component.js'

let lastId = 1

export default class Document {
  constructor(wasm) {
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
        id: lastId++,
        title: 'Top',
        position: new THREE.Vector3(0.0, 0.0, 9.0),
        target: new THREE.Vector3(0.0, 0.0, 0.0),
      },
      {
        id: lastId++,
        title: 'Front',
        position: new THREE.Vector3(0.0, 9.0, 0.0),
        target: new THREE.Vector3(0.0, 0.0, 0.0),
      },
      {
        id: lastId++,
        title: 'Side',
        position: new THREE.Vector3(9.0, 0.0, 0.0),
        target: new THREE.Vector3(0.0, 0.0, 0.0),
      },
      {
        id: lastId++,
        title: 'Perspective',
        position: new THREE.Vector3(9.0, 9.0, 9.0),
        target: new THREE.Vector3(0.0, 0.0, 0.0),
      },
    ]

    this.poses = [
      { title: 'Base', id: lastId++ },
    ]

    this.sets = [
      { title: 'Filet 14', id: lastId++ },
      { title: 'Extrude 2', id: lastId++ },
    ]
  }
}
