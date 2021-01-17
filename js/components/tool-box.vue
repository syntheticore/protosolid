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
            :title="tool.title"
            :disabled="!tool.tool && !tool.feature && !tool.action"
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
    background: rgba($dark2, 0.95)
    min-width: 565px
    // overflow: hidden

  .tabs
    display: flex
    // box-shadow: 0 0 4px rgba(black, 0.6)
    border-bottom: 1px solid $dark1 * 1.15
    li
      flex: 1 1 auto
      padding: 5px 10px
      background: $dark2 * 0.85
      font-size: 12px
      text-align: center
      transition: all 0.2s
      min-width: 60px
      & + li
        border-left: 1px solid $dark1 * 1.15
      &:hover
        background: $dark2 * 1.3
      &:active
      &.active
        background: $dark1 * 1.15

  .tools
    display: flex
    li
      max-width: 86px
      min-width: 65px
      margin: 4px
    .button
      text-align: center
      background: none
      border: none
      box-shadow: none
      padding: 5px 6px
      min-width: 55px
      padding-bottom: 4px
      // margin-right: 0
      margin: 0
      margin-bottom: 0
      text-shadow: none
      position: relative
      width: 100%
      &:hover, &.active
        background: $dark1 * 1.15
        .title
          color: $bright1
          transition: none
        svg
          transition: none
        .hot-key
          border-color: $dark1 * 1.9
      &:active
        background: $dark1 * 1.075
      &:disabled
        filter: brightness(50%)
      &.active
        svg
          color: lighten($highlight, 25%)
    svg
      font-size: 21px
      color: $bright1
      transition: all 0.15s
      filter: none
    .title
      color: $bright2
      font-size: 11px
      margin-top: 6px
      font-weight: bold
      white-space: nowrap
      // line-height: 1.3
      overflow: hidden
      text-overflow: ellipsis
      transition: all 0.15s
    .hot-key
      position: absolute
      top: 2px
      right: 2px
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

  import {
    ExtrudeFeature,
    RevolveFeature,
    SweepFeature,
    MaterialFeature,
  } from './../features.js'

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
            title: 'Construct',
            tools: [
              { title: 'Plane', tool: SetPlaneTool, icon: 'edit', hotKey: 'S', keyCode: 83 },
              { title: 'Axis', icon: 'edit' },
              { title: 'Point', icon: 'edit' },
              { title: 'Center of Mass', action: this.addCog, icon: 'atom' },
              { title: 'Parameter', action: this.addParameter, icon: 'square-root-alt' },
            ]
          },
          {
            title: 'Sketch',
            tools: [
              { title: 'Line', tool: LineTool, icon: 'project-diagram', hotKey: 'L', keyCode: 76 },
              { title: 'Rectangle', icon: 'vector-square', hotKey: 'R', keyCode: 82 },
              { title: 'Arc', tool: ArcTool, icon: 'bezier-curve' },
              { title: 'Circle', tool: CircleTool, icon: 'ban', hotKey: 'C', keyCode: 67 },
              { title: 'Spline', tool: SplineTool, icon: 'route'},
              { title: 'Polygon', icon: 'layer-group' },
              { title: 'Text', icon: 'layer-group' },
            ]
          },
          {
            title: 'Edit Sketch',
            tools: [
              { title: 'Trim', tool: TrimTool,  icon: 'route',  hotKey: 'T', keyCode: 84},
              { title: 'Break', icon: 'layer-group' },
              { title: 'Extend', icon: 'layer-group' },
              { title: 'Offset', icon: 'layer-group' },
              { title: 'Project', icon: 'layer-group' },
              { title: 'Intersect', icon: 'layer-group' },
            ]
          },
          {
            title: 'Solid',
            tools: [
              { title: 'Extrude', feature: ExtrudeFeature, icon: 'box', hotKey: 'E', keyCode: 69 },
              { title: 'Revolve', feature: RevolveFeature, icon: 'wave-square', hotKey: 'V', keyCode: 86 },
              { title: 'Loft', icon: 'layer-group' },
              { title: 'Sweep', feature: SweepFeature, icon: 'route' },
              { title: 'Thicken', icon: 'layer-group' },
              { title: 'Coil', icon: 'layer-group' },
              { title: 'Web', icon: 'layer-group' },
              { title: 'Pattern', icon: 'th' },
            ],
          },
          {
            title: 'Edit',
            tools: [
              { title: 'Shell', icon: 'magnet' },
              { title: 'Boolean', icon: 'boxes' },
              { title: 'Fillet', icon: 'clone', hotKey: 'F', keyCode: 70 },
              { title: 'Chamfer', icon: 'screwdriver', hotKey: 'H', keyCode: 72 },
              { title: 'Draft', icon: 'layer-group' },
              { title: 'Split', icon: 'layer-group' },
              { title: 'Align', icon: 'layer-group' }, //XXX also -> Replace Face
              { title: 'Mirror', icon: 'band-aid', hotKey: 'M', keyCode: 77 },
            ],
          },
          {
            title: 'Constrain',
            tools: [
              { title: 'Dimension', icon: 'traffic-light' },
              { title: 'Touch', icon: 'object-group' },
              { title: 'Parallel', icon: 'code-branch' }, //XXX Use also for hor/vert
              { title: 'Perpendicular', icon: 'object-group' },
              { title: 'Tangent', icon: 'object-group' },
              { title: 'Equal', icon: 'object-group' },
              { title: 'Fix', icon: 'object-group' },
            ],
          },
          {
            title: 'Assemble',
            tools: [
              { title: 'Joint', icon: 'traffic-light' },
              { title: 'Group', icon: 'object-group' },
              { title: 'Motion Link', icon: 'layer-group' },
              { title: 'Animation', icon: 'layer-group' },
            ],
          },
          {
            title: 'Analyze',
            tools: [
              { title: 'Interference', icon: 'traffic-light' }, //XXX Save as treelet
              { title: 'Curvature', icon: 'layer-group' },
              { title: 'Shading', icon: 'layer-group' },
              { title: 'Section View', icon: 'object-group' },
              { title: 'Canvas', icon: 'object-group' },
            ],
          },
          {
            title: 'Simulate',
            tools: [
              { title: 'Material', feature: MaterialFeature, icon: 'volleyball-ball' },
              { title: 'Static Load', icon: 'weight' },
              { title: 'Heat Flow', icon: 'thermometer' },
            ],
          },
        ]
      }
    },

    mounted: function() {
      window.addEventListener('keydown', (e) => {
        const tool = this.tabs.flatMap(tab => tab.tools).find(
          tool => tool.keyCode == e.keyCode
        )
        if(tool) {
          this.activateTool(tool)
        } else if(e.keyCode === 27) {
          this.$root.$emit('escape')
        }
        console.log(e.keyCode)
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
        setTimeout(() => {
          if(tool.feature) {
            this.activeFeature = new tool.feature(this.activeComponent)
          } else if(tool.action) {
            tool.action()
          } else {
            this.$root.$emit('activate-toolname', tool.title)
          }
        })
      },

      isActive: function(tool) {
        return (this.activeFeature && this.activeFeature.constructor === tool.feature)
        || (this.activeTool && this.activeTool.constructor === tool.tool)
      },

      closeFeature: function() {
        this.activeFeature = null
      },

      addCog: function() {
        this.activeComponent.cog = true
      },

      addParameter: function() {
        this.activeComponent.parameters.push({
          name: 'width',
          value: '512mm',
        })
      },
    },
  }
</script>
