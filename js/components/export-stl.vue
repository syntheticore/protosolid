<template lang="pug">
  .export-stl
    h2 Export STL
    button.button(@click="exportStl") Export STL
</template>


<style lang="stylus" scoped>
  h1
    font-size: 14px
    font-weight: bold
</style>


<script>
  import { saveFile } from './../utils.js'

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
        const stl = this.component.real.export_stl(this.component.title)
        const title = this.component.title.replace(new RegExp(' ', 'g'), '_')
        saveFile(stl, title + '.stl', 'STL')
      },
    },
  }
</script>
