import * as THREE from 'three'

import Serialize from './serialize.js'
import { Compound } from './geom3d.js'
import { PlaneHelper } from './helpers.js'
import { rotationFromNormal } from './utils.js'

export default class Component {
  constructor(parent, id) {
    this.parent = parent
    this.id = id
    this.transform = new THREE.Matrix4()
    this.compound = new Compound(this.id)
    this.sketches = []
    this.helpers = [
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

  getMaterial() {
    return this.material || (this.parent && this.parent.getMaterial())
  }

  // Returns zero for empty components,
  // but undefined when weight could not be determined
  getWeight() {
    if(this.compound.solids().length && !this.material) return
    try {
      let weight = this.children.reduce((acc, child) => {
        const childWeight = child.getWeight()
        if(childWeight === undefined) throw 'no weight'
        return acc + childWeight
      }, 0.0)
      return weight + (this.material ? this.getVolume() * this.material.density : 0.0)
    } catch(e) {
      if(e !== 'no weight') throw e
    }
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
