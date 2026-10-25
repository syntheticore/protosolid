<template lang="pug">

  ExpressionInput.number-input(:parameters="component.getParameters()")

    button.button.tape(type="button" tabindex="-1")
      Icon(icon="tape")

</template>


<style lang="stylus" scoped>
  .tape
    margin: 0
    padding: 0 5px
    text-shadow: none
    border-radius: 3px 0 0 3px
</style>


<script>
  export default {
    name: 'NumberInput',

    props: {
      // Provides the parameters usable in expressions
      component: Object,
    },
  }
</script>
