<template lang="pug">
  .side-bar
    slot
</template>


<style lang="stylus" scoped>
  .side-bar
    // padding-top: 38px
    pointer-events: none
    // overflow: hidden
  h1
    color: $bright2
    color: gray * 1.5
    font-size: 13px
    font-weight: bold
    letter-spacing: 1px
    margin-bottom: 8px
    text-transform: uppercase
    text-shadow: 0 1px 2px rgba(0,0,0, 0.7)
</style>


<script>
  // import TreeView from './tree-view.vue'

  export default {
    name: 'SideBar',
    components: {

    },
    data() {
      return {}
    }
  }
</script>
