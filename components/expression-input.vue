<template lang="pug">

  NumericInput.expression-input(
    ref="input"
    :parameterized="parameterized"
    :value="inner.expression"
    :problem="problem"
    :measures="measures"
    @commit="commit"
    @increase="increase"
    @decrease="decrease"
  )

    //- Optional button left of the input
    template(v-if="$slots.default" #default)
      slot

</template>


<script>
  import Expression from '../js/core/expression.js'
  import { measureIds } from '../js/core/measures.js'

  function truncate(number) {
    return String(Number(number.toFixed(3)))
  }

  export default {
    name: 'ExpressionInput',

    props: {
      value: Number,
      expression: String,
      // Parameters usable in expressions
      parameters: {
        type: Array,
        default: () => [],
      },
      dimension: {
        type: String,
        default: 'length',
      },
      step: {
        type: Number,
        default: 1,
      },
      allowNegative: Boolean,
      // Measures usable in expressions, as { [id]: { value, property, color, options } }
      measures: Object,
    },

    emits: ['update:value', 'expression', 'enter', 'error'],

    data() {
      // Measured expressions are shown as stored, as their values may not be known yet
      const measured = this.expression && measureIds(this.expression).length
      const inner = new Expression(measured ? this.value : this.expression ?? this.value, this.parameters, this.dimension, this.measureValues())
      if(measured) inner.expression = this.expression
      return {
        inner,
        problem: false,
      }
    },

    computed: {
      parameterized() {
        if(!this.expression) return false
        if(measureIds(this.expression).length) return true
        const names = this.parameters.map(parameter => parameter.name)
        const identifiers = this.expression.match(/[A-Za-z_]\w*/g) || []
        return identifiers.some(identifier => names.includes(identifier))
      },
    },

    watch: {
      measures() {
        this.inner.measures = this.measureValues()
      },

      // Only measured expressions are taken over, others follow the value
      expression(expression) {
        if(expression == this.inner.expression) return
        if(expression && measureIds(expression).length) this.inner.expression = expression
        // Show the plain value once measures are dropped, such as by gizmos
        else if(measureIds(this.inner.expression).length) this.inner.set(this.value)
      },

      value(value) {
        // Measured expressions follow their geometry instead
        if(this.expression && measureIds(this.expression).length) return
        if(this.inner.getBase() == value) return
        this.inner.setBase(value)
        this.update(true)
      },
    },

    methods: {
      measureValues() {
        return Object.fromEntries(Object.entries(this.measures || {}).map(([id, measure]) => [id, measure.value]))
      },

      commit(value, noFocus) {
        this.problem = false
        try {
          this.inner.set(value)
          this.$emit('expression', this.inner.expression)
          this.update(noFocus)
          this.$refs.input.syncValue()
          this.$emit('enter')
        } catch(e) {
          this.problem = true
          this.$emit('error', { type: 'error', msg: 'Enter a valid expression' })
          this.$refs.input.focusInput()
        }
      },

      // Remember where measures picked from now on will be inserted
      startMeasure() {
        this.insertion = this.$refs.input.selectionRange()
      },

      // Replaces the remembered text range with a measure, before the measure
      // has a value. The feature evaluates the expression when it updates.
      insertMeasure(id) {
        const text = this.$refs.input.text()
        const token = `{${id}}`
        const start = Math.min(this.insertion.start, text.length)
        const end = Math.min(this.insertion.end, text.length)
        this.inner.expression = text.slice(0, start) + token + text.slice(end)
        this.insertion = { start: start + token.length, end: start + token.length }
        this.problem = false
        this.$emit('expression', this.inner.expression)
      },

      update(noFocus) {
        this.$emit('update:value', this.inner.getBase())
        if(!noFocus) this.$refs.input.focusInput()
      },

      increase(factor) {
        const number = this.inner.parse()
        this.inner.set(truncate(number.value + this.step * factor) + number.unit)
        this.$emit('expression', this.inner.expression)
        this.update()
      },

      decrease(factor) {
        const number = this.inner.parse()
        const newValue = number.value - this.step * factor
        if(this.allowNegative || newValue >= 0) {
          this.inner.set(truncate(newValue) + number.unit)
          this.$emit('expression', this.inner.expression)
        }
        this.update()
      },
    },
  }
</script>
