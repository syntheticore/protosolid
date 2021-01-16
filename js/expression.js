import preferences from './preferences.js'

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
      this.expression = String(Number(input.toFixed(3))) + preferences.preferredUnit
    } else {
      // Allways leave a unit for display purposes
      const number = this.parsePlus(input)
      if(!number.unit && number.value == input) input += preferences.preferredUnit
      this.expression = input
    }
  }

  asPreferredUnit() {
    return this.as(preferences.preferredUnit)
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
    number.unit = number.unit || preferences.preferredUnit
    return number
  }

  // * / - +
  parsePlus(expr) {
    const numbers = this.split(expr, ['+']).map(chunk =>
      this.parseMinus(chunk.chunk)
    )
    return numbers.reduce((acc, number) => this.add(acc, number))
  }

  // * / -
  parseMinus(expr) {
    const numbers = this.split(expr, ['-']).map(chunk =>
      this.parseMultiplication(chunk.chunk)
    )
    return numbers.reduce((acc, number) => this.subtract(acc, number) )
  }

  // * /
  parseMultiplication(expr) {
    const chunks = this.split(expr, ['*', '/'])
    const numbers = chunks.map(chunk => chunk.chunk[0] == '(' ?
      this.parsePlus(chunk.chunk.substr(1, chunk.chunk.length - 2)) :
      this.parseNumber(chunk.chunk)
    )
    return numbers.reduce((acc, number, i) => chunks[i - 1].op == '*' ?
      this.multiply(acc, number) :
      this.divide(acc, number)
    )
  }

  parseNumber(expr) {
    const match = /(\d*\.?\d*)\s*(inch\b|mm\b|cm\b|m\b)?/.exec(expr)
    if(!match || match[1] === '') {
      // Propably a parameter
      const param = this.parameters.find(param => param.name == expr.trim() )
      if(!param) throw 'Unknown Parameter "' + expr + '"'
      return this.parsePlus(param.value)
    // Actual number literal with or without unit
    } else return {
      value: Number(match[1]),
      unit: match[2],
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
      ) / unit ? conversions[unit] : 1.0,
      unit,
    }
  }

  decideUnit(left, right) {
    return left.unit && right.unit ?
      left.unit == right.unit ? left.unit : preferences.preferredUnit :
      left.unit || right.unit
  }

  split(expression, operators) {
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
      const op = operators.find(op => op == token )
      if(braces == 0 && op) {
        result.push({
          op,
          chunk: currentChunk.trim(),
        })
        currentChunk = ''
      } else currentChunk += token
    }
    if(currentChunk != '') {
      result.push({
        chunk: currentChunk.trim(),
      })
    }
    return result
  }
}


const unit = new Expression('2mm / 2mm * 2inch')
console.log('RESULT', unit.asBaseUnit(), unit.parse().value, unit.parse().unit)
