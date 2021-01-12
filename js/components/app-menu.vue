<template lang="pug">
  .app-menu
    .about
      fa-icon(icon="atom")
      h1 Alchemy
      .version Version 0.11

    //- hr
    //- button.button(@click="$emit('create-document')") New Document

    ul.actions
      li
        button.button(@click="$emit('create-document')")
          fa-icon(icon="file")
          span New Document
      li
        button.button(disabled="disabled")
          fa-icon(icon="file-import")
          span Open File..
      li
        button.button(disabled="disabled")
          fa-icon(icon="save")
          span Save
      li
        button.button(disabled="disabled")
          fa-icon(icon="save")
          span Save as..
      li
        button.button(disabled="disabled")
          fa-icon(icon="file-import")
          span Import..
      li
        button.button(disabled="disabled")
          fa-icon(icon="network-wired")
          span Open Network Project..

    ul.recents
</template>


<style lang="stylus" scoped>
  .app-menu
    // hr
    //   border: 0
    //   height: 1px
    //   margin: 12px 0
    //   background-image: linear-gradient(to right, $dark2, $dark1 * 1.4, $dark2)

  .about
    padding: 12px
    text-align: center
    background: linear-gradient(0deg, $dark2 * 0.83, transparent)
    border-bottom: 1px solid $dark1
    svg
      font-size: 35px
      color: lighten($highlight, 25%)
    h1
      font-size: 16px
      font-weight: bold
      color: $bright1
      margin-top: 8px
    .version
      font-size: 11px
      margin-top: 4px

  .actions
    padding: 6px 5px
</style>


<script>
  // import Foo from './foo.vue'

  export default {
    name: 'AppMenu',
    components: {},
    props: {},
    data() {
      return {}
    },
    mounted() {},
    methods: {},
    beforeDestroy() {},
  }
</script>
