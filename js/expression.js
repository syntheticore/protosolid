const conversions = {
  mm: 1.0,
  cm: 10.0,
  m: 1000.0,
  inch: 25.4,
}

export default class Expression {
  constructor(input, parameters, preferredUnit) {
    this.parameters = parameters || []
    this.preferredUnit = preferredUnit || 'mm'
    this.set(input)
  }

  // Set in any unit
  set(input) {
    if(typeof input == 'number') {
      // Assume preferred unit for raw input
      this.expression = String(Number(input.toFixed(3))) + this.preferredUnit
    } else {
      // Allways leave a unit for display purposes
      const number = this.parsePlus(input)
      if(!number.unit && number.value == input) input += this.preferredUnit
      this.expression = input
    }
  }

  asPreferredUnit() {
    return this.as(this.preferredUnit)
  }

  asBaseUnit() {
    const number = this.parse()
    return number.value * conversions[number.unit]
  }

  as(unit) {
    return this.asBaseUnit() / conversions[unit]
  }

  format(unit) {
    return String(this.as(unit)) + unit
  }

  parse() {
    const number = this.parsePlus(this.expression)
    // Assume preferred unit if no unit could be determined by now
    number.unit = number.unit || this.preferredUnit
    return number
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
      // Propably a parameter
      const param = this.parameters.find(param => param.name == expression.trim() )
      if(!param) throw 'Unknown Parameter "' + expression + '"'
      return this.parsePlus(param.value)
    // Actual number literal with or without unit
    } else return {
      value: Number(number[1]),
      unit: number[2],
    }
  }

  add(left, right) {
    return this.operation(left, right, (l,r) => l + r, true)
  }

  subtract(left, right) {
    return this.operation(left, right, (l,r) => l - r, true)
  }

  divide(left, right) {
    return this.operation(left, right, (l,r) => l / r )
  }

  multiply(left, right) {
    return this.operation(left, right, (l,r) => l * r )
  }

  operation(left, right, op, convertBoth) {
    const unit = this.decideUnit(left, right)
    const leftUnit = (convertBoth ? left.unit || right.unit : left.unit) || 'mm'
    const rightUnit = (convertBoth ? right.unit || left.unit : right.unit) || 'mm'
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
      left.unit == right.unit ? left.unit : this.preferredUnit :
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


const unit = new Expression('2mm * 2mm / 2inch')
console.log('RESULT', unit.asBaseUnit(), unit.parse().value, unit.parse().unit)

b - a / 2 + 5
