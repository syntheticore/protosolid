<template lang="pug">
  li.tree-item
    fa-icon(icon="caret-down", :class="{blank: !isAssembly}")
    header
      fa-icon(icon="eye" fixed-width :class="{bright: !hidden}")
      fa-icon(icon="boxes" fixed-width v-if="isAssembly")
      fa-icon(icon="box" fixed-width v-else)
      span.name {{ node.name }}
    ul.children(v-if="isAssembly")
      li(is="tree-item",
         v-for="child in node.children",
         :key="child.id",
         :node="child",
      )
</template>


<style lang="stylus" scoped>
  .tree-item
    margin-left: 16px
    > svg
      margin-right: 5px
      font-size: 16px
  header
    background: #1b1b1b
    color: $bright1
    font-size: 0.75rem
    font-weight: bold
    padding: 4px 3px
    margin: 2px 0
    border: 1px solid #393939
    border-radius: 3px
    display: inline-block
  svg
    font-size: 13px
    margin-right: 4px
    color: gray
  .blank
    visibility: hidden
  .bright
    color: $bright1
</style>


<script>
  export default {
    name: 'TreeItem',
    props: {
      node: Object,
      hidden: Boolean
    },
    computed: {
      isAssembly: function() {
        return this.node.children && this.node.children.length;
      },
    }
  }
</script>
