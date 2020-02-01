<template lang="pug">
  #app
    header#main-header
    TreeView(msg="Hellas")
    main
      canvas#main-canvas
    footer#main-footer
      | Footer
</template>


<style lang="stylus" scoped>
  #app
    height: 100%
    display: grid
    grid-template-columns: 200px 1fr
    grid-template-rows: 1fr 26px
    // grid-gap: 1px
    // grid-auto-rows: minmax(100px, auto)

  #main-header
    -webkit-app-region: drag
    -webkit-user-select: none
    width: 100%
    height: 38px
    position: absolute
    z-index: 99
    pointer-events: none
    // background: rgb(41, 53, 67);
    // border-bottom: 1px solid #1d283f
    // padding: 11px 84px

  main
    grid-column: 2
    grid-row: 1
    position: relative
    overflow: hidden
    display: flex

  #main-canvas
    flex: 1 1 auto
    background: radial-gradient(0 0, farthest-corner, #2f4553, #08111b)

  #main-footer
    // height: 30px
    background: #1b1b1b
    border-top: 1px solid #2b3034
    font-size: 12px
    padding: 6px 12px
    color: #878787
    text-shadow: 0 1px 2px black
    // font-weight: bold
    grid-column: 1 / 3
    grid-row: 2
    background: linear-gradient(top, #202424, #1f1f1f)
</style>


<script>
  import TodoItem from './todo-item.vue'
  import TreeView from './tree-view.vue'
  import('../../rust/pkg/wasm_index.js').then(main).catch(console.error);

  function main(wasm) {
    // let canvas = document.getElementById('main-canvas');

    // function resizeCanvas() {
    //   canvas.width = canvas.offsetWidth;
    //   canvas.height = canvas.offsetHeight;
    //   console.log('resize')
    // }

    // resizeCanvas();
    // window.addEventListener('resize', resizeCanvas);
    
    // var alchemy = wasm.getAlchemy();

    // window.addEventListener('keydown', event => {
    //   console.log(event.keyCode);
    //   if(event.keyCode === 83) { // S
    //     return;
    //   }
    // });

    // function render() {
    //   // requestAnimationFrame(render);
    //   setTimeout(() => {render()}, 3000);
    //   // alchemy.render(canvas);
    // }
    // requestAnimationFrame(render);
  }

  export default {
    name: 'app',
    components: {
      TodoItem,
      TreeView,
    },
    data () {
      return {
        projectName: 'Untitled Project'
      }
    }
  }
</script>
