<template lang="pug">

  .numeric-input(:class="{ 'has-tool': icon, parameterized }")

    button.button(v-if="icon" type="button")
      Icon(:icon="icon")

    input.input(
      type="text"
      ref="input"
      :style="inputStyle"
      :value="value"
      :class="{ problem }"
      :inputmode="inputmode"
      spellcheck="false"
      @blur="onBlur"
      @keydown="keydown"
      @keydown.enter="commit()"
    )

    .controls

      button.button(type="button" @mousedown.prevent @click="$emit('increase')")
        Icon(icon="caret-up")

      button.button(type="button" @mousedown.prevent @click="$emit('decrease')")
        Icon(icon="caret-down")

</template>


<style lang="stylus" scoped>
  .numeric-input
    display: flex
    position: relative
    input
      width: 100%
      border-radius: 0
      text-align: center
      padding: 0
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

    &.parameterized

      input:focus
        background: lighten($purple, 80%)
        border-color: $purple

      input::selection
        background: lighten($purple, 50%)

    &:not(.has-tool) input
      border-top-left-radius: 3px
      border-bottom-left-radius: 3px

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
  export default {
    name: 'NumericInput',

    props: {
      value: [String, Number],
      autoFocus: Boolean,
      focusOnMount: Boolean,
      problem: Boolean,
      icon: {
        type: String,
        default: null,
      },
      inputmode: String,
      parameterized: Boolean,
    },

    emits: ['commit', 'increase', 'decrease'],

    data() {
      return {
        allowBlur: false,
      }
    },

    computed: {
      inputStyle() {
        const width = 24 + Math.max(2, String(this.value).length * 11)
        return { width: String(width) + 'px' }
      },
    },

    mounted() {
      if(this.autoFocus || this.focusOnMount) {
        this.focusInput(() => this.allowBlur = true)
      } else {
        this.allowBlur = true
      }
    },

    methods: {
      commit(noFocus) {
        this.$emit('commit', this.$refs.input.value, noFocus)
      },

      keydown(e) {
        const allowPropagation =
          e.keyCode == 27 ||
          e.altKey ||
          e.ctrlKey ||
          e.metaKey ||
          (e.keyCode == 13 && this.$refs.input.selectionStart != this.$refs.input.selectionEnd)
        if(!allowPropagation) e.stopPropagation()
      },

      focusInput(cb) {
        const input = this.$refs.input
        if(!input) return
        clearTimeout(this.timeout)
        this.timeout = setTimeout(() => {
          input.setSelectionRange(0, input.value.length)
          input.focus()
          if(cb) setTimeout(cb)
        })
      },

      onBlur() {
        if(this.autoFocus) {
          this.focusInput()
        } else if(this.allowBlur) {
          this.commit(true)
        }
      },
    },
  }
</script>
