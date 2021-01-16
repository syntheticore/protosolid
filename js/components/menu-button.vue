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
    .fullscreen .tab-bar &
    .maximized .tab-bar &
    [data-platform="browser"] .tab-bar &
      button
        padding: 4px 8px
    &.left
      .pop-up
        right: unset
        left: -12px
        &::before
          right: unset
          left: 20px

  .button
    margin: 0
    color: inherit
    font-size: 16px
    font-weight: normal
    display: flex
    align-items: center
    padding: 5px 8px
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
    top: 24px
    right: -12px
    // min-width: 195px
    color: $bright2
    font-size: 12px
    margin: 12px
    text-align: left
    &::before
      left: unset
      right: 20px

  .wrapper
    overflow: hidden
    border-radius: 5px
    right: -12px
</style>


<script>
  export default {
    name: 'MenuButton',

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
    },

    mounted() {
      this.$root.$on('close-widgets', () => this.isOpen = false )
    },
  }
</script>
