<template lang="pug">
  main.document-view
    ViewPort(
      :document="document"
      :active-component="document.activeComponent"
      :active-tool="activeTool"
      :selected-element="selectedElement"
      :active-view="document.activeView"
      :preview-view="document.previewView"
      @change-view="viewChanged"
      @change-pose="document.isPoseDirty = true"
      @activate-tool="activateTool"
      @element-selected="elementSelected"
    )
    ToolBox(
      :active-tool="activeTool"
      :active-component="document.activeComponent"
      @activate-tool="activateTool"
    )
    .side-bar.left
      TreeView(
        :top="document.tree"
        :data="document.data"
        :active-component="document.activeComponent"
        @create-component="createComponent"
        @activate-component="activateComponent"
      )
    .side-bar.right
      h1 Views
      ListChooser(
        :list="document.views"
        :active="document.activeView"
        :allow-create="document.isViewDirty"
        @create="createView"
        @activate="activateView"
        @hover="previewView"
        @unhover="unpreviewView"
      )
      h1 Poses
      ListChooser(
        :list="document.poses"
        :active="document.activePose"
        :allow-create="document.isPoseDirty"
        @create="createPose"
        @activate="activatePose"
      )
      h1 Sets
      ListChooser(
        v-if="document.sets.length || document.isSetDirty"
        :list="document.sets"
        :allow-create="document.isSetDirty"
        @create="createSet"
      )
    FooterView(
      :selected-element="selectedElement"
      :active-component="document.activeComponent"
    )
</template>


<style lang="stylus" scoped>
  .document-view
    overflow: hidden
    display: flex
    justify-content: center

  .tool-box
    position: absolute
    margin-top: 12px
    max-width: calc(100% - 450px)
    z-index: 2

  .side-bar
    position: absolute
    top: 0
    bottom: 55px
    padding-top: 14px
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
      right: 14px
      bottom: 35px
      display: flex
      flex-direction: column
      h1
        flex: 0 0 content
      .list-chooser
        flex: 0 1 auto

  .view-port
    width: 100%
    height: 100%

  .footer-view
    position: absolute
    left: 0
    bottom: 0
    width: 100%
</style>


<script>
  import * as THREE from 'three'

  import ViewPort from './view-port.vue'
  import TreeView from './tree-view.vue'
  import ToolBox from './tool-box.vue'
  import FooterView from './footer-view.vue'
  import ListChooser from './list-chooser.vue'

  let lastId = 9999;

  export default {
    name: 'DocumentView',

    components: {
      ViewPort,
      TreeView,
      ToolBox,
      FooterView,
      ListChooser,
    },

    props: {
      document: Object,
    },

    watch: {
      document: function() {
        this.integrateComponent(this.document.activeComponent)
        this.previewView(this.document.dirtyView)
      },
    },

    data() {
      return {
        // activeComponent: this.document.tree,
        activeTool: null,
        // activeFeature: null,
        selectedElement: null,
      }
    },

    created() {
      this.integrateComponent(this.document.tree)
      const part1 = this.createComponent(this.document.tree, 'Part 1')
        const assm1 = this.createComponent(this.document.tree, 'Sub Assembly 1')
          const part2 = this.createComponent(assm1, 'Part 2')
          const part3 = this.createComponent(assm1, 'Part 3')
          const assm2 = this.createComponent(assm1, 'Sub Assembly 2')
            const part4 = this.createComponent(assm2, 'Part 4')
            const part5 = this.createComponent(assm2, 'Part 5')
          const assm3 = this.createComponent(assm1, 'Sub Assembly 3')
            const part6 = this.createComponent(assm3, 'Part 6')
            const part7 = this.createComponent(assm3, 'Part 7')
            const part8 = this.createComponent(assm3, 'Part 8')
      this.document.activeComponent = assm2
    },

    mounted() {
      window.addEventListener('keydown', (e) => {
        console.log(e.keyCode)
        if(e.keyCode === 27) {
          this.$root.$emit('escape')
        } else if(e.keyCode === 76) { // L
          this.$root.$emit('activate-toolname', 'Line')
        } else if(e.keyCode === 67) { // C
          this.$root.$emit('activate-toolname', 'Circle')
        } else if(e.keyCode === 83) { // S
          this.$root.$emit('activate-toolname', 'Spline')
        }
      });

      this.$root.$on('escape', () => {
        this.$root.$emit('activate-toolname', 'Manipulate')
      })

      this.$root.$emit('activate-toolname', 'Manipulate')
    },

    methods: {
      createComponent: function(parent, title) {
        this.document.activeComponent = parent.create_component(title || 'New Component')
        this.integrateComponent(this.document.activeComponent)
        return this.document.activeComponent
      },

      integrateComponent: function(comp) {
        const id = comp.id()
        if(!this.document.data[id]) this.$set(this.document.data, id, {
          hidden: false,
          faces: [],
          wireframe: [],
          regions: [],
          curves: [],
        })
      },

      createView: function(title) {
        const view = {
          id: lastId++,
          title: title || 'Fresh View',
          position: this.document.dirtyView.position.clone(),
          target: this.document.dirtyView.target.clone(),
        }
        this.document.views.push(view)
        this.document.isViewDirty = false
        this.activateView(view)
      },

      createPose: function() {
        this.document.poses.push({ title: 'Untitled Pose', id: lastId++ })
        this.document.isPoseDirty = false
      },

      createSet: function() {
        this.document.sets.push({ title: 'Untitled Set', id: lastId++ })
        this.document.isSetDirty = false
      },

      viewChanged: function(position, target) {
        this.document.dirtyView = {position: position.clone(), target: target.clone()}
        this.document.isViewDirty = true
        this.document.activeView = null
      },

      previewView: function(view) {
        this.document.previewView = view
      },

      unpreviewView: function() {
        this.document.previewView = this.document.dirtyView
      },

      activateView: function(view) {
        this.document.activeView = view
        this.document.dirtyView = view
      },

      activatePose: function(pose) {
        this.document.activePose = pose
      },

      activateComponent: function(comp) {
        this.document.activeComponent = comp
      },

      activateTool: function(tool) {
        this.activeTool = tool
      },

      elementSelected: function(elem) {
        this.selectedElement = elem
      },
    },
  }
</script>
