<template lang="pug">

  li.tree-item(:class="{ hidden: !isVisible }")

    header(
      @dblclick="document.activateComponent(component)"
      @mouseenter="$emit('update:highlight', component)"
      @mouseleave="$emit('update:highlight', null)"
      @click="document.selection.handle(component, bus.isCtrlPressed)"
    )

      Icon.expander(
        icon="caret-down"
        :class="{blank: !canExpand || isTop, closed: !expanded}"
        @click.stop="toggle()"
        @dblclick.stop
        fixed-width
      )

      .box(:class="{ active: component === document.activeComponent, selected: document.selection.has(component) }")

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
      TreeletSection(
        v-for="(section, i) in component.creator.sectionViews"
        v-bind="$attrs"
        :document="document"
        :component="component"
        :section="section"
        :index="i"
      )

      //- Solids
      TreeletSolid(
        v-for="(solid, i) in component.compound.solids()"
        :key="solid.id"
        v-bind="$attrs"
        :document="document"
        :component="component"
        :solid="solid"
        :index="i"
      )

      //- Sketches
      TreeletSketch(
        v-for="(sketch, i) in component.sketches"
        :key="sketch.id"
        v-bind="$attrs"
        :document="document"
        :component="component"
        :sketch="sketch"
        :index="i"
      )

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
    color: $bright2

    &:hover
      color: $bright1

    &.closed
      transform: rotate(-0.25turn)

  .box header

    .component
      color: var(--color)

  .blank
    visibility: hidden

  .name
    margin-right: 6px
    white-space: nowrap

  .widgets
    margin-left: 43px
    display: flex
    flex-direction: column
    align-items: flex-start

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

  .tree-view

    .hidden

      .box
        background: rgba($dark2 * 1.3, 0.3) !important

        > header > *:not(.controls)
          opacity: 0.4

  .tree-item

    .box
      background: $dark2
      font-size: 0.75rem
      font-weight: bold
      border: 1px solid $dark1 * 1.4
      border-radius: 3px
      transition: opacity 0.2s
      box-shadow: 0 1px 3px rgba(black, 0.25)
      overflow: hidden
      // pointer-events: auto
      margin-right: 10px // Needed in FF when scrollbars are active
      transition: background 0.15s

      &:hover
        background: $dark2 * 1.15
        border-color: $dark1 * 1.85
        color: white

        .controls
          border-color: $dark1 * 1.85
          transition-delay: 0.1s

        .content
          border-color: $dark1 * 1.85

      .blurry &
        backdrop-filter: blur(16px)

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

      > header
        display: flex
        align-items: center

        svg
          font-size: 13px
          padding: 4px
          color: $bright2

        svg.eye
        .controls svg
          color: $bright1
          transition: all 0.1s

          &:hover
            color: white !important
            background: $dark1 * 1.85

          &:active
            background: $dark1 * 1.5
            transition: none

        .controls .delete
        .controls .delete:hover
          color: $cancel !important

    .controls
      border-left: 1px solid $dark1 * 1.4
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
      width: 25px

      &.wide
        width: 49px

      &.ultra-wide
        width: 74px

    .widgets li

      .box
        background: rgba($dark2 * 1.3, 0.7)
        font-size: 11px

        &:hover

          header svg
            color: $bright1

      h2
        margin-right: 8px

      .content
        border-top: 1px solid $dark1 * 1.4

      .form

        fieldset + fieldset
          border-top: 1px solid $dark1 * 1.4

        input[type="checkbox"]
          margin-left: 82px

</style>


<script>

  export default {
    name: 'TreeItem',

    inject: ['bus'],

    props: {
      isTop: Boolean,
      document: Object,
      component: Object,
      parentHidden: Boolean,
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
