class Feature {
  constructor(component) {
    this.component = component
  }

  isComplete() {}
  preview() {}
  confirm() {}
  cancel() {}

  update() {
    if(!this.isComplete()) return
    return this.preview()
  }
}

export class ExtrudeFeature extends Feature {
  constructor(component) {
    super(component)

    this.profile = null
    this.rail = null
    this.distance = 2
    this.side = true
    this.operation = 'join'

    this.settings = {
      profile: {
        title: 'Profile',
        type: 'profile',
      },
      rail: {
        title: 'Rail',
        type: 'curve',
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
      operation: {
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
      },
    }

    this.defaultSetting = 'profile'
  }

  isComplete() {
    return !!this.profile
  }

  preview() {
    return this.profile.extrude_preview(this.distance * (this.side ? 1 : -1))
  }

  confirm() {
    this.profile.extrude(this.distance * (this.side ? 1 : -1))
  }
}
