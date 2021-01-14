class Feature {
  constructor(component, booleanOutput, settings) {
    this.component = component
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

  preview() {}
  confirm() {}
  isComplete() {}

  update() {
    if(this.isComplete()) return this.preview()
  }

  dispose() {
    // Free all profile regions
    for(const key in this.settings) {
      const setting = this.settings[key]
      if(setting.type == 'profile') {
        const profile = this[key]
        if(profile) {
          if(profile.unused) profile.free()
          profile.noFree = false
          this[key] = null
        }
      }
    }
  }
}


export class ExtrudeFeature extends Feature {
  constructor(component) {
    super(component, true, {
      profile: {
        title: 'Profile',
        type: 'profile',
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

    this.profile = null
    this.rail = null
    this.distance = 1.0
    this.side = true
  }

  isComplete() {
    return !!this.profile
  }

  preview() {
    this.profile.noFree = true
    return this.profile.extrude_preview(this.distance * (this.side ? 1 : -1))
  }

  confirm() {
    this.profile.extrude(this.distance * (this.side ? 1 : -1))
  }
}


export class SweepFeature extends Feature {
  constructor(component) {
    super(component, true, {
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
  constructor(component) {
    super(component, true, {
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
    // this.profile.revolve(axis, this.angle * (this.side ? 1 : -1))
  }
}


export class MaterialFeature extends Feature {
  constructor(component) {
    super(component, false, {
      material: {
        title: 'Material Presets',
        type: 'material',
      },
    })

    this.material = null
  }

  isComplete() {
    return !!this.material
  }

  preview() {
    if(this.oldMaterial === undefined) this.oldMaterial = this.component.material
    this.component.material = this.material
  }

  confirm() {
    this.oldMaterial = undefined
  }

  dispose() {
    super.dispose()
    if(this.oldMaterial === undefined) return
    this.component.material = this.oldMaterial
  }
}
