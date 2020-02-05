<template lang="pug">
  ul.tree-view
    li(is="tree-item", :node="top" is-top)
</template>


<style lang="stylus" scoped>
  .tree-view
    margin-left: -10px
    margin-bottom: 22px
  .tree-item
    margin-left: 0
</style>


<script>
  import TreeItem from './tree-item.vue'

  export default {
    name: 'TreeView',
    components: {
      TreeItem,
    },
    props: {
      top: Object,
    },
  }
</script>
