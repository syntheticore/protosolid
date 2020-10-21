<template lang="pug">
  header.tab-bar

    MenuButton.left.app-menu-btn(icon="atom")
      .about
        fa-icon(icon="atom")
        h1 Alchemy
        .version Version 0.1
      hr
      button.button(@click="createDocument") New Document
      form.preferences
        IconView

    ul.tabs
      li(v-for="doc in documents"
         @click="activate(doc)"
         :class="{active: doc == activeDocument}")
        span.title {{ doc.title }}
        fa-icon(icon="times")

    .grab-handle.dynamic

    nav
      MenuButton(title="Tool Settings" icon="code-branch")
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

    .grab-handle.fixed

    .window-controls
      button(@click="minimize")
        fa-icon(icon="window-minimize")

      button(v-if="isMaximized" @click="unmaximize")
        fa-icon(icon="window-restore")

      button(v-else @click="maximize")
        fa-icon(icon="window-maximize")

      button(@click="close")
        fa-icon(icon="window-close")

</template>


<style lang="stylus" scoped>
  .tab-bar
    display: flex
    // overflow: hidden
    // background: $dark2
    background: linear-gradient(top, $dark1 * 0.9, $dark2 * 0.95)
    // background: linear-gradient(top, rgba($dark1, 0.92), rgba($dark2, 0.92))
    // border-top: 1px solid $dark1 * 1.3
    border-bottom: 1px solid black
    max-width: 100vw
    align-items: center
    [data-platform="darwin"] &
      border-radius: 4px 4px 0px 0px
      padding-left: 74px
    > *
      flex: 0 0 auto

  .fullscreen
  .maximized
  [data-platform="browser"]
    .tab-bar
      padding-left: 0px
      border-radius: 0
      // border-top: none
    .tab-bar
      border-top: none
    .window-controls button:last-child
      border-radius: 0

  .app-menu-btn
    color: $highlight * 1.2
    hr
      border: 0
      height: 1px
      margin: 12px 0
      background-image: linear-gradient(to right, $dark2, $dark1 * 1.4, $dark2)

  .about
    text-align: center
    svg
      font-size: 45px
      color: $bright1
    h1
      font-size: 16px
      font-weight: bold
      color: $bright1
      margin-top: 8px
    .version
      font-size: 11px
      margin-top: 4px

  .preferences
    min-height: 200px

  .tabs
    // display: inline-block
    display: flex
    flex: 1 1 auto
    white-space: nowrap
    overflow: auto
    -ms-overflow-style: none
    scrollbar-width: none
    height: 100%
    &::-webkit-scrollbar
      display: none
    li
      // display: inline-block
      display: flex
      align-items: center
      border-left: 1px solid $dark1 * 1.2
      // border-right: 1px solid black //#353535
      // box-shadow: 0 0 3px black
      // border-bottom: none
      padding: 0 12px
      // height: 36px
      height: 100%
      font-size: 12px
      font-weight: bold
      // min-width: 120px
      transition: all 0.2s
      color: $bright2
      text-shadow: 0 -1px 0px black
      // cursor: pointer
      &:hover
        // background: $dark1
        background: $dark2 * 1.2
        svg
          opacity: 1
      &.active
        color: $bright1
        background: $dark1 * 1.2
        // border-top: 1px solid $highlight
        // border-left: 1px solid $dark1 * 1.45
      // & + li
      //   border-left: 1px solid $dark1 * 1.2
      svg
        margin-left: 12px
        color: $bright2
        cursor: pointer
        transition: all 0.2s
        // float: right
        filter: drop-shadow(0 1px 0px rgba(0,0,0, 0.9))
        opacity: 0
        // transition-delay: 0.1s
        &:hover
          color: $bright1
          // transition-delay: 0

  .grab-handle
    -webkit-app-region: drag
    -webkit-user-select: none
    min-width: 24px
    height: 100%
    &.dynamic
      flex: 100 1 auto
      min-width: 44px
    &.fixed
      display: none
      [data-platform="win32"] &
        display: block

  .menu-button
    z-index: 3

  .window-controls
    display: none
    height: 100%
    [data-platform="win32"] &
      display: block
    button
      background: none
      border: none
      color: $bright1
      font-size: 12px
      margin: 0
      padding: 0 12px
      height: 100%
      transition: all 0.15s
      &:last-child
        // border-radius: 0px 4px 0px 0px
      &:hover
        background: $dark1
        color: white
        &:last-child
          background: #b70f0f

  // .account
  //   font-weight: bold
  //   flex: 0 0 auto
  //   .name
  //     position: relative
  //     top: -3px
  //   svg
  //     font-size: 18px
  //     margin-left: 6px

  input[type="text"]
  input[type="number"]
    width: 47px

  .inset
    margin-left: 8px
</style>


<script>
  import MenuButton from './menu-button.vue'
  import IconView from './icon-view.vue'

  export default {
    name: 'ToolBar',

    components: {
      MenuButton,
      IconView,
    },

    props: {
      documents: Array,
      activeDocument: Object,
      isMaximized: Boolean,
    },

    data() {
      return {}
    },

    methods: {
      createDocument: function() {
        this.$emit('create-document')
      },

      activate: function(doc) {
        this.$emit('change-document', doc)
      },

      minimize: function() {
        window.ipcRenderer.send('minimize')
      },

      maximize: function() {
        window.ipcRenderer.send('maximize')
      },

      unmaximize: function() {
        window.ipcRenderer.send('unmaximize')
      },

      close: function() {
        window.ipcRenderer.send('close')
      },
    }
  }
</script>
