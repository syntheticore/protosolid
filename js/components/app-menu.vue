<template lang="pug">
  .app-menu(:class="{open: open}")
    button.button.settings-btn(@click="open = !open" title="Preferences")
      fa-icon(icon="sliders-h")

    .file-menu
      .about
        fa-icon(icon="atom")
        h1 Alchemy
        .version Version 0.11

      ul.actions
        li
          button(@click="$emit('create-document')")
            fa-icon(icon="file" fixed-width)
            span New Document
        li
          button(disabled="disabled")
            fa-icon(icon="file-import" fixed-width)
            span Open...
        li
          button(disabled="disabled")
            fa-icon(icon="save" fixed-width)
            span Save
        li
          button(disabled="disabled")
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

    PreferencesView

</template>


<style lang="stylus" scoped>
  .app-menu
    display: flex
    width: 165px
    height: 279px
    transition: all 0.4s
    &.open
      width: 590px
      height: 210px

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

  .file-menu
  .preferences-view
    overflow: hidden
    transition: all 0.4s
    width: 165px
    flex: 0 0 auto
    .open &
      transform: translateX(-165px)
      opacity: 0

  .preferences-view
    width: 590px
    opacity: 0
    padding-top: 0
    .open &
      opacity: 1

  .about
    padding: 12px
    text-align: center
    background: linear-gradient(0deg, $dark2 * 0.83, transparent)
    border-bottom: 1px solid $dark1
    svg
      font-size: 35px
      // color: lighten($highlight, 25%)
      // color: #ff4075
      color: #ff9f90
      transition: all 0.8s
      .open &
        opacity: 0
        transform: scale(0.2)
    h1
      font-size: 16px
      font-weight: bold
      color: $bright1
      margin-top: 8px
    .version
      font-size: 11px
      margin-top: 4px

  .actions
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
      &:hover
        background: rgba(white, 0.07)
        color: white
        transition: none
      &:active
        background: $dark1 * 0.65
      &:disabled
        pointer-events: none
        opacity: 0.5
      svg
        color: $bright1 * 0.85
        font-size: 16px
      span
        margin-left: 8px
        white-space: nowrap

  @keyframes rotate
    50%
      transform: rotate(180deg) scale(1.2)
    100%
      transform: rotate(360deg) scale(1)

</style>


<script>
  import PreferencesView from './preferences-view.vue'

  export default {
    name: 'AppMenu',

    components: {
      PreferencesView,
    },

    props: {},

    data() {
      return {
        open: false,
        tabs: ['Units', 'User Interface', 'Deviations'],
      }
    },

    mounted() {},

    methods: {},
  }
</script>
