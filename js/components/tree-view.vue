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
    max-height: 100%
    overflow-x: hidden
    overflow-y: auto
    padding-top: 10px
    pointer-events: all
    // width: 100%
    // padding-right: 24px
    // box-sizing: content-box
    padding-right: 1px // Somehow needed for highlighted borders to look right
    direction: rtl
    > *
      direction: ltr

  .tree-item
    margin-left: -7px
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
