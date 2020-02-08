<template lang="pug">
  ul.bordered.list-chooser
    li(v-for="elem in list", :class="{active: elem.active}")
      | {{ elem.title }}
    li
      button
        fa-icon(icon="plus")
</template>


<style lang="stylus" scoped>
  .list-chooser
    margin-bottom: 16px
    pointer-events: all
    overflow: hidden
  
  li
    padding: 8px 12px
    font-size: 12px
    font-weight: bold
    text-align: center
    position: relative
    &:last-child
      display: flex
      margin-top: 1px
      padding: 0
    &:only-child
      margin-top: 0
      button
        border: none
    &:hover
      background: $dark1
    &.active
      background: $highlight
  
  button
    display: block
    width: 100% 
    color: $bright1
    padding: 8px
    border: none
    margin: 0
    border-top: 1px solid $dark1 * 1.1
    box-shadow: 0 0 6px rgba(black, 0.4)
    text-shadow: 0 -1px 0px black
    background: $dark2 * 1.1
    transition: all 0.2s
    &:hover
      background: $dark1
    &:active
      background: $dark2 * 0.8
      transition: none

  svg
    font-size: 13px
</style>


<script>
  // import TreeItem from './tree-item.vue'

  export default {
    name: 'ListChooser',
    components: {

    },
    props: {
      list: Array,
    },
  }
</script>
