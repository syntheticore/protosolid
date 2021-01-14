<template lang="pug">
  .parameter-treelet
    header
      fa-icon(icon="square-root-alt" fixed-width)
      input.variable(type="text" v-model="parameter.name")
      span =
      input.value(type="text" v-model="value")
</template>


<style lang="stylus" scoped>
  header
    display: flex
    align-items: center

  input[type="text"]
    padding: 0px 3px
    font-weight: 800
    font-size: 9px
    box-shadow: none
    background: none
    color: white
    &:hover
    &:focus
      background: $bright1
      color: $dark1

  .variable
    max-width: 38px !important
    text-align: right

  span
    margin: 0 3px !important

  .value
    max-width: 46px
    margin-right: 2px
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
