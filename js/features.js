class Feature {
  constructor(component) {
    this.component = component
  }

  confirm() {}
  cancel() {}
  update() {}
  isComplete() {}
}

export class ExtrudeFeature extends Feature {
  constructor(component) {
    super(component)

    this.profile = null
    this.rail = null
    this.distance = 2
    this.direction = true
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
      direction: {
        title: 'Direction',
        type: 'bool',
      },
      operation: {
        title: 'Operation',
        type: 'select',
        options: ['join', 'cut', 'intersect', 'create'],
      },
    }

    this.defaultSetting = 'profile'
  }

  update() {
    return this.profile.extrude_preview(this.distance * (this.direction ? 1 : -1))
  }

  confirm() {
    this.profile.extrude(this.distance * (this.direction ? 1 : -1))
  }

  isComplete() {
    return !!this.profile
  }
}
