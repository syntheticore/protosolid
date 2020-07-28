<template lang="pug">
  main.document-view
    ViewPort(
      :tree="document.tree"
      @change-view="document.isViewDirty = true"
      @change-pose="document.isPoseDirty = true"
    )
    ToolBox
    SideBar.bar-left
      TreeView(
        :top="document.tree"
        :active-component="activeComponent"
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
    FooterView
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
      }
    },

    created() {

    },

    mounted() {

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

      createComponent: function(parent) {
        console.log('cr1')
        this.activeComponent = parent.create_component('New Component')
      },

      activateComponent: function(comp) {
        console.log('ac1', comp)
        this.activeComponent = comp
      },
    },
  }
</script>
