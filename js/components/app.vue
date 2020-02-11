<template lang="pug">
  #app(:class="{fullscreen: isFullscreen}")
    ToolBar(
      :documents="documents"
      :active-document="activeDocument"
      @change-document="changeDocument"
    )
    main
      ViewPort(
        @change-view="activeDocument.isViewDirty = true"
        @change-pose="activeDocument.isPoseDirty = true"
      )
      .ui-layer
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

  .ui-layer
    position: absolute
    top: 0
    left: 0
    right: 0
    bottom: 0
    display: grid

  .tool-box
    position: absolute
    top: 16px
    left: 50%

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
  import('../../rust/pkg/wasm-index.js').then(main).catch(console.error);

  function main(wasm) {
    var alchemy = wasm.getAlchemy();
  }

  window.addEventListener('keydown', (e) => {
    console.log(e.keyCode)
    if(e.keyCode === 83) { // S
      return
    }
  });

  let lastId = 1;

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
      this.activeDocument = this.documents[0]
      this.activeDocument.activeView = this.activeDocument.views[1]
      this.activeDocument.activePose = this.activeDocument.poses[0]

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
      }
    },
    
    data() {
      return {
        isFullscreen: false,
        activeDocument: null,
        documents: [
          {
            title: 'Rocket Engine',
            activeView: null,
            activePose: null,
            isViewDirty: false,
            isPoseDirty: false,
            isSetDirty: false,
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
            tree: {
              id: '1',
              name: 'Main Assembly',
              children: [
                {
                  id: '2',
                  name: 'Part 1',
                },
                {
                  id: '3',
                  name: 'Sub Assembly 1',
                  children: [
                    {
                      id: '4',
                      name: 'Part 2',
                    },
                    {
                      id: '5',
                      name: 'Part 3',
                      selected: true,
                    },
                    {
                      id: '6',
                      name: 'Sub Assembly 2',
                      children: [
                        {
                          id: '7',
                          name: 'Part 4',
                          selected: false,
                        },
                        {
                          id: '8',
                          name: 'Part 5',
                        },
                      ]
                    },
                    {
                      id: '9',
                      name: 'Sub Assembly 2',
                      children: [
                        {
                          id: '10',
                          name: 'Part 6',
                        },
                        {
                          id: '11',
                          name: 'Part 7',
                        },
                        {
                          id: '12',
                          name: 'Part 8',
                        },
                      ]
                    },
                  ]
                },
              ]
            }
          },
          { title: 'Print Head', views: [], poses: [], sets: [], tree: {} },
          { title: 'Tool Holder', views: [], poses: [], sets: [], tree: {} },
        ],
      }
    }
  }
</script>
