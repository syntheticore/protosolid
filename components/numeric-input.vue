<template lang="pug">

  .numeric-input(:class="{ 'has-tool': $slots.default, labeled: label, parameterized, stretch }")

    //- Optional button left of the input
    slot

    span.label(v-if="label") {{ label }}

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

    .spinner

      button.button(type="button" tabindex="-1" @mousedown.prevent @click="$emit('increase', stepFactor($event))")
        Icon(icon="caret-up")

      button.button(type="button" tabindex="-1" @mousedown.prevent @click="$emit('decrease', stepFactor($event))")
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

    // Match the spinner height, also within forms
    .input
      height: auto

    &:not(.has-tool) input
      border-top-left-radius: 3px
      border-bottom-left-radius: 3px

    &.stretch input
      flex: 1 1 0
      min-width: 0

    // Shown inside the input, left of the value
    .label
      position: absolute
      top: 0
      bottom: 0
      left: 6px
      z-index: 2
      display: flex
      align-items: center
      font-size: 9px
      font-weight: 900
      color: $bright2 * 0.6
      pointer-events: none

    &.labeled input
      padding-left: 12px

  .spinner
    display: flex
    flex-direction: column
    flex: 0 0 auto
    .button
      flex: 1 1 auto
      height: auto
      margin: 0
      padding: 0 5px 1px
      text-shadow: none
      border-radius: 0
      font-size: 9px
      // Explicit, as browsers round the normal line height differently
      line-height: 10px
      &:first-child
        border-top-right-radius: 3px
      &:last-child
        border-bottom-right-radius: 3px
</style>


<script>
  export default {
    name: 'NumericInput',

    inject: ['documentBusy'],

    props: {
      value: [String, Number],
      autoFocus: Boolean,
      focusOnMount: Boolean,
      problem: Boolean,
      // Short text shown inside the input
      label: String,
      inputmode: String,
      parameterized: Boolean,
      // Fill the available width instead of sizing to the value
      stretch: Boolean,
    },

    emits: ['commit', 'increase', 'decrease'],

    data() {
      return {
        allowBlur: false,
      }
    },

    computed: {
      inputStyle() {
        if(this.stretch) return null
        const width = 24 + Math.max(2, String(this.value).length * 11)
        return { width: String(width) + 'px' }
      },
    },

    watch: {
      autoFocus(value) {
        if(value) this.focusInput()
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

      // Show the normalized value, even if it equals the previous one
      syncValue() {
        const input = this.$refs.input
        this.$nextTick(() => input.value = this.value)
      },

      // Alt steps finer, Shift coarser
      stepFactor(e) {
        if(e.altKey) return 0.1
        if(e.shiftKey) return 10
        return 1
      },

      keydown(e) {
        if((e.key == 'ArrowUp' || e.key == 'ArrowDown') && !e.ctrlKey && !e.metaKey) {
          e.preventDefault()
          e.stopPropagation()
          this.$emit(e.key == 'ArrowUp' ? 'increase' : 'decrease', this.stepFactor(e))
          return
        }
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
          if(this.documentBusy) {
            this.$watch('documentBusy', () => this.focusInput(cb), { once: true })
            return
          }
          input.setSelectionRange(0, input.value.length)
          input.focus()
          if(cb) setTimeout(cb)
        })
      },

      onBlur() {
        // Let the newly focused input take ownership before restoring focus.
        setTimeout(() => {
          if(!this.$refs.input) return
          if(this.autoFocus) this.focusInput()
          else if(this.allowBlur) this.commit(true)
        })
      },
    },
  }
</script>
