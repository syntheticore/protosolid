<template lang="pug">
  .app-menu(:class="{open: open}")
    .file-menu
      button.button.settings-btn(@click="toggleSettings" title="Preferences")
        fa-icon(icon="sliders-h")
      .about
        fa-icon(icon="atom")
        h1 ProtoSolid
        .version Version 0.11

      ul.actions(@click="$root.$emit('close-widgets')")
        li
          button(@click="$emit('create-document')")
            fa-icon(icon="file" fixed-width)
            span New Document
        li
          button(@click="$emit('open-document')")
            fa-icon(icon="folder-open" fixed-width)
            span Open...
        li
          button(@click="$emit('save-document')")
            fa-icon(icon="save" fixed-width)
            span Save
        li
          button(@click="$emit('save-document-as')")
            fa-icon(icon="save" fixed-width)
            span Save as...
        li
          button(disabled="disabled")
            fa-icon(icon="file-import" fixed-width)
            span Import...

        //- li
        //-   button(disabled="disabled")
        //-     fa-icon(icon="network-wired" fixed-width)
        //-     span Open Network Project...

      ul.recents

    PreferencesView.panel

</template>


<style lang="stylus" scoped>
  .app-menu
    display: flex
    width: 164px
    transition: all 0.3s
    &.open
      width: 755px

  .file-menu
  .panel
    transition: all 0.3s
    width: 165px
    flex: 0 0 auto
    overflow: hidden
    .open &
      transition-delay: 0.15s

  .file-menu
    position: relative
    border-right: 1px solid $dark1

  .settings-btn
    position: absolute
    right: 0
    margin: 4px
    font-size: 16px
    padding: 3px 6px
    padding-bottom: 2px
    z-index: 1
    &:not(:hover)
      background: none
      border-color: transparent
      box-shadow: none

  .about
    padding: 12px
    text-align: center
    background: linear-gradient(0deg, $dark2 * 0.83, transparent)
    svg
      font-size: 35px
      // color: #ff9f90
      color: $highlight * 1.2
    h1
      font-size: 16px
      font-weight: bold
      color: $bright1
      margin-top: 8px
    .version
      font-size: 11px
      margin-top: 4px

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

  .panel
    width: 590px
    opacity: 0
    padding-top: 0
    .open &
      opacity: 1

</style>

<style lang="stylus">
  .app-menu
    h2
      font-size: 14px
      font-weight: 600
      text-align: center
      padding: 9px
      border-bottom: 1px solid $dark1
      background: linear-gradient(0deg, $dark2 * 0.83, transparent)

</style>


<script>
  import PreferencesView from './preferences-view.vue'

  export default {
    name: 'AppMenu',

    props: {
      document: Object,
    },

    components: {
      PreferencesView,
    },

    data() {
      return {
        open: false,
      }
    },

    mounted() {},

    methods: {
      toggleSettings: function() {
        this.open = !this.open
      },
    },
  }
</script>
