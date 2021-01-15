const conversions = {
  mm: 1.0,
  cm: 10.0,
  m: 1000.0,
  inch: 25.4,
}

export default class Expression {
  constructor(input, parameters) {
    this.parameters = parameters || []
    this.value = input
  }

  // Set in any unit. Defaults to millimeter.
  set value(input) {
    if(typeof input == 'number') {
      this.expression = String(Number(input.toFixed(3))) + 'mm'
    } else {
      // Don't leave the saved expression totally without unit
      const number = this.parsePlus(input)
      if(!number.unit) input += 'mm'
      this.expression = input
    }
  }

  // Returns millimeters
  get value() {
    return this.asMM(this.parse())
  }

  as(unit) {
    return this.value / conversions[unit]
  }

  format(unit) {
    return String(this.as(unit)) + unit
  }

  parse() {
    return this.parsePlus(this.expression)
  }

  // * - +
  parsePlus(expression) {
    const numbers = this.split(expression, '+').map(expr => this.parseMinus(expr) )
    return numbers.reduce((acc, number) => this.add(acc, number))
  }

  // * -
  parseMinus(expression) {
    const numbers = this.split(expression, '-').map(expr => this.parseMultiplication(expr) )
    return numbers.reduce((acc, number) => this.subtract(acc, number) )
  }

  // *
  parseMultiplication(expression) {
    const numbers = this.split(expression, '*').map(expr => {
      if(expr[0] == '(') {
        const expr = expr.substr(1, expr.length - 2)
        return this.parsePlus(expr)
      }
      return this.parseNumber(expr)
    })
    return numbers.reduce((acc, number) => this.multiply(acc, number) )
  }

  parseNumber(expression) {
    const number = /(\d+\.?\d*)\s*(inch|mm|cm|m)?/.exec(expression)
    if(!number) {
      const param = this.parameters.find(param => param.name == expression.trim() )
      if(!param) throw 'Unknown Parameter "' + expression + '"'
      return this.parseNumber(param.value)
    } else return {
      value: Number(number[1]),
      unit: number[2] || undefined,
    }
  }

  add(left, right) {
    return this.operation(left, right, (l,r) => l + r )
  }

  subtract(left, right) {
    return this.operation(left, right, (l,r) => l - r )
  }

  multiply(left, right) {
    return this.operation(left, right, (l,r) => l * r )
  }

  operation(left, right, op) {
    const unit = this.decideUnit(left, right)
    return {
      value: op(this.asMM(left), this.asMM(right)) / conversions[unit],
      unit,
    }
  }

  decideUnit(left, right) {
    return left.unit && right.unit ?
      left.unit == right.unit ? left.unit : undefined :
      left.unit || right.unit || undefined
  }

  asMM(number) {
    return number.value * conversions[number.unit || 'mm']
  }

  split(expression, operator) {
    const result = []
    let braces = 0
    let currentChunk = ""
    for(let i = 0; i < expression.length; ++i) {
      const curCh = expression[i]
      if(curCh == '(') {
        braces++
      } else if (curCh == ')') {
        braces--
      }
      if(braces == 0 && operator == curCh) {
        result.push(currentChunk)
        currentChunk = ""
      } else currentChunk += curCh
    }
    if(currentChunk != "") {
      result.push(currentChunk)
    }
    return result
  }
}
