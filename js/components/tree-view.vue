<template lang="pug">
ul.tree-view
  TreeItem(:node="top")
</template>


<style lang="stylus" scoped>
.tree-view
  padding-top: 40px
  background: #1b1b1b
  border-right: black
  grid-column: 1
  grid-row: 1
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
