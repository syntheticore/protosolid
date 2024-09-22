<template lang="pug">
  .anchor
    .pop-up(open)
      h1 Tool Settings
      form
        label
          | Transform:
          select
            option World
            option Local
        fieldset
          legend Snapping
          label
            input(type="checkbox")
            | Snap to Grid
          label
            input(type="checkbox")
            | Snap to Angles
</template>


<style lang="stylus" scoped>
  .anchor
    position: absolute
    z-index: 2
  .pop-up
    position: relative
    top: 3px
    left: -145px
    width: 180px
    height: 200px
    background: rgba($dark2, 0.95)
    box-shadow: 0 0 0 1px $dark2 * 0.5, 0 2px 10px rgba(black, 0.15)
    border: 1px solid $dark1 * 1.1
    border-top-color: $dark1 * 1.25
    border-radius: 5px
    color: $bright2
    font-size: 12px
    padding: 10px 14px
    margin: 12px
    text-align: left
    &::before
      content: ''
      display: block
      position: absolute
      width: 0
      height: 0
      triWidth = 7px
      top: - triWidth
      right: 20px
      border-left: triWidth solid transparent
      border-right: triWidth solid transparent
      border-bottom: triWidth solid $dark2
      filter: drop-shadow(0 -1px 0px $dark1 * 1.7) drop-shadow(0 -1px 0px $dark2 * 0.5)
  h1
    // letter-spacing: 0.1em
    font-size: 14px
    font-weight: bold
    color: $bright1
    margin-bottom: 8px
  fieldset
    padding-left: 6px
    margin: 6px 0
  legend
    color: $bright1
    font-size: 13px
    font-weight: bold
    margin: 6px 0
    margin-left: -6px
  label
    display: block
  select
    margin-left: 8px
  input
    margin-right: 8px
</style>


<script>
  // import TreeItem from './tree-item.vue'

  export default {
    name: 'PopUp',
    components: {
      // TreeItem,
    },
    props: {

    },
    data() {
      return {}
    }
  }
</script>
