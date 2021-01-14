<template lang="pug">
  ul.material-selector
    li(
      v-for="material in materials"
      :class="{chosen: material == chosen}"
      @click="$emit('update:chosen', material)"
    )
      .preview
      h1 {{ material.title }}
</template>


<style lang="stylus" scoped>
  .material-selector
    display: flex

  li
    flex: 1 1 0
    display: flex
    flex-direction: column
    align-items: center
    padding: 8px
    border-radius: 6px
    margin-top: 4px
    transition: all 0.15s
    border: 2px solid transparent
    & + li
      margin-left: 12px
    &:nth-child(1) .preview
      background: lighten($blue, 25%)
    &:nth-child(2) .preview
      background: $pink
    &:nth-child(3) .preview
      background: darken($purple, 30%)
    &:hover
      background: $dark1
    &.chosen
      background: $dark1
      border-color: $highlight

  .preview
    width = 46px
    width: width
    height: width
    border-radius: 999px
    border: 4px solid $bright1

  h1
    margin-top: 6px
    font-size: 11px
    white-space: nowrap

</style>


<script>
  import {
    Material,
    Aluminum,
    Polystyrene,
    DarkViolet,
  } from './../material.js'

  export default {
    name: 'MaterialSelector',

    props: {
      chosen: Object,
    },

    data() {
      return {
        materials: [
          new Aluminum(),
          new Polystyrene(),
          new DarkViolet(),
        ],
      }
    },
  }
</script>
