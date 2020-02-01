<template lang="pug">
  .side-bar
    h1 Views
    TreeView(:top="views")
    h1 Poses
    TreeView(:top="poses")
    h1 Tree
    TreeView(:top="tree")
</template>


<style lang="stylus" scoped>
  .side-bar
    padding-top: 38px
    overflow: hidden
    background: $dark2
  h1
    color: $bright2
    font-size: 0.8rem
    font-weight: bold
    letter-spacing: 1px
    margin-left: 12px
    margin-bottom: 6px
    text-transform: uppercase
  .tree-view
    margin-bottom: 22px
</style>


<script>
  import TreeView from './tree-view.vue'

  export default {
    name: 'SideBar',
    components: {
      TreeView,
    },
    data() {
      return {
        views: {

        },
        poses: {

        },
        tree: {
          id: '1',
          name: 'Main Assembly',
          children: [
            {
              id: '2',
              name: 'Part 1',
            },
            {
              id: '3',
              name: 'Sub Assembly 1',
              children: [
                {
                  id: '4',
                  name: 'Part 2',
                },
              ]
            },
          ]
        }
      }
    }
  }
</script>
