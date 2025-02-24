import * as THREE from 'three'

import { Component, ComponentDefinition } from './component.js'
import { Sketch } from './sketch.js'
import { Compound } from './geom3d.js'
import { Reference } from './references.js'
import { PlaneHelper } from './helpers.js'
import { normalFromMatrix, ocCatch } from './utils.js'
import Serialize from './serialize.js'
import { LengthGizmo, AngleGizmo } from '../three/gizmos.js'
import { makeID } from './id.js'


export class Feature {
  constructor(document, booleanOutput, title, settings) {
    this.document = document
    this.title = title
    this.settings = settings
    this.error = null
    this.componentId = this.document.activeComponent.id
    this.id = makeID()

    if(!booleanOutput) return
    this.operation = 'join'
    this.settings.operation = {
      title: 'Operation',
      type: 'select',
      options: {
        join: {
          title: 'Join',
          icon: 'magnet',
        },
        cut: {
          title: 'Cut',
          icon: 'clone',
        },
        intersect: {
          title: 'Intersect',
          icon: 'box',
        },
        create: {
          title: 'Create',
          icon: 'edit',
        },
      },
    }
  }

  typename() {
    return this.title
  }

  execute(tree) {
    if(this.suppressUpdate || !this.isComplete()) return

    this.error = null
    const references = this.updateReferences(tree)

    if(this.error && this.error.type == 'error') return

    this.updateFeature(tree, references)

    try {
      const comp = tree.findChild(this.componentId)
      comp.compound = comp.compound.repair()
    } catch(err) { this.error = (this.error && this.error.type == 'error') ? this.error : err }
  }

  updateReferences(tree) {
    const results = {}
    const errors = []

    Object.keys(this.settings).forEach(key => {
      if(!this[key] || !this.needsPicker(key, true)) return

      const setting = this.settings[key]
      const refs = setting.multi ? this[key]() : [this[key]()]

      let settingError

      const items = refs.map(ref => {
        const clone = ref.clone()
        const error = clone.update(tree)
        const severe = (error && error.type == 'error')
        settingError = (severe ? error : (settingError || error))
        if(severe) return
        if(!error) ref.update(tree) // Update intact references without user consent, such that their center etc. can be calculated
        return clone.getItem()
      }).filter(Boolean)

      if(items.length && settingError) settingError.type = 'warning'

      if(settingError) errors.push(settingError)
      results[key] = setting.multi ? items : items[0]
    })

    const severe = errors.filter(error => error.type == 'error' )
    this.error = severe.length ? severe[0] : errors[0]

    return results
  }

  needsPicker(setting, includeOptionals) {
    setting = this.settings[setting]
    return ['profile', 'curve', 'axis', 'plane', 'face', 'edge', 'solid'].some(type =>
      type == setting.type && (!setting.optional || includeOptionals)
    )
  }

  isComplete() {
    return Object.keys(this.settings)
      .filter(key => this.needsPicker(key) )
      .every(key => this[key] && (!this.settings[key].multi || this[key]().length) )
  }

  preview() { return this.previewBody }

  confirm() {
    const comp = this.document.top().findChild(this.componentId)
    const sketches = Object.entries(this.settings)
      .filter(([key, setting]) => this[key] && ['profile', 'curve'].some(type => type == setting.type ) )
      .forEach(([key, _]) =>
        [this[key]()].flat().forEach(elem => comp.creator.itemsHidden[elem.getItem().sketch.id] = true )
      )
    this.document.emit('component-changed', comp) //XXX should not re-tesselate, only update sketch visibility
  }

  updateFeature(tree, references) {}

  getGizmo() {}

  updateGizmos() {
    if(this.isComplete()) {
      let gizmo = this.getGizmo()
      if(!gizmo) return
      gizmo = gizmo.distance
      if(!gizmo) return
      if(this.distanceGizmo) {
        this.distanceGizmo().set(gizmo.distance, gizmo.side)
      } else {
        const distanceGizmo = new LengthGizmo(gizmo.center, gizmo.direction, gizmo.distance, gizmo.side, gizmo.cb)
        this.distanceGizmo = () => distanceGizmo
        window.alcRenderer.addGizmo(distanceGizmo)
      }
    } else {
      if(this.distanceGizmo) window.alcRenderer.removeGizmo(this.distanceGizmo())
      this.distanceGizmo = null
    }
  }

  modifiedComponents() { return [this.componentId] }

  repair() {}

  involvedSketches() {
    return Object.keys(this.settings).flatMap(key => {
      if(this.settings[key].type != 'profile') return []
      return this[key] ? this[key]().map(ref => ref.getItem().sketch ) : []
    })
  }

  getValues() {
    const values = {}
    Object.keys(this.settings).forEach(key => {
      values[key] = (this[key] && this.needsPicker(key, true) ? this[key]() : this[key])
    })
    return values
  }

  setValues(values) {
    Object.keys(values).forEach(key => {
      const value = values[key]
      this[key] = (value && this.needsPicker(key, true) ? () => value : value)
    })
  }

  dispose() {
    if(this.distanceGizmo) window.alcRenderer.removeGizmo(this.distanceGizmo())
    this.distanceGizmo = null
  }

  dump() {
    return {
      id: this.id,
      componentId: this.componentId,
      values: this.getValues(),
    }
  }

  static undump(dump, context) {
    const feature = new this(context.document)
    feature.id = dump.id
    feature.componentId = dump.componentId
    feature.setValues(dump.values)
    return feature
  }
}


export class CreateComponentFeature extends Feature {
  static icon = 'box'
  constructor(doc, parentId) {
    super(doc, false, 'New Component', {
      parent: {
        title: 'Parent Component',
        type: 'component',
      },
      title: {
        title: 'Title',
        type: 'string',
      },
    })

    this.parent = parentId
    this.title = "New Component"

    this.definition = new ComponentDefinition(doc.timeline.makeColor())
  }

  updateFeature(tree) {
    let parent = tree.findChild(this.parent)
    const comp = new Component(parent, this.id)
    this.definition.parent = this.parent
    this.definition.title = this.title
    comp.creator = this.definition
    parent.children.push(comp)
  }

  modifiedComponents() {
    return [this.parent]
  }

  dump() {
    return {
      ...super.dump(),
      definition: this.definition,
    }
  }

  static undump(dump, context) {
    const feature = super.undump(dump, context)
    Object.assign(feature, dump)
    delete feature.values
    return feature
  }
}

Serialize.register(CreateComponentFeature, 'CreateComponentFeature')


export class CreateSketchFeature extends Feature {
  static icon = 'solar-panel'
  constructor(doc) {
    super(doc, false, 'Sketch', {
      plane: {
        title: 'Plane',
        type: 'plane',
        autoConfirm: true,
      },
    })

    this.plane = null

    this.sketch = new Sketch()
    this.sketch.creator = this
  }

  updateFeature(tree, references) {
    const plane = references.plane
    this.sketch.workplane = plane
    tree.findChild(this.componentId).sketches.push(this.sketch)
    this.sketch.solve(tree)
  }

  involvedSketches() {
    return [this.sketch]
  }

  confirm() {
    this.document.activateFeature(this)
  }

  dump() {
    return {
      ...super.dump(),
      sketch: this.sketch,
    }
  }

  static undump(dump, context) {
    const feature = super.undump(dump, context)
    feature.sketch = dump.sketch
    feature.sketch.creator = this
    return feature
  }
}

Serialize.register(CreateSketchFeature, 'CreateSketchFeature')


export class PlaneFeature extends Feature {
  static icon = 'map'
  constructor(doc) {
    super(doc, false, 'Plane', {
      base: {
        title: 'Base',
        type: 'plane',
      },
      offset: {
        title: 'Offset',
        type: 'length',
      },
    })

    this.base = null
    this.offset = 1.0

    this.planeId = makeID()
  }

  updateFeature(tree, references) {
    const vec = normalFromMatrix(references.base).multiplyScalar(this.offset)
    const pos = new THREE.Vector3().setFromMatrixPosition(references.base)
    const plane = references.base.clone().setPosition(pos.add(vec))
    const helper = new PlaneHelper(this.componentId, plane, this.planeId)
    tree.findChild(this.componentId).helpers.push(helper)
  }

  dump() {
    return {
      ...super.dump(),
      planeId: this.planeId,
    }
  }

  static undump(dump, context) {
    return Object.assign(super.undump(dump, context), {
      planeId: dump.planeId,
    })
  }
}

Serialize.register(PlaneFeature, 'PlaneFeature')


export class ExtrudeFeature extends Feature {
  static icon = 'layer-group'
  constructor(doc) {
    super(doc, true, 'Extrusion', {
      profiles: {
        title: 'Profiles',
        type: 'profile',
        multi: true,
      },
      axis: {
        title: '(Axis)',
        type: 'axis',
        optional: true,
      },
      distance: {
        title: 'Distance',
        type: 'length',
      },
      // limit: {
      //   type: 'OR',
      //   settings: [
      //     { title: 'Distance', type: 'length' },
      //     { title: '(Up to)', type: 'surface|point' },
      //   ]
      // },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-up', 'caret-down']
      },
    })

    this.profiles = null
    this.axis = null
    this.distance = 1.0
    this.side = true
  }

  updateFeature(tree, references) {
    const distance = this.distance * (this.side ? 1 : -1)
    const comp = tree.findChild(this.componentId)

    let tool = new Compound(this.componentId)
    references.profiles.forEach(profile => {
      try {
        const extrusion = profile.extrude(this.componentId, distance, this.id)
        tool = tool.boolean(extrusion, 'join')
      } catch(err) { this.error = err || this.error }
    })
    try {
      comp.compound = comp.compound.boolean(tool, this.operation)
    } catch(err) { this.error = err || this.error }

    this.previewBody = tool
  }

  getGizmo() {
    const profile = this.profiles()[0].getItem()
    return {
      distance: {
        center: profile.center(),
        direction: (this.axis && this.axis()) || profile.normal(),
        distance: this.distance,
        side: this.side,
        cb: (dist, side) => {
          this.distance = dist
          this.side = side
        },
      }
    }
  }

  repair() {
    const newProfiles = this.profiles().filter(profileRef => {
      const error = profileRef.update(this.document.top())
      return !error || error.type == 'warning'
    })
    this.profiles = () => newProfiles
    this.error = null
  }
}

Serialize.register(ExtrudeFeature, 'ExtrudeFeature')


export class RevolveFeature extends Feature {
  static icon = 'wave-square'
  constructor(document) {
    super(document, true, 'Revolution', {
      profiles: {
        title: 'Profiles',
        type: 'profile',
        multi: true,
      },
      axis: {
        title: 'Axis',
        type: 'axis',
      },
      angle: {
        title: 'Angle',
        type: 'length',
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-right', 'caret-left'],
      },
    })

    this.profiles = null
    this.axis = null
    this.angle = 1.0
    this.side = true
  }

  updateFeature(tree, references) {
    const angle = this.angle * (this.side ? 1 : -1)

    let tool = new Compound(this.componentId)
    references.profiles.forEach(profile => {
      try {
        const revolution = profile.revolve(this.componentId, references.axis, angle, this.id)
        tool = tool.boolean(revolution, 'join')
      } catch(err) { this.error = err || this.error }
    })

    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.boolean(tool, this.operation)
    } catch(err) { this.error = err || this.error }

    this.previewBody = tool
  }
}

Serialize.register(RevolveFeature, 'RevolveFeature')


export class SplitFeature extends Feature {
  static icon = 'divide'
  constructor(document) {
    super(document, false, 'Split', {
      plane: {
        title: 'Tool',
        type: 'plane',
      },
      // bodies: {
      //   title: 'Bodies',
      //   type: 'solid',
      //   multi: true,
      // },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-right', 'caret-left'],
      },
    })

    this.plane = null
    this.side = true
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.split(references.plane, this.side, this.id)
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(SplitFeature, 'SplitFeature')


export class DraftFeature extends Feature {
  static icon = 'clone'
  constructor(document) {
    super(document, true, 'Draft', {
      ref_plane: {
        title: 'Reference',
        type: 'plane',
      },
      faces: {
        title: 'Faces',
        type: 'face',
        multi: true,
      },
      angle: {
        title: 'Angle',
        type: 'angle',
      },
    })

    this.ref_plane = null
    this.faces = null
    this.angle = 0.0
  }

  updateFeature() {
    const list = new window.alcWasm.JsFaceRefList()
    this.faces().forEach(face => {
      list.push(face)
    })
    this.real.draft(list, this.ref_plane(), this.angle)
  }

  updateGizmos() {
    if(this.isComplete()) {
      if(this.angleGizmo) {
        this.angleGizmo.set(this.angle)
      } else {
        const center = new THREE.Vector3()
        this.angleGizmo = new AngleGizmo(center, new THREE.Euler(), this.angle, (angle) => {
          this.angle = angle
        })
        window.alcRenderer.addGizmo(this.angleGizmo)
      }
    } else {
      window.alcRenderer.removeGizmo(this.angleGizmo)
      this.angleGizmo = null
    }
  }

  // confirm() {
  //   // Refetch faces in case they've been repaired
  //   this.faces().forEach(faceRef => faceRef.free())
  //   const faces = this.real.face_refs()
  //   this.faces = () => faces
  // }

  dispose() {
    super.dispose()
    window.alcRenderer.removeGizmo(this.angleGizmo)
    this.angleGizmo = null
  }
}

Serialize.register(DraftFeature, 'DraftFeature')


export class SweepFeature extends Feature {
  static icon = 'route'
  constructor(document) {
    super(document, true, 'Sweep', {
      profile: {
        title: 'Profile',
        type: 'profile',
      },
      rail: {
        title: 'Rail',
        type: 'curve',
      },
      bounds: {
        title: 'Bounds',
        type: 'bounds',
      },
    })

    this.profile = null
    this.rail = null
    this.bounds = [0.0, 1.0]
  }
}

Serialize.register(SweepFeature, 'SweepFeature')


export class FilletFeature extends Feature {
  static icon = 'scroll'
  constructor(doc) {
    super(doc, false, 'Fillet', {
      edges: {
        title: 'Edges',
        type: 'edge',
        multi: true,
      },
      radius: {
        title: 'Radius',
        type: 'length',
        gizmo: true,
      },
    })

    this.edges = null
    this.radius = 1.0
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.fillet(references.edges, this.radius)
    } catch(err) { this.error = err || this.error }
  }

  getGizmo() {
    return {
      distance: {
        center: this.edges()[0].getItem().center(),
        direction: new THREE.Vector3(0,1,0),
        distance: this.radius,
        side: true,
        cb: (dist, _side) => { this.radius = dist },
      }
    }
  }

  repair() {
    const tree = this.document.top(this.document.timeline.previousFeature(this))
    const remainingEdges = this.edges().filter(edgeRef => !edgeRef.update(tree) )
    this.edges = () => remainingEdges
    this.error = null
  }
}

Serialize.register(FilletFeature, 'FilletFeature')


export class OffsetFeature extends Feature {
  static icon = 'magnet'
  constructor(doc) {
    super(doc, false, 'Shell', {
      faces: {
        title: '(Openings)',
        type: 'face',
        multi: true,
        optional: true,
      },
      thickness: {
        title: 'Thickness',
        type: 'length',
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-up', 'caret-down']
      },
    })

    this.faces = null
    this.thickness = 1.0
    this.side = false
  }

  updateFeature(tree, references) {
    const thickness = this.thickness * (this.side ? 1 : -1)
    const comp = tree.findChild(this.componentId)

    try {
      comp.compound = comp.compound.offset(references.faces, thickness)
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(OffsetFeature, 'OffsetFeature')


export class BooleanFeature extends Feature {
  static icon = 'boxes'
  constructor(doc) {
    super(doc, true, 'Boolean', {
      tools: {
        title: 'Tools',
        type: 'solid',
        multi: true,
      },
      keepTools: {
        title: 'Keep tools',
        type: 'bool',
        icons: ['hand-paper', 'trash'],
      },
    })

    this.tools = null
    this.keepTools = false
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      const tools = references.tools.map(tool => tool.toCompound() )
      const tool = tools.reduce((acc, tool) => acc.boolean(tool, 'join') )
      comp.compound = comp.compound.removeSolids(references.tools).boolean(tool, this.operation)
      if(this.keepTools) comp.compound = comp.compound.merge(tools)
      this.previewBody = tool
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(BooleanFeature, 'BooleanFeature')


export class RemoveSolidsFeature extends Feature {
  static icon = 'trash'
  constructor(doc) {
    super(doc, false, 'Remove', {
      solids: {
        title: 'Solids',
        type: 'solid',
        multi: true,
      },
    })

    this.solids = null
  }

  updateFeature(tree, references) {
    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.removeSolids(references.solids)
    } catch(err) { this.error = err || this.error }
  }
}

Serialize.register(RemoveSolidsFeature, 'RemoveSolidsFeature')


// export class MaterialFeature extends Feature {
//   static icon = 'volleyball-ball'
//   constructor(component) {
//     super(component, false, {
//       material: {
//         title: 'Material Presets',
//         type: 'material',
//       },
//     })

//     this.material = null
//   }

//   isComplete() {
//     return !!this.material
//   }

//   preview() {
//     if(this.oldMaterial === undefined) this.oldMaterial = this.component.material
//     this.component.material = this.material
//   }

//   confirm() {
//     this.oldMaterial = undefined
//   }

//   dispose() {
//     super.dispose()
//     if(this.oldMaterial === undefined) return
//     this.component.material = this.oldMaterial
//   }
// }
