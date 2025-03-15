import * as THREE from 'three'

import { makeID } from './../js/core/id.js'
import Serialize from './serialize.js'
import { Compound } from './geom3d.js'
import { CreateComponentFeature } from './features.js'
import { PlaneHelper, AxisHelper, PointHelper } from './helpers.js'
import { rotationFromNormal, rad } from './utils.js'
import materials from '../materials.js'


export class ComponentDefinition {
  constructor(title, color) {
    this.title = title
    this.hidden = false
    this.material = null
    this.cog = false
    this.sectionViews = []
    this.parameters = []
    this.exportConfigs = []
    this.canvases = []
    this.itemsHidden = {}
    this.color = color
    this.compMaterial = materials.surface.clone()
    this.compLineMaterial = materials.wire.clone()
    this.updateMaterials()
  }

  updateMaterials() {
    this.compMaterial.color.set(this.color)
    this.compLineMaterial.color.set(this.color)
  }

  dump() {
    return Object.assign({}, this, { compMaterial: undefined, compLineMaterial: undefined })
  }

  static undump(dump) {
    const def = Object.assign(new ComponentDefinition(), dump)
    def.updateMaterials()
    return def
  }
}
Serialize.register(ComponentDefinition, 'ComponentDefinition')


export class SectionView {
  constructor(id, orientation) {
    this.id = id || makeID()
    this.orientation = orientation || {
      x: 0,
      y: 0,
    }
    this.side = false
  }

  getTransform() {
    const euler = new THREE.Euler(rad(this.orientation.x), rad(this.orientation.y), 0)
    return new THREE.Matrix4().makeRotationFromEuler(euler)
  }

  static undump(dump) { return new this(dump.id, dump.orientation) }
}
Serialize.register(SectionView, 'SectionView')


export class Canvas {
  constructor(id, src, scale, hidden) {
    this.id = id || makeID()
    this.src = src
    this.scale = 1.0
    this.hidden = hidden
  }

  static undump(dump) { return new this(dump.id, dump.src, dump.scale, dump.hidden) }
}
Serialize.register(Canvas, 'Canvas')


export class Component {
  constructor(parent, id) {
    this.parent = parent
    this.id = id
    this.transform = new THREE.Matrix4()
    this.compound = new Compound(this.id)
    this.sketches = []
    this.helpers = [
      new PointHelper(this.id, rotationFromNormal(new THREE.Vector3(0.0, 0.0, 1.0)), this.id + '/origin'),

      new AxisHelper(this.id, rotationFromNormal(new THREE.Vector3(1.0, 0.0, 0.0)), this.id + '/X'),
      new AxisHelper(this.id, rotationFromNormal(new THREE.Vector3(0.0, 1.0, 0.0)), this.id + '/Y'),
      new AxisHelper(this.id, rotationFromNormal(new THREE.Vector3(0.0, 0.0, 1.0)), this.id + '/Z'),

      new PlaneHelper(this.id, rotationFromNormal(new THREE.Vector3(1.0, 0.0, 0.0)), this.id + '/YZ'),
      new PlaneHelper(this.id, rotationFromNormal(new THREE.Vector3(0.0, 1.0, 0.0)), this.id + '/XZ'),
      new PlaneHelper(this.id, rotationFromNormal(new THREE.Vector3(0.0, 0.0, 1.0)), this.id + '/XY'),
    ]
    this.children = []
  }

  typename() { return 'Component' }

  deepClone(parent) {
    const clone = new Component(parent, this.id)
    Object.assign(clone, {
      transform: this.transform.clone(),
      compound: this.compound.cloneCached(),
      sketches: [...this.sketches],
      helpers: [...this.helpers],
      children: this.children.map(child => child.deepClone(clone) ),
      creator: this.creator,
    })
    return clone
  }

  findChild(id) {
    if(this.id == id) return this
    for(const child of this.children) {
      const found = child.findChild(id)
      if(found) return found
    }
  }

  findSketch(id) {
    const sketch = this.sketches.find(sketch => sketch.id == id )
    if(sketch) return sketch
    for(const child of this.children) {
      const found = child.findSketch(id)
      if(found) return found
    }
  }

  getChildIds() {
    let ids = [this.id]
    for(const child of this.children) {
      ids = ids.concat(child.getChildIds())
    }
    return ids
  }

  getChildren() {
    let children = [this]
    for(const child of this.children) {
      children = children.concat(child.getChildren())
    }
    return children
  }

  getMaterial() {
    return this.creator.material || (this.parent && this.parent.getMaterial())
  }

  // Returns zero for empty components,
  // but undefined when weight could not be determined
  getWeight() {
    if(!this.compound.solids().length) return 0.0
    if(!this.creator.material) return
    try {
      let weight = this.children.reduce((acc, child) => {
        const childWeight = child.getWeight()
        if(childWeight === undefined) throw 'no weight'
        return acc + childWeight
      }, 0.0)
      return weight + this.compound.volume() * this.creator.material.density
    } catch(e) {
      if(e !== 'no weight') throw e
    }
  }

  getVolume() {
    return this.compound.volume() + this.children.reduce((acc, child) => { acc + child.getVolume() }, 0.0)
  }

  hasAncestor(parent) {
    if(this.id == parent.id) return true
    return this.parent && this.parent.hasAncestor(parent)
  }

  getParameters() {
    const params = [...this.creator.parameters]
    const parentParams = this.parent ? this.parent.getParameters() : []
    parentParams.forEach(other => {
      const index = params.findIndex(own => own.name == other.name)
      if(index == -1) params.push(other)
    })
    return params
  }

  // Terminate component during serialization to avoid cyclic references
  dump() { return {} }
  static undump() { return null }
}
Serialize.register(Component, 'Component')
