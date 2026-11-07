<template lang="pug">

  li.treelet-variant(:class="{ expanded }")

    .box

      header(@click="expanded = !expanded")

        Icon(icon="calculator" fixed-width)

        h2 Variant

        .expand-toggle

          Icon.expand(icon="angle-right")

        .variant-picker(@click.stop)

          ComboBox.variant-option(
            :model-value="variant.activeOption"
            label="Variant"
            :options="variant.options.map((option, i) => ({ value: i, label: option.title }))"
            @update:model-value="changeOption"
          )

        .controls(:class="{ wide: variant.options.length == 1, 'ultra-wide': variant.options.length > 1 }")

          Icon(
            icon="plus-circle" fixed-width
            title="Add option"
            @click.stop="addOption"
          )

          Icon.option-delete(
            v-if="variant.options.length > 1"
            icon="minus-circle" fixed-width
            title="Delete option"
            @click.stop="removeOption"
          )

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="remove"
          )

      .content(v-if="expanded")

        input.input.option-title(
          type="text"
          v-model.trim="option.title"
          @change="document.regenerateParameters()"
          @keydown.stop
        )

</template>


<style lang="stylus" scoped>

  header h2
    margin: 0 !important

  .expand-toggle
    display: flex
    align-self: stretch
    align-items: center
    margin: 0 5px 0 2px
    padding-right: 6px
    border-right: 1px solid dark1(1.4)

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
    padding: 12px

  .variant-option
    height: 21px
    color: bright1()
    background: transparent
    border: 0
    font-size: 10px
    font-weight: bold
    min-width: 0

    :deep(.trigger)
      color: inherit
      background: transparent
      border: 0
      padding-top: 0
      padding-bottom: 0

    :deep(.expander)
      padding: 0

    &:hover
      color: var(--fg)

  .option-title
    flex: 1 1 auto
    min-width: 0

    &:not(:hover):not(:focus)
      padding: 0
      background: none
      color: var(--fg)

  .option-delete
    color: bright1()

    &:hover
      color: var(--cancel)

</style>


<script>

  export default {
    name: 'TreeletVariant',

    props: {
      variant: Object,
      component: Object,
      document: Object,
    },

    computed: {
      expanded: {
        get() {
          return this.document.isTreeletExpanded(`variant:${this.component.id}`)
        },
        set(value) {
          this.document.setTreeletExpanded(`variant:${this.component.id}`, value)
        },
      },

      option() {
        return this.variant.options[this.variant.activeOption] || this.variant.options[0]
      },
    },

    methods: {
      addOption() {
        this.component.creator.syncActiveVariantOption()
        this.variant.options.push({
          title: `Option ${this.variant.options.length + 1}`,
          values: Object.fromEntries(this.component.creator.parameters.map(parameter =>
            [parameter.id, parameter.value]
          )),
        })
        this.variant.activeOption = this.variant.options.length - 1
        this.document.regenerateParameters()
      },

      changeOption(value) {
        this.component.creator.selectVariantOption(value)
        this.document.regenerateParameters()
      },

      removeOption() {
        const index = this.variant.options.indexOf(this.option)
        this.variant.options.splice(index, 1)
        this.variant.activeOption = Math.min(this.variant.activeOption, this.variant.options.length - 1)
        this.component.creator.selectVariantOption(this.variant.activeOption, false)
        this.document.regenerateParameters()
      },

      remove() {
        this.component.creator.variant = null
        this.document.regenerateParameters()
      },
    },
  }

</script>
