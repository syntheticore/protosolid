<template lang="pug">
  li.tree-item
    fa-icon.expander(:icon="expanded ? 'caret-down' : 'caret-right'",
                     :class="{blank: !isAssembly || isTop}",
                     v-on:click="toggle()"
                     fixed-width)
    header(:class="{hidden: !isVisible}")
      fa-icon.eye(icon="eye" fixed-width v-on:click="hidden = !hidden")
      fa-icon(icon="boxes" fixed-width v-if="isAssembly")
      fa-icon(icon="box" fixed-width v-else)
      span.name {{ node.name }}
    ul.children(v-show="isAssembly && expanded")
      li(is="tree-item",
         v-for="child in node.children",
         :key="child.id",
         :node="child",
         :parent-hidden="!isVisible")
</template>


<style lang="stylus" scoped>
  .tree-item
    margin-left: 20px
  .expander
    margin-right: 0
    font-size: 16px
    cursor: pointer
    &:hover
      color: $bright1
  header
    background: $dark1
    color: $bright1
    font-size: 0.75rem
    font-weight: bold
    padding: 4px 3px
    margin: 1px 0
    border: 0.5px solid #393939
    border-radius: 3px
    display: inline-block
    &.hidden
      opacity: 0.5
  svg
    font-size: 13px
    margin-right: 4px
    color: $bright2
    &.eye
      color: $bright1
      cursor: pointer
      &:hover
        color: $bright1 * 2
    &.blank
      visibility: hidden
</style>


<script>
  export default {
    name: 'TreeItem',
    props: {
      isTop: Boolean,
      node: Object,
      parentHidden: Boolean,
    },
    data: function() {
      return {
        hidden: false,
        expanded: true,
      };
    },
    methods: {
      toggle: function() {
        this.expanded = !this.expanded;
      },
    },
    computed: {
      isAssembly: function() {
        return this.node.children && this.node.children.length;
      },
      isVisible: function() {
        return !this.hidden && !this.parentHidden;
      },
    }
  }
</script>
