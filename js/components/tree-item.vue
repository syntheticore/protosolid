<template lang="pug">
  li.tree-item

    header(
      @dblclick="$emit('update:active-component', component)"
      @mouseenter="$emit('update:highlight', component)"
      @mouseleave="$emit('update:highlight', null)"
      @click="$emit('update:selection', selection.handle(component, $root.isCtrlPressed))"
    )
      fa-icon.expander(
        icon="caret-down"
        :class="{blank: !canExpand || isTop, closed: !expanded}"
        @click.stop="toggle()"
        @dblclick.stop
        fixed-width
      )
      .box(:class="{hidden: !isVisible, active: component === activeComponent, selected: selection.has(component)}")
        header
          fa-icon.eye(
            v-if="!isTop"
            icon="eye" fixed-width
            @click.stop="component.UIData.hidden = !component.UIData.hidden"
          )
          fa-icon.assembly(icon="boxes" v-if="isAssembly")
          fa-icon.part(icon="box" v-else)
          span.name {{ component.UIData.title }}
          .controls.wide(:class="{'ultra-wide': !isTop}")
            fa-icon(
              icon="check-circle" fixed-width
              title="Activate"
              @click.stop="$emit('update:active-component', component)"
            )
            fa-icon(
              icon="plus-circle" fixed-width
              title="Create Component"
              @click.stop="$emit('create-component', component)"
              @dblclick.stop
            )
            fa-icon.delete(
              v-if="!isTop"
              icon="trash-alt" fixed-width
              title="Delete Component"
              @click.stop="$emit('delete-component', component)"
            )

    ul.widgets(
      v-if="expanded"
      :class="{hidden: !isVisible}"
    )
      //- Material
      MaterialTreelet(
        v-if="component.UIData.material"
        :material="component.UIData.material"
        :component="component"
      )

      //- Center of Mass
      li(v-if="component.cog")
        .box
          header
            fa-icon(icon="atom" fixed-width)
            h2 Center of Mass

      //- Parameters
      li(v-for="param in component.UIData.parameters")
        ParameterTreelet(
          :parameter="param"
          :component="component"
        )

      //- Export Configs
      li(v-for="config in component.UIData.exportConfigs")
        ExportTreelet(:config="config", :component="component")

      //- Section Views
      li(v-for="view in component.UIData.sectionViews")
        .box
          header
            fa-icon(icon="object-group" fixed-width)
            h2 Section View 1
            input(type="checkbox")

      //- Solids
      SolidTreelet(
        v-for="(solid, i) in component.solids"
        :key="'solid' + i"
        :solid="solid"
        :component="component"
        :selection="selection"
        :index="i"
        v-on="$listeners"
      )

      //- Sketches
      SketchTreelet(
        v-for="(sketch, i) in component.sketches"
        :key="'sketch' + i"
        :sketch="sketch"
        :index="i"
        v-on="$listeners"
      )

    //- Children
    transition-group(name="list" tag="ul" v-if="isAssembly && expanded")
      li(
        is="tree-item"
        v-for="child in component.children"
        :key="child.id"
        :component="child"
        :active-component="activeComponent"
        :parent-hidden="!isVisible"
        :selection="selection"
        v-on="$listeners"
      )
</template>


<style lang="stylus" scoped>
  .tree-item
    margin-left: 23px
    > header
      display: flex
      align-items: center
      padding: 1px 0

  .expander
    margin-right: 0
    font-size: 16px
    padding: 0
    transition: color 0.2s, transform 0.2s
    &:hover
      color: $bright1
    &.closed
      transform: rotate(-0.25turn)

  svg
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

  .widgets
    margin-left: 43px
    transition: opacity 0.2s
    display: flex
    flex-direction: column
    align-items: flex-start
    &.hidden
      opacity: 0.5
    li
      padding: 1px 0
      &:hover
        border-color: $dark1 * 1.85

  .list-enter-active
  .list-leave-active
    transition: all 0.3s
  .list-enter
  .list-leave-to
    opacity: 0
    margin: 0
    height: 0

</style>

<style lang="stylus">
  .tree-item

    .delete
      color: $cancel !important

    .box
      background: $dark2
      font-size: 0.75rem
      font-weight: bold
      border: 0.5px solid $dark1 * 1.3
      border-radius: 3px
      transition: opacity 0.2s
      box-shadow: 0 1px 3px rgba(black, 0.25)
      overflow: hidden
      > header
        display: flex
        align-items: center
      &:hover
        background: $dark2 * 1.15
        border-color: $dark1 * 1.85
        color: white
        .controls
          border-color: $dark1 * 1.85
          transition-delay: 0.1s
        .content
          border-color: $dark1 * 1.85
      &.active
        border-color: $highlight * 1.2
        box-shadow: 0 0 0px 1px $highlight * 1.2
        color: white
      &.selected
        background: $highlight * 0.5 !important
        border-color: $highlight * 1.2
      &.hidden
        opacity: 0.5

    .controls
      border-left: 0.5px solid $dark1 * 1.3
      white-space: nowrap
      overflow: hidden
      width: 0
      opacity: 0
      transition-property: opacity, width
      transition-duration: 0.15s
      transition-delay: 0.5s

    .box:hover .controls
    .expanded .controls
      opacity: 1
      width: 27px
      &.wide
        width: 53px
      &.ultra-wide
        width: 79px

    svg
      font-size: 21px
      padding: 4px
      color: $bright2

    svg.eye
    .controls svg
      color: $bright1
      transition: all 0.1s
      &:hover
        color: white
        background: $dark1 * 1.85
      &:active
        background: $dark1 * 1.5
        transition: none

    .widgets li
      .box
        background: rgba($dark2 * 1.3, 0.93)
        backdrop-filter: blur(8px)
        font-size: 11px
        &:hover
          header svg
            color: $bright1

      h2
        margin-right: 8px

      .content
        border-top: 1px solid $dark1 * 1.3

      .form
        fieldset + fieldset
          border-top: 1px solid $dark1 * 1.3

        input[type="checkbox"]
          margin-left: 60px
</style>


<script>
  import ParameterTreelet from './treelet-parameter.vue'
  import MaterialTreelet from './treelet-material.vue'
  import SolidTreelet from './treelet-solid.vue'
  import SketchTreelet from './treelet-sketch.vue'
  import ExportTreelet from './treelet-export.vue'

  export default {
    name: 'TreeItem',

    components: {
      ParameterTreelet,
      MaterialTreelet,
      SolidTreelet,
      ExportTreelet,
      SketchTreelet,
    },

    props: {
      isTop: Boolean,
      component: Object,
      activeComponent: Object,
      parentHidden: Boolean,
      selection: Object,
    },

    data() {
      return {
        expanded: true,
      };
    },

    watch: {
      'component.UIData.hidden': function(hidden) {
        this.$root.$emit('component-changed', this.component, true)
      }
    },

    computed: {
      isAssembly: function() {
        return !!this.component.children.length;
      },

      canExpand: function() {
        return this.component.children.length ||
          this.component.solids.length ||
          this.component.sketches.length ||
          this.component.UIData.cog ||
          this.component.UIData.material ||
          this.component.UIData.parameters.length ||
          this.component.UIData.sectionViews.length ||
          this.component.UIData.exportConfigs.length
      },

      isVisible: function() {
        return !this.component.UIData.hidden && !this.parentHidden;
      },
    },

    methods: {
      toggle: function() {
        this.expanded = !this.expanded;
      },
    },
  }
</script>
