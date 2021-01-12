<template lang="pug">
  .app-menu
    .about
      fa-icon(icon="atom")
      h1 Alchemy
      .version Version 0.11

    //- hr

    ul.actions
      li
        button(@click="$emit('create-document')")
          fa-icon(icon="file" fixed-width)
          span New Document
      li
        button(disabled="disabled")
          fa-icon(icon="file-import" fixed-width)
          span Open...
      li
        button(disabled="disabled")
          fa-icon(icon="save" fixed-width)
          span Save
      li
        button(disabled="disabled")
          fa-icon(icon="save" fixed-width)
          span Save as...
      li
        button(disabled="disabled")
          fa-icon(icon="file-import" fixed-width)
          span Import...
      //- li
      //-   button(disabled="disabled")
      //-     fa-icon(icon="network-wired" fixed-width)
      //-     span Open Network Project...

    ul.recents
</template>


<style lang="stylus" scoped>
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
    button
      background: none
      border: none
      padding: 10px 8px
      font-size: 14px
      font-weight: 600
      color: $bright1
      width: 100%
      text-align: left
      border-bottom: 1px solid $dark1
      display: flex
      align-items: center
      &:hover
        background: $dark1
        color: white
      &:active
        background: $dark1 * 0.65
      &:disabled
        pointer-events: none
        opacity: 0.5
      svg
        color: $bright1 * 0.85
        font-size: 16px
      span
        margin-left: 8px
        white-space: nowrap
</style>


<script>
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
