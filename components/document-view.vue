<template lang="pug">

  main.document-view(@pointerdown="bus.emit('close-widgets')")
    ViewPort(
      :document="document"
      :active-view="document.previewView || document.activeView"
      :display-mode="previewDisplayMode || currentDisplayMode"
      v-model:active-tool="activeTool"
      v-model:highlight="highlight"
      v-model:selection="document.selection"
      :active-component="document.activeComponent"
      :active-sketch="document.activeSketch"
      :active-feature="document.activeFeature"
      @change-view="(...args) => document.viewChanged(...args)"
    )
    //- @change-pose="document.isPoseDirty = true"

    ToolBox(
      :document="document"
      :active-tool="activeTool"
    )
    //- @remove-feature="document.removeFeature($event)"
    //- @add-feature="document.addFeature($event)"
    //- :active-component="document.activeComponent"
    //- :active-sketch="document.activeSketch"
    //- @update:active-sketch="document.activateSketch($event)"

    .side-bar.left
      TreeView(
        :top="tree"
        :document="document"
        v-model:highlight="highlight"
      )
      //- @create-component="document.createComponent($event)"
      //- @delete-component="document.deleteComponent($event)"
      //- @delete-solid="document.deleteSolid($event)"
      //- @update:active-sketch="document.activateSketch($event)"
      //- v-model:selection="document.selection"
      //- v-model:active-component="document.activeComponent"
      //- @update:active-component="document.activateComponent($event)"

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

    FooterView(
      :class="{top: !!document.activeFeature}"
      :selection="document.selection"
      :active-component="document.activeComponent"
      :active-feature="document.activeFeature"
    )

    FeatureBar(
      :document="document"
      :active-tool="activeTool"
    )
    //- @update:active-feature="document.activateFeature($event)"
    //- @delete-feature="document.removeFeature($event)"
    //- v-model:selection="document.selection"
    //- :active-component="document.activeComponent"
    //- :active-feature="document.activeFeature"

    //- button.button(@click="splitAll") Split all

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

  import { inject } from 'vue'

  import * as THREE from 'three'

  // import { Selection } from './../js/selection.js'
  import {
    CreateComponentFeature,
    CreateSketchFeature,
  } from './../js/core/features.js'

  export default {
    name: 'DocumentView',

    inject: ['bus'],

    props: {
      document: Object,
    },

    data() {
      return {
        activeTool: null,
        // activeFeature: null,
        // selection: new Selection(),
        highlight: null,
        // previewView: null,
        // dirtyView: null,
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
          // this.previewView = null
          // this.dirtyView = null
          // this.selection = new Selection()
          this.highlight = null
          // this.activeFeature = null
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
      // this.bus.on('regenerate', () => this.document.regenerate() )
      this.bus.emit('activate-toolname', 'Manipulate')
    },

    methods: {
      // regenerate: function() {
      //   if(this.activeFeature) this.activeFeature.invalidate()
      //   const compIds = this.document.evaluate()
      //   compIds.forEach(id => {
      //     const comp = this.document.getComponent(id)
      //     this.bus.emit('component-deleted', comp)
      //     comp.update()
      //     this.bus.emit('component-changed', comp, true)
      //   })
      //   this.document.activeComponent = this.findValidComponent(this.document.activeComponent)
      //   this.selection = new Selection()
      //   this.document.activeSketch = null
      // },

      // findValidComponent(comp) {
      //   if(comp.real) return comp
      //   return this.findValidComponent(comp.parent)
      // },

      // getComponent(id) {
      //   return this.document.tree.findChild(id)
      // },

      // addFeature(feature) {
      //   this.activeFeature = null
      //   this.document.insertFeature(feature)
      //   // this.document.activeSketch = null
      // },

      // removeFeature(feature) {
      //   this.document.removeFeature(feature)
      //   this.bus.emit('regenerate')
      // },

      // createComponent: function(parent, title) {
      //   // const feature = new CreateComponentFeature(this.document, parent.id)
      //   // feature.parent = () => parent.id
      //   // this.addFeature(feature)
      //   // feature.execute()
      //   const newComp = this.document.createComponent(parent)
      //   this.bus.emit('regenerate')
      //   this.activateComponent(newComp)
      // },

      // activateComponent: function(comp) {
      //   comp.UIData.hidden = false

      //   this.document.activeComponent = comp
      //   this.selection = this.selection.clear()
      // },

      // activateFeature: function(feature, doReset) {
      //   if(feature) {
      //     if(this.activeFeature) this.activateFeature(null, true)
      //     // Store marker, visibility and active component
      //     this.previousMarker = this.document.real.marker
      //     this.previousComponent = this.document.activeComponent
      //     const compIds = feature.real.modified_components()
      //     const comps = compIds.map(id => this.getComponent(id) ).filter(Boolean)
      //     this.oldVisibility = {}
      //     comps.forEach(comp => this.oldVisibility[comp.id] = comp.UIData.hidden )
      //     // Regenerate at feature position
      //     this.document.real.move_marker_to_feature(feature.real)
      //     this.bus.emit('regenerate')
      //     // Activate sketch for sketch features
      //     if(feature.constructor === CreateSketchFeature) {
      //       this.document.activeSketch = this.document.tree.findSketchByFeature(feature.id)
      //       if(this.document.activeSketch) {
      //         // Activate matching component
      //         this.document.activeComponent = this.getComponent(this.document.activeSketch.component_id())
      //         // Make sketch visible & store previous visibility
      //         this.previousSketchVisibility = this.document.activeComponent.UIData.itemsHidden[this.document.activeSketch.id()]
      //         this.$set(this.document.activeComponent.UIData.itemsHidden, this.document.activeSketch.id(), false)
      //       }
      //     }
      //     // else {
      //     //   this.document.activeSketch = null
      //     // }
      //     // Make affected components visible
      //     compIds.forEach(id => this.getComponent(id).UIData.hidden = false )
      //     // Moving marker will cause feature bar to deactivate active feature -> Restore
      //     const activeSketch = this.document.activeSketch
      //     setTimeout(() => {
      //       this.activeFeature = feature
      //       this.document.activeSketch = activeSketch
      //     }, 0)
      //   } else {
      //     if(this.activeFeature) this.bus.emit('deactivate-feature', this.activeFeature)
      //     // Restore previous state
      //     if(doReset && this.previousComponent) {
      //       if(this.previousSketchVisibility !== null && this.document.activeSketch) {
      //         this.$set(this.document.activeComponent.UIData.itemsHidden, this.document.activeSketch.id(), this.previousSketchVisibility)
      //       }
      //       this.previousSketchVisibility = null
      //       Object.keys(this.oldVisibility).forEach(id => this.getComponent(id).UIData.hidden = this.oldVisibility[id] )
      //       this.document.real.marker = this.previousMarker
      //       this.bus.emit('regenerate')
      //       this.activateComponent(this.getComponent(this.previousComponent.id))
      //     }
      //     this.activeFeature = null
      //     this.document.activeSketch = null
      //   }
      // },

      // activateSketch: function(sketch) {
      //   const featureId = sketch.feature_id()
      //   const feature = this.document.features.find(f => f.id == featureId )
      //   this.activateFeature(feature)
      // },

      // deleteComponent: function(comp) {
      //   this.bus.emit('component-deleted', comp)
      //   comp.parent.deleteComponent(comp)
      //   if(this.document.activeComponent.hasAncestor(comp)) {
      //     this.document.activeComponent = comp.parent
      //   }
      // },

      // deleteSolid: function(solid) {
      //   solid.remove()
      //   this.selection = this.selection.delete(solid)
      //   this.bus.emit('component-changed', solid.component)
      // },

      // createView: function(title) {
      //   const newView = {
      //     id: this.document.lastId++,
      //     title: title || 'Custom ' + this.document.lastId,
      //     position: this.dirtyView.position.clone(),
      //     target: this.dirtyView.target.clone(),
      //   }
      //   this.document.views.push(newView)
      //   this.dirtyView = null
      //   this.activateView(newView)
      // },

      // // User changed camera from viewport
      // viewChanged: function(position, target) {
      //   this.dirtyView = {position: position.clone(), target: target.clone()}
      //   this.document.activeView = null
      // },

      // activateView: function(view) {
      //   this.document.activeView = view
      //   this.previewView = null
      //   this.dirtyView = null
      // },

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

      splitAll: function() {
        const splits = this.document.activeSketch.get_all_split()
        this.document.emit('component-changed', this.document.activeComponent)
      },
    },
  }
</script>
