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
        v-for="item in list"
        :key="item.id || item.title",
        :class="{active: isActive(item)}"
        @click="$emit('update:active', item)"
        @mouseenter="$emit('hover', item)"
      )
        | {{ item.title }}
    transition(name="fade" mode="out-in")
      button(
        v-show="allowCreate"
        @click="$emit('create')"
        @mouseenter="$emit('unhover')"
      )
        Icon(icon="plus")
</template>


<style lang="stylus" scoped>
  .list-chooser
    pointer-events: auto
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
    // position: relative
    &:hover
      background: $dark1
    &:active
      background: $dark1 * 0.9
    &.active
      background: $highlight
      color: white

  button
    display: block
    width: 100%
    color: $bright1
    padding: 6px
    height: 28px
    border: none
    margin: 0
    margin-top: 1px
    border-top: 1px solid $dark1 * 1.1
    box-shadow: 0 0 6px rgba(black, 0.4)
    // text-shadow: 0 -1px 0px black
    background: $dark2 * 1.1
    transition: all 0.25s
    overflow: hidden
    flex: 0 0 auto
    font-size: 13px
    &:hover
      background: $dark1
    &:active
      background: $dark2 * 0.8
      transition: none
    &:only-child
      margin-top: 0
      border: none

  .fade-enter-from
  .fade-leave-to
    opacity: 0
    height: 0
    padding: 0
    margin: 0

  .list-enter-active
  .list-leave-active
    transition: all 0.25s
  .list-enter-from
  .list-leave-to
    opacity: 0
    padding: 0
    margin: 0
    margin-bottom: -12px
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

    methods: {
      isActive: function(item) {
        return this.active == item
      },
    },
  }
</script>
