<template lang="pug">

  NumericInput.number-input(
    ref="input"
    icon="tape"
    :parameterized="parameterized"
    :value="inner.expression"
    :auto-focus="autoFocus"
    :focus-on-mount="focusOnMount"
    :problem="problem"
    @commit="commit"
    @increase="increase"
    @decrease="decrease"
  )

</template>


<script>
  import Expression from '../js/core/expression.js'

  function truncate(number) {
    return String(Number(number.toFixed(3)))
  }

  export default {
    name: 'NumberInput',

    props: {
      component: Object,
      value: Number,
      expression: String,
      dimension: {
        type: String,
        default: 'length',
      },
      autoFocus: Boolean,
      focusOnMount: Boolean,
    },

    emits: ['update:value', 'expression', 'enter', 'error'],

    data() {
      return {
        inner: new Expression(this.expression ?? this.value, this.component.getParameters(), this.dimension),
        problem: false,
      }
    },

    computed: {
      parameterized() {
        if(!this.expression) return false
        const names = this.component.getParameters().map(parameter => parameter.name)
        const identifiers = this.expression.match(/[A-Za-z_]\w*/g) || []
        return identifiers.some(identifier => names.includes(identifier))
      },
    },

    watch: {
      value(value) {
        if(this.inner.getBase() == value) return
        this.inner.setBase(value)
        this.update()
      },
    },

    methods: {
      commit(value, noFocus) {
        this.problem = false
        try {
          this.inner.set(value)
          this.$emit('expression', this.inner.expression)
          this.update(noFocus)
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
        this.inner.set(truncate(number.value + 1) + number.unit)
        this.$emit('expression', this.inner.expression)
        this.update()
      },

      decrease() {
        const number = this.inner.parse()
        const newValue = number.value - 1
        if(newValue >= 0) {
          this.inner.set(truncate(newValue) + number.unit)
          this.$emit('expression', this.inner.expression)
        }
        this.update()
      },
    },
  }
</script>
