import * as THREE from 'three'

import { saveFile, loadFile } from './utils.js'
import { deserialize } from './features.js'
import Component from './component.js'

window.registry ||= new FinalizationRegistry(real => real.free() )

export default class Document {
  constructor() {
    this.lastId = 1

    const componentData = {}
    this.componentData = () => { return componentData }
    this.colors = []

    this.real = new window.alcWasm.JsDocument()
    this.tree = new Component(this.real.tree(), null, this)

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
    window.registry.register(feature, feature.real)
    this.features.splice(index, 0, feature)
  }

  removeFeature(feature) {
    this.features = this.features.filter(f => f != feature )
    feature.real.remove()
    // feature.real.free()
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
    const tree = this.real.final_tree()
    const comp = this.getFutureComp(compId, tree)
    return this.getChildIds(comp)
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
    this.tree = new Component(this.real.tree(), null, this)
    this.features = this.real.features().map((feature, i) => deserialize(this, feature, data.features[i]) )
    this.activeComponent = this.tree
  }

  dispose() {
    this.tree.free()
  }
}
