<template lang="pug">

  main.document-view(@pointerdown="bus.emit('close-widgets')")

    ViewPort(
      :document="document"
      :active-view="document.previewView || document.activeView"
      :display-mode="effectiveDisplayMode"
      :color-mode="effectiveColorMode"
      :projection-mode="effectiveProjectionMode"
      :sketch-section="sketchSection"
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

        ViewChooser(
          :document="document"
          :projection-mode="projectionMode"
          :display-mode="displayMode"
          :color-mode="colorMode"
        )

        .buttons

          .view-row
            .side-label Zoom

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
            .side-label Nearest Plane

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
          .side-label Projection
          RadioBar(
            :items="projectionModes"
            hot-key="O"
            v-model:chosen="projectionMode"
            @hover="previewProjectionMode = $event"
            @unhover="previewProjectionMode = null"
          )

        .display-row
          .side-label Shading
          RadioBar(
            :items="displayModes"
            hot-key="W"
            v-model:chosen="displayMode"
            @hover="previewDisplayMode = $event === 'pathTraced' ? null : $event"
            @unhover="previewDisplayMode = null"
          )

        .display-row
          .side-label Material
          RadioBar(
            :items="colorModes"
            hot-key="M"
            v-model:chosen="colorMode"
            @hover="previewColorMode = $event"
            @unhover="previewColorMode = null"
          )

        .display-row(v-if="document.activeSketch")
          label.display-toggle(title="Section the model at the active sketch plane")
            input(type="checkbox" v-model="sketchSection")
            span Section

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
    margin-top: 12px
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
      width: 140px
      display: flex
      flex-direction: column

      h1
        margin-top: 1rem

      .side-section
        display: flex
        flex-direction: column
        pointer-events: auto

        &:hover
          .side-label
            opacity: 1

      .view-chooser
      .radio-bar
        margin-bottom: 0.5rem

      .buttons
      .display-row
        align-items: center
        align-self: flex-end
        display: grid
        column-gap: 0.5rem
        grid-template-columns: 72px 140px

      .view-row
        display: contents

      .side-label
        color: $bright2
        font-size: 11px
        justify-self: end
        opacity: 0
        pointer-events: none
        text-align: right
        transition: opacity 0.15s
        white-space: nowrap

      .view-row
        > .flex
          min-width: 0

        .button
          min-width: 0

      .buttons
        pointer-events: auto
        row-gap: 0.25rem

      .display-row
        margin-bottom: 0.5rem

        .radio-bar
          margin-bottom: 0
          min-width: 0

          :deep(.button)
            padding: 4px

        .display-toggle
          display: flex
          gap: 0.35rem
          grid-column: 2

          span
            font-size: 14px
            font-weight: bold

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

  .view-port
    width: 100%
    height: 100%

  .footer-view
    position: absolute
    bottom: 70px

    &.top
      bottom: unset
      top: 132px

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
            icon: 'display-shaded-edges',
          },
          wireframe: {
            title: 'Wireframe',
            icon: 'display-wireframe',
          },
          hiddenLines: {
            title: 'Shaded + Hidden Lines',
            icon: 'display-hidden-lines',
          },
          shaded: {
            title: 'Shaded',
            icon: 'display-shaded',
          },
          pathTraced: {
            title: 'Path traced',
            icon: 'display-path-traced',
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
        displayMode: 'hiddenLines',
        previewDisplayMode: null,
        colorMode: 'material',
        previewColorMode: null,
        sketchSection: true,
      }
    },

    watch: {
      document: {
        immediate: true,
        handler: function(document, oldDocument) {
          if(oldDocument) {
            oldDocument.clearViewPreview()
            oldDocument.activeView = oldDocument.activeView || oldDocument.dirtyView
          }
          if(!document.activeView) {
            document.activeView = document.dirtyView || document.currentCameraView
          }
          this.highlight = null
        },
      },

      'document.activeView': function(view) {
        const settings = view?.displaySettings
        if(!settings) return
        if(this.projectionModes[settings.projectionMode]) this.projectionMode = settings.projectionMode
        if(this.displayModes[settings.displayMode]) this.displayMode = settings.displayMode
        if(this.colorModes[settings.colorMode]) this.colorMode = settings.colorMode
      },
    },

    computed: {
      tree: function() {
        return this.document.top()
      },

      effectiveColorMode: function() {
        if(this.document.activeSimulation?.result && !this.document.activeSimulationPicker) return this.document.activeSimulation.mode
        if(this.activeTool?.interferenceMode) return 'interference'
        const savedMode = this.document.previewView?.displaySettings?.colorMode
        return this.activeTool?.diagnosticMode
          ? `diagnostic:${this.activeTool.diagnosticMode}:${this.activeTool.draftDirection}:${this.activeTool.zebraFrequency}`
          : this.previewColorMode || (this.colorModes[savedMode] && savedMode) || this.colorMode
      },

      effectiveProjectionMode: function() {
        const savedMode = this.document.previewView?.displaySettings?.projectionMode
        return this.previewProjectionMode || (this.projectionModes[savedMode] && savedMode) || this.projectionMode
      },

      effectiveDisplayMode: function() {
        if(this.document.activeSimulation?.result && !this.document.activeSimulationPicker) return 'shaded'
        const exportConfig = this.document.activeExportConfig
        if(exportConfig) return 'exportPreview'
        const savedMode = this.document.previewView?.displaySettings?.displayMode
        return this.activeTool?.diagnosticMode || this.activeTool?.interferenceMode
          ? 'shaded'
          : this.previewDisplayMode || (this.displayModes[savedMode] && savedMode) || this.displayMode
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
          const modes = Object.keys(this.displayModes).filter(mode => mode != 'pathTraced')
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
              const component = item.component || this.document.getComponent(item.compound?.componentId)
              if(!component?.readOnly) this.document.deleteSolid(item)
            }
          })
        }
      },
    },
  }
</script>
