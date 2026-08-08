<template lang="pug">

  NumericInput.integer-input(
    ref="input"
    inputmode="numeric"
    :value="value"
    :problem="problem"
    @commit="commit"
    @increase="change(1)"
    @decrease="change(-1)"
  )

</template>


<script>
  export default {
    name: 'IntegerInput',

    props: {
      value: Number,
      min: {
        type: Number,
        default: Number.MIN_SAFE_INTEGER,
      },
      max: {
        type: Number,
        default: Number.MAX_SAFE_INTEGER,
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
        if(!Number.isInteger(value) || value < this.min || value > this.max) {
          this.problem = true
          this.$emit('error', { type: 'error', msg: 'Enter a valid whole number' })
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
        this.$emit('update:value', value)
        this.$refs.input.focusInput()
      },
    },
  }
</script>
