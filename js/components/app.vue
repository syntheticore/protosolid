<template lang="pug">
  #app
    ToolBar
    main
      ViewPort
      SideBar.bar-left
        h1 Tree
        TreeView(:top="tree")
      SideBar.bar-right
        h1 Views
        TreeView(:top="views")
        h1 Poses
        TreeView(:top="poses")
      footer
        b Select Tool
        fa-icon(icon="mouse" fixed-width)
        | Select geometry
        fa-icon(icon="mouse" fixed-width)
        | Bring up actions
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
    background: $dark1
    color: $bright1

  .tool-bar
    grid-area: header

  main
    grid-area: main
    position: relative
    overflow: hidden

  .side-bar
    pointer-events: none
    position: absolute
    top: 0
    &.bar-left
      left: 0
    &.bar-right
      right: 0

  .view-port
    width: 100%
    height: 100%

  footer
    position: absolute
    left: 0
    bottom: 0
    width: 100%
    font-size: 13px
    padding: 22px
    color: $bright2
    text-shadow: 0 1px 3px black
    b
      margin-right: 6px
      color: $bright1 * 0.9
    svg
      margin-left: 9px
      margin-right: 3px
      color: $bright1
</style>


<script>
  import ToolBar from './tool-bar.vue'
  import SideBar from './side-bar.vue'
  import ViewPort from './view-port.vue'
  import TreeView from './tree-view.vue'
  import('../../rust/pkg/wasm-index.js').then(main).catch(console.error);

  function main(wasm) {
    var alchemy = wasm.getAlchemy();

    window.addEventListener('keydown', event => {
      console.log(event.keyCode);
      if(event.keyCode === 83) { // S
        return;
      }
    });
  }

  export default {
    name: 'app',

    components: {
      ToolBar,
      SideBar,
      ViewPort,
      TreeView,
    },

    mounted() {

    },
    
    data() {
      return {
        projectName: 'Untitled Project',
        views: {

        },
        poses: {

        },
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
                },
                {
                  id: '6',
                  name: 'Sub Assembly 2',
                  children: [
                    {
                      id: '7',
                      name: 'Part 4',
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
