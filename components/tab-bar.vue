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

    //- Linux draws the thin symbols of GNOME's title bars
    .window-controls(v-if="linux")

      button(@click="minimize")
        svg(viewBox="0 0 16 16")
          path(d="M4.5 8.5h7")

      button(v-if="maximized" @click="unmaximize")
        svg(viewBox="0 0 16 16")
          rect(x="4.5" y="6.5" width="5" height="5" rx="1")
          path(d="M6.5 4.5h4a1 1 0 0 1 1 1v4")

      button(v-else @click="maximize")
        svg(viewBox="0 0 16 16")
          rect(x="4.5" y="4.5" width="7" height="7" rx="1")

      button(@click="close")
        svg(viewBox="0 0 16 16")
          path(d="M5 5l6 6M11 5l-6 6")

    .window-controls(v-else)

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
      padding-left: 86px

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

    [data-platform="linux"] #app:not(.fullscreen):not(.maximized) &
      margin-top: 3px

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
      [data-platform="linux"] &
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

    // Round buttons like the title bars of GNOME apps
    [data-platform="linux"] &
      display: flex
      align-items: center
      gap: 12px
      padding: 0 10px 0 2px

      button
        width: 24px
        height: 24px
        padding: 0
        border-radius: 50%
        background: tint(10%)

        svg
          display: block
          width: 16px
          height: 16px
          margin: auto
          fill: none
          stroke: currentColor
          stroke-width: 1.2
          stroke-linecap: round

        &:hover
          background: tint(18%)
          color: var(--fg)

</style>


<script>

  export default {
    name: 'TabBar',

    props: {
      documents: Array,
      activeDocument: Object,
      maximized: Boolean,
    },

    data() {
      return {
        linux: window.platform == 'linux',
      }
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
