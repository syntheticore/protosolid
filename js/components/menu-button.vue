<template lang="pug">
  .menu-button
    button.button(:class="{pressed: isOpen}" @click="toggle()")
      fa-icon(:icon="icon" fixed-width)
      fa-icon.expander(icon="angle-down")
    transition(name="fade" mode="out-in")
      .pop-up.bordered(v-if="isOpen")
        .wrapper
          h1(v-if="title") {{ title }}
          slot
</template>


<style lang="stylus" scoped>
  .menu-button
    position: relative
    display: inline-block
    margin: 4px 5px
    // color: $bright1
    .fullscreen .tool-bar &
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
    // overflow: hidden
    &::before
      content: ''
      display: block
      position: absolute
      width: 0
      height: 0
      triWidth = 7px
      top: - triWidth
      right: 20px
      border-left: triWidth solid transparent
      border-right: triWidth solid transparent
      border-bottom: triWidth solid $dark2 * 1.1
      filter: drop-shadow(0 -1px 0px $dark1 * 1.7) drop-shadow(0 -1px 0px $dark2 * 0.5)

  .wrapper
    overflow: hidden
    padding: 14px
    border-radius: 5px
    right: -12px
  
  h1
    // letter-spacing: 0.1em
    text-align: center
    font-size: 14px
    font-weight: bold
    color: $bright1
    margin: -14px
    padding: 9px 14px
    box-shadow: 0 0 8px rgba(black, 0.6)
    text-shadow: 0 -1px 0px black
    background: $dark2 * 1.1
    margin-bottom: 14px
    border-bottom: 1px solid $dark1 * 1.2
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
