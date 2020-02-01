<template lang="pug">
  .tree-view
    h1 Tree
    ul
      li(is="tree-item", :node="top")
</template>


<style lang="stylus" scoped>
  .tree-view
    padding-top: 38px
    overflow: hidden
    background: #1b1b1b
    border-right: black
    grid-column: 1
    grid-row: 1
  h1
    color: $bright2
    font-size: 0.8rem
    font-weight: bold
    letter-spacing: 1px
    margin-left: 12px
    margin-bottom: 6px
    text-transform: uppercase
  ul
    margin-left: -6px
</style>


<script>
  import TreeItem from './tree-item.vue'

  export default {
    name: 'TreeView',
    components: {
      TreeItem,
    },
    data() {
      return {
        top: {
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
