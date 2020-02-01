<template lang="pug">
  main.view-port
    canvas
</template>


<style lang="stylus" scoped>
  .view-port
    position: relative
    overflow: hidden
    display: flex
  canvas
    flex: 1 1 auto
    background: radial-gradient(0 0, farthest-corner, #2f4553, #08111b)
</style>


<script>
  export default {
    name: 'ViewPort',
  }
</script>
