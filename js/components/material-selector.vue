<template lang="pug">
  ul.material-selector
    li(
      v-for="material in materials"
      :class="{chosen: material == chosen}"
      @click="$emit('update:chosen', material)"
    )
      fa-icon.preview(icon="volleyball-ball")
      h1 {{ material.title }}
</template>


<style lang="stylus" scoped>
  .material-selector
    display: flex
    align-items: start
    flex-wrap: wrap
    margin: -10px
    margin-top: 9px !important
    overflow-y: auto
    max-height: 280px
    border-top: 1px solid $dark1

  li
    flex: 1 1 0
    display: flex
    flex-direction: column
    align-items: center
    padding: 8px
    border-radius: 6px
    transition: all 0.15s
    border: 2px solid transparent
    // width: 80px
    margin: 8px
    & + li
      // margin-left: 12px
    &:nth-child(1) .preview
      background: lighten($blue, 25%)
      color: lighten($blue, 50%)
    &:nth-child(2) .preview
      background: darken($purple, 30%)
      color: lighten($purple, 20%)
    &:nth-child(3) .preview
      background: $pink
      color: lighten($pink, 50%)
    &:nth-child(4) .preview
      background: $red
      color: lighten($red, 50%)
    &:nth-child(5) .preview
      background: white
      color: $bright1
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
    padding: 3px
    border: 4px solid $bright1

  h1
    margin-top: 6px
    font-size: 11px
    text-align: center
    line-height: 1.3

</style>


<script>
  import {
    Material,
    Aluminum,
    Polystyrene,
    DarkViolet,
    CarPaint,
    Glass,
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
          new DarkViolet(),
          new Polystyrene(),
          new CarPaint(),
          new Glass(),
          new Aluminum(),
          new DarkViolet(),
          new Polystyrene(),
          new CarPaint(),
          new Glass(),
        ],
      }
    },
  }
</script>
