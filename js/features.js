import * as THREE from 'three'

import { vec2three } from './utils.js'
import { LengthGizmo, AngleGizmo } from './gizmos.js'


class Feature {
  constructor(document, real, booleanOutput, title, icon, settings) {
    this.document = document
    this.real = real || new window.alcWasm.JsFeature(document.real)
    this.title = title
    this.icon = icon
    this.settings = settings

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

  execute() {
    this.updateFeature()
    this.id = this.real.id()
  }

  confirm() {}
  isComplete() {}
  updateGizmos() {}

  update() {
    this.updateGizmos()
    if(!this.isComplete()) return
    this.execute()
  }

  getValues() {
    const values = {}
    Object.keys(this.settings).forEach(key => values[key] = this[key] )
    return values
  }

  setValues(values) {
    Object.keys(values).forEach(key => this[key] = values[key] )
  }

  dispose() {}
}


export class CreateComponentFeature extends Feature {
  constructor(document, real) {
    super(document, real, false, 'New Component', 'box', {
      parent: {
        title: 'Parent Component',
        type: 'component',
      },
    })

    this.parent = null
  }

  isComplete() {
    return !!this.parent
  }

  updateFeature() {
    this.real.create_component(this.parent())
  }
}


export class CreateSketchFeature extends Feature {
  constructor(document, real) {
    super(document, real, false, 'Sketch', 'edit', {
      plane: {
        title: 'Plane',
        type: 'plane',
        autoConfirm: true,
      },
    })

    this.plane = null
  }

  isComplete() {
    return !!this.plane
  }

  updateFeature() {
    const plane = this.plane()
    this.real.create_sketch(this.document.activeComponent.real.id(), plane)
  }

  confirm(featureBox) {
    featureBox.$emit('update:active-sketch', this.document.activeComponent.sketches.slice(-1)[0])
  }
}


export class ExtrudeFeature extends Feature {
  constructor(document, real) {
    super(document, real, true, 'Extrude', 'layer-group', {
      profiles: {
        title: 'Profile',
        type: 'profile',
        multi: true,
      },
      rail: {
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

  updateFeature() {
    const list = new window.alcWasm.JsProfileRefList()
    this.profiles().forEach(profile => {
      list.push(profile.make_reference())
    })
    const sketch = this.document.tree.findSketch(this.profiles()[0].sketch_id())
    const comp_ref = sketch.component_id()
    const distance = this.distance * (this.side ? 1 : -1)
    this.real.extrusion(comp_ref, list, distance, this.operation)
  }

  updateGizmos() {
    if(this.isComplete()) {
      if(this.lengthGizmo) {
        this.lengthGizmo.set(this.distance, this.side)
      } else {
        const center = vec2three(this.profiles()[0].get_center())
        const axis = this.axis && this.axis()
        const direction = axis || vec2three(this.profiles()[0].get_normal())
        this.lengthGizmo = new LengthGizmo(center, direction, this.side, this.distance, (dist, side) => {
          this.distance = dist
          this.side = side
        })
        window.alcRenderer.addGizmo(this.lengthGizmo)
      }
    } else {
      window.alcRenderer.removeGizmo(this.lengthGizmo)
      this.lengthGizmo = null
    }
  }

  confirm() {
    // Refetch profiles in case they've been repaired
    this.profiles().forEach(profile => profile.free())
    const profiles = this.real.get_profiles()
    this.profiles = () => profiles
  }

  dispose() {
    super.dispose()
    window.alcRenderer.removeGizmo(this.lengthGizmo)
    this.lengthGizmo = null
  }
}


export class DraftFeature extends Feature {
  constructor(document, real) {
    super(document, real, true, 'Draft', 'clone', {
      ref_plane: {
        title: 'Reference',
        type: 'plane',
      },
      faces: {
        title: 'Faces',
        type: 'face',
        multi: true,
        superMulti: true,
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
          console.log(angle)
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
    const faces = this.real.get_face_refs()
    this.faces = () => faces
  }

  dispose() {
    super.dispose()
    window.alcRenderer.removeGizmo(this.angleGizmo)
    this.angleGizmo = null
  }
}


export class SweepFeature extends Feature {
  constructor(document, real) {
    super(document, real, true, 'Sweep', 'edit', {
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


export class RevolveFeature extends Feature {
  constructor(document, real) {
    super(document, real, true, 'Revolve', 'edit', {
      profile: {
        title: 'Profile',
        type: 'profile',
      },
      axis: {
        title: 'Axis',
        type: 'axis',
      },
      angle: {
        title: 'Angle',
        type: 'angle',
      },
      side: {
        title: 'Side',
        type: 'bool',
        icons: ['caret-right', 'caret-left']
      },
    })

    this.profile = null
    this.axis = null
    this.angle = 360.0
    this.side = true
  }

  isComplete() {
    return this.profile && this.axis
  }

  preview() {
    this.profile.noFree = true
    return this.profile.extrude_preview(this.angle * (this.side ? 1 : -1))
  }

  confirm() {
    this.profile.revolve(axis, this.angle * (this.side ? 1 : -1))
  }
}


// export class MaterialFeature extends Feature {
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
