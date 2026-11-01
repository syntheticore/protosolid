<template lang="pug">

  ExpressionInput.number-input(ref="expression" :parameters="component.getParameters()")

    button.button.tape(
      ref="tape"
      type="button"
      tabindex="-1"
      title="Measure geometry"
      @mousedown.prevent
      @click="measure"
    )
      Icon(icon="tape" :style="{ color: measuring || undefined }")

</template>


<style lang="stylus" scoped>
  .tape
    margin: 0
    padding: 0 5px
    text-shadow: none
    border-radius: 3px 0 0 3px
</style>


<script>
  export default {
    name: 'NumberInput',

    props: {
      // Provides the parameters usable in expressions
      component: Object,
      // Color of the measure being picked, if any
      measuring: String,
    },

    emits: ['measure'],

    methods: {
      measure() {
        this.$refs.expression.startMeasure()
        this.$emit('measure')
      },

      insertMeasure(id) {
        this.$refs.expression.insertMeasure(id)
      },

      tapeCenter() {
        const rect = this.$refs.tape.getBoundingClientRect()
        return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
      },
    },
  }
</script>
