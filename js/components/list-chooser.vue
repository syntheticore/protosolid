<template lang="pug">
  ul.list-chooser
    li(is="tree-item", :node="top")
</template>


<style lang="stylus" scoped>
  .list-chooser
    margin-left: -6px
  h1
    color: $bright2
    font-size: 0.8rem
    font-weight: bold
    letter-spacing: 1px
    margin-left: 12px
    margin-bottom: 6px
    text-transform: uppercase
</style>


<script>
  import TreeItem from './tree-item.vue'

  export default {
    name: 'ListChooser',
    components: {
      TreeItem,
    },
    props: {
      top: Object,
    },
  }
</script>
