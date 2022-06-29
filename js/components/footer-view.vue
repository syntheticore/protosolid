<template lang="pug">
  footer.footer-view

    //- .tool-info.bordered
    //-   b Select Tool
    //-   fa-icon(icon="mouse" fixed-width)
    //-   | Select geometry
    //-   fa-icon(icon="mouse" fixed-width)
    //-   | Bring up actions

    transition(name="fade")
      .selection-info.bordered(v-if="selection")
        div
          //- span # Objects
          span 1 {{type}}

        div(v-if="type == 'Solid'")
          span Weight
          span(v-if="selection.component.material")
            | {{ (selection.volume * selection.component.material.density).toFixed(2) }} g
          span.warn(v-else) No Material

        div(v-if="type == 'Component'")
          span Weight
          span(v-if="selection.getWeight() !== undefined")
            | {{ selection.getWeight().toFixed(2) }} g
          span.warn(v-else) No Material

        div(v-if="type == 'Solid'")
          span Volume
          span {{ selection.volume.toFixed(2) }} cm³

        div(v-if="type == 'Solid'")
          span Surface Area
          span {{ selection.area.toFixed(2) }} cm²

        div(v-if="type == 'Line' || type == 'BezierSpline'")
          span Length
          span {{ selection.get_length().toFixed(2) }} mm

        div(v-if="type == 'Circle'")
          span Radius
          span {{ selection.get_radius().toFixed(2) }} mm

        div(v-if="type == 'Circle'")
          span Diameter
          span {{ (selection.get_radius() * 2).toFixed(2) }} mm

        div(v-if="type == 'Circle'")
          span Circumfence
          span {{ selection.get_length().toFixed(2) }} mm

        div(v-if="type == 'Circle'")
          span Area
          span {{ selection.get_area().toFixed(2) }} mm²

    .debug-panel
      button.button(@click="splitAll") Split all
      button.button(@click="makeCube") Make Cube
      button.button(@click="makeCylinder") Make Cylinder
</template>


<style lang="stylus" scoped>
  .footer-view
    font-size: 13px
    padding: 12px
    color: $bright2
    display: flex
    // justify-content: space-between
    justify-content: flex-end
    align-items: flex-end
    pointer-events: none

  .tool-info
    // flex: 0 0 content
    padding: 16px 20px
    border-radius: 99px

  .selection-info
    line-height: 1.9
    padding: 4px 14px
    // table-layout: fixed
    white-space: nowrap
    padding: 0
    align-self: flex-end
    display: flex
    align-items: center

    div
      padding: 2px 14px
      &:first-child
        padding: 1px 14px
        font-size: 14px
        span
          padding-right: 0
      &:not(:last-child)
        border-right: 1px solid $dark1

    span:first-child
      // text-align: right
      padding-right: 8px

    span:last-child
      color: $bright1
      font-weight: bold

  b
    margin-right: 6px
    color: $bright1 * 0.9

  svg
    margin-left: 9px
    margin-right: 3px
    color: $bright1

  .warn
    color: $warn !important

  .debug-panel
    position: absolute
    right: calc(100vw / 2)
    bottom: 7px
    pointer-events: auto
    display: none
    white-space: nowrap
    [data-platform="browser"] &
      display: block

  .fade-enter-active
  .fade-leave-active
    transition: all 0.4s

  .fade-enter
  .fade-leave-to
    opacity: 0
    transform: translateY(12px)

</style>


<script>
  export default {
    name: 'FooterView',

    components: {},

    props: {
      selection: Object,
      activeComponent: Object,
    },

    computed: {
      type: function() {
        return this.selection.typename()
      },
    },

    methods: {
      splitAll: function() {
        const splits = this.activeComponent.real.get_sketch().get_all_split()
        this.$root.$emit('component-changed', this.activeComponent)
      },

      makeCube: function() {
        this.activeComponent.real.make_cube()
        this.$root.$emit('component-changed', this.activeComponent)
      },

      makeCylinder: function() {
        this.activeComponent.real.make_cylinder()
        this.$root.$emit('component-changed', this.activeComponent)
      },
    },
  }
</script>
