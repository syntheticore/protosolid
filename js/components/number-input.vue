<template lang="pug">
  .number-input
    input(
      type="text"
      ref="input"
      v-model="inner"
      @blur="focusInput"
      @keydown.enter.prevent="focusInput"
    )
    .unit
      | mm
    .controls
      button.button(@click.prevent="increase") +
      button.button(@click.prevent="decrease") -
</template>


<style lang="stylus" scoped>
  .number-input
    display: flex
    position: relative
    input
      flex: 1 1 auto
      border-top-right-radius: 0
      border-bottom-right-radius: 0
      max-width: 60px
      text-align: right
      padding: 0
      padding-right: 26px
      font-size: 12px
      font-weight: 900
      color: $dark2
      &:focus
        border-right: none
        background: lighten($highlight, 75%)
      &::selection
        background: none

  .unit
    position: absolute
    right: 22px
    bottom: 6px
    color: rgba(black, 0.65)
    font-size: 10px
    pointer-events: none

  .controls
    display: flex
    flex-direction: column
    flex: 0 0 auto
    .button
      margin: 0
      padding: 0px 4px
      text-shadow: none
      font-size: 9px
      border-radius: 0
      &:first-child
        // margin-top: 3px
        border-top-right-radius: 3px
      &:last-child
        // margin-top: 3px
        border-bottom-right-radius: 3px
</style>


<script>
  export default {
    name: 'NumberInput',

    props: {
      value: Number,
    },

    watch: {
      inner: function() {
        this.$emit('update:value', Number(this.inner))
      },
    },

    data() {
      return {
        inner: String,
      }
    },

    mounted() {
      this.inner = this.value
      this.focusInput()
    },

    methods: {
      increase: function() {
        console.log('increase')
        this.inner = Number(this.inner) + 1
        this.focusInput()
      },

      decrease: function() {
        this.inner = Number(this.inner) - 1
        this.focusInput()
      },

      focusInput: function() {
        const input = this.$refs.input
        if(!input) return
        clearTimeout(this.timeout)
        this.timeout = setTimeout(() => {
          input.setSelectionRange(0, input.value.length)
          input.focus()
        })
      },
    },
  }
</script>
