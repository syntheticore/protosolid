<template lang="pug">
  main.document-view
    ViewPort(
      :tree="document.tree"
      :active-component="activeComponent"
      :active-tool="activeTool"
      :selected-element="selectedElement"
      :data="data"
      @change-view="document.isViewDirty = true"
      @change-pose="document.isPoseDirty = true"
      @activate-tool="activateTool"
      @element-selected="elementSelected"
    )
    ToolBox(
      :active-tool="activeTool"
      @activate-tool="activateTool"
    )
    SideBar.bar-left
      TreeView(
        :top="document.tree"
        :active-component="activeComponent"
        :data="data"
        @create-component="createComponent"
        @activate-component="activateComponent"
      )
    SideBar.bar-right
      h1 Views
      ListChooser(
        :list="document.views"
        :active="document.activeView"
        :allow-create="document.isViewDirty"
        @create="createView"
        @activate="activateView"
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

  .side-bar
    position: absolute
    top: 0
    bottom: 55px
    padding-top: 14px
    h1
      text-align: center
    &.bar-left
      left: 0
      overflow: hidden
      // pointer-events: all
      // h1
      //   margin-left: 14px
    &.bar-right
      right: 14px
      bottom: 155px
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
  import SideBar from './side-bar.vue'
  import ViewPort from './view-port.vue'
  import TreeView from './tree-view.vue'
  import ToolBox from './tool-box.vue'
  import FooterView from './footer-view.vue'
  import ListChooser from './list-chooser.vue'

  let lastId = 9999;

  export default {
    name: 'DocumentView',

    components: {
      SideBar,
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
        this.activeComponent = this.document.tree
      },
    },

    data() {
      return {
        activeComponent: this.document.tree,
        activeTool: null,
        selectedElement: null,
        data: {},
      }
    },

    created() {
      this.data[this.document.tree.id()] = {}
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
      // part3.create_sketch().add_segment()
      part3.add_segment()
      // part3.get_sketches()[0].add_segment()
      // console.log(part3.get_sketches()[0].get_segments())
    },

    mounted() {
      this.$root.$on('escape', () => {
        if(!this.activeTool) return
        this.activeTool.dispose()
        this.activeTool = null
      })

      // this.$root.$on('delete', () => {
      //   this.selectedElement
      // })
    },

    methods: {
      createView: function() {
        this.document.views.push({ title: 'Fresh View', id: lastId++ })
        this.document.isViewDirty = false
      },

      createPose: function() {
        this.document.poses.push({ title: 'Untitled Pose', id: lastId++ })
        this.document.isPoseDirty = false
      },

      createSet: function() {
        this.document.sets.push({ title: 'Untitled Set', id: lastId++ })
        this.document.isSetDirty = false
      },

      activateView: function(view) {
        this.document.activeView = view
      },

      activatePose: function(pose) {
        this.document.activePose = pose
      },

      createComponent: function(parent, title) {
        console.log('cr1')
        this.activeComponent = parent.create_component(title || 'New Component')
        console.log(this.activeComponent.id())
        this.$set(this.data, this.activeComponent.id(), {hidden: false, threeObjects: []})
        return this.activeComponent
      },

      activateComponent: function(comp) {
        console.log('ac1', comp)
        this.activeComponent = comp
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
