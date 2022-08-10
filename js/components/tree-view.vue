<template lang="pug">
  ul.tree-view
    li(
      is="tree-item"
      :component="top"
      :active-component="activeComponent"
      :selection="selection"
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
    padding-bottom: 26px // Neccessary to avoid scroll container flickering
    pointer-events: auto
    scrollbar-color: $dark1 * 1.15 $dark2 * 0.9
    scrollbar-width: thin
    &::-webkit-scrollbar
      width: 8px
      background-color: $dark2
    &::-webkit-scrollbar-thumb
      background: $dark1 * 1.15
      &:hover
        background: $dark1 * 1.3
    // width: 100%
    // padding-right: 24px
    // box-sizing: content-box
    padding-right: 1px // Somehow needed for highlighted borders to look right
    direction: rtl
    > *
      direction: ltr
      text-align: left

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
      selection: Object,
    },
  }
</script>
