import * as THREE from 'three'

import Component from './component.js'
import { Sketch, Compound, EdgeReference } from './kernel.js'
import { LengthGizmo, AngleGizmo } from '../three/gizmos.js'


export class Feature {
  constructor(document, booleanOutput, title, settings) {
    this.document = document
    // this.real = real || new window.alcWasm.JsFeature(document.real)
    this.title = title
    // this.icon = icon
    this.settings = settings
    this.error = null
    this.componentId = this.document.activeComponent.id

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
    this.updateFeature(tree)
  }

  preview() {}
  confirm() {}
  isComplete() {}
  updateGizmos() {}
  modifiedComponents() { return [this.componentId] }
  repair() {}

  // update() {
  //   this.updateGizmos()
  //   this.execute()
  // }

  getValues() {
    const values = {}
    Object.keys(this.settings).forEach(key => values[key] = this[key] )
    return values
  }

  setValues(values) {
    Object.keys(values).forEach(key => this[key] = values[key] )
  }

  serialize() {
    return {
      title: this.title,
      values: this.getValues(),
    }
  }

  dispose() {}
}

export function deserialize(document, real, dump) {
  const Klass = {
    'New Component': CreateComponentFeature,
    'Sketch': CreateSketchFeature,
    'Extrusion': ExtrudeFeature,
    'Revolution': RevolveFeature,
    'Draft': DraftFeature,
    'Sweep': SweepFeature,
  }[dump.title]
  const feature = new Klass(document, real)
  feature.setValues(dump.values)
  feature.id = real.id()
  return feature
}


export class CreateComponentFeature extends Feature {
  static icon = 'box'
  constructor(doc, parent) {
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

    const parentId = parent.id
    this.parent = () => parentId
    // this.newComponentId = crypto.randomUUID()

    this.component = new Component(parent, crypto.randomUUID())
    this.component.creator = this
    console.log('CREATED', this.component.id)

    this.title = "New Component"
    this.hidden = false
    this.material = null
    this.cog = false
    this.sectionViews = []
    this.parameters = []
    this.exportConfigs = []
    this.itemsHidden = {}
    this.color = doc.makeColor()

    const cache = {
      faces: [],
      edges: [],
      regions: [],
      curves: [],
      helpers: [],
    }
    // Hide cache from Vue
    this.cache = () => cache
  }

  isComplete() {
    return !!this.parent
  }

  updateFeature(tree) {
    // let parent = this.document.top().findChild(this.parent())
    let parent = tree.findChild(this.parent())
    // const child = comp.createChild(this.newComponentId)
    parent.children.push(this.component)
  }

  modifiedComponents() {
    return [this.parent()]
  }
}


export class CreateSketchFeature extends Feature {
  static icon = 'edit'
  constructor(doc) {
    super(doc, false, 'Sketch', {
      plane: {
        title: 'Plane',
        type: 'plane',
        autoConfirm: true,
      },
    })

    this.plane = null
    // this.sketch = new Sketch(this.document.activeComponent.id)
    this.sketch = new Sketch()
    this.sketch.creator = this
  }

  isComplete() {
    return !!this.plane
  }

  updateFeature(tree) {
    // let comp = this.document.getComponent(this.sketch.componentId)
    const planeRef = this.plane()
    this.error = planeRef.update(tree)

    if(this.error) {
      this.error = { type: 'error', msg: 'Sketch plane was lost' }
      return
    }

    const plane = planeRef.getPlane()
    this.sketch.workplane = plane
    tree.findChild(this.componentId).sketches.push(this.sketch)
  }

  confirm() {
    // this.document.activeSketch = this.document.activeComponent.sketches.slice(-1)[0]
    // this.document.timeline.marker++
    this.document.activateFeature(this)
  }

  // modifiedComponents() {
  //   return [this.sketch.componentId]
  // }
}


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

  isComplete() {
    return this.profiles && this.profiles().length
  }

  updateFeature(tree) {
    const distance = this.distance * (this.side ? 1 : -1)

    this.error = null

    const profiles = this.profiles().map(profileRef => {
      const ref = profileRef.clone()
      const error = ref.update(tree)
      const profile = ref.getItem()
      this.error = (error && error.type == 'error' ? error : this.error || error)
      if(error && error.type == 'error') return
      return profile
    }).filter(Boolean)

    if(profiles.length && this.error) this.error.type = 'warning'

    let tool = new Compound(this.componentId)
    profiles.forEach(profile => {
      try {
        const extrusion = profile.extrude(this.componentId, distance)
        tool = tool.boolean(extrusion, 'join')
      } catch(err) { this.error = err || this.error }
    })

    const comp = tree.findChild(this.componentId)
    try {
      comp.compound = comp.compound.boolean(tool, this.operation)
    } catch(err) { this.error = err || this.error }

    return tool
  }

  updateGizmos() {
    if(this.isComplete()) {
      if(this.lengthGizmo) {
        this.lengthGizmo().set(this.distance, this.side)
      } else {
        const profile = this.profiles()[0].getItem()
        const center = profile.center()
        const axis = this.axis && this.axis()
        const direction = axis || profile.normal()
        const lengthGizmo = new LengthGizmo(center, direction, this.distance, this.side, (dist, side) => {
          this.distance = dist
          this.side = side
        })
        this.lengthGizmo = () => lengthGizmo
        window.alcRenderer.addGizmo(this.lengthGizmo())
      }
    } else {
      if(this.lengthGizmo) window.alcRenderer.removeGizmo(this.lengthGizmo())
      this.lengthGizmo = null
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

  dispose() {
    super.dispose()
    if(this.lengthGizmo) window.alcRenderer.removeGizmo(this.lengthGizmo())
    this.lengthGizmo = null
  }
}


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
        icons: ['caret-right', 'caret-left']
      },
    })

    this.profiles = null
    this.axis = null
    this.angle = 1.0
    this.side = true
  }

  isComplete() {
    return this.axis && this.profiles && this.profiles().length
  }

  updateFeature() {
    const list = new window.alcWasm.JsProfileRefList()
    this.profiles().forEach(profile => {
      list.push(profile)
    })
    const comp_ref = this.document.activeComponent.id
    const angle = this.angle * (this.side ? 1 : -1)
    this.real.revolution(comp_ref, list, this.axis(), angle, this.operation)
  }

  updateGizmos() {
    if(this.isComplete()) {
      if(this.lengthGizmo) {
        this.lengthGizmo.set(this.angle, this.side)
      } else {
        const item = this.profiles()[0].item()
        const center = vecToThree(item.center())
        const axis = this.axis && this.axis()
        const direction = vecToThree(item.normal())
        item.free()
        this.lengthGizmo = new LengthGizmo(center, direction, this.side, this.angle, (dist, side) => {
          this.angle = dist
          this.side = side
        })
        // this.angleGizmo = new AngleGizmo(center, new THREE.Euler(), this.angle, (angle) => {
        //   console.log(angle)
        //   this.angle = angle
        // })
        window.alcRenderer.addGizmo(this.lengthGizmo)
      }
    } else {
      window.alcRenderer.removeGizmo(this.lengthGizmo)
      this.lengthGizmo = null
    }
  }

  confirm() {
    // Refetch profiles in case they've been repaired
    this.profiles().forEach(profile => profile.update())
  }

  dispose() {
    super.dispose()
    window.alcRenderer.removeGizmo(this.lengthGizmo)
    this.lengthGizmo = null
  }
}


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
        autoMulti: true,
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

  isComplete() {
    return this.ref_plane && this.faces && this.faces().length
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

  confirm() {
    // Refetch faces in case they've been repaired
    this.faces().forEach(faceRef => faceRef.free())
    const faces = this.real.face_refs()
    this.faces = () => faces
  }

  dispose() {
    super.dispose()
    window.alcRenderer.removeGizmo(this.angleGizmo)
    this.angleGizmo = null
  }
}


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

  isComplete() {
    return this.profile && this.rail
  }

  preview() {
    this.profile.noFree = true
    return this.profile.extrude_preview(1.0)
  }

  confirm() {
    this.profile.extrude(1.0)
  }
}


export class FilletFeature extends Feature {
  static icon = 'scroll'
  constructor(doc) {
    super(doc, false, 'Fillet', {
      edges: {
        title: 'Edges',
        type: 'edge',
        autoMulti: true,
      },
      radius: {
        title: 'Radius',
        type: 'length',
      },
    })

    this.edges = null
    this.radius = 1.0
  }

  isComplete() {
    return this.edges && this.edges().length
  }

  updateFeature(tree) {
    const comp = tree.findChild(this.componentId)
    this.error = null

    const edges = this.edges().map(edgeRef => {
      const clone = edgeRef.clone()
      const error = clone.update(tree)
      this.error ||= error
      if(error) return
      edgeRef.update(tree) // Update intact references such that their center etc. can be calculated
      return edgeRef.getItem()
    }).filter(Boolean)

    if(edges.length && this.error) this.error.type = 'warning'

    try {
      comp.compound = comp.compound.fillet(edges, this.radius)
    } catch(err) { this.error = err || this.error }
  }

  updateGizmos() {
    if(this.isComplete()) {
      if(this.lengthGizmo) {
        this.lengthGizmo().set(this.radius, true)
      } else {
        const edge = this.edges()[0].getItem()
        const center = edge.center()
        const direction = new THREE.Vector3(0,1,0)
        const lengthGizmo = new LengthGizmo(center, direction, this.radius, true, (dist, _side) => {
          this.radius = dist
        })
        this.lengthGizmo = () => lengthGizmo
        window.alcRenderer.addGizmo(this.lengthGizmo())
      }
    } else {
      if(this.lengthGizmo) window.alcRenderer.removeGizmo(this.lengthGizmo())
      this.lengthGizmo = null
    }
  }

  repair() {
    const tree = this.document.top(this.document.timeline.previousFeature(this))
    const remainingEdges = this.edges().filter(edgeRef => !edgeRef.update(tree) )
    this.edges = () => remainingEdges
    this.error = null
  }

  dispose() {
    super.dispose()
    if(this.lengthGizmo) window.alcRenderer.removeGizmo(this.lengthGizmo())
    this.lengthGizmo = null
  }
}


export class OffsetFeature extends Feature {
  static icon = 'magnet'
  constructor(doc) {
    super(doc, false, 'Shell', {
      faces: {
        title: 'Open Faces',
        type: 'face',
        multi: true,
      },
      distance: {
        title: 'Distance',
        type: 'length',
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-up', 'caret-down']
      },
    })

    this.faces = null
    this.distance = 1.0
    this.side = false
  }

  isComplete() {
    return this.faces && this.faces().length
  }

  updateFeature(tree) {
    this.error = null

    const faces = this.faces().map(faceRef => {
      const clone = faceRef.clone()
      const error = clone.update(tree)
      this.error ||= error
      if(error) return
      faceRef.update(tree)
      return faceRef.getItem()
    }).filter(Boolean)

    if(faces.length && this.error) this.error.type = 'warning'

    const distance = this.distance * (this.side ? 1 : -1)
    const comp = tree.findChild(this.componentId)

    try {
      comp.compound = comp.compound.offset(faces, distance)
    } catch(err) { this.error = err || this.error }
  }
}


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
