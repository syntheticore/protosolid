<template lang="pug">
  .number-input
    button.button
      fa-icon(icon="tape")
    input(
      type="text"
      ref="input"
      :style="inputStyle"
      :value="inner.expression"
      spellcheck="false"
      @blur="focusInput"
      @keydown="keydown"
      @keydown.enter="enter"
    )
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
      text-align: center
      padding: 0
      // padding-right: 23px
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
  import Expression from './../expression.js'

  function truncate(number) {
    return String(Number(number.toFixed(3)))
  }

  export default {
    name: 'NumberInput',

    props: {
      component: Object,
      value: Number,
    },

    data() {
      return {
        inner: new Expression(this.value, this.component.getParameters()),
      }
    },

    computed: {
      inputStyle: function() {
        const numChars = String(this.inner.expression).length
        return {
          'width': String(24 + Math.max(2, numChars * 11)) + 'px'
        }
      },
    },

    watch: {
      // 'inner.expression': function() {
      //   this.update()
      // },

      value: function(value) {
        this.inner.setBase(value)
        this.update()
      },
    },

    mounted() {
      // Update immediately to make features use preferred unit
      this.update()
      this.focusInput()
    },

    methods: {
      enter: function() {
        try {
          this.inner.set(this.$refs.input.value)
          this.update()
        } catch(e) {
          this.$emit('error', "Please enter a valid expression")
        }
      },

      update: function() {
        this.$emit('update:value', this.inner.getBase())
        this.focusInput()
      },

      keydown: function(e) {
        const allowPropagation =
          e.keyCode == 27 || // Escape
          e.altKey ||
          (e.keyCode == 13 && (this.$refs.input.selectionStart != this.$refs.input.selectionEnd)) // Enter pressed without cursor in input
        if(!allowPropagation) e.stopPropagation()
      },

      increase: function() {
        const number = this.inner.parse()
        this.inner.set(truncate(number.value + 1) + number.unit)
        this.update()
      },

      decrease: function() {
        const number = this.inner.parse()
        const newValue = number.value - 1
        if(newValue >= 0) this.inner.set(truncate(newValue) + number.unit)
        this.update()
      },

      focusInput: function() {
        const input = this.$refs.input
        if(!input) return
        clearTimeout(this.timeout)
        this.timeout = setTimeout(() => {
          input.setSelectionRange(0, input.value.length)
          // console.log(input.selectionStart)
          input.focus()
        })
      },
    },
  }
</script>
