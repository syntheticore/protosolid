<template lang="pug">
  li.tree-item
    header
      fa-icon.expander(icon="caret-down",
                       :class="{blank: !isAssembly || isTop, closed: !expanded}",
                       @click="toggle()"
                       fixed-width)
      .box(:class="{hidden: !isVisible, selected: node.selected}")
        fa-icon.eye(icon="eye" fixed-width @click="hidden = !hidden" v-if="!isTop")
        fa-icon.assembly(icon="boxes"  v-if="isAssembly")
        fa-icon.part(icon="box"  v-else)
        span.name {{ node.get_title() }}
        .controls
          fa-icon.menu(icon="ellipsis-v" fixed-width @click="createComponent(node)")
          fa-icon.activate(icon="edit" fixed-width title="Activate" @click="activate(node)")
    //- ul.children(v-show="isAssembly && expanded")
    transition(name="fade" mode="out-in")
      transition-group.children(name="list" tag="ul" v-show="expanded" v-if="isAssembly")
        li(
          is="tree-item",
          v-for="child in node.get_children()",
          :key="child.get_id()",
          :node="child",
          :parent-hidden="!isVisible",
          v-on="$listeners")
        //- @activate="activate"
</template>


<style lang="stylus" scoped>
  .tree-item
    margin-left: 23px

  header
    pointer-events: all
    // display: inline-block
    display: flex
    align-items: center

  .expander
    margin-right: 0
    font-size: 16px
    padding: 0
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
    // padding: 4px
    margin: 1px 0
    border: 0.5px solid $dark1 * 1.3
    border-radius: 3px
    // display: inline-block
    display: flex
    align-items: center
    transition: opacity 0.2s
    pointer-events: all
    overflow: hidden
    &:hover
      background: $dark2 * 1.3
      border-color: $dark1 * 1.85
      .controls
        margin-right: 0
        opacity: 1
        transition-delay: 0.15s
    &.selected
      border-color: $highlight * 1.2
      box-shadow: 0 0 0px 1px $highlight * 1.2
    &.hidden
      opacity: 0.5

  svg
    font-size: 21px
    padding: 4px
    // padding-right: 3px
    color: $bright2
    &.eye, .controls &
      color: $bright1
      // cursor: pointer
      transition: all 0.15s
      &:hover
        color: $bright1 * 2
        background: $dark1 * 1.85
      &:active
        background: $dark1 * 1.5
        transition: none
    &.part
      color: #139a8f
    &.assembly
      color: #1789ad
    &.part, &.assembly
      padding-left: 2px
    &.blank
      visibility: hidden

  .name
    margin-right: 6px

  .controls
    border-left: 1px solid $dark1 * 1.3
    margin-right: -54px
    opacity: 0
    transition: all 0.2s
    transition-delay: 0.4s

  .fade-enter-active
    transition: all 0.6s
  .fade-leave-active
    transition: all 0.2s
  .fade-enter
  .fade-leave-to
    opacity: 0

  .list-enter-active
  .list-leave-active
    transition: all 0.3s
  .list-enter
  .list-leave-to
    opacity: 0
    height: 0
    padding: 0
    margin: 0
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

      activate: function(node) {
        this.$emit('activate-component', node)
      },

      createComponent: function(parent) {
        console.log('CREATE')
        this.$emit('create-component', parent)
      },
    },
    computed: {
      isAssembly: function() {
        // return this.node.children && this.node.children.length;
        return this.node.get_children().length;
      },
      isVisible: function() {
        return !this.hidden && !this.parentHidden;
      },
    }
  }
</script>
