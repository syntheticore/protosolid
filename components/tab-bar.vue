<template lang="pug">

  header.tab-bar

    MenuButton.left.app-menu-btn(icon="bars")

      AppMenu(
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

    .grab-handle.dynamic(data-tauri-drag-region)

    .grab-handle.fixed(data-tauri-drag-region)

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
    background: dark2()
    display: flex
    // background: linear-gradient(to bottom, dark1(0.9), dark2(0.95))
    border-bottom: 1px solid var(--ring)
    box-shadow: 0 1px 0 dark1(1.15)
    max-width: 100vw
    align-items: center
    z-index: 3

    .blurry &
      // backdrop-filter: blur(32px)
      // background: dark2(1, 0.925)
      background: dark2()

    [data-platform="darwin"] &
      border-radius: 4px 4px 0px 0px
      padding-left: 74px

    > *
      flex: 0 0 auto

  .fullscreen
  [data-platform="win32"] .maximized
  [data-platform="browser"]

    .tab-bar
      padding-left: 0px
      border-radius: 0 !important

    .window-controls button:last-child
      border-radius: 0

    .app-menu-btn
      margin: 3px 4px 3px 3px

    .history-btn
      margin-right: 3px

  .app-menu-btn
    margin-top: 5px
    margin-bottom: 3px

    // Matches the shorter tab bar of windowed frameless windows on Windows 11
    [data-platform="win32"] #app:not(.fullscreen):not(.maximized) &
      margin-top: 4px

    :deep(.button)
      border-radius: 4px

      &:hover
        color: $highlight * 1.2

    [data-platform="darwin"] &
      display: none

  .tabs
    display: flex
    flex: 1 1 auto
    min-width: 0
    white-space: nowrap
    overflow: hidden
    height: 100%

  .grab-handle
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
      color: bright1()
      font-size: 12px
      margin: 0
      padding: 0 12px
      height: 100%
      transition: all 0.15s

      &:last-child
        // border-radius: 0px 4px 0px 0px

      &:hover
        background: dark1()
        color: var(--fg)

        &:last-child
          background: #b70f0f
          color: white

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
        window.desktop.send('minimize')
      },

      maximize: function() {
        window.desktop.send('maximize')
      },

      unmaximize: function() {
        window.desktop.send('unmaximize')
      },

      close: function() {
        window.desktop.send('close')
      },
    }
  }

</script>
