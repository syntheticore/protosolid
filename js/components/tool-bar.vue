<template lang="pug">
  header.tool-bar

    MenuButton(title="App menu" icon="atom")

    ul.tabs
      li(v-for="doc in documents" :class="{active: doc.active}")
        span.title {{ doc.title }}
        fa-icon(icon="times")
    
    .grab-handle
    
    nav
      MenuButton(title="Tool Settings" icon="pen")
        form
          label
            | Transform:
            select
              option World
              option Local
          fieldset
            legend Selection
            label
              input(type="checkbox" checked)
              | Select invisible Geometry
          fieldset
            legend Snapping
            label
              input(type="checkbox" checked)
              | Snap to Grid
            label.inset
              | Increment
              input(type="number" value="10")
            label
              input(type="checkbox" checked)
              | Snap to Angles
            label.inset
              | Increment
              input(type="number" value="45")
      MenuButton(title="Tool Settings" icon="cloud")
      //- MenuButton(title="Snapping" icon="ruler")
    
    MenuButton.account(title="Account" icon="user-circle")
      span.name Björn
</template>


<style lang="stylus" scoped>
  .tool-bar
    display: flex
    // overflow: hidden
    padding-left: 74px
    // background: $dark2
    background: linear-gradient(top, $dark1, $dark2 * 0.95)
    // background: linear-gradient(top, rgba($dark1, 0.92), rgba($dark2, 0.92))
    border-top: 1px solid $dark1 * 1.3
    border-bottom: 1px solid black
    border-radius: 5px 5px 0px 0px
    border-radius-bottom: 0
    .fullscreen &
      padding-left: 1px
  
  .tabs
    display: inline-block
    flex: 1 1 content
    li
      display: inline-block
      border-left: 1px solid $dark1 * 1.25
      border-right: 1px solid black //#353535
      // box-shadow: 0 0 3px black
      border-bottom: none
      padding: 12px 12px
      height: 36px
      font-size: 12px
      font-weight: bold
      min-width: 160px
      transition: all 0.2s
      color: $bright2
      text-shadow: 0 -1px 0px black
      // cursor: pointer
      &:hover
        background: $dark1
      &.active
        color: $bright1
        background: $dark1 * 1.2
        // border-top: 1px solid $highlight
        border-left: 1px solid $dark1 * 1.45
      & + li
        border-left: 1px solid $dark1 * 1.1
      svg
        margin-left: 8px
        color: $bright2
        cursor: pointer
        transition: all 0.2s
        float: right
        filter: drop-shadow(0 1px 0px rgba(0,0,0, 0.9))
        &:hover
          color: $bright1
  
  .grab-handle
    -webkit-app-region: drag
    -webkit-user-select: none
    flex: 1 1 auto
  
  nav
    margin-right: 6px
  
  .account
    // padding: 4px 4px 22px 14px
    margin: 4px 5px
    // font-size: 12px
    font-weight: bold
    flex: 0 0 auto
    .name
      position: relative
      top: -3px
    svg
      font-size: 18px
      margin-left: 6px

  input[type="text"]
  input[type="number"]
    width: 47px
  
  .inset
    margin-left: 8px
</style>


<script>
  import MenuButton from './menu-button.vue'

  export default {
    name: 'ToolBar',

    components: {
      MenuButton,
    },
    
    props: {

    },

    data() {
      return {
        isFullscreen: false,
        documents: [
          { title: 'Steam Engine', active: false },
          { title: 'Print Head', active: true },
          { title: 'Tool Holder' },
        ]
      };
    },

    methods: {

    }
  }
</script>
