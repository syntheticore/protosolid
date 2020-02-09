<template lang="pug">
  footer.footer-view
    .tool-info
      b Select Tool
      fa-icon(icon="mouse" fixed-width)
      | Select geometry
      fa-icon(icon="mouse" fixed-width)
      | Bring up actions
    table.selection-info
      tr
        td # Objects
        td 4
      tr
        td Total Weight
        td 12.3g
      tr
        td Total Volume
        td 140cm³
      tr
        td Total Surface Area
        td 20cm²
      tr
        td # Materials used
        td 3
</template>


<style lang="stylus" scoped>
  .footer-view
    font-size: 13px
    padding: 22px
    color: $bright2
    text-shadow: 0 1px 3px black
    display: flex
    justify-content: space-between
    align-items: flex-end
    pointer-events: none
  
  .tool-info
    // flex: 0 0 content
  
  .selection-info
    line-height: 2
  
  td:first-child
    text-align: right
    padding-right: 12px
  
  td:last-child
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
    props: {},
    // data() {
    //   return {}
    // },
    // mounted() {},
    // beforeDestroy() {},
  }
</script>
