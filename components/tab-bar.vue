<template lang="pug">

  header.tab-bar

    MenuButton.left.app-menu-btn(icon="atom")

      AppMenu(
        :document="activeDocument"
        v-bind="$attrs"
      )

    ul.tabs

      Tab(
        v-for="doc in documents"
        :document="doc"
        :active-document="activeDocument"
        @click="$emit('update:active-document', doc)"
        @delete-document="$emit('delete-document', $event)"
      )

    .grab-handle.dynamic

    nav

      //- MenuButton(title="Tool Settings" icon="cloud")
      //-   IconView

      MenuButton.naked.account(title="Björn Breitgoff" icon="user")

        .user-info

    .grab-handle.fixed

    .window-controls
      button(@click="minimize")
        Icon(icon="window-minimize")

      button(v-if="maximized" @click="unmaximize")
        Icon(icon="window-restore")

      button(v-else @click="maximize")
        Icon(icon="window-maximize")

      button(@click="close")
        Icon(icon="window-close")

</template>


<style lang="stylus" scoped>

  .tab-bar
    background: $dark2
    display: flex
    // background: linear-gradient(to bottom, $dark1 * 0.9, $dark2 * 0.95)
    border-bottom: 1px solid black
    box-shadow: 0 1px 0 #323840
    max-width: 100vw
    align-items: center
    z-index: 2

    .blurry &
      // backdrop-filter: blur(32px)
      background: rgba($dark2, 0.925)

    [data-platform="darwin"] &
      border-radius: 4px 4px 0px 0px
      padding-left: 74px

    > *
      flex: 0 0 auto

  .fullscreen
  .maximized[data-platform="win32"]
  [data-platform="browser"]

    .tab-bar
      padding-left: 0px
      border-radius: 0 !important

    .window-controls button:last-child
      border-radius: 0

    .app-menu-btn
      margin-left: 3px
      margin-right: 4px

    .history-btn
      margin-right: 3px

  .app-menu-btn
    // color: #ff9f90
    color: $highlight * 1.2

    [data-platform="darwin"] &
      display: none

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

  .account
    font-weight: bold
    flex: 0 0 auto
    white-space: nowrap

  .user-info
    padding: 1rem

  input[type="text"]
  input[type="number"]
    width: 47px

  .inset
    margin-left: 8px

</style>


<script>

  export default {
    name: 'TabBar',

    props: {
      documents: Array,
      activeDocument: Object,
      maximized: Boolean,
    },

    methods: {
      minimize: function() {
        window.ipc.send('minimize')
      },

      maximize: function() {
        window.ipc.send('maximize')
      },

      unmaximize: function() {
        window.ipc.send('unmaximize')
      },

      close: function() {
        window.ipc.send('close')
      },
    }
  }

</script>
