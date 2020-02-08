<template lang="pug">
  footer.footer-view
    b Select Tool
    fa-icon(icon="mouse" fixed-width)
    | Select geometry
    fa-icon(icon="mouse" fixed-width)
    | Bring up actions
</template>


<style lang="stylus" scoped>
  .footer-view
    font-size: 13px
    padding: 22px
    color: $bright2
    text-shadow: 0 1px 3px black
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
