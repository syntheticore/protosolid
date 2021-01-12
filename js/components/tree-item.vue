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
        :class="{hidden: !isVisible, selected: component.id() == activeComponent.id()}"
        @dblclick="$emit('update:active-component', component)"
        @mouseenter="$emit('highlight-component', component)"
        @mouseleave="$emit('highlight-component', null)"
      )
        fa-icon.eye(
          v-if="!isTop"
          icon="eye" fixed-width
          @click="hidden = !hidden"
        )
        fa-icon.assembly(icon="boxes" v-if="isAssembly")
        fa-icon.part(icon="box" v-else)
        span.name {{ component.get_title() }}
        .controls
          fa-icon.activate(
            icon="check-circle" fixed-width
            title="Activate"
            @click="$emit('update:active-component', component)"
          )
          fa-icon.new-component(
            icon="plus-circle" fixed-width
            title="Create Component"
            @click="createComponent(component)"
          )
          //- fa-icon.new-variable(
          //-   icon="sliders-h" fixed-width
          //-   title="Create Variable"
          //-   @click="createVariable(component)"
          //- )

    ul.content(
      v-if="expanded"
      :class="{hidden: !isVisible}"
    )
      li
        fa-icon(icon="atom")
        span Center of Mass
      li
        fa-icon(icon="volleyball-ball")
        span Polycarbonate
        fa-icon.expand(icon="angle-right")
      li
        fa-icon(icon="object-group")
        span Section View 1

    transition-group.children(name="list" tag="ul" v-if="isAssembly && expanded")
      li(
        is="tree-item"
        v-for="child in component.get_children()"
        :key="child.id()"
        :component="child"
        :active-component="activeComponent"
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
      color: white
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
      transition: all 0.1s
      &:hover
        // color: $bright1 * 2
        color: white
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
    margin-right: -53px
    opacity: 0
    transition: all 0.15s
    transition-delay: 0.25s

  .content
    // margin-left: 23px
    margin-left: 43px
    transition: opacity 0.2s
    display: flex
    flex-direction: column
    align-items: flex-start
    &.hidden
      opacity: 0.5
    li
      display: flex
      font-size: 12px
      align-items: center
      background: $dark2
      border: 0.5px solid $dark1 * 1.3
      border-radius: 3px
      margin: 1px 0
      padding: 1px 5px 1px 2px
      &:hover
        background: $dark2 * 1.3
        border-color: $dark1 * 1.85
        svg
          color: $bright1
      svg
        font-size: 19px
        &.expand
          padding: 2px
          margin-left: 4px
      span
        margin-left: 1px

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
      component: Object,
      activeComponent: Object,
      parentHidden: Boolean,
      data: Object,
    },

    data() {
      return {
        hidden: this.data[this.component.id()].hidden,
        expanded: true,
      };
    },

    watch: {
      hidden: function(hidden) {
        this.$set(this.data[this.component.id()], 'hidden', !this.isVisible)
        this.$root.$emit('component-changed', this.component, true)
      }
    },

    methods: {
      toggle: function() {
        this.expanded = !this.expanded;
      },

      createComponent: function(parent) {
        this.$emit('create-component', parent)
      },
    },

    computed: {
      isAssembly: function() {
        return !!this.component.get_children().length;
      },

      isVisible: function() {
        return !this.hidden && !this.parentHidden;
      },
    }
  }
</script>
