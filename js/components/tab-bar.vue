<template lang="pug">
  header.tab-bar

    MenuButton.left.app-menu-btn(icon="atom")
      AppMenu(
        :document="activeDocument"
        v-on="$listeners"
      )

    ul.tabs
      li(v-for="doc in documents"
         @click="$emit('update:active-document', doc)"
         :class="{active: doc == activeDocument}")
        span.title {{ doc.filePath || 'Untitled Document' }} {{ doc.hasChanges ? '*' : null }}
        button(@click.stop="$emit('delete-document', doc)")
          fa-icon(icon="times")

    .grab-handle.dynamic

    nav
      MenuButton(title="Tool Settings" icon="cloud")
        IconView
      //- MenuButton(title="History" icon="code-branch")
      //-   form
      //-     label
      //-       | Transform:
      //-       select
      //-         option World
      //-         option Local
      //-     fieldset
      //-       legend Selection
      //-       label
      //-         input(type="checkbox" checked)
      //-         | Select invisible Geometry
      //-     fieldset
      //-       legend Snapping
      //-       label
      //-         input(type="checkbox" checked)
      //-         | Snap to Grid
      //-       label.inset
      //-         | Increment
      //-         input(type="number" value="10")
      //-       label
      //-         input(type="checkbox" checked)
      //-         | Snap to Angles
      //-       label.inset
      //-         | Increment
      //-         input(type="number" value="45")
      //- MenuButton.account(title="Account" icon="user")
      //-   span.name Björn

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
    background: linear-gradient(top, $dark1 * 0.9, $dark2 * 0.95)
    border-bottom: 1px solid black
    max-width: 100vw
    align-items: center
    [data-platform="darwin"]:not([data-darwin-old]) &
      border-radius: 4px 4px 0px 0px
    [data-platform="darwin"] &
      padding-left: 74px
    > *
      flex: 0 0 auto

  .fullscreen
  .maximized
  [data-platform="browser"]
    .tab-bar
      padding-left: 0px
      border-radius: 0 !important
    .window-controls button:last-child
      border-radius: 0
    .app-menu-btn
      margin-left: 3px
      margin-right: 4px

  .app-menu-btn
    // color: #ff9f90
    color: $highlight * 1.2

  .tabs
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
      display: flex
      align-items: center
      border-left: 1px solid $dark1 * 1.2
      padding-left: 12px
      height: 100%
      font-size: 12px
      font-weight: bold
      transition: all 0.2s
      color: $bright2
      text-shadow: 0 -1px 0px black
      &:hover
        background: $dark2 * 1.2
        svg
          opacity: 1
      &.active
        color: $bright1
        background: $dark1 * 1.2
      button
        background: none
        border: none
        padding: 6px 11px 6px 10px
        &:hover svg
          color: $bright1
        svg
          color: $bright2
          transition: all 0.2s
          filter: drop-shadow(0 1px 0px rgba(0,0,0, 0.9))
          opacity: 0

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
  import AppMenu from './app-menu.vue'
  import MenuButton from './menu-button.vue'
  import IconView from './icon-view.vue'

  export default {
    name: 'ToolBar',

    components: {
      AppMenu,
      MenuButton,
      IconView,
    },

    props: {
      documents: Array,
      activeDocument: Object,
      isMaximized: Boolean,
    },

    methods: {
      minimize: function() {
        window.electron.ipc.send('minimize')
      },

      maximize: function() {
        window.electron.ipc.send('maximize')
      },

      unmaximize: function() {
        window.electron.ipc.send('unmaximize')
      },

      close: function() {
        window.electron.ipc.send('close')
      },
    }
  }
</script>
