<template lang="pug">
  #app(:class="{fullscreen: isFullscreen}")
    ToolBar
    main
      ViewPort
      SideBar.bar-left
        //- h1 Tree
        TreeView(:top="tree")
      SideBar.bar-right
        h1 Views
        ListChooser(:list="views")
        h1 Poses
        ListChooser(:list="poses")
        h1 Sets
        ListChooser(:list="sets")
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

  .tool-bar
    grid-area: header

  main
    grid-area: main
    position: relative
    overflow: hidden

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
      right: 0
      margin-right: 14px

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

  export default {
    name: 'app',

    components: {
      ToolBar,
      SideBar,
      ViewPort,
      TreeView,
      FooterView,
      ListChooser,
    },

    created() {
      if(!window.ipcRenderer) {
        this.isFullscreen = true
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
    
    data() {
      return {
        isFullscreen: false,
        projectName: 'Untitled Project',
        views: [
          { title: 'Top' },
          { title: 'Left', active: true },
          { title: 'Front' },
          { title: 'Perspective' },
        ],
        poses: [
          { title: 'Base' },
          { title: 'Activated' },
        ],
        sets: [
          { title: 'Filet 14' },
          { title: 'Extrude 2' },
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
      }
    }
  }
</script>
