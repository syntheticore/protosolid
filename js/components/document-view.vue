<template lang="pug">
  main.document-view(
    @pointerdown="mouseDown"
    @keydown="keyDown"
  )
    ViewPort(
      :document="document"
      :active-component="document.activeComponent"
      :highlighted-component="highlightedComponent"
      :active-tool.sync="activeTool"
      :selection.sync="selection"
      :active-view="previewView || document.activeView"
      :display-mode="previewDisplayMode || currentDisplayMode"
      @change-view="viewChanged"
      @change-pose="document.isPoseDirty = true"
    )
    ToolBox(
      :active-tool="activeTool"
      :active-component="document.activeComponent"
    )
    .side-bar.left
      TreeView(
        :top="document.tree"
        :active-component="document.activeComponent"
        :selection.sync="selection"
        @update:active-component="activateComponent"
        @highlight-component="highlightComponent"
        @create-component="createComponent"
        @delete-component="deleteComponent"
        @delete-solid="deleteSolid"
      )
    .side-bar.right
      h1 View
      ListChooser(
        :list="document.views"
        :active="document.activeView"
        :allow-create="!!dirtyView"
        @update:active="activateView"
        @create="createView"
        @hover="previewView = $event"
        @unhover="previewView = dirtyView"
      )
      RadioBar(
        :items="displayModes"
        :chosen.sync="currentDisplayMode"
        @hover="previewDisplayMode = $event"
        @unhover="previewDisplayMode = null"
      )
      h1 Poses
      ListChooser(
        :list="document.poses"
        :active.sync="document.activePose"
        :allow-create="document.isPoseDirty"
        @create="createPose"
      )
      //- h1 Sets
      //- ListChooser(
      //-   v-if="document.sets.length || document.isSetDirty"
      //-   :list="document.sets"
      //-   :allow-create="document.isSetDirty"
      //-   @create="createSet"
      //- )
    FooterView(
      :selection="selection"
      :active-component="document.activeComponent"
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
    right: 0
    bottom: 0
</style>


<script>
  import * as THREE from 'three'

  import ViewPort from './view-port.vue'
  import TreeView from './tree-view.vue'
  import ToolBox from './tool-box.vue'
  import FooterView from './footer-view.vue'
  import ListChooser from './list-chooser.vue'
  import RadioBar from './radio-bar.vue'

  export default {
    name: 'DocumentView',

    components: {
      ViewPort,
      TreeView,
      ToolBox,
      FooterView,
      ListChooser,
      RadioBar,
    },

    props: {
      document: Object,
    },

    watch: {
      document: {
        immediate: true,
        handler: function(document, oldDocument) {
          if(oldDocument) oldDocument.activeView = oldDocument.activeView || this.dirtyView
          if(!document.activeView) {
            document.activeView = document.views[3]
            this.createComponent(document.tree, 'Component 1')
          }
          this.previewView = null
          this.dirtyView = null
        },
      },
    },

    data() {
      return {
        activeTool: null,
        selection: null,
        highlightedComponent: null,
        previewView: null,
        dirtyView: null,
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

    mounted() {
      this.currentDisplayMode = 'wireShade'
      this.$root.$emit('activate-toolname', 'Manipulate')
    },

    methods: {
      createComponent: function(parent, title) {
        const comp = parent.createComponent(title)
        this.activateComponent(comp)
        return comp
      },

      activateComponent: function(comp) {
        comp.hidden = false
        this.document.activeComponent = comp
      },

      deleteComponent: function(comp) {
        this.$root.$emit('component-deleted', comp)
        comp.parent.deleteComponent(comp)
        if(!this.document.activeComponent.hasAncestor(comp)) return
        this.document.activeComponent = comp.parent
      },

      deleteSolid: function(solid) {
        solid.remove()
        this.$root.$emit('component-changed', solid.component)
      },

      highlightComponent: function(comp, solidId) {
        this.highlightedComponent = comp ? { comp, solidId } : null
      },

      createView: function(title) {
        const newView = {
          id: this.document.lastId++,
          title: title || 'Custom View',
          position: this.dirtyView.position.clone(),
          target: this.dirtyView.target.clone(),
        }
        this.document.views.push(newView)
        this.dirtyView = null
        this.activateView(newView)
      },

      createPose: function() {
        this.document.poses.push({ title: 'Untitled Pose', id: this.document.lastId++ })
        this.document.isPoseDirty = false
      },

      createSet: function() {
        this.document.sets.push({ title: 'Untitled Set', id: this.document.lastId++ })
        this.document.isSetDirty = false
      },

      // User changed camera from viewport
      viewChanged: function(position, target) {
        this.dirtyView = {position: position.clone(), target: target.clone()}
        this.document.activeView = null
      },

      activateView: function(view) {
        this.document.activeView = view
        this.previewView = null
        this.dirtyView = null
      },

      mouseDown: function() {
        this.$root.$emit('close-widgets')
      },

      keyDown: function(e) {
        console.log('DOC DOWN')
        if(e.keyCode == 46 || e.keyCode == 8) { // Del / Backspace
          // Delete Selection
          if(this.selection) {
            console.log(this.selection)
            if(this.selection.typename() == 'Component') {
              this.deleteComponent(this.selection)
            } else if(this.selection.typename() == 'Solid') {
              this.deleteSolid(this.selection)
            }
          }
        }
      },
    },
  }
</script>
