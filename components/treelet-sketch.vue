<template lang="pug">

  li.treelet-sketch(
    @click="document.selection.handle(sketch, bus.isCtrlPressed)"
    @dblclick="document.activateSketch(sketch)"
    @mouseenter="$emit('update:highlight', sketch)"
    @mouseleave="$emit('update:highlight', null)"
    :class="{ hidden: isHidden }"
  )

    .box

      header

        Icon.eye(
          icon="eye"
          @click.stop="toggleVisibility"
          @dblclick.stop
        )

        Icon.icon(icon="solar-panel" fixed-width)

        h2 Sketch {{ index + 1 }}

        .controls

          Icon(
            icon="check-circle" fixed-width
            title="Activate"
            @click.stop="document.activateSketch(sketch)"
            @dblclick.stop
          )

</template>


<style lang="stylus" scoped>

  .icon
    padding-left: 3px

</style>


<script>

  export default {
    name: 'TreeletSketch',

    inject: ['bus'],

    props: {
      document: Object,
      component: Object,
      sketch: Object,
      index: Number,
    },

    computed: {
      isHidden: function() {
        return this.component.creator.itemsHidden[this.sketch.id]
      },
    },

    methods: {
      toggleVisibility: function() {
        this.component.creator.itemsHidden[this.sketch.id] = !this.isHidden
      },
    },
  }

</script>
