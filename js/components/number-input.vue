<template lang="pug">
  .number-input
    button.button
      fa-icon(icon="tape")
    input(
      type="text"
      ref="input"
      :style="inputStyle"
      :value="inner.asGiven()"
      @blur="focusInput"
      @keydown.enter="enter"
    )
    .unit
      | {{ inner.parse().unit }}
    .controls
      button.button(@click="increase")
        fa-icon(icon="caret-up")
      button.button(@click="decrease")
        fa-icon(icon="caret-down")
</template>


<style lang="stylus" scoped>
  .number-input
    display: flex
    position: relative
    input
      width: 100%
      border-radius: 0
      text-align: right
      padding: 0
      padding-right: 23px
      font-size: 12px
      font-weight: 900
      color: $dark2
      margin: 0
      z-index: 1
      font-family: Orbitron
      &:focus
        background: lighten($highlight, 80%)
      &::selection
        background: none

  .unit
    position: absolute
    right: 22px
    bottom: 6px
    color: rgba(black, 0.65)
    font-size: 10px
    pointer-events: none
    z-index: 2
    width: 18px

  .button
    margin: 0
    padding: 0px 5px
    text-shadow: none
    border-radius: 3px
    border-top-right-radius: 0
    border-bottom-right-radius: 0

  .controls
    display: flex
    flex-direction: column
    flex: 0 0 auto
    .button
      padding-bottom: 1px
      border-radius: 0
      font-size: 9px
      &:first-child
        border-top-right-radius: 3px
      &:last-child
        border-bottom-right-radius: 3px
</style>


<script>
  import Unit from './../units.js'

  export default {
    name: 'NumberInput',

    props: {
      value: Number,
    },

    data() {
      return {
        inner: new Unit(this.value),
      }
    },

    computed: {
      inputStyle: function() {
        const numChars = String(this.inner.asGiven()).length
        return {
          'width': String(36 + Math.max(2, numChars * 10)) + 'px'
        }
      },
    },

    watch: {
      'inner.value': function() {
        this.$emit('update:value', this.inner.value)
      },
    },

    mounted() {
      this.focusInput()
    },

    methods: {
      enter: function() {
        this.inner.value = this.$refs.input.value
        this.focusInput()
      },

      increase: function() {
        this.inner.value += 1
        this.focusInput()
      },

      decrease: function() {
        this.inner.value -= 1
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
