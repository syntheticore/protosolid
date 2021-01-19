<template lang="pug">
  .export-stl
    h2 Export STL
    button.button(@click="exportStl") Export STL
    button.button(@click="export3mf") Export 3MF
</template>


<style lang="stylus" scoped>
  h1
    font-size: 14px
    font-weight: bold
</style>


<script>
  import { export3mf, exportStl } from './../export.js'

  export default {
    name: 'ExportStl',

    props: {
      component: Object,
    },

    data() {
      return {}
    },

    methods: {
      exportStl: function() {
        exportStl(this.component)
      },

      export3mf: function() {
        export3mf(this.component)
      },
    },
  }
</script>
