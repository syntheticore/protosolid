const conversions = {
  mm: 1.0,
  cm: 10.0,
  g: 1000.0,
  inch: 25.4,
}

export default class Unit {
  constructor(input) {
    this.value = input
  }

  // Set in any unit. Defaults to millimeter.
  set value(input) {
    this.string = input
    if(typeof(input) == 'number') {
      this.string = String(Number(input.toFixed(3))) + 'mm'
    }
  }

  // Returns millimeters
  get value() {
    const match = this.parse()
    return match.value * conversions[match.unit]
  }

  parse() {
    const match = /([+-]?\d+\.?\d*)\s*(\w*)/.exec(this.string)
    return match && {
      value: Number(match[1]),
      unit: match[2] || 'mm'
    }
  }

  asGiven() {
    return this.parse().value
  }

  as(unit) {
    return this.value / conversions[unit]
  }

  format(unit) {
    return String(this.as(unit)) + unit
  }
}
