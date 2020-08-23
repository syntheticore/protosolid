<template lang="pug">
  .tool-box
    .bordered.shelf
      ul.tabs
        li(
          v-for="(tab, index) in tabs"
          @click="activeTab = index"
          :class="{active: index == activeTab}"
        )
          | {{ tab.title }}
      ul.tools
        li.button(
          v-for="(tool, index) in tabs[activeTab].tools"
          :class="{active: index == 0}"
          @click="tool.type == 'feature' ? activateFeature(tool.title) : activateTool(tool.title)"
        )
          fa-icon(:icon="tool.icon" fixed-width)
          .title(v-html="tool.title")

    component(:is="activeFeatureComponent")
</template>


<style lang="stylus" scoped>
  .tool-box
    display: flex
    flex-direction: column
    align-items: center
    // align-items: start

  .shelf
    border-top-left-radius: 3px
    border-top-right-radius: 3px
    border-bottom-left-radius: 6px
    border-bottom-right-radius: 6px
    background: rgba($dark2, 0.9)
    overflow: hidden
    // min-width: 405px

  .tabs
    box-shadow: 0 0 4px rgba(black, 0.6)
    border-bottom: 1px solid $dark1 * 1.15
    display: flex
    overflow-x: auto
    li
      padding: 5px 10px
      background: $dark2
      font-size: 12px
      text-align: center
      transition: all 0.2s
      & + li
        border-left: 1px solid $dark1 * 1.15
      &:hover
        background: $dark2 * 1.3
      &:active
      &.active
        background: $dark1 * 1.15

  .tools
    padding-right: 4px
    padding-bottom: 4px
    li
      display: inline-block
      text-align: center
      // background: $dark1
      background: none
      border: none
      box-shadow: none
      padding: 5px 6px
      min-width: 55px
      padding-bottom: 4px
      margin-right: 0
      margin-bottom: 0
      text-shadow: none
      &:hover, &.active
        background: $dark1 * 1.15
        svg
          color: $highlight
        .title
          color: $bright1
      &:active
        background: $dark1 * 0.9
      svg
        font-size: 21px
        color: $bright1
        // transition: all 0.15s
        filter: none
      .title
        color: $bright2
        font-size: 11px
        margin-top: 6px
        font-weight: bold

  .feature-box
    // display: inline-block
    margin-top: 12px
</style>


<script>
  import FeatureBox from './feature-box.vue'

  export default {
    name: 'ToolBox',

    components: {
      FeatureBox,
    },

    props: {
      activeTool: Object,
    },

    data() {
      return {
        activeTab: 1,
        activeFeatureComponent: null,
        tabs: [
          {
            title: 'Sketch',
            tools: [
              { title: 'Line', icon: 'project-diagram' },
              { title: 'Rectangle', icon: 'vector-square' },
              { title: 'Arc', icon: 'bezier-curve' },
              { title: 'Circle', icon: 'ban' },
              { title: 'Spline', icon: 'route' },
            ]
          },
          {
            title: 'Create',
            tools: [
              { title: 'Extrude', icon: 'box', type: 'feature' },
              { title: 'Revolve', icon: 'wave-square' },
              { title: 'Loft', icon: 'layer-group' },
              { title: 'Sweep', icon: 'route' },
              { title: 'Mirror', icon: 'band-aid' },
              { title: 'Array', icon: 'th' },
            ],
          },
          {
            title: 'Edit',
            tools: [
              { title: 'Shell', icon: 'magnet' },
              { title: 'Boolean', icon: 'boxes' },
              { title: 'Fillet', icon: 'clone' },
              { title: 'Chamfer', icon: 'screwdriver' },
              { title: 'Split', icon: 'code-branch' },
            ],
          },
          { title: 'Construct', tools: [] },
          { title: 'Constrain', tools: [] },
          { title: 'Simulate', tools: [] },
          { title: 'Make', tools: [] },
        ]
      }
    },

    mounted() {
      this.activateFeature('FeatureBox')
    },

    methods: {
      activateTool: function(toolName) {
        this.$root.$emit('activate-toolname', toolName)
      },

      activateFeature: function(featureName) {
        this.activeFeatureComponent = featureName
      },
    },
  }
</script>
