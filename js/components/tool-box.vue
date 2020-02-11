<template lang="pug">
  .bordered.tool-box
    ul.tools
      li.bordered
        fa-icon(icon="tools")
        h2 Extrude
      li.bordered
        fa-icon(icon="thumbtack")
        h2 Revolve
      li.bordered
        fa-icon(icon="magnet")
        h2 Shell
</template>


<style lang="stylus" scoped>
  .tool-box
    padding: 8px
    background: rgba($dark2, 0.8)

  .tools li
    padding: 6px
    display: inline-block
</style>


<script>
  // import Foo from './foo.vue'

  export default {
    name: 'ToolBox',
    components: {},
    props: {},
    data() {
      return {
        creationTools: [

        ]
      }
    },
    mounted() {},
    beforeDestroy() {},
  }
</script>
