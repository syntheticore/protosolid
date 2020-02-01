<template lang="pug">
  #app
    header#main-header
    SideBar
    ViewPort
    footer#main-footer
      | Footer
</template>


<style lang="stylus" scoped>
  #app
    height: 100%
    display: grid
    grid-template-columns: 220px 1fr
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

  .side-bar
    grid-column: 1
    grid-row: 1
    border-right: black

  .view-port
    grid-column: 2
    grid-row: 1

  #main-footer
    grid-column: 1 / 3
    grid-row: 2
    background: #1b1b1b
    border-top: 1px solid #2b3034
    font-size: 12px
    padding: 6px 12px
    color: #878787
    text-shadow: 0 1px 2px black
    background: linear-gradient(top, $dark1, $dark2)
</style>


<script>
  import SideBar from './side-bar.vue'
  import ViewPort from './view-port.vue'
  import('../../rust/pkg/wasm-index.js').then(main).catch(console.error);

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
      SideBar,
      ViewPort,
    },
    data () {
      return {
        projectName: 'Untitled Project'
      }
    }
  }
</script>
