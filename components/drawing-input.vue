<template lang="pug">

  .drawing-input(:style="{ left: Math.round(position.x) + 'px', top: Math.round(position.y) + 'px' }")

    .panel.bordered.tipped

      .field(
        v-for="(field, index) in fields"
        v-pixel-snap
        :key="field.key"
        :class="{ locked: field.key in tool.locks, focused: focused == index, problem: problems[field.key] }"
      )
        span.label
          span(:class="{ 'diameter-symbol': field.key == 'diameter' }") {{ field.label }}

        .value(:style="{ width: Math.max(2, display(field).length * 10.5) + 'px' }")

          input(
            type="text"
            :ref="el => inputs[index] = el"
            :value="display(field)"
            spellcheck="false"
            @input="drafts[field.key] = $event.target.value"
            @keydown="keydown($event, index)"
            @keyup="$event.key.length == 1 && !isShortcut($event) && $event.stopPropagation()"
            @focus="focused = index"
            @blur="blur(index)"
          )

</template>


<style lang="stylus" scoped>

  // Fields and box keep the same distance all around, including at the corners
  radius = 3px
  spacing = 4px

  .drawing-input
    position: absolute
    pointer-events: none

  // The tip of the arrow points at the cursor, leaving room for its pointer below
  .panel
    display: flex
    gap: spacing
    width: max-content
    margin-top: 29px
    margin-left: -23px
    padding: spacing
    border-radius: radius + spacing

  .field
    display: flex
    align-items: center
    height: 20px
    padding-left: 6px
    border-radius: radius
    background: var(--input-bg)
    border: 2px solid transparent
    box-shadow: 0 1px 3px shadow(0.5)
    transition: background 0.1s, border-color 0.1s

    &.focused
      border-color: $highlight
      background: lighten($highlight, 80%)

    &.locked .value
      color: $highlight

    &.problem
      border-color: $red
      background: lighten($red, 80%)

  .label
    padding-right: 5px
    font-size: 11px
    font-weight: 900
    color: var(--input-fg)

    .diameter-symbol
      font-size: 1.7em
      line-height: 0
      position: relative
      top: 2px
      margin-left: -4px

  .value
    color: var(--input-fg)
    transition: width 0.2s

    input
      font-size: 12px
      font-weight: 900
      font-family: Orbitron
      line-height: 16px
      white-space: pre
      width: 100%
      border: none
      background: none
      outline: none
      padding: 0
      margin: 0
      color: inherit

      &::selection
        background: none

</style>


<script>
  import Expression from '../js/core/expression.js'

  export default {
    name: 'DrawingInput',

    props: {
      tool: Object,
      component: Object,
      position: Object,
    },

    data() {
      return {
        drafts: {},
        expressions: {},
        problems: {},
        focused: 0,
      }
    },

    computed: {
      fields() {
        return this.tool.inputFields()
      },
    },

    created() {
      this.inputs = []
    },

    mounted() {
      setTimeout(() => this.focus(0))
    },

    updated() {
      // Live values replace the input text, which drops the selection.
      const field = this.fields[this.focused]
      if(field && !(field.key in this.drafts)) this.select(this.focused)
    },

    beforeUnmount() {
      this.unmounted = true
    },

    methods: {
      display(field) {
        if(field.key in this.drafts) return this.drafts[field.key]
        if(field.key in this.tool.locks) return this.expressions[field.key]
        return new Expression(field.value, [], field.dimension).expression
      },

      focus(index) {
        const input = this.inputs[index]
        if(!input || this.unmounted) return
        this.focused = index
        input.focus()
        this.select(index)
      },

      select(index) {
        const input = this.inputs[index]
        input.setSelectionRange(0, input.value.length)
      },

      // Returns false if the entered text could not be parsed.
      commit(index) {
        const field = this.fields[index]
        const text = this.drafts[field.key].trim()
        delete this.drafts[field.key]
        this.problems[field.key] = false
        if(!text) {
          delete this.expressions[field.key]
          this.tool.setLock(field.key, undefined)
          return true
        }
        try {
          const expression = new Expression(text, this.component.getParameters(), field.dimension)
          const value = expression.getBase()
          if(!Number.isFinite(value) || (field.dimension == 'length' && value <= 0)) throw new Error()
          this.expressions[field.key] = expression.expression
          this.tool.setLock(field.key, value)
          return true
        } catch(e) {
          this.drafts[field.key] = text
          this.problems[field.key] = true
          return false
        }
      },

      keydown(e, index) {
        if(e.key == 'Escape' || e.altKey || e.ctrlKey || e.metaKey) return
        if(this.isShortcut(e)) return e.preventDefault()
        e.stopPropagation()
        const field = this.fields[index]
        if(e.key == 'Enter') {
          e.preventDefault()
          if(field.key in this.drafts) {
            this.commit(index)
            this.$nextTick(() => this.select(index))
          } else {
            this.tool.submitInput()
          }
        } else if(e.key == 'Tab') {
          e.preventDefault()
          if(field.key in this.drafts && !this.commit(index)) return
          const count = this.fields.length
          this.$nextTick(() => this.focus((index + (e.shiftKey ? count - 1 : 1)) % count))
        }
      },

      // Letters can only follow a value (units), so a leading letter
      // is passed on as a tool shortcut instead of being typed.
      isShortcut(e) {
        return /^\p{L}$/u.test(e.key) && e.target.selectionStart == 0
      },

      blur(index) {
        // Canvas clicks blur the input before the tool receives the click,
        // so pending text is applied to the point being placed.
        const field = this.fields[index]
        if(field && field.key in this.drafts) this.commit(index)
        setTimeout(() => {
          const active = document.activeElement
          if(!active || active == document.body) this.focus(this.focused)
        })
      },
    },
  }
</script>
