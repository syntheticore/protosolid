<template lang="pug">

  .feature(
    :title="featureTitle"
    :class="featureStyle"
    :draggable="true"
    @dragstart="dragstart"
    @dragenter="dragenter"
    @dragleave="dragleave"
    @dragover.prevent="dragover"
    @drop.prevent="drop"
    @click="document.selection.handle(feature, bus.isCtrlPressed)"
    @dblclick="openFeature()"
  )

    Icon(:icon="feature.constructor.icon" fixed-width :class="{ future: isFuture }")

    nav.actions.bordered(@dblclick.stop)

      button(
        title="Move marker here"
        @click.stop="moveMarker()"
      )
        Icon(icon="directions" fixed-width)

      button(
        title="Edit"
        @click.stop="openFeature()"
      )
        Icon(icon="pen" fixed-width)

      button.delete(
        title="Delete"
        @click.stop="deleteFeature()"
      )
        Icon(icon="trash" fixed-width)

</template>


<style lang="stylus" scoped>

  .feature
    position: relative
    padding: 13px
    transition: all 0.15s
    border-inline: 1px solid transparent

    > svg
      transition: color 0.15s
      pointer-events: none

    &.active
    &.selected

      > svg
        color: $highlight !important

    &:hover.selected
      padding-top: 0
      padding-bottom: 26px

      .actions
        opacity: 1
        pointer-events: all
        transform: translateY(-7px) scale(1.0)

    &.error > svg
      color: lighten($red, 30%) !important

    &.warning > svg
      color: $warn !important

    &:hover

      > svg
        color: $bright1

    &.dragTarget
      border-right-color: $highlight

      &.targetLeft
        border-right-color: transparent
        border-left-color: $highlight

  .future
    opacity: 0.3

  .actions
    position: absolute
    bottom: -10px
    left: -17px
    overflow: hidden
    display: flex
    opacity: 0
    pointer-events: none
    transition: all 0.15s
    transform: scale(0.85)
    z-index: 1
    border-radius: 5px
    background: $dark2

  button
    background: none
    border: none
    color: $bright2
    padding: 4px
    transition: all 0.15s

    &:hover
      color: $bright1
      background: $dark1
      transition: none

    &:active
      background: $dark2 * 0.85

    &:disabled
      opacity: 0.3

    &.delete
      color: $cancel

</style>


<script>

  export default {
    name: 'FeatureIcon',

    inject: ['bus'],

    props: {
      document: Object,
      isFuture: Boolean,
      // selection: Object,
      feature: Object,
      isActive: Boolean,
    },

    data() {
      return {
        isDragTarget: false,
        targetLeft: false,
      }
    },

    computed: {
      featureTitle() {
        let title = this.feature.title
        const error = this.feature.error
        if(error) title += ': ' + error.msg
        return title
      },

      featureStyle() {
        const error = this.feature.error
        const style = {
          active: this.isActive,
          selected: this.document.selection.has(this.feature),
          dragTarget: this.isDragTarget,
          targetLeft: this.targetLeft,
        }
        if(error) style[error.type] = true
        return style
      },
    },

    mounted() {},

    methods: {
      openFeature() {
        this.bus.emit('close-feature')
        setTimeout(() => this.document.activateFeature(this.feature) )
      },

      deleteFeature() {
        this.document.removeFeature(this.feature)
      },

      moveMarker() {
        this.$emit('move-marker')
      },

      dragstart(e) {
        e.dataTransfer.setData('text/plain', this.feature.id)
      },

      dragenter(e) {
        const dragged = this.getDragged(e)
        this.isDragTarget = (this.feature != dragged)
        if(this.isDragTarget) e.dataTransfer.dropEffect = 'move'
      },

      dragleave() {
        this.isDragTarget = false
      },

      dragover(e) {
        const ratio = e.layerX / e.target.offsetWidth
        this.targetLeft = ratio < 0.5
      },

      drop(e) {
        if(e.dataTransfer.getData('text/plain') == 'marker') {
          const i = this.document.timeline.features.indexOf(this.feature) + (this.targetLeft ? 0 : 1)
          this.document.moveMarker(i)

        } else {
          const dragged = this.getDragged(e)
          if(this.feature == dragged) return
          this.document.reorder(dragged, this.feature, !this.targetLeft)
        }
        this.isDragTarget = false
      },

      getDragged(e) {
        const draggedId = e.dataTransfer.getData('text/plain')
        return this.document.timeline.features.find(f => f.id == draggedId )
      },
    },
  }

</script>
