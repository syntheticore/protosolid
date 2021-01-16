const conversions = {
  mm: 1.0,
  cm: 10.0,
  m: 1000.0,
  inch: 25.4,
}

export default class Expression {
  constructor(input, parameters, preferredUnit) {
    this.parameters = parameters || []
    this.preferredUnit = preferredUnit
    this.value = input
  }

  // Set in any unit. Defaults to millimeter.
  set value(input) {
    if(typeof input == 'number') {
      this.expression = String(Number(input.toFixed(3))) + 'mm'
    } else {
      // Don't leave the saved expression totally without unit
      // for display purposes
      const number = this.parsePlus(input)
      if(!number.unit && number.value == input) input += 'mm'
      this.expression = input
    }
  }

  // Return millimeters
  get value() {
    const number = this.parse()
    return number.value * conversions[number.unit || 'mm']
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

  // * / - +
  parsePlus(expression) {
    const numbers = this.split(expression, '+').map(expr => this.parseMinus(expr) )
    return numbers.reduce((acc, number) => this.add(acc, number))
  }

  // * / -
  parseMinus(expression) {
    const numbers = this.split(expression, '-').map(expr => this.parseDivision(expr) )
    return numbers.reduce((acc, number) => this.subtract(acc, number) )
  }

  // * /
  parseDivision(expression) {
    const numbers = this.split(expression, '/').map(expr => this.parseMultiplication(expr) )
    return numbers.reduce((acc, number) => this.divide(acc, number) )
  }

  // *
  parseMultiplication(expression) {
    const numbers = this.split(expression, '*').map(expr => {
      if(expr[0] == '(') return this.parsePlus(expr.substr(1, expr.length - 2))
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
      unit: number[2],
    }
  }

  add(left, right) {
    return this.operation(left, right, (l,r) => l + r )
  }

  subtract(left, right) {
    return this.operation(left, right, (l,r) => l - r )
  }

  divide(left, right) {
    return this.operation(left, right, (l,r) => l / r, true )
  }

  multiply(left, right) {
    return this.operation(left, right, (l,r) => l * r, true )
  }

  operation(left, right, op, quadratic) {
    const unit = this.decideUnit(left, right)
    const leftUnit = (quadratic ? left.unit : left.unit || right.unit) || 'mm'
    const rightUnit = (quadratic ? right.unit : right.unit || left.unit) || 'mm'
    return {
      value: op(
        left.value * conversions[leftUnit],
        right.value * conversions[rightUnit],
      ) / conversions[unit || 'mm'],
      unit,
    }
  }

  decideUnit(left, right) {
    return left.unit && right.unit ?
      left.unit == right.unit ? left.unit : 'mm' :
      left.unit || right.unit
  }

  split(expression, operator) {
    const result = []
    let braces = 0
    let currentChunk = ''
    for(let i = 0; i < expression.length; ++i) {
      const token = expression[i]
      if(token == '(') {
        braces++
      } else if (token == ')') {
        braces--
      }
      if(braces == 0 && operator == token) {
        result.push(currentChunk.trim())
        currentChunk = ''
      } else currentChunk += token
    }
    if(currentChunk != '') {
      result.push(currentChunk.trim())
    }
    return result
  }
}


const unit = new Expression('2 * (1inch + 1mm)')
console.log('RESULT', unit.value, unit.parse().value, unit.parse().unit)
