<template lang="pug">
  .parameter-treelet
    header
      fa-icon(icon="square-root-alt" fixed-width)
      input.variable(type="text" v-model="parameter.title")
      span =
      input.value(type="text" v-model="value")
</template>


<style lang="stylus" scoped>
  header
    display: flex
    align-items: center

  span
    margin: 0 6px !important

  input[type="text"]
    max-width: 30px
    padding: 0px 3px
    border-radius: 2px
    font-weight: 800
    font-size: 9px
    box-shadow: none
    background: $dark2 * 0.9
    color: $bright1
    &:last-child
      margin-right: -3px
    &:hover
    &:focus
      background: $bright1
      color: $dark1

  .variable
    max-width: 50px !important
</style>


<script>
  export default {
    name: 'ParameterTreelet',

    props: {
      parameter: Object,
    },

    data() {
      return {
        value: this.parameter.value,
      }
    },

    watch: {
      value: function(value) {
        this.parameter.value = value
      },
    },
  }
</script>
