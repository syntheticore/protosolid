<template lang="pug">

  main.document-view(@pointerdown="bus.emit('close-widgets')")
    ViewPort(
      :document="document"
      :active-view="document.previewView || document.activeView"
      :display-mode="previewDisplayMode || currentDisplayMode"
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
      h1 View
      ListChooser(
        :list="document.views"
        :active="document.activeView"
        :allow-create="!!document.dirtyView"
        @update:active="document.activateView($event)"
        @create="document.createView($event)"
        @hover="document.previewView = $event"
        @unhover="document.previewView = document.dirtyView"
      )
      RadioBar(
        :items="displayModes"
        v-model:chosen="currentDisplayMode"
        @hover="previewDisplayMode = $event"
        @unhover="previewDisplayMode = null"
      )
      //- Icon(icon="users-viewfinder")

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
    border-top: 1px solid #323840

  .tool-box
    position: absolute
    margin-top: 12px
    z-index: 2

  .side-bar
    position: absolute
    top: 0
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
      left: 0
      overflow: hidden
    &.right
      top: 14px
      right: 14px
      bottom: 35px
      display: flex
      flex-direction: column
      h1
        flex: 0 0 content
      .list-chooser
      .radio-bar
        flex: 0 1 auto
        margin-bottom: 16px

  .view-port
    width: 100%
    height: 100%

  .footer-view
    position: absolute
    bottom: 70px
    &.top
      bottom: unset
      top: 100px

  .feature-bar
    position: absolute
    bottom: 12px
    max-width: calc(100% - 24px)

</style>


<script>

  import {
    CreateComponentFeature,
    CreateSketchFeature,
  } from './../js/core/features.js'

  import { ManipulationTool } from './../js/tools.js'

  export default {
    name: 'DocumentView',

    inject: ['bus'],

    props: {
      document: Object,
    },

    data() {
      return {
        activeTool: null,
        highlight: null,
        displayModes: {
          wireShade: {
            title: 'Shaded + Wire',
            icon: 'magnet',
          },
          wireframe: {
            title: 'Wireframe',
            icon: 'clone',
          },
          shaded: {
            title: 'Shaded',
            icon: 'box',
          },
        },
        currentDisplayMode: null,
        previewDisplayMode: null,
      }
    },

    watch: {
      document: {
        immediate: true,
        handler: function(document, oldDocument) {
          if(oldDocument) oldDocument.activeView = oldDocument.activeView || this.dirtyView
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
    },

    mounted() {
      this.currentDisplayMode = 'wireShade'
      this.bus.on('keydown', this.keyDown)
      this.bus.emit('activate-tool', ManipulationTool)
    },

    methods: {
      keyDown: function(keyCode) {
        if(keyCode == 46 || keyCode == 8) { // Del / Backspace
          // Delete Selection
          if(!this.document.selection.set.size) return
          this.document.selection.set.forEach(item => {
            const type = item.typename()
            if(type == 'Component') {
              this.document.deleteComponent(item)
            } else if(type == 'Solid') {
              this.document.deleteSolid(item)
            }
          })
        }
      },
    },
  }
</script>
