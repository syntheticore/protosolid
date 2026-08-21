<template lang="pug">

  .box.treelet-parameter

    header

      Icon(icon="square-root-alt" fixed-width)

      input.input.variable(type="text" v-model.trim="name" @keydown.stop @change="commit" @keydown.enter="commitAndSelect")

      span =

      input.input.value(type="text" v-model.trim="value" @keydown.stop @change="commit" @keydown.enter="commitAndSelect")

      .controls

        Icon.delete(
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
    name: 'TreeletParameter',

    props: {
      parameter: Object,
      component: Object,
      document: Object,
    },

    data() {
      return {
        name: this.parameter.name,
        value: this.parameter.value,
      }
    },

    methods: {
      commit: function() {
        if(this.name == this.parameter.name && this.value == this.parameter.value) return
        const oldName = this.parameter.name
        if(this.name != oldName) this.document.renameParameterReferences(oldName, this.name)
        this.parameter.name = this.name
        this.parameter.value = this.value
        this.document.regenerateParameters()
      },

      commitAndSelect: function(event) {
        event.preventDefault()
        this.commit()
        event.target.select()
      },

      remove: function() {
        this.document.removeParameterReferences(this.parameter, this.component)
        this.component.creator.parameters =
          this.component.creator.parameters.filter(param => param !== this.parameter )
        this.document.regenerateParameters()
      },
    },
  }

</script>
