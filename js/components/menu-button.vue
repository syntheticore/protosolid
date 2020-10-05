<template lang="pug">
  .menu-button
    button.button(:class="{pressed: isOpen}" @click="toggle()")
      fa-icon(:icon="icon" fixed-width)
      fa-icon.expander(icon="angle-down")
    transition(name="fade" mode="out-in")
      .pop-up.bordered.tipped(v-if="isOpen")
        .wrapper
          h1.header(v-if="title") {{ title }}
          slot
</template>


<style lang="stylus" scoped>
  .menu-button
    position: relative
    display: inline-block
    margin: 4px 5px
    // color: $bright1
    .fullscreen .tab-bar &
      margin: 0 4px
      top: -1px
      &:first-child
        margin-left: 0
      &:last-child
        margin-right: 0
      button
        padding: 3px 12px
        border-radius: 0
        // height: 23px
        border-top: none
        border-bottom: none
    &.left
      .pop-up
        right: unset
        left: -12px
        &::before
          right: unset
          left: 20px

  button
    margin: 0
    color: inherit
    &.pressed
    &:active
      .expander
        transform: rotate(180deg)
    .expander
      margin-left: 8px
      color: $bright2
      transition: transform 0.2s

  .fade-enter
  .fade-leave-to
    opacity: 0
    transform: translate(0, 12px)

  .pop-up
    position: absolute
    z-index: 2
    transition: all 0.2s
    pointer-events: all
    top: 27px
    right: -12px
    min-width: 195px
    color: $bright2
    font-size: 12px
    margin: 12px
    text-align: left
    &::before
      left: unset
      right: 20px

  .wrapper
    overflow: hidden
    padding: 14px
    border-radius: 5px
    right: -12px
</style>


<script>
  // import TreeItem from './tree-item.vue'

  export default {
    name: 'MenuButton',

    components: {
      // TreeItem,
    },

    props: {
      title: String,
      icon: String,
    },

    data() {
      return {
        isOpen: false
      }
    },

    methods: {
      toggle: function() {
        this.isOpen = !this.isOpen;
      }
    }
  }
</script>
