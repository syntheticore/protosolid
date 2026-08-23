<template lang="pug">

  li.treelet-variant(:class="{ expanded }")

    .box

      header(@click="expanded = !expanded")

        Icon(icon="calculator" fixed-width)

        h2 Variant {{ index + 1 }}

        .expand-toggle

          Icon.expand(icon="angle-right")

        .variant-picker(@click.stop="openOptionPicker")

          select.variant-option(
            ref="optionSelect"
            v-model.number="variant.activeOption"
            @click.stop
            @change="changeOption"
          )
            option(v-for="(option, i) in variant.options" :value="i") {{ option.title }}

        .controls.ultra-wide

          Icon(
            icon="plus-circle" fixed-width
            title="Add option"
            @click.stop="addOption"
          )

          Icon(
            icon="square-root-alt" fixed-width
            title="Add variable"
            @click.stop="addParameter"
          )

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="remove"
          )

      .content(v-if="expanded")

        .option-title-row
          input.input.option-title(
            type="text"
            v-model.trim="option.title"
            @change="changeOption"
            @keydown.stop
          )

          Icon.option-delete(
            v-if="variant.options.length > 1"
            icon="trash-alt" fixed-width
            title="Delete option"
            @click.stop="removeOption"
          )

        ul.parameters

          li.parameter(v-for="parameter in option.parameters" :key="parameter")

            Icon(icon="square-root-alt" fixed-width)

            input.input.variable(
              type="text"
              v-model.trim="parameter.name"
              @focus="rememberParameterName(parameter)"
              @keydown.stop
              @change="commit(parameter)"
            )

            span =

            input.input.value(
              type="text"
              v-model.trim="parameter.value"
              @keydown.stop
              @change="commit(parameter)"
            )

            Icon.delete(
              icon="times-circle" fixed-width
              title="Delete variable"
              @click.stop="removeParameter(parameter)"
            )

</template>


<style lang="stylus" scoped>

  header h2
    margin: 0 !important

  .expand-toggle
    display: flex
    align-items: center
    margin: 0 5px 0 2px
    padding-right: 6px
    border-right: 1px solid $dark1 * 1.4

    .expand
      padding: 3px !important
      margin: 0
      transition: transform 0.15s

  .treelet-variant.expanded .expand-toggle .expand
    transform: rotate(90deg)

  .variant-picker
    display: flex
    align-items: center

  .content
    padding: 12px 12px 4px

  .variant-option
    height: auto
    color: $bright1
    background: transparent
    border: 0
    font-size: 10px
    font-weight: bold
    min-width: 0

    &:hover
      color: white

  .option-title
    flex: 1 1 auto
    min-width: 0

    &:not(:hover):not(:focus)
      padding: 0
      background: none
      color: white

  .option-title-row
    display: flex
    align-items: center
    gap: 12px
    margin-bottom: 8px

  .option-delete
    color: $bright1

    &:hover
      color: $cancel

  .parameters
    margin: 0

  .parameter
    display: flex
    align-items: center
    min-height: 23px

    svg
      flex: 0 0 auto

    input
      width: 0
      padding: 0 3px
      font-weight: 800
      font-size: 9px
      box-shadow: none

      &:not(:hover):not(:focus)
        background: none
        color: white

    .variable
      flex: 1 1 0
      min-width: 0
      margin-left: 5px
      text-align: right

    span
      margin: 0 3px

    .value
      flex: 1 1 0
      min-width: 0
      margin-right: 5px

    .delete
      margin-left: auto
      color: $bright1

      &:hover
        color: $cancel

</style>


<script>

  import { makeID } from '../js/core/id.js'

  export default {
    name: 'TreeletVariant',

    props: {
      variant: Object,
      component: Object,
      document: Object,
      index: Number,
    },

    data() {
      return {
        expanded: this.document.treeletsExpandedByDefault,
        parameterNames: new Map(),
      }
    },

    computed: {
      option() {
        return this.variant.options[this.variant.activeOption] || this.variant.options[0]
      },
    },

    methods: {
      addParameter() {
        const id = makeID()
        this.variant.options.forEach(option => {
          option.parameters.push({ id, name: 'width', value: '512mm' })
        })
        this.document.regenerateParameters()
      },

      addOption() {
        const source = this.option
        this.variant.options.push({
          title: `Option ${this.variant.options.length + 1}`,
          parameters: source.parameters.map(parameter => ({ ...parameter })),
        })
        this.variant.activeOption = this.variant.options.length - 1
        this.document.regenerateParameters()
      },

      changeOption() {
        this.document.regenerateParameters()
      },

      openOptionPicker() {
        const select = this.$refs.optionSelect
        if(select?.showPicker) select.showPicker()
        else select?.click()
      },

      commit(parameter) {
        const oldName = this.parameterNames.get(parameter) || parameter.name
        this.parameterNames.delete(parameter)
        if(parameter.id) {
          this.variant.options.forEach(option => {
            const other = option.parameters.find(item => item.id === parameter.id)
            if(other) other.name = parameter.name
          })
        }
        if(oldName != parameter.name) this.document.renameParameterReferences(oldName, parameter.name)
        this.document.regenerateParameters()
      },

      rememberParameterName(parameter) {
        this.parameterNames.set(parameter, parameter.name)
      },

      removeParameter(parameter) {
        this.document.removeParameterReferences(parameter, this.component)
        if(parameter.id) {
          this.variant.options.forEach(option => {
            option.parameters = option.parameters.filter(item => item.id !== parameter.id)
          })
        } else {
          this.option.parameters = this.option.parameters.filter(item => item !== parameter)
        }
        this.document.regenerateParameters()
      },

      removeOption() {
        const index = this.variant.options.indexOf(this.option)
        this.variant.options.splice(index, 1)
        this.variant.activeOption = Math.min(this.variant.activeOption, this.variant.options.length - 1)
        this.document.regenerateParameters()
      },

      remove() {
        this.option.parameters.forEach(parameter =>
          this.document.removeParameterReferences(parameter, this.component)
        )
        this.component.creator.variants =
          this.component.creator.variants.filter(variant => variant !== this.variant)
        this.document.regenerateParameters()
      },
    },
  }

</script>
