<template lang="pug">

  .feature(
    :title="featureTitle"
    :class="featureStyle"
    @click="document.selection = document.selection.handle(feature, bus.isCtrlPressed)"
    @dblclick="openFeature()"
  )
    Icon(:icon="feature.constructor.icon" fixed-width :class="{future: isFuture}")

    nav.actions.bordered
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
    > svg
      transition: color 0.15s
    &:hover > svg
      color: $bright1
    &.active > svg
      color: $highlight
    &.error > svg
      color: darken($red, 30%) !important
    &.warning > svg
      color: $warn !important
    &:hover
      padding-top: 0
      padding-bottom: 26px
      .actions
        opacity: 1
        pointer-events: all
        transform: translateY(-7px) scale(1.0)

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
    border-radius: 99px
    background: rgb($dark2)
    -webkit-backdrop-filter: none
    backdrop-filter: none

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
      return {}
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
          active: this.isActive || this.document.selection.has(this.feature),
        }
        if(error) style[error.type] = true
        return style
      },
    },

    mounted() {},

    methods: {
      openFeature() {
        this.bus.emit('close-feature')
        // setTimeout(() => this.$emit('update:active-feature', this.feature), 0)
        setTimeout(() => this.document.activateFeature(this.feature) )
      },

      deleteFeature() {
        // this.$emit('delete-feature', this.feature)
        this.document.removeFeature(this.feature)
      },

      moveMarker() {
        this.$emit('move-marker')
      },
    },
  }

</script>
