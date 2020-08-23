<template lang="pug">
  form.bordered.tipped.feature-box
    //- h1.header Extrude
    .options
      label(@click="pickProfile")
        | Profile
        .picker.one
      label
        | Rail
        .picker.two
      label
        | Distance
        input(type="number" value="2")
      label
        | Direction
        input(type="checkbox")
      label
        | Operation
        select
          option Join
          option Cut
          option Intersect
          option New Body
</template>


<style lang="stylus" scoped>
  .feature-box
    font-size: 12px
    padding: 10px 14px
    &::before
      // left: 96px

  .options
    display: flex

  label
    display: flex
    flex-direction: column
    align-items: center
    font-size: 12px
    color: $bright1
    font-weight: bold
    & + label
      margin-left: 12px
    > *
      margin-top: 6px
    input, select
      max-width: 65px

  .picker
    width: 23px
    height: 23px
    border: 6px solid #caffd7
    border-radius: 40px
    background: #56d556
    cursor: pointer
    transition: all 0.05s
    &:hover
      // background: #56d556 * 0.5
      border-width: 0px
    &.one
      // animation: 1s infinite linear rotate
      // border: 2px dashed #caffd7
    &.two
      background: #d55662
      border-color: #ffdbdb

  @keyframes rotate
    50%
      transform: scale(0.8) rotate(-180deg)
      // border-color: black
      border-width: 1px
      border-style: dashed
    100%
      transform: scale(1) rotate(-360deg)

</style>


<script>
  // import Foo from './foo.vue'

  export default {
    name: 'FeatureBox',

    components: {},

    props: {
      activeTool: Object,
    },

    data() {
      return {

      }
    },

    mounted() {},

    methods: {
      pickProfile: function(e) {

      },
    },

    beforeDestroy() {},
  }
</script>
