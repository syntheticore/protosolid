class Feature {
  confirm() {}
  cancel() {}
}

export class ExtrudeFeature extends Feature {
  constructor() {
    super()
    this.profile = null
    this.rail = null
    this.distance = 0
    this.direction = true
    this.operation = 'join'
  }

  settings() {
    return {
      profile: {
        title: 'Profile',
        type: 'profile',
        color: 'pink',
      },
      rail: {
        title: 'Rail',
        type: 'curve',
        color: 'green',
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
}
