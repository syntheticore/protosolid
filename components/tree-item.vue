<template lang="pug">

  li.tree-item
    header(
      @dblclick="document.activateComponent(component)"
      @mouseenter="$emit('update:highlight', component)"
      @mouseleave="$emit('update:highlight', null)"
      @click="document.selection = document.selection.handle(component, bus.isCtrlPressed)"
    )
      Icon.expander(
        icon="caret-down"
        :class="{blank: !canExpand || isTop, closed: !expanded}"
        @click.stop="toggle()"
        @dblclick.stop
        fixed-width
      )
      .box(:class="{hidden: !isVisible, active: component === document.activeComponent, selected: document.selection.has(component)}")
        header
          Icon.eye(
            v-if="!isTop"
            icon="eye" fixed-width
            @click.stop="component.creator.hidden = !component.creator.hidden"
          )
          Icon.component(
            :icon="isAssembly ? 'boxes' : 'box'"
            :style="{'--color': component.creator.color}"
          )
          span.name {{ component.creator.title }}
          .controls.wide(:class="{'ultra-wide': !isTop}")
            Icon(
              icon="check-circle" fixed-width
              title="Activate"
              @click.stop="document.activateComponent(component)"
            )
            Icon(
              icon="plus-circle" fixed-width
              title="Create Component"
              @click.stop="document.createComponent(component)"
              @dblclick.stop
            )
            Icon.delete(
              v-if="!isTop"
              icon="trash-alt" fixed-width
              title="Delete Component"
              @click.stop="document.deleteComponent(component)"
            )

    ul.widgets(
      v-if="expanded"
      :class="{hidden: !isVisible}"
    )
      //- Material
      TreeletMaterial(
        v-if="component.creator.material"
        :document="document"
        :material="component.creator.material"
        :component="component"
      )

      //- Center of Mass
      li(v-if="component.cog")
        .box
          header
            Icon(icon="atom" fixed-width)
            h2 Center of Mass

      //- Parameters
      li(v-for="param in component.creator.parameters")
        TreeletParameter(
          :parameter="param"
          :component="component"
        )

      //- Export Configs
      li(v-for="config in component.creator.exportConfigs")
        TreeletExport(:config="config", :component="component")

      //- Section Views
      li(v-for="view in component.creator.sectionViews")
        .box
          header
            Icon(icon="object-group" fixed-width)
            h2 Section View 1
            input(type="checkbox")

      //- Solids
      TreeletSolid(
        v-for="(solid, i) in component.compound.solids()"
        :key="'solid' + i"
        v-bind="$attrs"
        :document="document"
        :solid="solid"
        :index="i"
      )
      //- v-on="$listeners"
      //- :component="component"
      //- :selection="selection"

      //- Sketches
      TreeletSketch(
        v-for="(sketch, i) in component.sketches"
        :key="'sketch' + i"
        v-bind="$attrs"
        :document="document"
        :component="component"
        :sketch="sketch"
        :index="i"
      )
      //- v-on="$listeners"

    //- Children
    transition-group(name="list" tag="ul" v-if="isAssembly && expanded")
      TreeItem(
        v-for="child in component.children"
        :key="child.id"
        v-bind="$attrs"
        :document="document"
        :component="child"
        :parent-hidden="!isVisible"
      )
      //- v-on="$listeners"
      //- :active-component="document.activeComponent"
      //- :selection="selection"

</template>


<style lang="stylus" scoped>
  .tree-item
    margin-left: 23px
    > header
      display: inline-flex
      align-items: center
      padding: 1px 0
      // pointer-events: auto
      margin-right: 10px // Needed in FF when scrollbars are active

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
    &.component
      padding-left: 2px
      color: var(--color)
    &.blank
      visibility: hidden

  .name
    margin-right: 6px
    white-space: nowrap

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
  .list-enter-from
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
      // pointer-events: auto
      margin-right: 10px // Needed in FF when scrollbars are active
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
        background: $highlight * 0.7 !important
        border-color: $highlight * 1.1
        h2
        header svg
          color: white
        .controls
          border-color: $highlight * 1.1
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
        transition: background-color 0.15s
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
  // import ParameterTreelet from './treelet-parameter.vue'
  // import MaterialTreelet from './treelet-material.vue'
  // import SolidTreelet from './treelet-solid.vue'
  // import SketchTreelet from './treelet-sketch.vue'
  // import ExportTreelet from './treelet-export.vue'

  export default {
    name: 'TreeItem',

    inject: ['bus'],

    // components: {
    //   ParameterTreelet,
    //   MaterialTreelet,
    //   SolidTreelet,
    //   ExportTreelet,
    //   SketchTreelet,
    // },

    props: {
      isTop: Boolean,
      document: Object,
      component: Object,
      // activeComponent: Object,
      parentHidden: Boolean,
      // selection: Object,
    },

    data() {
      return {
        expanded: true,
        foo: Math.random(),
      };
    },

    watch: {
      'component.creator.hidden': function(hidden) {
        this.document.emit('component-changed', this.component, true)
      }
    },

    computed: {
      isAssembly: function() {
        return !!this.component.children.length
      },

      canExpand: function() {
        return this.component.children.length ||
          this.component.compound.solids().length ||
          this.component.sketches.length ||
          this.component.creator.cog ||
          this.component.creator.material ||
          this.component.creator.parameters.length ||
          this.component.creator.sectionViews.length ||
          this.component.creator.exportConfigs.length
      },

      isVisible: function() {
        return !this.component.creator.hidden && !this.parentHidden
      },
    },

    methods: {
      toggle: function() {
        this.expanded = !this.expanded
      },
    },
  }
</script>
