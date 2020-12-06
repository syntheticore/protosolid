class Feature {
  constructor(component) {
    this.component = component
  }

  confirm() {}
  cancel() {}
  update() {}
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
        color: 'pink',
      },
      rail: {
        title: 'Rail',
        type: 'curve',
        color: 'purple',
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
  }

  update() {
    console.log('Extruding ' + this.distance + 'mm')
    this.profile.extrude(this.distance * (this.direction ? 1 : -1))
  }
}
