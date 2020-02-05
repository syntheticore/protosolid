<template lang="pug">
  li.tree-item
    header
      fa-icon.expander(icon="caret-down",
                       :class="{blank: !isAssembly || isTop, closed: !expanded}",
                       v-on:click="toggle()"
                       fixed-width)
      .box(:class="{hidden: !isVisible}")
        fa-icon.eye(icon="eye" fixed-width v-on:click="hidden = !hidden" v-if="!isTop")
        fa-icon.assembly(icon="boxes" fixed-width v-if="isAssembly")
        fa-icon.part(icon="box" fixed-width v-else)
        span.name {{ node.name }}
    //- ul.children(v-show="isAssembly && expanded")
    transition(name="fade" mode="out-in")
      transition-group.children(name="list" tag="ul" v-show="expanded" v-if="isAssembly")
        li(is="tree-item",
           v-for="child in node.children",
           :key="child.id",
           :node="child",
           :parent-hidden="!isVisible")
</template>


<style lang="stylus" scoped>
  .tree-item
    margin-left: 21px
  header
    pointer-events: all
    display: inline-block
  .expander
    margin-right: 0
    font-size: 16px
    // cursor: pointer
    transition: all 0.2s
    &:hover
      color: $bright1
    &.closed
      transform: rotate(-0.25turn)
  .box
    background: $dark2
    font-size: 0.75rem
    font-weight: bold
    padding: 4px 3px
    margin: 1px 0
    border: 0.5px solid $dark1 * 1.2
    border-radius: 3px
    display: inline-block
    transition: all 0.2s
    // box-shadow: 0 0 0 1px $dark2 * 0.75
    // text-shadow: 0 -1px 1px black
    &.hidden
      opacity: 0.5
  svg
    font-size: 13px
    margin-right: 4px
    color: $bright2
    &.eye
      color: $bright1
      // cursor: pointer
      transition: all 0.2s
      &:hover
        color: $bright1 * 2
    &.part
      color: #528e9e
    &.assembly
      color: #ca6f8e
    &.blank
      visibility: hidden
  .fade-enter-active
    transition: all 0.6s
  .fade-leave-active
    transition: all 0.2s
  .fade-enter
  .fade-leave-to
    opacity: 0
  .list-enter
  .list-leave-to
    opacity: 0
    transform: translateY(30px)
  .list-leave-active
    position: absolute;
  .list-move
    transition: transform 1s
</style>


<script>
  export default {
    name: 'TreeItem',
    props: {
      isTop: Boolean,
      node: Object,
      parentHidden: Boolean,
    },
    data() {
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
