class Feature {
  confirm() {}
  cancel() {}
  update() {}
}

export class ExtrudeFeature extends Feature {
  constructor(component) {
    super()

    this.profile = null
    this.rail = null
    this.distance = 0
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
    console.log(this.profile)
  }
}
