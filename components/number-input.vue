<template lang="pug">

  NumericInput.number-input(
    ref="input"
    icon="tape"
    :value="inner.expression"
    :auto-focus="autoFocus"
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
      autoFocus: Boolean,
    },

    emits: ['update:value', 'enter', 'error'],

    data() {
      return {
        inner: new Expression(this.value, this.component.getParameters()),
        problem: false,
      }
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
        this.update()
      },

      decrease() {
        const number = this.inner.parse()
        const newValue = number.value - 1
        if(newValue >= 0) this.inner.set(truncate(newValue) + number.unit)
        this.update()
      },
    },
  }
</script>
