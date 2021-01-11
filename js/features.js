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
  dispose() {}
  isComplete() {}

  update() {
    if(this.isComplete()) return this.preview()
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
        title: '(Rail)',
        type: 'curve',
        optional: true,
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

  dispose() {
    // if(this.profile) this.profile.free()
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

  dispose() {
    // if(this.profile) this.profile.free()
  }
}
