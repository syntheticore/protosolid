<template lang="pug">

  NumericInput.scalar-input(
    ref="input"
    inputmode="decimal"
    :value="value"
    :problem="problem"
    @commit="commit"
    @increase="change(step)"
    @decrease="change(-step)"
  )

</template>


<script>
  export default {
    name: 'ScalarInput',

    props: {
      value: Number,
      step: {
        type: Number,
        default: 1,
      },
      min: {
        type: Number,
        default: -Infinity,
      },
      max: {
        type: Number,
        default: Infinity,
      },
    },

    emits: ['update:value', 'error'],

    data() {
      return {
        problem: false,
      }
    },

    methods: {
      commit(rawValue, noFocus) {
        const value = Number(rawValue)
        if(!Number.isFinite(value) || value < this.min || value > this.max) {
          this.problem = true
          this.$emit('error', { type: 'error', msg: 'Enter a valid number' })
          this.$refs.input.focusInput()
          return
        }
        this.problem = false
        this.$emit('update:value', value)
        if(!noFocus) this.$refs.input.focusInput()
      },

      change(offset) {
        const value = Math.min(this.max, Math.max(this.min, this.value + offset))
        this.problem = false
        this.$emit('update:value', Number(value.toFixed(12)))
        this.$refs.input.focusInput()
      },
    },
  }
</script>
