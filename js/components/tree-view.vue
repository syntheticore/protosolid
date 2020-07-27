<template lang="pug">
  ul.tree-view
    li(is="tree-item", :node="top", v-on:create-component="createComponent", is-top)
</template>


<style lang="stylus" scoped>
  .tree-view
    margin-left: -7px
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
    methods: {
      createComponent: function(parent) {
        parent.create_component('New Component')
      },
    }
  }
</script>
