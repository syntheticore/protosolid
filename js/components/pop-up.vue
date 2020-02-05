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
        //- input(type="range")
</template>


<style lang="stylus" scoped>
  .anchor
    position: absolute
    z-index: 2
  .pop-up
    position: relative
    background: $dark2
    box-shadow: 0 0 0 1px $dark2 * 0.5
    border: 1px solid $dark1 * 1.1
    border-top-color: $dark1 * 1.25
    border-radius: 5px
    color: $bright2
    font-size: 12px
    padding: 14px
    margin: 12px
  h1
    // letter-spacing: 0.1em
    font-size: 14px
    font-weight: bold
    color: $bright1
    margin-bottom: 8px
  select
    margin-left: 8px
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
