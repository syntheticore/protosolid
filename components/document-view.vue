<template lang="pug">

  main.document-view(@pointerdown="bus.emit('close-widgets')")

    ViewPort(
      :document="document"
      :active-view="document.previewView || document.activeView"
      :display-mode="effectiveDisplayMode"
      :color-mode="effectiveColorMode"
      :projection-mode="effectiveProjectionMode"
      @update:projection-mode="projectionMode = $event"
      v-model:active-tool="activeTool"
      v-model:highlight="highlight"
    )

    ToolBox(
      :document="document"
      :active-tool="activeTool"
    )

    .side-bar.left

      TreeView(
        :top="tree"
        :document="document"
        v-model:highlight="highlight"
      )

    .side-bar.right

      section.side-section

        h1 View

        ViewChooser(:document="document")

        .buttons

          .view-row
            .view-label Zoom

            .flex.gap

              IconButton(
                icon="camera"
                title="Fit All"
                @mouseenter="bus.emit('preview-zoom-all')"
                @mouseleave="bus.emit('unpreview-camera')"
                @click="bus.emit('zoom-all')"
              )

              IconButton(
                icon="crop-alt"
                title="Fit Active"
                @mouseenter="bus.emit('preview-zoom-active')"
                @mouseleave="bus.emit('unpreview-camera')"
                @click="bus.emit('zoom-active')"
              )

              IconButton(
                v-if="document.selection.items.length"
                icon="search-plus"
                title="Fit Selection"
                @mouseenter="bus.emit('preview-zoom-selection')"
                @mouseleave="bus.emit('unpreview-camera')"
                @click="bus.emit('zoom-selection')"
              )

          .view-row
            .view-label Nearest Plane

            IconButton(
              icon="solar-panel"
              title="Look at nearest plane"
              @mouseenter="lookAtPlane(true)"
              @mouseleave="bus.emit('unpreview-camera')"
              @click="lookAtPlane()"
            )


      section.side-section

        h1 DISPLAY

        .display-row
          .display-label Projection
          RadioBar(
            :items="projectionModes"
            hot-key="O"
            v-model:chosen="projectionMode"
            @hover="previewProjectionMode = $event"
            @unhover="previewProjectionMode = null"
          )

        .display-row
          .display-label Shading
          RadioBar(
            :items="displayModes"
            hot-key="W"
            v-model:chosen="displayMode"
            @hover="previewDisplayMode = $event"
            @unhover="previewDisplayMode = null"
          )

        .display-row
          .display-label Material
          RadioBar(
            :items="colorModes"
            hot-key="M"
            v-model:chosen="colorMode"
            @hover="previewColorMode = $event"
            @unhover="previewColorMode = null"
          )

      .pose(:class="{ hidden: !document.top().hasPoseChange() }")

        h1 POSE

        Acceptor(
          @accept="document.keepPose()"
          @reject="document.top().resetPose()"
        )

    FooterView(
      :document="document"
      :class="{ top: !!document.activeFeature }"
    )

    FeatureBar(
      :document="document"
      :active-tool="activeTool"
    )

</template>


<style lang="stylus" scoped>

  .document-view
    overflow: hidden
    display: flex
    justify-content: center
    min-width: 770px

  .tool-box
    position: absolute
    margin-top: (12 + 38)px
    z-index: 2

  .side-bar
    position: absolute
    bottom: 0
    pointer-events: none

    h1
      text-align: center
      color: $bright2
      color: gray * 1.5
      font-size: 13px
      font-weight: bold
      letter-spacing: 1px
      margin-bottom: 8px
      text-transform: uppercase
      text-shadow: 0 1px 2px rgba(0,0,0, 0.7)

    &.left
      top: 38px
      left: 0
      overflow: hidden

    &.right
      // top: (14 + 38)px
      top: 34px
      right: 12px
      bottom: 0.25rem
      width: 110px
      display: flex
      flex-direction: column

      h1
        margin-top: 1rem

      .side-section
        display: flex
        flex-direction: column
        pointer-events: auto

        &:hover
          .view-label
          .display-label
            opacity: 1

      .view-chooser
      .radio-bar
        margin-bottom: 0.5rem

      .view-row
      .display-row
        align-items: center
        align-self: flex-end
        display: grid
        gap: 0.5rem
        grid-template-columns: 64px 110px

      .view-label
      .display-label
        color: $bright2
        font-size: 11px
        opacity: 0
        text-align: right
        transition: opacity 0.15s
        white-space: nowrap

      .view-row
        > .flex
          min-width: 0

        .button
          min-width: 0

      .display-row
        margin-bottom: 0.5rem

        .radio-bar
          margin-bottom: 0
          min-width: 0

    .pose
      transition: all 0.25s
      height: 85px
      flex: 0 0 auto

      &.hidden
        opacity: 0
        height: 0
        transform: translateY(10px)

      h1
        margin-top: 0.5rem

  .buttons
    pointer-events: auto
    display: flex
    flex-direction: column
    gap: 0.25rem

  .view-port
    width: 100%
    height: 100%

  .footer-view
    position: absolute
    bottom: 70px

    &.top
      bottom: unset
      top: (100 + 38)px

  .feature-bar
    position: absolute
    bottom: 12px
    max-width: calc(100% - 24px)

</style>


<script>

  import * as THREE from 'three'

  import {
    CreateComponentFeature,
    CreateSketchFeature,
  } from './../js/core/features.js'

  import { ManipulationTool } from './../js/tools.js'
  import { rotationFromNormal } from './../js/core/utils.js'

  export default {
    name: 'DocumentView',

    inject: ['bus', 'highlight'],

    props: {
      document: Object,
    },

    data() {
      return {
        activeTool: null,
        projectionModes: {
          perspective: {
            title: 'Perspective',
            icon: 'cube',
          },
          orthographic: {
            title: 'Orthographic',
            icon: 'vector-square',
          },
        },
        projectionMode: 'orthographic',
        previewProjectionMode: null,
        displayModes: {
          wireShade: {
            title: 'Shaded + Wire',
            icon: 'wine-bottle',
          },
          wireframe: {
            title: 'Wireframe',
            icon: 'atlas',
          },
          shaded: {
            title: 'Shaded',
            icon: 'umbrella-beach',
          },
        },
        colorModes: {
          component: {
            title: 'Component Colors',
            icon: 'box',
          },
          // random: {
          //   title: 'Random Colors',
          //   icon: 'dice',
          // },
          material: {
            title: 'Materials',
            icon: 'volleyball-ball',
          },
        },
        displayMode: 'wireShade',
        previewDisplayMode: null,
        colorMode: 'material',
        previewColorMode: null,
      }
    },

    watch: {
      document: {
        immediate: true,
        handler: function(document, oldDocument) {
          if(oldDocument) oldDocument.activeView = oldDocument.activeView || oldDocument.dirtyView
          if(!document.activeView) {
            document.activeView = document.views[3]
          }
          this.highlight = null
        },
      },
    },

    computed: {
      tree: function() {
        return this.document.top()
      },

      effectiveColorMode: function() {
        if(this.document.activeSimulation) return this.document.activeSimulation.mode
        if(this.activeTool?.interferenceMode) return 'interference'
        return this.activeTool?.diagnosticMode
          ? `diagnostic:${this.activeTool.diagnosticMode}:${this.activeTool.draftDirection}:${this.activeTool.zebraFrequency}`
          : this.previewColorMode || this.colorMode
      },

      effectiveProjectionMode: function() {
        return this.previewProjectionMode || this.projectionMode
      },

      effectiveDisplayMode: function() {
        if(this.document.activeSimulation) return 'shaded'
        const exportConfig = this.document.activeExportConfig
        if(exportConfig && exportConfig.format != 'STEP') return 'exportPreview'
        return this.activeTool?.diagnosticMode || this.activeTool?.interferenceMode
          ? 'shaded'
          : this.previewDisplayMode || this.displayMode
      },
    },

    mounted() {
      this.bus.on('keydown', this.keyDown)
      this.bus.emit('activate-tool', ManipulationTool)
    },

    methods: {
      lookAtPlane: function(preview=false) {
        let plane
        if(this.document.activeSketch) {
          plane = this.document.activeSketch.workplane

        } else {
          const forward = window.alcRenderer.activeCamera.getWorldDirection(new THREE.Vector3()).toArray()
          const abs = forward.map(v => Math.abs(v) )
          let i = abs.indexOf(Math.max(...abs))
          const normal = [0,0,0]
          normal[i] = Math.sign(forward[i])
          plane = rotationFromNormal(new THREE.Vector3().fromArray(normal))
        }
        if(preview) {
          this.bus.emit('preview-look-at', plane)
        } else {
          this.bus.emit('commit-camera-preview')
          this.document.emit('look-at', plane)
        }
      },

      keyDown: function(keyCode) {
        if(keyCode.toLowerCase() == 'm') {
          this.previewColorMode = null
          this.colorMode = this.colorMode == 'material' ? 'component' : 'material'
          return
        }

        if(keyCode.toLowerCase() == 'w') {
          this.previewDisplayMode = null
          const modes = Object.keys(this.displayModes)
          const index = modes.indexOf(this.displayMode)
          this.displayMode = modes[(index + 1) % modes.length]
          return
        }

        if(keyCode == 46 || keyCode == 8) { // Del / Backspace
          // Delete Selection
          if(!this.document.selection.items.length) return;
          [...this.document.selection.items].forEach(item => {
            const type = item.typename()
            if(type == 'Solid') {
              this.document.deleteSolid(item)
            }
          })
        }
      },
    },
  }
</script>
