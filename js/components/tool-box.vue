<template lang="pug">
  .tool-box.bordered

      ul.tabs
        li(
          v-for="(tab, index) in tabs"
          @click="activateTab(index)"
          :class="{active: index == activeTab}"
        )
          | {{ tab.title }}

      ul.tools
        li(v-for="(tool, index) in tabs[activeTab].tools")

          button.button(
            :class="{active: isActive(tool)}"
            @click="activateTool(tool)"
          )
            fa-icon(:icon="tool.icon" fixed-width)
            .title(v-html="tool.title")
            .hot-key(v-if="tool.hotKey") {{ tool.hotKey }}

          transition(name="fade")
            FeatureBox(
              v-if="activeFeature && isActive(tool)"
              :active-tool="activeTool"
              :active-feature="activeFeature"
              @close="closeFeature"
            )
</template>


<style lang="stylus" scoped>
  .tool-box
    border-top-left-radius: 3px
    border-top-right-radius: 3px
    border-bottom-left-radius: 6px
    border-bottom-right-radius: 6px
    background: rgba($dark2, 0.9)
    // overflow: hidden

  .tabs
    display: flex
    // box-shadow: 0 0 4px rgba(black, 0.6)
    border-bottom: 1px solid $dark1 * 1.15
    overflow-x: auto
    li
      flex: 1 1 auto
      padding: 5px 10px
      background: $dark2 * 0.85
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
    .button
      text-align: center
      background: none
      border: none
      box-shadow: none
      padding: 5px 6px
      min-width: 55px
      padding-bottom: 4px
      margin-right: 0
      margin-bottom: 0
      text-shadow: none
      position: relative
      &:hover, &.active
        background: $dark1 * 1.15
        .title
          color: $bright1
      &:active
        background: $dark1 * 0.9
      &.active
        svg
          color: lighten($highlight, 25%)
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
    .hot-key
      position: absolute
      top: 1px
      right: 1px
      font-size: 9px
      color: $bright2
      background: $dark1
      padding: 0
      width: 12px
      border-radius: 2px
      border: 0.5px solid $dark1 * 1.4

  .feature-box
    margin-top: 10px
    position: absolute

  .fade-enter-active, .fade-leave-active
    transition: all 0.15s ease-out

  .fade-enter, .fade-leave-to
    opacity: 0
    transform: translateY(10px)
</style>


<script>
  import FeatureBox from './feature-box.vue'
  import { ExtrudeFeature, RevolveFeature } from './../features.js'

  import {
    ManipulationTool,
    ObjectSelectionTool,
    ProfileSelectionTool,
    LineTool,
    SplineTool,
    CircleTool,
    ArcTool,
    SetPlaneTool,
    TrimTool,
  } from './../tools.js'

  export default {
    name: 'ToolBox',

    components: {
      FeatureBox,
    },

    props: {
      activeTool: Object,
      activeComponent: Object,
    },

    data() {
      return {
        activeTab: 1,
        activeFeature: null,
        tabs: [
          {
            title: 'Sketch',
            tools: [
              { title: 'Line', tool: LineTool, icon: 'project-diagram', hotKey: 'L', keyCode: 76 },
              { title: 'Rectangle', icon: 'vector-square', hotKey: 'R', keyCode: 82 },
              { title: 'Arc', tool: ArcTool, icon: 'bezier-curve' },
              { title: 'Circle', tool: CircleTool, icon: 'ban', hotKey: 'C', keyCode: 67 },
              { title: 'Spline', tool: SplineTool, icon: 'route'},
              { title: 'Trim', tool: TrimTool,  icon: 'route',  hotKey: 'T', keyCode: 84},
            ]
          },
          {
            title: 'Create',
            tools: [
              { title: 'Extrude', feature: ExtrudeFeature, icon: 'box', hotKey: 'E', keyCode: 69 },
              { title: 'Revolve', feature: RevolveFeature, icon: 'wave-square', hotKey: 'V', keyCode: 86 },
              { title: 'Loft', icon: 'layer-group' },
              { title: 'Sweep', icon: 'route' },
              { title: 'Mirror', icon: 'band-aid', hotKey: 'M', keyCode: 77 },
              { title: 'Array', icon: 'th' },
            ],
          },
          {
            title: 'Edit',
            tools: [
              { title: 'Shell', icon: 'magnet' },
              { title: 'Boolean', icon: 'boxes' },
              { title: 'Fillet', icon: 'clone', hotKey: 'F', keyCode: 70 },
              { title: 'Chamfer', icon: 'screwdriver', hotKey: 'H', keyCode: 72 },
              { title: 'Split', icon: 'code-branch' },
            ],
          },
          {
            title: 'Construct',
            tools: [
              { title: 'Set Plane', tools: SetPlaneTool, icon: 'edit', hotKey: 'S', keyCode: 83 },
            ]
          },
          { title: 'Constrain', tools: [] },
          { title: 'Simulate', tools: [] },
          { title: 'Make', tools: [] },
        ]
      }
    },

    mounted: function() {
      window.addEventListener('keydown', (e) => {
        console.log(e.keyCode)
        const tool = this.tabs.flatMap(tab => tab.tools).find(
          tool => tool.keyCode == e.keyCode
        )
        if(tool) {
          this.activateTool(tool)
        } else if(e.keyCode === 27) {
          this.$root.$emit('escape')
        }
      });

      this.$root.$on('escape', () => {
        if(this.activeTool.constructor === ManipulationTool) {
          this.closeFeature()
        } else {
          this.$root.$emit('activate-toolname', 'Manipulate')
        }
      })
    },

    methods: {
      activateTab: function(index) {
        this.closeFeature()
        this.activeTab = index
      },

      activateTool: function(tool) {
        // Don't activate features twice
        if(this.activeFeature && this.activeFeature.constructor === tool.feature) return
        this.activateTab(this.tabs.findIndex(tab => tab.tools.some(t => t === tool)))
        if(tool.feature) {
          this.activeFeature = new tool.feature(this.activeComponent)
        } else {
          setTimeout(() => this.$root.$emit('activate-toolname', tool.title) )
        }
      },

      isActive: function(tool) {
        return (this.activeFeature && this.activeFeature.constructor === tool.feature)
        || (this.activeTool && this.activeTool.constructor === tool.tool)
      },

      closeFeature: function() {
        this.activeFeature = null
      },
    },
  }
</script>
