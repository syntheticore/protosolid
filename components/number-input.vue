<template lang="pug">

  .number-input

    button.button
      Icon(icon="tape")

    input(
      type="text"
      ref="input"
      :style="inputStyle"
      :value="inner.expression"
      :class="{ problem }"
      spellcheck="false"
      @blur="onBlur"
      @keydown="keydown"
      @keydown.enter="enter()"
    )

    .controls

      button.button(@click="increase")
        Icon(icon="caret-up")

      button.button(@click="decrease")
        Icon(icon="caret-down")

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
      &.problem
        border-color: $red
        background: lighten($red, 80%)

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
  import Expression from '../js/core/expression.js'

  function truncate(number) {
    return String(Number(number.toFixed(3)))
  }

  export default {
    name: 'NumberInput',

    props: {
      component: Object,
      value: Number,
      autoFocus: Boolean,
    },

    data() {
      return {
        inner: new Expression(this.value, this.component.getParameters()),
        allowBlur: false,
        problem: false,
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
        if(this.inner.getBase() == value) return
        this.inner.setBase(value)
        this.update()
      },
    },

    mounted() {
      // Update immediately to make features use preferred unit
      // this.update()
      this.focusInput(() => this.allowBlur = true )
    },

    methods: {
      enter: function(noFocus) {
        this.problem = false
        try {
          this.inner.set(this.$refs.input.value)
          this.update(noFocus)
          this.$emit('enter')
        } catch(e) {
          this.problem = true
          this.$emit('error', { type: 'error', msg: "Please enter a valid expression" })
          this.focusInput()
        }
      },

      update: function(noFocus) {
        this.$emit('update:value', this.inner.getBase())
        if(!noFocus) this.focusInput()
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

      focusInput: function(cb) {
        const input = this.$refs.input
        if(!input) return
        clearTimeout(this.timeout)
        this.timeout = setTimeout(() => {
          input.setSelectionRange(0, input.value.length)
          input.focus()
          if(cb) setTimeout(cb)
        })
      },

      onBlur: function() {
        if(this.autoFocus ) {
          this.focusInput()
        } else if(this.allowBlur) {
          this.enter(true)
        }
      },
    },
  }
</script>
