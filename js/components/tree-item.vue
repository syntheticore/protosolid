<template lang="pug">
li.tree-item
  header
    img
    span.name {{ node.name }}
  ul.children
    li(is="tree-item",
       v-for="child in node.children",
       :key="child.id",
       :node="child",
    )
</template>


<style lang="stylus" scoped>
// .tree-item
header
  background: #1b1b1b
  color: gray
  font-size: 0.85rem
  font-weight: bold
  padding: 6px 12px
  border-bottom: 1px solid black
</style>


<script>
export default {
  name: 'TreeItem',
  props: ['node']
  // props: {
  //   node: Object,
  // },
}
</script>
