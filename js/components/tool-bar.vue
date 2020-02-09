<template lang="pug">
  header.tool-bar

    MenuButton.app-menu-btn(title="App menu" icon="atom")

    ul.tabs
      li(v-for="doc in documents"
         @click="activate(doc)"
         :class="{active: doc == activeDocument}")
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
  
  .fullscreen
    .tool-bar
      padding-left: 0px
      background: $dark2
    .tabs li
      padding: 6px 12px
      height: 22px

  .app-menu-btn
    color: $highlight * 1.2
  
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
      min-width: 120px
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
        margin-left: 12px
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
      documents: Array,
      activeDocument: Object,
    },

    data() {
      return {}
    },

    methods: {
      activate: function(doc) {
        this.$emit('change-document', doc)
      },
    }
  }
</script>
