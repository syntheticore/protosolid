<template lang="pug">
  .bordered.list-chooser(
    @mouseleave="$emit('unhover')"
  )
    transition-group(
      name="list"
      tag="ul"
      v-if="list.length"
    )
      li(
        v-for="elem in list"
        :key="elem.id",
        :class="{active: active && active.id == elem.id}"
        @click="$emit('activate', elem)"
        @mouseenter="$emit('hover', elem)"
      )
        | {{ elem.title }}
    transition(name="fade" mode="out-in")
      button(
        v-show="allowCreate"
        @click="$emit('create')"
        @mouseenter="$emit('unhover')"
      )
        fa-icon(icon="plus")
</template>


<style lang="stylus" scoped>
  .list-chooser
    margin-bottom: 16px
    pointer-events: all
    overflow: hidden
    display: flex
    flex-direction: column

  ul
    flex: 1 1 auto
    overflow: auto
    -webkit-overflow-scrolling: touch
    max-height: 210px
    -ms-overflow-style: none
    scrollbar-width: none
    &::-webkit-scrollbar
      display: none

  li
    padding: 10px 16px
    // height: 28px
    font-size: 12px
    font-weight: bold
    text-align: center
    position: relative
    &:hover
      background: $dark1
    &.active
      background: $highlight
      color: white

  button
    display: block
    width: 100%
    color: $bright1
    padding: 8px
    height: 30px
    border: none
    margin: 0
    margin-top: 1px
    border-top: 1px solid $dark1 * 1.1
    box-shadow: 0 0 6px rgba(black, 0.4)
    text-shadow: 0 -1px 0px black
    background: $dark2 * 1.1
    transition: all 0.3s
    overflow: hidden
    flex: 0 0 auto
    &:hover
      background: $dark1
    &:active
      background: $dark2 * 0.8
      transition: none
    &:only-child
      margin-top: 0
      border: none

  .fade-enter
  .fade-leave-to
    opacity: 0
    height: 0
    padding: 0
    margin: 0

  .list-enter-active
  .list-leave-active
    transition: all 0.3s
  .list-enter
  .list-leave-to
    opacity: 0
    height: 0
    padding: 0
    margin: 0

  svg
    font-size: 13px
</style>


<script>
  export default {
    name: 'ListChooser',
    components: {},
    props: {
      list: Array,
      active: Object,
      allowCreate: Boolean,
    },
  }
</script>
