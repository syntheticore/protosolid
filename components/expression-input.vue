<template lang="pug">

  NumericInput.expression-input(
    ref="input"
    :parameterized="parameterized"
    :value="inner.expression"
    :problem="problem"
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
    },

    emits: ['update:value', 'expression', 'enter', 'error'],

    data() {
      return {
        inner: new Expression(this.expression ?? this.value, this.parameters, this.dimension),
        problem: false,
      }
    },

    computed: {
      parameterized() {
        if(!this.expression) return false
        const names = this.parameters.map(parameter => parameter.name)
        const identifiers = this.expression.match(/[A-Za-z_]\w*/g) || []
        return identifiers.some(identifier => names.includes(identifier))
      },
    },

    watch: {
      value(value) {
        if(this.inner.getBase() == value) return
        this.inner.setBase(value)
        this.update(true)
      },
    },

    methods: {
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

      update(noFocus) {
        this.$emit('update:value', this.inner.getBase())
        if(!noFocus) this.$refs.input.focusInput()
      },

      increase() {
        const number = this.inner.parse()
        this.inner.set(truncate(number.value + this.step) + number.unit)
        this.$emit('expression', this.inner.expression)
        this.update()
      },

      decrease() {
        const number = this.inner.parse()
        const newValue = number.value - this.step
        if(this.allowNegative || newValue >= 0) {
          this.inner.set(truncate(newValue) + number.unit)
          this.$emit('expression', this.inner.expression)
        }
        this.update()
      },
    },
  }
</script>
