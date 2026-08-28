<template lang="pug">

  .radio-bar(@mouseleave="$emit('unhover', chosen)")

    button.button(
      v-for="(item, key) in items"
      :title="item.title"
      :class="{pressed: key === chosen}"
      @click.prevent="$emit('update:chosen', key)"
      @mouseenter="$emit('hover', key)"
    )

      Icon(:icon="item.icon" fixed-width)

    .hot-key(v-if="hotKey") {{ hotKey }}

</template>


<style lang="stylus" scoped>

  .radio-bar
    pointer-events: auto
    display: flex
    position: relative

  .button
    margin: 0
    border-radius: 0
    flex: 1 1 auto
    min-width: 0
    text-shadow: none

    & + .button
      border-left: none

    .button + &
      border-right: none

    &:first-of-type
      border-top-left-radius: 4px
      border-bottom-left-radius: 4px

    &:last-of-type
      border-top-right-radius: 4px
      border-bottom-right-radius: 4px

    &.pressed
      background: $highlight
      border-color: lighten($highlight, 0%)
      color: white

  .hot-key
    position: absolute
    top: 2px
    left: 2px
    font-size: 9px
    color: white
    font-weight: bold
    background: $bright2 * 0.75
    width: 12px
    height: 12px
    border-radius: 3px
    border: 0.5px solid $dark2
    display: flex
    align-items: center
    justify-content: center
    pointer-events: none

</style>


<script>

  export default {
    name: 'RadioBar',
    props: {
      items: Object,
      chosen: String,
      hotKey: String,
    },
  }

</script>
