<template lang="pug">
  .bordered.tool-box
    ul.tabs
      li(
        v-for="(tab, index) in tabs"
        v-on:click="activeTab = index"
        :class="{active: index == activeTab}"
      )
        | {{ tab.title }}
    ul.tools
      li.button(v-for="tool in tabs[activeTab].tools")
        fa-icon(:icon="tool.icon" fixed-width)
        .title(v-html="tool.title")
</template>


<style lang="stylus" scoped>
  .tool-box
    border-top-left-radius: 3px
    border-top-right-radius: 3px
    border-bottom-left-radius: 6px
    border-bottom-right-radius: 6px
    background: rgba($dark2, 0.9)
    overflow: hidden
    // min-width: 405px

  .tabs
    box-shadow: 0 0 4px rgba(black, 0.6)
    border-bottom: 1px solid $dark1 * 1.15
    display: flex
    overflow-x: auto
    li
      padding: 5px 10px
      background: $dark2
      font-size: 12px
      text-align: center
      transition: all 0.2s
      & + li
        border-left: 1px solid $dark1 * 1.15
      &:hover
        background: $dark2 * 1.2
      &:active
      &.active
        background: $dark1 * 1.15

  .tools
    padding-right: 4px
    padding-bottom: 4px
    li
      display: inline-block
      text-align: center
      background: $dark1
      padding: 5px 6px
      min-width: 55px
      padding-bottom: 4px
      margin-right: 0
      margin-bottom: 0
      &:hover
        background: $dark1 * 1.15
        svg
          color: $highlight
      &:active
        background: $dark1 * 0.9
      svg
        font-size: 21px
        color: $bright1
        transition: all 0.4s
      .title
        color: $bright2
        font-size: 11px
        margin-top: 6px
        font-weight: bold
</style>


<script>
  // import Foo from './foo.vue'

  export default {
    name: 'ToolBox',
    components: {},
    props: {},
    data() {
      return {
        activeTab: 1,
        tabs: [
          {
            title: 'Sketch',
            tools: [
              { title: 'Line', icon: 'project-diagram' },
              { title: 'Rectangle', icon: 'vector-square' },
              { title: 'Arc', icon: 'bezier-curve' },
              { title: 'Circle', icon: 'ban' },
              { title: 'Spline', icon: 'route' },
            ]
          },
          {
            title: 'Create',
            tools: [
              { title: 'Extrude', icon: 'box' },
              { title: 'Revolve', icon: 'wave-square' },
              { title: 'Loft', icon: 'layer-group' },
              { title: 'Sweep', icon: 'route' },
              { title: 'Mirror', icon: 'band-aid' },
              { title: 'Array', icon: 'th' },
            ],
          },
          {
            title: 'Edit',
            tools: [
              { title: 'Shell', icon: 'magnet' },
              { title: 'Boolean', icon: 'boxes' },
              { title: 'Fillet', icon: 'clone' },
              { title: 'Chamfer', icon: 'screwdriver' },
              { title: 'Split', icon: 'code-branch' },
            ],
          },
          { title: 'Construct', tools: [] },
          { title: 'Constrain', tools: [] },
          { title: 'Simulate', tools: [] },
          { title: 'Make', tools: [] },
        ]
      }
    },
    mounted() {},
    beforeDestroy() {},
  }
</script>
