<template lang="pug">
  li.sketch-treelet(
    :class="{hidden: isHidden}"
    @dblclick="$emit('update:active-sketch', sketch)"
  )
    //- @click="$emit('update:selection', selection.handle(sketch, $root.isCtrlPressed))"
    //- @mouseenter="$emit('update:highlight', sketch)"
    //- @mouseleave="$emit('update:highlight', null)"
    .box
      header
        fa-icon.eye(
          icon="eye"
          @click.stop="toggleVisibility"
        )
        fa-icon.icon(icon="edit" fixed-width)
        h2 Sketch {{ index + 1 }}
        .controls
          fa-icon(
              icon="check-circle" fixed-width
              title="Activate"
              @click.stop="$emit('update:active-sketch', sketch)"
            )
</template>


<style lang="stylus" scoped>
  .sketch-treelet
    &.hidden
      opacity: 0.5

    .icon
      padding-left: 3px
      width: auto
</style>


<script>
  export default {
    name: 'SketchTreelet',

    props: {
      sketch: Object,
      index: Number,
    },

    data() {
      return {
        sketchId: this.sketch.id(),
      }
    },

    computed: {
      isHidden: function() {
        return this.sketch.component.UIData.itemsHidden[this.sketchId]
      },
    },

    watch: {
      isHidden: function() {
        this.$root.$emit('component-changed', this.sketch.component)
      }
    },

    methods: {
      toggleVisibility: function() {
        this.$set(this.sketch.component.UIData.itemsHidden, this.sketchId, !this.isHidden)
      },
    },
  }
</script>
