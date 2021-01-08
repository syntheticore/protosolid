<template lang="pug">
  li.tree-item
    header
      fa-icon.expander(
        icon="caret-down"
        :class="{blank: !isAssembly || isTop, closed: !expanded}"
        @click="toggle()"
        fixed-width
      )
      .box(
        :class="{hidden: !isVisible, selected: node.id() == activeNode.id()}"
        @dblclick="activateComponent(node)"
      )
        fa-icon.eye(
          v-if="!isTop"
          icon="eye" fixed-width
          @click="hidden = !hidden"
        )
        fa-icon.assembly(icon="boxes" v-if="isAssembly")
        fa-icon.part(icon="box" v-else)
        span.name {{ node.get_title() }}
        .controls
          fa-icon.activate(
            icon="check-circle" fixed-width
            title="Activate"
            @click="activateComponent(node)"
          )
          fa-icon.new-component(
            icon="plus-circle" fixed-width
            title="Create Component"
            @click="createComponent(node)"
          )
          //- fa-icon.new-sketch(
          //-   icon="edit" fixed-width
          //-   title="Create Sketch"
          //-   @click="createSketch(node)"
          //- )
          fa-icon.new-variable(
            icon="sliders-h" fixed-width
            title="Create Variable"
            @click="createVariable(node)"
          )
    ul.content(v-if="expanded")
      //- li
      //-   fa-icon(icon="atom")
      //-   span COG
    //- transition(name="fade" mode="out-in")
    //- transition-group.children(name="list" tag="ul" v-show="expanded" v-if="isAssembly")
    transition-group.children(name="list" tag="ul" v-if="isAssembly && expanded")
      li(
        is="tree-item"
        v-for="child in node.get_children()"
        :key="child.id()"
        :node="child"
        :active-node="activeNode"
        :parent-hidden="!isVisible"
        :data="data"
        v-on="$listeners"
      )
</template>


<style lang="stylus" scoped>
  .tree-item
    margin-left: 23px

  header
    // pointer-events: all
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
    // pointer-events: all
    overflow: hidden
    &:hover
      background: $dark2 * 1.3
      border-color: $dark1 * 1.85
      .controls
        margin-right: 0
        opacity: 1
        transition-delay: 0.05s
    &.selected
      border-color: $highlight * 1.2
      box-shadow: 0 0 0px 1px $highlight * 1.2
      color: white
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
    border-left: 0.5px solid $dark1 * 1.3
    margin-right: -80px
    opacity: 0
    transition: all 0.2s
    transition-delay: 0.25s
    // svg
    //   margin-bottom: 1px

  .content
    margin-left: 25px
    li
      display: flex
      font-size: 12px
      align-items: center

  // .fade-enter-active
  //   transition: all 0.6s
  // .fade-leave-active
  //   transition: all 0.2s
  // .fade-enter
  // .fade-leave-to
  //   opacity: 0

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
      activeNode: Object,
      parentHidden: Boolean,
      data: Object,
    },

    data() {
      return {
        hidden: this.data[this.node.id()].hidden,
        expanded: true,
      };
    },

    watch: {
      hidden: function(hidden) {
        this.$set(this.data[this.node.id()], 'hidden', !this.isVisible)
        this.$root.$emit('component-changed', this.node, true)
      }
    },

    methods: {
      toggle: function() {
        this.expanded = !this.expanded;
      },

      activateComponent: function(node) {
        this.$emit('activate-component', node)
      },

      createComponent: function(parent) {
        this.$emit('create-component', parent)
      },
    },

    computed: {
      isAssembly: function() {
        return !!this.node.get_children().length;
      },

      isVisible: function() {
        return !this.hidden && !this.parentHidden;
      },
    }
  }
</script>
