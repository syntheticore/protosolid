<template lang="pug">

  .numeric-input(:class="{ 'has-tool': $slots.default, labeled: label, parameterized, stretch }")

    //- Optional button left of the input
    slot

    span.label(v-if="label") {{ label }}

    input.input(
      v-if="!hasPills"
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

    //- Expressions with measures show them inline
    ExpressionEditor.input.rich(
      v-else
      ref="editor"
      :style="inputStyle"
      :value="String(value)"
      :measures="measures"
      :class="{ problem }"
      @input="fitWidth"
      @blur="onBlur"
      @keydown="keydown"
      @keydown.enter.prevent="commit()"
      @measure-property="(id, property) => $emit('measure-property', id, property)"
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
    .rich
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

    // With pills, the cursor and selection work as in normal text
    .rich
      overflow: hidden
      line-height: 22px
      // Inherited by pills, which would show their white text color otherwise
      caret-color: $dark2
      &::selection
      :deep(*)::selection
        background: lighten($purple, 50%)

    &.parameterized

      input:focus
      .rich:focus
        background: lighten($purple, 80%)
        border-color: $purple

      input::selection
        background: lighten($purple, 50%)

    // Match the spinner height, also within forms
    .input
      height: auto

    &:not(.has-tool) input
    &:not(.has-tool) .rich
      border-top-left-radius: 3px
      border-bottom-left-radius: 3px

    &.stretch input
    &.stretch .rich
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
  import { measureIds } from '../js/core/measures.js'

  function textWidth(text) {
    return 24 + Math.max(2, text.length * 11)
  }

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
      // Shows measures as pills, see ExpressionEditor
      measures: Object,
    },

    emits: ['commit', 'increase', 'decrease', 'measure-property'],

    data() {
      return {
        allowBlur: false,
        // Width of the pill editor's current content, see fitWidth. Without
        // pills, only set while the input takes over from the editor.
        fittedWidth: null,
      }
    },

    computed: {
      hasPills() {
        return !!this.measures && measureIds(String(this.value)).length > 0
      },

      inputStyle() {
        if(this.stretch) return null
        const width = this.fittedWidth ?? textWidth(String(this.value))
        return { width: String(width) + 'px' }
      },
    },

    watch: {
      autoFocus(value) {
        if(value) this.focusInput()
      },

      // Pill labels change with their values
      measures() {
        if(this.hasPills) this.$nextTick(this.fitWidth)
      },

      // The input and the pill editor take over each other's width when they
      // replace each other, so the change animates
      value(value, previous) {
        const hadPills = !!this.measures && measureIds(String(previous)).length > 0
        if(this.hasPills) {
          if(!hadPills) this.fittedWidth = textWidth(String(previous))
          this.$nextTick(this.fitWidth)
        } else if(hadPills) {
          this.$nextTick(() => {
            this.$refs.input.offsetWidth
            this.fittedWidth = null
          })
        }
      },
    },

    mounted() {
      if(this.hasPills) this.fitWidth()
      if(this.autoFocus || this.focusOnMount) {
        this.focusInput(() => this.allowBlur = true)
      } else {
        this.allowBlur = true
      }
    },

    methods: {
      commit(noFocus) {
        this.$emit('commit', this.text(), noFocus)
      },

      // Current, possibly uncommitted text
      text() {
        return this.hasPills ? this.$refs.editor.read() : this.$refs.input.value
      },

      // Show the normalized value, even if it equals the previous one
      syncValue() {
        if(this.hasPills) return this.$nextTick(() => this.$refs.editor?.sync())
        const input = this.$refs.input
        this.$nextTick(() => input.value = this.value)
      },

      // Size the pill editor to its current content, including uncommitted text
      fitWidth() {
        const range = document.createRange()
        range.selectNodeContents(this.$refs.editor.$el)
        this.fittedWidth = 24 + Math.ceil(range.getBoundingClientRect().width)
      },

      // Text range that a measure should replace
      selectionRange() {
        if(this.hasPills) return this.$refs.editor.selectionRange()
        return { start: this.$refs.input.selectionStart, end: this.$refs.input.selectionEnd }
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
        const hasSelection = this.hasPills ? this.$refs.editor.hasSelection() :
          this.$refs.input.selectionStart != this.$refs.input.selectionEnd
        const allowPropagation =
          e.keyCode == 27 ||
          e.altKey ||
          e.ctrlKey ||
          e.metaKey ||
          (e.keyCode == 13 && hasSelection)
        if(!allowPropagation) e.stopPropagation()
      },

      focusInput(cb) {
        if(!this.$refs.input && !this.$refs.editor) return
        clearTimeout(this.timeout)
        this.timeout = setTimeout(() => {
          if(this.documentBusy) {
            this.$watch('documentBusy', () => this.focusInput(cb), { once: true })
            return
          }
          // Pills may have been added or removed meanwhile
          const input = this.$refs.input
          if(input) {
            input.setSelectionRange(0, input.value.length)
            input.focus()
          } else {
            this.$refs.editor?.focusCaret()
          }
          if(cb) setTimeout(cb)
        })
      },

      onBlur() {
        // Let the newly focused input take ownership before restoring focus.
        setTimeout(() => {
          if(!this.$refs.input && !this.$refs.editor) return
          // Measure pills keep focus within the input
          if(this.$refs.editor && this.$el.contains(document.activeElement)) return
          if(this.autoFocus) this.focusInput()
          else if(this.allowBlur) this.commit(true)
        })
      },
    },
  }
</script>
