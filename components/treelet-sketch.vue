<template lang="pug">

  li.treelet-sketch(
    :class="{hidden: isHidden}"
    @click="document.selection = document.selection.handle(sketch, bus.isCtrlPressed)"
    @dblclick="document.activateSketch(sketch)"
    @mouseenter="$emit('update:highlight', sketch)"
    @mouseleave="$emit('update:highlight', null)"
  )

    .box

      header

        Icon.eye(
          icon="eye"
          @click.stop="toggleVisibility"
        )

        Icon.icon(icon="edit" fixed-width)

        h2 Sketch {{ index + 1 }}

        .controls

          Icon(
            icon="check-circle" fixed-width
            title="Activate"
            @click.stop="document.activateSketch(sketch)"
          )

</template>


<style lang="stylus" scoped>

  .treelet-sketch

    &.hidden
      opacity: 0.5

    .icon
      padding-left: 3px
      width: auto

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
        this.document.emit('component-changed', this.component)
      },
    },
  }

</script>
