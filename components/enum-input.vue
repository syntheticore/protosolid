<template lang="pug">

  details.enum-input(ref="root")

    summary.input(
      role="combobox"
      aria-haspopup="listbox"
      :aria-expanded="open"
      @pointerdown.stop
      @click.stop
    )
      span {{ label }}
      Icon.expander(icon="angle-down")

    .options(role="listbox" @pointerdown.stop @click.stop)

      button(
        v-for="(name, option) in options"
        :key="option"
        type="button"
        role="option"
        :aria-selected="String(option) === String(value)"
        :class="{ selected: String(option) === String(value) }"
        @click="choose(option)"
      ) {{ name }}

</template>


<style lang="stylus" scoped>

  .enum-input
    position: relative
    min-width: 140px
    font-weight: normal

  summary
    box-sizing: border-box
    height: 28px
    display: flex
    align-items: center
    justify-content: space-between
    gap: 12px
    cursor: default
    list-style: none

    &::-webkit-details-marker
      display: none

  .expander
    color: $dark2
    transition: transform 0.15s

  details[open] .expander
    transform: rotate(180deg)

  .options
    position: absolute
    z-index: 20
    top: calc(100% + 3px)
    left: 0
    right: 0
    overflow: hidden
    padding: 2px
    background: $bright1
    border: 1px solid $dark1
    border-radius: 3px
    box-shadow: 0 4px 12px rgba(black, 0.35)

    button
      display: block
      width: 100%
      padding: 5px 7px
      border: 0
      border-radius: 2px
      background: transparent
      color: $dark1
      font: inherit
      text-align: left
      white-space: nowrap

      &:hover
      &:focus
      &.selected
        color: white
        background: $highlight
        outline: none

</style>


<script>

  export default {
    name: 'EnumInput',

    props: {
      value: [String, Number],
      options: {
        type: Object,
        required: true,
      },
    },

    emits: ['update:value'],

    data() {
      return { open: false }
    },

    computed: {
      label() {
        return this.options[this.value] ?? this.value
      },
    },

    mounted() {
      this.$refs.root.addEventListener('toggle', this.onToggle)
      window.addEventListener('pointerdown', this.close)
      window.addEventListener('keydown', this.onKeydown)
    },

    beforeUnmount() {
      this.$refs.root?.removeEventListener('toggle', this.onToggle)
      window.removeEventListener('pointerdown', this.close)
      window.removeEventListener('keydown', this.onKeydown)
    },

    methods: {
      choose(option) {
        this.$emit('update:value', option)
        this.$refs.root.open = false
      },

      close(event) {
        if(!event || !this.$el.contains(event.target)) this.$refs.root.open = false
      },

      onKeydown(event) {
        if(event.key === 'Escape') this.close()
      },

      onToggle() {
        this.open = this.$refs.root.open
      },
    },
  }

</script>
