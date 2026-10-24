<template lang="pug">

  .app-menu

    .file-menu

      button.button.settings-btn(@click="openSettings" title="Settings")
        Icon(icon="sliders-h")

      .about

        AppLogo.logo
        h1 ProtoSolid
        .version Version 0.11

      ul.actions(@click="bus.emit('close-widgets')")

        li
          button(@click="$emit('create-document')")
            Icon(icon="file" fixed-width)
            span New Document

        li
          button(@click="$emit('open-document')")
            Icon(icon="folder-open" fixed-width)
            span Open...

        li
          button(:disabled="!canSave" @click="$emit('save-document')")
            Icon(icon="save" fixed-width)
            span Save

        li
          button(@click="$emit('save-document-as')")
            Icon(icon="save" fixed-width)
            span Save as...

        li
          button(@click="$emit('import-file')")
            Icon(icon="file-import" fixed-width)
            span Import...

        li
          button(@click="$emit('export-file')")
            Icon(icon="file-export" fixed-width)
            span Export...

        //- li
        //-   button(disabled="disabled")
        //-     Icon(icon="network-wired" fixed-width)
        //-     span Open Network Project...

</template>


<style lang="stylus" scoped>
  .app-menu
    display: flex
    width: 164px

  .file-menu
    width: 165px
    flex: 0 0 auto
    overflow: hidden
    position: relative
    border-right: 1px solid $dark1

  .settings-btn
    position: absolute
    right: 0
    margin: 4px
    font-size: 16px
    padding: 3px 6px
    padding-bottom: 2px
    border-radius: 2px
    z-index: 1
    &:not(:hover)
      background: none
      border-color: transparent
      box-shadow: none

  .about
    padding: 12px
    text-align: center
    background: linear-gradient(0deg, $dark2 * 0.83, transparent)
    .logo
      width: 48px
      height: 48px
    h1
      font-family: 'Comfortaa'
      font-size: 16px
      font-weight: bold
      color: $bright1
      margin-top: 8px
    .version
      font-size: 11px
      margin-top: 4px
      color: $bright2

  .actions
    &:hover
      button
        transition: none
    li:first-child button
        border-top: 1px solid $dark1
    button
      background: none
      border: none
      padding: 10px 8px
      font-size: 12px
      font-weight: 600
      color: $bright1
      width: 100%
      text-align: left
      display: flex
      align-items: center
      transition: all 0.15s
      &:active
        background: $dark1
      &:disabled
        opacity: 0.5
      &:hover
        border-top-color: rgba(white, 0.07)
        background: rgba(white, 0.07)
        color: white
      svg
        color: $bright1 * 0.85
        font-size: 16px
      span
        margin-left: 8px
        white-space: nowrap

</style>


<script>
  import { openPreferences } from './../js/preferences.js'

  export default {
    name: 'AppMenu',

    inject: ['bus'],

    props: {
      canSave: Boolean,
    },

    methods: {
      openSettings: function() {
        this.bus.emit('close-widgets')
        openPreferences()
      },
    },
  }
</script>
