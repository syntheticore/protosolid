import * as THREE from 'three'

import { makeID } from './id.js'
import Serialize from './serialize.js'
import { Compound } from './geom3d.js'
import { CreateComponentFeature } from './features.js'
import { PlaneHelper, AxisHelper, PointHelper } from './helpers.js'
import { rotationFromNormal, rad } from './utils.js'
import materials from '../materials.js'
import { localTransform } from './assembly.js'
import { ComponentReference } from './references.js'


export class ComponentDefinition {
  constructor(title, color) {
    this.title = title
    this.hidden = false
    this.material = null
    this.cog = false
    this.sectionViews = []
    this.parameters = []
    this.variants = []
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
    def.parameters ||= []
    def.variants ||= []
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
  constructor(id, src, scale, hidden, x, y) {
    this.id = id || makeID()
    this.src = src
    this.scale = 1.0
    this.hidden = hidden
    this.x = x ?? 0
    this.y = y ?? 0
  }

  static undump(dump) { return new this(dump.id, dump.src, dump.scale, dump.hidden, dump.x, dump.y) }
}
Serialize.register(Canvas, 'Canvas')


export class Component {
  constructor(parent, id) {
    this.parent = parent
    this.id = id
    // designTransform is timeline-owned. transform is a transient pose delta
    // produced by assembly manipulation and is intentionally not serialized.
    this.designTransform = null
    this.transform = null
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
    this.assemblyJoints = []
  }

  typename() { return 'Component' }

  componentReference() { return new ComponentReference(this) }

  center() {
    return this.compound.center() || new THREE.Vector3()
  }

  deepClone(parent) {
    const clone = new Component(parent, this.id)
    Object.assign(clone, {
      designTransform: this.designTransform && this.designTransform.clone(),
      transform: null,
      compound: this.compound.cloneCached(),
      sketches: [...this.sketches],
      helpers: [...this.helpers],
      children: this.children.map(child => child.deepClone(clone) ),
      creator: this.creator,
      instanceOf: this.instanceOf,
      sourceOccurrence: this.sourceOccurrence,
      assemblyJoints: this.assemblyJoints && this.assemblyJoints.map(joint => ({
        ...joint,
        frameA: joint.frameA && joint.frameA.clone(),
        frameB: joint.frameB && joint.frameB.clone(),
        fixedWorld: joint.fixedWorld && joint.fixedWorld.clone(),
      })),
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
    const variants = this.creator.variants || []
    variants.forEach(variant => {
      const option = variant.options[variant.activeOption] || variant.options[0]
      if(option) params.push(...option.parameters)
    })
    const parentParams = this.parent ? this.parent.getParameters() : []
    parentParams.forEach(other => {
      const index = params.findIndex(own => own.name == other.name)
      if(index == -1) params.push(other)
    })
    return params
  }

  hasPoseChange() {
    return !!this.transform || this.children.some(child => child.hasPoseChange() )
  }

  resetPose() {
    this.transform = null
    this.children.forEach(child => child.resetPose() )
  }

  localTransform() {
    return localTransform(this)
  }

  sourceId() { return this.instanceOf || this.id }

  sourceItemId(itemId) {
    if(!this.instanceOf || !itemId.startsWith(this.id + '/')) return itemId
    return this.instanceOf + itemId.slice(this.id.length)
  }

  isItemHidden(itemId) {
    return !!this.creator.itemsHidden[this.sourceItemId(itemId)]
  }

  // Terminate component during serialization to avoid cyclic references
  dump() { return {} }
  static undump() { return null }
}
Serialize.register(Component, 'Component')


export function createComponentInstance(parent, source, id, designTransform) {
  const component = new Component(parent, id)
  component.instanceOf = source.sourceId()
  component.sourceOccurrence = source.id
  component.creator = occurrenceDefinition(source.creator)
  component.designTransform = designTransform ? designTransform.clone() : source.localTransform()
  component.compound = source.compound.cloneForComponent(component.id)
  parent.children.push(component)

  source.children.forEach(child => {
    createComponentInstance(component, child, `${id}/${child.id}`, child.localTransform())
  })
  return component
}

export function syncComponentInstances(tree) {
  tree.getChildren().filter(component => component.instanceOf).forEach(component => {
    const source = tree.findChild(component.instanceOf)
    const sourceOccurrence = tree.findChild(component.sourceOccurrence) || source
    if(!source || !sourceOccurrence || source == component) return
    component.compound = source.compound.cloneForComponent(component.id)
    syncInstanceChildren(component, sourceOccurrence)
  })
}

function syncInstanceChildren(instance, source) {
  source.children.forEach(sourceChild => {
    const existing = instance.children.find(child => child.sourceOccurrence == sourceChild.id)
    if(!existing) {
      createComponentInstance(instance, sourceChild, `${instance.id}/${sourceChild.id}`, sourceChild.localTransform())
    }
  })
}

function occurrenceDefinition(definition) {
  const occurrence = Object.create(definition)
  occurrence.hidden = false
  return occurrence
}
