<template lang="pug">
  ul.tree-view
    li(
      is="tree-item"
      :component="top"
      :active-component="activeComponent"
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
    padding-top: 14px
    padding-right: 24px
    box-sizing: content-box
    pointer-events: all

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
