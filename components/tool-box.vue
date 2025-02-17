<template lang="pug">

  .tool-box.bordered


    ul.tabs

      template(v-for="(tab, index) in tabs")

        li(
          v-if="!!document.activeSketch == !!tab.sketchOnly"
          :class="{ active: index == activeTab }"
          @click="activateTab(index)"
        )
          | {{ tab.title }}

    .main

      label.reference(v-if="document.activeSketch" title="Reference geometry")

        Icon(icon="asterisk")
        .hot-key X

        input(
          type="checkbox"
          :checked="refChecked"
          :indeterminate.prop="refIndeterminate"
          :key="Math.random()"
          @click.prevent="toggleReference"
        )

      ul.tools

        li(v-for="(tool, index) in tabs[activeTab].tools")

          button.button(
            :class="{active: isActive(tool)}"
            :title="tool.title"
            :disabled="!tool.tool && !tool.feature && !tool.action"
            @click="activateTool(tool)"
          )
            Icon(:icon="(tool.feature && tool.feature.icon) || (tool.tool && tool.tool.icon) || tool.icon" fixed-width)
            .title(v-html="tool.title")
            .hot-key(v-if="tool.hotKey") {{ tool.hotKey }}

          transition(name="fade")
            FeatureBox.tipped(
              v-if="feature && isActive(tool)"
              :document="document"
              :active-tool="activeTool"
              :active-feature="feature"
              @close="closeFeature"
            )

</template>


<style lang="stylus" scoped>

  .tool-box
    border-top-left-radius: 3px
    border-top-right-radius: 3px
    border-bottom-left-radius: 6px
    border-bottom-right-radius: 6px
    min-width: 565px
    // overflow: hidden

  .main
    display: flex

  .reference
    position: relative
    border-right: 1px solid $dark1
    display: flex
    flex-direction: column
    font-size: 0.7rem
    justify-content: center
    gap: 0.5rem
    padding-inline: 0.5rem

    .hot-key
      top: 6px
      right: 2px

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
      min-width: 100px

      &:first-child
        border-top-left-radius: 2px

      &:last-child
        border-top-right-radius: 2px

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
      // text-align: center
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
    width: 12px
    height: 12px
    border-radius: 2px
    border: 0.5px solid $dark1 * 1.4
    display: flex
    align-items: center
    justify-content: center

  .feature-box
    margin-top: 9px
    position: absolute

  .fade-enter-active, .fade-leave-active
    transition: all 0.15s ease-out

  .fade-enter-from, .fade-leave-to
    opacity: 0
    transform: translateY(10px)

</style>


<script>

  import * as THREE from 'three'
  import { makeID } from './../js/core/id.js'
  import { rotationFromNormal } from './../js/core/utils.js'

  import {
    SketchElement,
  } from './../js/core/geom2d.js'

  import {
    CreateSketchFeature,
    PlaneFeature,
    ExtrudeFeature,
    FilletFeature,
    DraftFeature,
    RevolveFeature,
    SweepFeature,
    OffsetFeature,
    SplitFeature,
  } from './../js/core/features.js'

  import {
    ManipulationTool,
    LineTool,
    SplineTool,
    CircleTool,
    ArcTool,
    TrimTool,
    PerpendicularConstraintTool,
    HorVertConstraintTool,
    ParallelConstraintTool,
    EqualConstraintTool,
    TangentConstraintTool,
    FixConstraintTool,
    DimensionTool,
    ProjectTool,
  } from './../js/tools.js'

  export default {
    name: 'ToolBox',

    inject: ['bus'],

    props: {
      document: Object,
      activeTool: Object,
    },

    watch: {
      'document.activeSketch': function(sketch) {
        if(!this.feature) this.activeTab = this.document.activeSketch ? 1 : 4
      },
    },

    computed: {
      refChecked: function() {
        if(this.document.selection.items.length) {
          return this.document.selection.items.some(sel => sel.isReference )
        } else {
          return this.document.referenceMode
        }
      },

      refIndeterminate: function() {
        const first = this.document.selection.items[0]
        if(!first) return false
        return this.document.selection.items.some(sel => sel.isReference != first.isReference )
      },
    },

    data() {
      return {
        activeTab: 0,
        feature: null,
        tabs: [
          {
            title: 'Construct',
            tools: [
              { title: 'Sketch', feature: CreateSketchFeature, hotKey: 'S', keyCode: 83 },
              // { title: 'Plane', tool: PlaneTool, icon: 'edit', hotKey: 'P', keyCode: 80 },
              { title: 'Plane', feature: PlaneFeature },
              { title: 'Axis', icon: 'grip-lines' },
              { title: 'Point', icon: 'asterisk' },
              { title: 'Center of Mass', action: this.addCog, icon: 'atom' },
              { title: 'Parameter', action: this.addParameter, icon: 'square-root-alt' },
            ]
          },
          {
            title: 'Sketch',
            sketchOnly: true,
            tools: [
              { title: 'Line', tool: LineTool, hotKey: 'L', keyCode: 76 },
              { title: 'Rectangle', icon: 'vector-square', hotKey: 'R', keyCode: 82 },
              { title: 'Arc', tool: ArcTool, hotKey: 'A', keyCode: 65 },
              { title: 'Circle', tool: CircleTool, hotKey: 'C', keyCode: 67 },
              { title: 'Spline', tool: SplineTool, hotKey: 'B', keyCode: 66},
              { title: 'Polygon', icon: 'layer-group' },
              { title: 'Text', icon: 'layer-group' },
              { title: 'Split all', action: this.splitAll, icon: 'file-export' },
            ]
          },
          {
            title: 'Edit Sketch',
            sketchOnly: true,
            tools: [
              { title: 'Reference', action: this.toggleReference, icon: 'asterisk' },
              { title: 'Trim', tool: TrimTool, hotKey: 'T', keyCode: 84},
              { title: 'Break', icon: 'layer-group' },
              { title: 'Extend', icon: 'layer-group' },
              { title: 'Offset', icon: 'layer-group' },
              { title: 'Project', tool: ProjectTool, hotKey: 'P', keyCode: 80 },
              { title: 'Intersect', icon: 'layer-group' },
            ]
          },
          {
            title: 'Constrain',
            sketchOnly: true,
            tools: [
              { title: 'Dimension', tool: DimensionTool, hotKey: 'D', keyCode: 68 },
              { title: 'Touch', icon: 'object-group' },
              { title: 'Parallel', tool: ParallelConstraintTool }, //XXX Use also for hor/vert
              { title: 'Perpendicular', tool: PerpendicularConstraintTool },
              { title: 'Hor/Vert', tool: HorVertConstraintTool },
              { title: 'Tangent', tool: TangentConstraintTool },
              { title: 'Equal', tool: EqualConstraintTool },
              { title: 'Fix', tool: FixConstraintTool }, //XXX also ground for assemblies
            ],
          },
          {
            title: 'Solid',
            tools: [
              { title: 'Extrude', feature: ExtrudeFeature, hotKey: 'E', keyCode: 69 },
              { title: 'Revolve', feature: RevolveFeature, hotKey: 'V', keyCode: 86 },
              { title: 'Loft', icon: 'layer-group' },
              { title: 'Sweep', feature: SweepFeature },
              { title: 'Thicken', icon: 'layer-group' },
              { title: 'Coil', icon: 'layer-group' },
              { title: 'Web', icon: 'layer-group' },
              { title: 'Pattern', icon: 'th' },
            ],
          },
          {
            title: 'Edit Solid',
            tools: [
              { title: 'Offset', feature: OffsetFeature },
              { title: 'Boolean', icon: 'boxes' },
              { title: 'Fillet', feature: FilletFeature, hotKey: 'F', keyCode: 70 },
              { title: 'Chamfer', icon: 'screwdriver', hotKey: 'H', keyCode: 72 },
              { title: 'Draft', feature: DraftFeature },
              { title: 'Split', feature: SplitFeature },
              { title: 'Align', icon: 'layer-group' }, //XXX also -> Replace Face
              { title: 'Mirror', icon: 'band-aid', hotKey: 'M', keyCode: 77 },
            ],
          },
          {
            title: 'Simulate',
            tools: [
              // { title: 'Material', feature: MaterialFeature },
              { title: 'Joint', icon: 'code-branch' },
              { title: 'Group', icon: 'object-group' },
              { title: 'Motion Link', icon: 'link' },
              { title: 'Animation', icon: 'feather' }, //icon: 'clapperboard'
              { title: 'Static Load', icon: 'weight' },
              { title: 'Heat Flow', icon: 'thermometer' },
              // { title: 'Modal Frequencies', icon: 'wave-square' },
            ],
          },
          {
            title: 'Inspect',
            tools: [
              { title: 'Interference', icon: 'traffic-light' }, //XXX Save as treelet
              { title: 'Curvature', icon: 'route' },
              { title: 'Shading', icon: 'palette' },
              { title: 'Section View', icon: 'object-group', action: this.addSectionView },
            ],
          },
          {
            title: 'Tools',
            tools: [
              { title: 'Render', icon: 'lightbulb' },
              { title: 'Canvas', icon: 'layer-group' },
              { title: 'Export Configuration', action: this.addExportConfig, icon: 'file-export' },
            ],
          },
        ]
      }
    },

    mounted: function() {
      this.bus.on('keydown', (key) => {
        const tool = this.tabs.flatMap(tab => tab.tools ).find(
          tool => tool.hotKey && tool.hotKey.toLowerCase() == key
        )
        if(tool) this.activateTool(tool)
      })
      this.bus.on('close-feature', this.closeFeature)
    },

    methods: {
      activateTab: function(index) {
        this.closeFeature()
        this.activeTab = index
      },

      activateTool: function(tool) {
        // Don't activate features twice
        if(this.feature && this.feature.constructor === tool.feature) return
        this.activateTab(this.tabs.findIndex(tab => tab.tools.some(t => t === tool)))
        setTimeout(() => {
          if(tool.feature) {
            this.feature = new tool.feature(this.document)
            // this.$emit('add-feature', this.feature)
            this.document.addFeature(this.feature)

          } else if(tool.action) {
            tool.action()

          } else if(tool.tool) {
            this.bus.emit('activate-tool', tool.tool)
          }
        })
      },

      isActive: function(tool) {
        return (this.feature && this.feature.constructor === tool.feature)
        || (this.activeTool && this.activeTool.constructor === tool.tool)
      },

      closeFeature: function() {
        this.feature = null
      },

      addCog: function() {
        this.document.activeComponent.creator.cog = true
      },

      addParameter: function() {
        this.document.activeComponent.creator.parameters.push({
          name: 'width',
          value: '512mm',
        })
      },

      addSectionView: function() {
        this.document.activeComponent.creator.sectionViews.push({
          id: makeID(),
          transform: rotationFromNormal(new THREE.Vector3(0.0, 0.0, 1.0)),
        })
        this.document.emit('component-changed', this.document.activeComponent)
      },

      addExportConfig: function() {
        this.document.activeComponent.creator.exportConfigs.push({
          title: 'High Detail',
          path: null,
          format: 'STL',
          maxDistance: 0.01,
          maxAngle: 1.0,
          autoSave: false,
        })
      },

      toggleReference: function() {
        if(!this.document.selection.items.length) {
          this.document.referenceMode = !this.document.referenceMode
        } else {
          this.document.selection.items
            .filter(item => item instanceof SketchElement )
            .forEach(elem => elem.isReference = !elem.isReference )
          this.document.emit('component-changed', this.document.activeComponent)
        }
      },

      splitAll: function() {
        const elems = this.document.activeSketch.elements
        const elements = this.document.activeSketch.removeEmpties(elems)
        elems.forEach(elem => this.document.activeSketch.remove(elem))
        const cutElements = elements.flatMap(elem => elem.split(elements) )
        const removed = this.document.activeSketch.removeDanglingSegments(cutElements)
        removed.forEach(elem => this.document.activeSketch.add(elem) )
        this.document.emit('component-changed', this.document.activeComponent)
      },
    },
  }

</script>
