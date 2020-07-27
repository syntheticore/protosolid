<template lang="pug">
  #app(:class="{fullscreen: isFullscreen, browser: isBrowser}" v-if="activeDocument")
    ToolBar(
      :documents="documents"
      :active-document="activeDocument"
      @create-document="createDocument"
      @change-document="changeDocument"
    )
    main
      ViewPort(
        :tree="activeDocument.tree"
        @change-view="activeDocument.isViewDirty = true"
        @change-pose="activeDocument.isPoseDirty = true"
      )
      ToolBox
      SideBar.bar-left
        TreeView(:top="activeDocument.tree")
      SideBar.bar-right
        h1 Views
        ListChooser(
          :list="activeDocument.views"
          :active="activeDocument.activeView"
          :allow-create="activeDocument.isViewDirty"
          @create="createView"
          @activate="activateView"
        )
        h1 Poses
        ListChooser(
          :list="activeDocument.poses"
          :active="activeDocument.activePose"
          :allow-create="activeDocument.isPoseDirty"
          @create="createPose"
          @activate="activatePose"
        )
        h1 Sets
        ListChooser(
          v-if="activeDocument.sets.length || activeDocument.isSetDirty"
          :list="activeDocument.sets"
          :allow-create="activeDocument.isSetDirty"
          @create="createSet"
        )
      FooterView
</template>


<style lang="stylus" scoped>
  #app
    width: 100%
    height: 100%
    display: grid
    grid-template-rows: 38px 1fr
    // grid-gap: 1px
    // grid-auto-rows: minmax(100px, auto)
    grid-template-areas:
      "header"\
      "main"
    user-select: none
    cursor: default
    overflow: hidden
    color: $bright1
    &.fullscreen
      grid-template-rows: 24px 1fr
  .tool-bar
    grid-area: header

  main
    grid-area: main
    position: relative
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
    padding-top: 14px
    h1
      text-align: center
    &.bar-left
      left: 0
      h1
        margin-left: 14px
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
  import ToolBar from './tool-bar.vue'
  import SideBar from './side-bar.vue'
  import ViewPort from './view-port.vue'
  import TreeView from './tree-view.vue'
  import ToolBox from './tool-box.vue'
  import FooterView from './footer-view.vue'
  import ListChooser from './list-chooser.vue'
  const wasmP = import('../../rust/pkg/wasm-index.js')

  let lastId = 1;

  window.addEventListener('keydown', (e) => {
    // console.log(e.keyCode)
    if(e.keyCode === 83) { // S
      return
    }
  });

  export default {
    name: 'app',

    components: {
      ToolBar,
      SideBar,
      ViewPort,
      TreeView,
      ToolBox,
      FooterView,
      ListChooser,
    },

    created() {
      // this.changeDocument(this.documents[0])
      this.createDocument().then(() => this.changeDocument(this.documents[0]) )

      if(!window.ipcRenderer) {
        // this.isFullscreen = true
        return
      }

      window.ipcRenderer.on('fullscreen-changed', (e, isFullscreen) => {
        console.log(event)
        console.log(isFullscreen)
        this.isFullscreen = isFullscreen
      })

      window.ipcRenderer.on('pong', () => {
        console.log('pong')
      })
      window.ipcRenderer.send('ping')
    },

    mounted() {
      if(!window.ipcRenderer) return
      window.ipcRenderer.send('vue-ready')
    },

    methods: {
      createDocument: function() {
        return wasmP.then((wasm) => {
          const proxy = new wasm.AlchemyProxy()
          const tree = proxy.get_main_assembly()
            const part1 = tree.create_component('Part 1')
            const assm1 = tree.create_component('Sub Assembly 1')
              const part2 = assm1.create_component('Part 2')
              const part3 = assm1.create_component('Part 3')
              part3.selected = true
              console.log(part3)
              const assm2 = assm1.create_component('Sub Assembly 2')
                const part4 = assm2.create_component('Part 4')
                const part5 = assm2.create_component('Part 5')
              const assm3 = assm1.create_component('Sub Assembly 3')
                const part6 = assm3.create_component('Part 6')
                const part7 = assm3.create_component('Part 7')
                const part8 = assm3.create_component('Part 8')
          part3.create_sketch().add_segment()
          part3.get_sketches()[0].add_segment()
          console.log(part3.get_sketches()[0].get_segments())
          this.documents.push({
            title: 'Untitled Document',
            proxy: proxy,
            views: [
              { title: 'Top', id: lastId++ },
              { title: 'Left', id: lastId++ },
              { title: 'Front', id: lastId++ },
              { title: 'Perspective', id: lastId++ },
            ],
            poses: [
              { title: 'Base', id: lastId++ },
              { title: 'Activated', id: lastId++ },
            ],
            sets: [
              { title: 'Filet 14', id: lastId++ },
              { title: 'Extrude 2', id: lastId++ },
            ],
            tree: tree,
            activeView: null,
            activePose: null,
            isViewDirty: false,
            isPoseDirty: false,
            isSetDirty: true,
          })
        })
      },

      createView: function() {
        this.activeDocument.views.push({ title: 'Fresh View', id: lastId++ })
        this.activeDocument.isViewDirty = false
      },

      createPose: function() {
        this.activeDocument.poses.push({ title: 'Untitled Pose', id: lastId++ })
        this.activeDocument.isPoseDirty = false
      },

      createSet: function() {
        this.activeDocument.sets.push({ title: 'Untitled Set', id: lastId++ })
        this.activeDocument.isSetDirty = false
      },

      activateView: function(view) {
        this.activeDocument.activeView = view
      },

      activatePose: function(pose) {
        this.activeDocument.activePose = pose
      },

      changeDocument: function(doc) {
        this.activeDocument = doc
        this.activeDocument.activeView = this.activeDocument.views[0]
        this.activeDocument.activePose = this.activeDocument.poses[0]
      }
    },

    data() {
      return {
        isFullscreen: false,
        isBrowser: !window.ipcRenderer,
        activeDocument: null,
        documents: [],
      }
    }
  }
</script>
