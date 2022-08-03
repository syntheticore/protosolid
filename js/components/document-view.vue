<template lang="pug">
  main.document-view(@pointerdown="$root.$emit('close-widgets')")
    ViewPort(
      :document="document"
      :active-component="document.activeComponent"
      :active-sketch="document.activeSketch"
      :active-feature="activeFeature"
      :active-tool.sync="activeTool"
      :selection.sync="selection"
      :highlight.sync="highlight"
      :active-view="previewView || document.activeView"
      :display-mode="previewDisplayMode || currentDisplayMode"
      @change-view="viewChanged"
      @change-pose="document.isPoseDirty = true"
    )

    ToolBox(
      :document="document"
      :active-tool="activeTool"
      :active-component="document.activeComponent"
      :active-sketch="document.activeSketch"
      @update:active-sketch="activateSketch"
      @add-feature="addFeature"
      @remove-feature="removeFeature"
    )

    .side-bar.left
      TreeView(
        :top="document.tree"
        :active-component="document.activeComponent"
        :selection.sync="selection"
        :highlight.sync="highlight"
        @update:active-component="activateComponent"
        @update:active-sketch="activateSketch"
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

    FooterView(
      :class="{top: !!activeFeature}"
      :selection="selection"
      :active-component="document.activeComponent"
      :active-feature="activeFeature"
    )

    FeatureBar(
      :document="document"
      :active-tool="activeTool"
      :active-feature="activeFeature"
      :selection.sync="selection"
      @update:active-feature="activateFeature"
      @delete-feature="removeFeature"
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
    max-width: 80%
</style>


<script>
  import * as THREE from 'three'

  import ViewPort from './view-port.vue'
  import TreeView from './tree-view.vue'
  import ToolBox from './tool-box.vue'
  import FooterView from './footer-view.vue'
  import ListChooser from './list-chooser.vue'
  import RadioBar from './radio-bar.vue'
  import FeatureBar from './feature-bar.vue'

  import { Selection } from './../selection.js'
  import {
    CreateComponentFeature,
    CreateSketchFeature,
  } from './../features.js'

  export default {
    name: 'DocumentView',

    components: {
      ViewPort,
      TreeView,
      ToolBox,
      FooterView,
      ListChooser,
      RadioBar,
      FeatureBar,
    },

    props: {
      document: Object,
    },

    data() {
      return {
        activeTool: null,
        activeFeature: null,
        selection: new Selection(),
        highlight: null,
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

    watch: {
      document: {
        immediate: true,
        handler: function(document, oldDocument) {
          if(oldDocument) oldDocument.activeView = oldDocument.activeView || this.dirtyView
          if(!document.activeView) {
            document.activeView = document.views[3]
          }
          this.previewView = null
          this.dirtyView = null
          this.selection = new Selection()
          this.highlight = null
          this.activeFeature = null
        },
      },
    },

    mounted() {
      this.currentDisplayMode = 'wireShade'
      this.$root.$on('keydown', this.keyDown)
      this.$root.$on('regenerate', this.regenerate)
      this.$root.$emit('activate-toolname', 'Manipulate')
    },

    methods: {
      regenerate: function() {
        if(this.activeFeature) this.activeFeature.real.invalidate()
        const compIds = this.document.real.evaluate()
        console.log(compIds)
        compIds.forEach(id => {
          const comp = this.getComponent(id)
          this.$root.$emit('component-deleted', comp)
          this.updateComponent(comp)
          this.$root.$emit('component-changed', comp, true)
        })
        this.document.activeComponent = this.findValidComponent(this.document.activeComponent)
        this.selection = new Selection()
        this.document.activeSketch = null
      },

      findValidComponent(comp) {
        if(comp.real) return comp
        return this.findValidComponent(comp.parent)
      },

      getComponent(id) {
        return this.document.tree.findChild(id)
      },

      updateComponent(comp) {
        comp.update(this.document.componentData())
      },

      addFeature(feature) {
        this.activeFeature = null
        this.document.insertFeature(feature, this.document.real.marker)
        // this.document.activeSketch = null
      },

      removeFeature(feature) {
        this.document.removeFeature(feature)
        this.$root.$emit('regenerate')
      },

      createComponent: function(parent, title) {
        const feature = new CreateComponentFeature(this.document)
        feature.parent = () => parent.id
        this.addFeature(feature)
        feature.execute()
        this.$root.$emit('regenerate')
        const newComp = parent.children.slice(-1)[0]
        this.activateComponent(newComp)
      },

      activateComponent: function(comp) {
        comp.UIData.hidden = false
        this.document.activeComponent = comp
        this.selection = this.selection.clear()
      },

      activateFeature: function(feature, doReset) {
        if(feature) {
          if(this.activeFeature) this.activateFeature(null, true)
          // Store marker, visibility and active component
          this.previousMarker = this.document.real.marker
          this.previousComponent = this.document.activeComponent
          const compIds = feature.real.modified_components()
          const comps = compIds.map(id => this.getComponent(id) ).filter(Boolean)
          this.oldVisibility = {}
          comps.forEach(comp => this.oldVisibility[comp.id] = comp.UIData.hidden )
          // Regenerate at feature position
          this.document.real.move_marker_to_feature(feature.real)
          this.$root.$emit('regenerate')
          // Activate sketch for sketch features
          if(feature.constructor === CreateSketchFeature) {
            this.document.activeSketch = this.document.tree.findSketchByFeature(feature.id)
            if(this.document.activeSketch) {
              // Activate matching component
              this.document.activeComponent = this.getComponent(this.document.activeSketch.component_id())
              // Make sketch visible & store previous visibility
              this.previousSketchVisibility = this.document.activeComponent.UIData.itemsHidden[this.document.activeSketch.id()]
              this.$set(this.document.activeComponent.UIData.itemsHidden, this.document.activeSketch.id(), false)
            }
          } else {
            this.document.activeSketch = null
          }
          // Make affected components visible
          compIds.forEach(id => this.getComponent(id).UIData.hidden = false )
          // Moving marker will cause feature bar to deactivate active feature -> Restore
          const activeSketch = this.document.activeSketch
          setTimeout(() => {
            this.activeFeature = feature
            this.document.activeSketch = activeSketch
          }, 0)
        } else {
          if(this.activeFeature) this.$root.$emit('deactivate-feature', this.activeFeature)
          // Restore previous state
          if(doReset && this.previousComponent) {
            if(this.previousSketchVisibility !== null && this.document.activeSketch) {
              this.$set(this.document.activeComponent.UIData.itemsHidden, this.document.activeSketch.id(), this.previousSketchVisibility)
            }
            this.previousSketchVisibility = null
            Object.keys(this.oldVisibility).forEach(id => this.getComponent(id).UIData.hidden = this.oldVisibility[id] )
            this.document.real.marker = this.previousMarker
            this.$root.$emit('regenerate')
            this.activateComponent(this.getComponent(this.previousComponent.id))
          }
          this.activeFeature = null
          this.document.activeSketch = null
        }
      },

      activateSketch: function(sketch) {
        const featureId = sketch.get_feature_id()
        const feature = this.document.features.find(f => f.id == featureId )
        this.activateFeature(feature)
      },

      deleteComponent: function(comp) {
        this.$root.$emit('component-deleted', comp)
        comp.parent.deleteComponent(comp)
        if(this.document.activeComponent.hasAncestor(comp)) {
          this.document.activeComponent = comp.parent
        }
      },

      deleteSolid: function(solid) {
        solid.remove()
        this.selection = this.selection.delete(solid)
        this.$root.$emit('component-changed', solid.component)
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

      keyDown: function(keyCode) {
        if(keyCode == 46 || keyCode == 8) { // Del / Backspace
          // Delete Selection
          if(!this.selection.set.size) return
          this.selection.set.forEach(item => {
            const type = item.typename()
            if(type == 'Component') {
              this.deleteComponent(item)
            } else if(type == 'Solid') {
              this.deleteSolid(item)
            }
          })
        }
      },
    },
  }
</script>
