<template lang="pug">

  .vector-input

    ScalarInput(
      v-for="axis in axes"
      :key="axis"
      :label="axis.toUpperCase()"
      :value="value[axis]"
      :step="step"
      :min="min"
      :max="max"
      stretch
      @update:value="$emit('update', axis, $event)"
    )

</template>


<style lang="stylus" scoped>

  .vector-input
    display: flex

    > *
      flex: 1 1 0
      min-width: 0

    // Fuse the components into one control
    > :not(:first-child) :deep(.input)
      border-top-left-radius: 0
      border-bottom-left-radius: 0

    > :not(:last-child) :deep(.spinner .button)
      border-radius: 0

    :deep(.spinner .button)
      padding: 0 3px 1px

</style>


<script>
  export default {
    name: 'VectorInput',

    props: {
      // Object holding one number per axis
      value: Object,
      axes: {
        type: Array,
        default: () => ['x', 'y', 'z'],
      },
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

    emits: ['update'],
  }
</script>
