import preferences from '../preferences.js'

const dimensions = {
  length: {
    conversions: { mm: 1.0, cm: 10.0, m: 1000.0, inch: 25.4 },
    preferred: () => preferences.preferredUnit,
    units: '(inch\\b|mm\\b|cm\\b|m\\b)',
  },
  angle: {
    conversions: { '°': 1.0, deg: 1.0, rad: 180.0 / Math.PI },
    preferred: () => '°',
    units: '(°|deg\\b|rad\\b)',
  },
}

function fixDecimals(number) {
  return String(Number(number.toFixed(3)))
}

export default class Expression {
  constructor(input, parameters, dimension = 'length') {
    this.parameters = parameters || []
    this.dimension = dimensions[dimension]
    if(!this.dimension) throw new Error(`Unknown expression dimension: ${dimension}`)
    this.set(input)
  }

  get defaultUnit() {
    return this.dimension.preferred()
  }

  // Set in any unit
  set(input) {
    if(typeof input == 'number') {
      // Assume preferred unit for raw input
      this.expression = fixDecimals(
        input / this.dimension.conversions[this.defaultUnit]
      ) + this.defaultUnit
    } else {
      // Allways leave a unit for display purposes
      input = input.replace(/,/g, '.')
      // Write .75 as 0.75
      input = input.replace(/(?<![\w.])\.(?=\d)/g, '0.')
      const number = this.parsePlus(input)
      if(!number.unit && number.value == input) input += this.defaultUnit
      this.expression = input
    }
  }

  getBase() {
    const number = this.parse()
    return number.value * this.dimension.conversions[number.unit]
  }

  setBase(mmValue) {
    const unit = this.parse().unit
    // Keep current unit
    const value = mmValue / this.dimension.conversions[unit]
    this.set(fixDecimals(value) + unit)
  }

  as(unit) {
    if(!(unit in this.dimension.conversions)) throw new Error(`Invalid unit: ${unit}`)
    return this.getBase() / this.dimension.conversions[unit]
  }

  format(unit) {
    return fixDecimals(this.as(unit)) + unit
  }

  parse() {
    const number = this.parsePlus(this.expression)
    // Assume preferred unit if no unit could be determined by now
    number.unit = number.unit || this.defaultUnit
    if(!(number.unit in this.dimension.conversions)) {
      throw new Error(`Invalid unit for this value: ${number.unit}`)
    }
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
    expr = expr.trim()
    const negative = expr.startsWith('-')
    if(negative) expr = expr.slice(1)
    const numbers = this.split(expr, ['-']).map(chunk =>
      this.parseMultiplication(chunk.chunk)
    )
    if(negative && numbers.length) numbers[0].value *= -1
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
      this.multiply(acc, number) : this.divide(acc, number)
    )
  }

  parseNumber(expr) {
    const match = new RegExp('(\\d*\\.?\\d*)\\s*' + this.dimension.units + '?').exec(expr)
    if(!match || match[1] === '' || match[0].trim() != expr.trim()) {
      // Probably a parameter
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
    const unit = this.decideUnit(left, right) || this.defaultUnit
    const leftUnit = (convertBoth ? left.unit || right.unit : left.unit) || this.defaultUnit
    const rightUnit = (convertBoth ? right.unit || left.unit : right.unit) || this.defaultUnit
    return {
      value: op(
        left.value * this.dimension.conversions[leftUnit],
        right.value * this.dimension.conversions[rightUnit],
      ) / (unit ? this.dimension.conversions[unit] : 1.0),
      unit,
    }
  }

  decideUnit(left, right) {
    return left.unit && right.unit ?
      left.unit == right.unit ? left.unit : this.defaultUnit :
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
