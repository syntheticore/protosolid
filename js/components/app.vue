<template lang="pug">
  #app
    ToolBar
    main
      ViewPort
      SideBar.left
      //- SideBar.right
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
    // grid-template-columns: 220px 1fr
    grid-template-rows: 38px 1fr // 26px
    // grid-gap: 1px
    // grid-auto-rows: minmax(100px, auto)
    grid-template-areas: 
      "header"\
      "main"
    user-select: none
    cursor: default
    overflow: hidden
    background: $dark1

  .tool-bar
    // grid-row: 1
    grid-area: header

  main
    // grid-row: 2
    grid-area: main
    // display: flex
    position: relative
    overflow: hidden

  .side-bar
    // height: 100%
    pointer-events: none
    position: absolute
    top: 0
    &.left
      left: 0
    &.right
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
    // background: linear-gradient(top, $dark1, $dark2)
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
    },

    mounted: function() {

    },
    
    data () {
      return {
        projectName: 'Untitled Project'
      }
    }
  }
</script>
