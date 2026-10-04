<template lang="pug">

  .radio-bar(role="radiogroup" :aria-label="label" @mouseleave="$emit('unhover', chosen)")

    button.button(
      v-for="(item, key) in items"
      :key="key"
      type="button"
      role="radio"
      :aria-checked="key === chosen"
      :aria-label="item.title"
      :title="item.title"
      :class="{pressed: key === chosen}"
      :tabindex="key === chosen || (!Object.hasOwn(items, chosen) && key === Object.keys(items)[0]) ? 0 : -1"
      @click.prevent="$emit('update:chosen', key)"
      @keydown.stop="onKeydown($event, key)"
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
      label: String,
    },
    methods: {
      onKeydown(event, key) {
        if(!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return
        event.preventDefault()
        event.stopPropagation()
        const keys = Object.keys(this.items)
        let index = keys.indexOf(key)
        if(event.key === 'Home') index = 0
        else if(event.key === 'End') index = keys.length - 1
        else index = (index + (['ArrowRight', 'ArrowDown'].includes(event.key) ? 1 : -1) + keys.length) % keys.length
        this.$emit('update:chosen', keys[index])
        this.$el.querySelectorAll('button')[index].focus()
      },
    },
  }

</script>
