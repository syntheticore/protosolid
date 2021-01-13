<template lang="pug">
  ul.material-selector
    li(v-for="material in materials" @click="$emit('update:chosen', material)")
      .preview
      h1 {{ material.title }}
</template>


<style lang="stylus" scoped>
  .material-selector
    width: 300px
    height: 150px
    display: flex
    align-items: flex-start

  li
    display: flex
    align-items: center
    flex-direction: column
    padding: 8px
    border-radius: 4px
    margin-top: 4px
    transition: all 0.15s
    & + li
      margin-left: 12px
    &:nth-child(1) .preview
      background: $blue
    &:nth-child(2) .preview
      background: $purple
    &:hover
      background: $dark1

  .preview
    width = 46px
    width: width
    height: width
    border-radius: 999px

  h1
    margin-top: 6px
    font-size: 11px

</style>


<script>
  import {
    Material,
    Aluminum,
    Polystyrene,
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
        ],
      }
    },
  }
</script>
