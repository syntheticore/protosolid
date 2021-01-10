<template lang="pug">
  .radio-bar(@mouseleave="$emit('unhover', chosen)")
    button.button(
      v-for="(item, key) in items"
      :title="item.title"
      :class="{pressed: key === chosen}"
      @click.prevent="$emit('update:chosen', key)"
      @mouseenter="$emit('hover', key)"
    )
      fa-icon(:icon="item.icon")
</template>


<style lang="stylus" scoped>
  .radio-bar
    pointer-events: all
    display: flex

  .button
    margin: 0
    border-radius: 0
    flex: 1 1 auto
    text-shadow: none
    & + .button
      border-left: none
    .button + &
      border-right: none
    &:first-child
      border-top-left-radius: 4px
      border-bottom-left-radius: 4px
    &:last-child
      border-top-right-radius: 4px
      border-bottom-right-radius: 4px
    &.pressed
      background: $highlight
      border-color: lighten($highlight, 0%)
      color: white

</style>


<script>
  export default {
    name: 'RadioBar',
    props: {
      items: Object,
      chosen: String,
    },
  }
</script>
