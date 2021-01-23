<template lang="pug">
  .box.parameter-treelet
    header
      fa-icon(icon="square-root-alt" fixed-width)
      input.variable(type="text" v-model.trim="parameter.name" @keydown.stop)
      span =
      input.value(type="text" v-model.trim="value" @keydown.stop)
      .controls
        fa-icon.delete(
          icon="trash-alt" fixed-width
          title="Delete"
          @click.stop="remove"
        )
</template>


<style lang="stylus" scoped>
  input[type="text"]
    padding: 0px 3px
    font-weight: 800
    font-size: 9px
    box-shadow: none
    &:not(:hover):not(:focus)
      background: none
      color: white

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
      component: Object,
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

    methods: {
      remove: function() {
        this.component.parameters =
          this.component.parameters.filter(param => param !== this.parameter )
      },
    },
  }
</script>
