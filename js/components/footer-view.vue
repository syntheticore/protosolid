<template lang="pug">
  footer.footer-view
    .tool-info.bordered
      b Select Tool
      fa-icon(icon="mouse" fixed-width)
      | Select geometry
      fa-icon(icon="mouse" fixed-width)
      | Bring up actions
    .selection-info.bordered
      div
        span # Objects
        span {{selectedElement ? 1 : 0}}
      div
        span Weight
        span 12.3g
      div
        span Volume
        span 140cm³
      div
        span Surface Area
        span 20cm²
</template>


<style lang="stylus" scoped>
  .footer-view
    font-size: 13px
    padding: 12px
    color: $bright2
    text-shadow: 0 1px 3px black
    display: flex
    justify-content: space-between
    align-items: flex-end
    pointer-events: none

  .tool-info
    // flex: 0 0 content
    padding: 16px 20px
    border-radius: 99px

  .selection-info
    line-height: 1.9
    padding: 4px 14px
    // table-layout: fixed
    white-space: nowrap
    padding: 0

    div
      display: inline-block
      padding: 2px 14px
      &:not(:last-child)
        border-right: 1px solid $dark1

    span:first-child
      // text-align: right
      padding-right: 4px

    span:last-child
      color: $bright1
      font-weight: bold

  b
    margin-right: 6px
    color: $bright1 * 0.9

  svg
    margin-left: 9px
    margin-right: 3px
    color: $bright1
</style>


<script>
  // import Foo from './foo.vue'

  export default {
    name: 'FooterView',

    components: {},

    props: {
      selectedElement: Object
    },

    // data() {
    //   return {}
    // },

    // mounted() {},
  }
</script>
