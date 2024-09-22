import * as THREE from 'three'

import { Compound, PlaneHelper, rotationFromNormal } from './kernel.js'

export default class Component {
  constructor(parent, id) {
    // this.real = realComponent
    this.parent = parent
    // this.document = doc

    // this.id = realComponent.id()
    // this.id = crypto.randomUUID()

    this.id = id

    // // Fixed component data that is not changed by timeline and shared between component instances
    // const componentData = doc.componentData()
    // this.UIData = componentData[this.id] || {
    //   title: parent ? "New Component" : "Main Assembly",
    //   hidden: false,
    //   material: null,
    //   cog: false,
    //   sectionViews: [],
    //   parameters: [],
    //   exportConfigs: [],
    //   itemsHidden: {},
    //   color: doc.makeColor(),
    // }
    // componentData[this.id] = this.UIData

    // Transient component data that is managed by timeline and differs for every timestep of this component
    this.transform = new THREE.Matrix4()
    this.compound = new Compound(this.id)
    // this.solids = []
    this.sketches = []
    this.helpers = [
      new PlaneHelper(rotationFromNormal(new THREE.Vector3(1.0, 0.0, 0.0))),
      new PlaneHelper(rotationFromNormal(new THREE.Vector3(0.0, 1.0, 0.0))),
      new PlaneHelper(rotationFromNormal(new THREE.Vector3(0.0, 0.0, 1.0))),
    ]
    this.children = []
    // this.update()

    // const cache = {
    //   faces: [],
    //   edges: [],
    //   regions: [],
    //   curves: [],
    // }
    // // Hide cache from Vue
    // this.cache = () => cache
  }

  typename() {
    return 'Component'
  }

  // update() {
  //   this.updateChildren()
  //   // this.updateSolids()
  //   this.updateSketches()
  //   this.updateHelpers()
  // }

  // updateChildren() {
  //   return
  //   this.freeChildren()
  //   this.children = this.real.children().map(realChild => new Component(realChild, this, this.document) )
  // }

  // updateSolids() {
  //   return
  //   this.freeSolids()
  //   this.solids = this.real.solids()
  //   this.solids.forEach(solid => solid.component = this )
  // }

  // updateSketches() {
  //   return
  //   this.freeSketches()
  //   this.sketches = this.real.sketches()
  //   this.sketches.forEach(sketch => sketch.component = this )
  // }

  // updateHelpers() {
  //   return
  //   this.freeHelpers()
  //   this.helpers = this.real.planes()
  //   this.helpers.forEach(helper => helper.component = this )
  // }

  // createChild(id) {
  //   const child = new Component(this, id)
  //   this.children.push(child)
  //   return child
  // }

  deepClone(parent) {
    const clone = new Component(parent, this.id)
    Object.assign(clone, {
      transform: this.transform.clone(),
      compound: this.compound.clone(),
      sketches: [...this.sketches],
      helpers: [...this.helpers],
      children: this.children.map(child => child.deepClone(clone) ),
      creator: this.creator,
    })
    return clone
  }

  // freeChildren() {
  //   // this.children.forEach(child => child.free() )
  //   this.children = []
  // }

  // freeSolids() {
  //   this.solids.forEach(solid => {
  //     solid.free()
  //     solid.deallocated = true
  //   })
  //   this.solids = []
  // }

  // freeSketches() {
  //   // this.sketches.forEach(sketch => sketch.free() )
  //   this.sketches = []
  // }

  // freeHelpers() {
  //   // this.helpers.forEach(helper => helper.free() )
  //   this.helpers = []
  // }

  // free(keepSelf) {
  //   this.freeChildren()
  //   this.freeSolids()
  //   this.freeSketches()
  //   this.freeHelpers()
  //   if(!this.real || keepSelf) return
  //   this.real.free()
  //   this.real = null
  // }

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

  // findSketchByFeature(id) {
  //   const sketch = this.sketches.find(sketch => sketch.feature_id() == id )
  //   if(sketch) return sketch
  //   for(const child of this.children) {
  //     const found = child.findSketchByFeature(id)
  //     if(found) return found
  //   }
  // }

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

  // serialize() {
  //   return {
  //     title: this.title,
  //     hidden: this.hidden,
  //     cog: this.cog,
  //     sectionViews: this.sectionViews,
  //     parameters: this.UIData.parameters,
  //     exportConfigs: this.exportConfigs,
  //     real: this.real.serialize(),
  //     children: this.children.map(child => child.serialize() ),
  //   }
  // }

  // unserialize(dump) {
  //   console.log(dump)
  //   this.title = dump.title
  //   this.hidden = dump.hidden
  //   this.cog = dump.cog
  //   this.sectionViews = dump.sectionViews || []
  //   this.UIData.parameters = dump.parameters || []
  //   this.exportConfigs = dump.exportConfigs || []
  //   this.real.unserialize(dump.real)
  //   dump.children.forEach(childDump => {
  //     let child = this.createComponent()
  //     child.unserialize(childDump)
  //   })
  // }
}
