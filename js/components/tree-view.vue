<template lang="pug">
  ul.tree-view
    li(
      is="tree-item"
      :node="top"
      :active-node="activeComponent"
      v-on="$listeners"
      is-top
    )
</template>


<style lang="stylus" scoped>
  .tree-view
    width: 100%
    max-height: 100%
    margin-left: -7px
    margin-bottom: 22px
    overflow-y: auto
    padding-right: 17px
    box-sizing: content-box

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
      activeComponent: Object,
    },

    methods: {

    }
  }
</script>
