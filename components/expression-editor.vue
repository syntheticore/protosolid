<template lang="pug">

  //- Expression text with measures shown as pills in its flow. The browser
  //- edits the content directly, so it is built by hand instead of by Vue.
  span.expression-editor(
    contenteditable="true"
    spellcheck="false"
    @mousedown="mousedown"
    @keydown="keydown"
    @input="endAfterPill"
    @paste.prevent="paste"
    @drop.prevent
  )

</template>


<style lang="stylus" scoped>
  .expression-editor
    display: inline-block
    white-space: nowrap
    outline: none
    cursor: text
    // The app disables selection, which also hides the cursor here
    user-select: text

  // Pills are single, uneditable characters of the text. Their label is drawn
  // by CSS, as Firefox would place the cursor in any text within them.
  .measure
    display: inline-flex
    align-items: center
    vertical-align: middle
    box-sizing: border-box
    margin: -2px 2px 0
    padding: 0 5px 0 6px
    height: 16px
    line-height: 14px
    // A darker, fully saturated shade of the pill color
    border: 1px solid unquote('hsl(from var(--color) h 100% calc(l * 0.7))')
    border-radius: 99px
    background: var(--color)
    color: white
    font-family: "Helvetica Neue", Helvetica, "Inter Variable", Arial, sans-serif
    font-size: 11px
    font-weight: 700
    cursor: default
    transition: background 0.25s

    // Highlighted like combo boxes, also while the menu is open
    &:hover
    &.open
      background: unquote('hsl(from var(--color) h s calc(l * 1.1))')
      transition: none

    &::before
      content: attr(data-label)
      // Center the digits themselves, as browsers place fonts within lines differently
      text-box: trim-both cap alphabetic

    // Drawn, as fonts may lack a matching glyph
    &::after
      content: ''
      margin-left: 4px
      border: 3px solid transparent
      border-top: 4px solid
      // The pill's border color
      border-top-color: inherit
      border-bottom: none
      transition: transform 0.15s

    &.open::after
      transform: rotate(180deg)
</style>


<script>
  import { h, render } from 'vue'
  import PopupMenu from './popup-menu.vue'

  // Expression text of nodes, where pills stand for their tokens
  function serialize(node) {
    return [...node.childNodes].map(child =>
      child.nodeType == Node.TEXT_NODE ? child.nodeValue.replace(/ /g, ' ') :
      child.dataset?.measure ? `{${child.dataset.measure}}` :
      serialize(child)
    ).join('')
  }

  // Number of characters of nodes, where pills count as one
  function characters(node) {
    return [...node.childNodes].reduce((sum, child) => sum + (
      child.nodeType == Node.TEXT_NODE ? child.length :
      child.dataset?.measure ? 1 :
      characters(child)
    ), 0)
  }

  export default {
    name: 'ExpressionEditor',

    props: {
      value: String,
      // Pills by measure id, as { value, property, color, options }
      measures: Object,
    },

    emits: ['measure-property'],

    data() {
      return {
        lastSelection: null,
      }
    },

    watch: {
      value() {
        this.sync()
      },

      measures() {
        this.updatePills()
      },
    },

    mounted() {
      this.build()
      document.addEventListener('selectionchange', this.recordSelection)
    },

    beforeUnmount() {
      document.removeEventListener('selectionchange', this.recordSelection)
      this.closeMenu()
    },

    methods: {
      read() {
        return serialize(this.$el)
      },

      // Build the value again, unless the edited text already matches it
      sync() {
        if(this.read() != this.value) this.build()
      },

      build() {
        this.closeMenu()
        this.$el.replaceChildren(...this.value.split(/(\{\d+\})/).filter(Boolean).map(part => {
          const match = /^\{(\d+)\}$/.exec(part)
          return match ? this.createPill(match[1]) : document.createTextNode(part)
        }))
        this.endAfterPill()
        this.updatePills()
      },

      // Firefox only places the cursor behind a pill if something follows it
      endAfterPill() {
        if(this.$el.lastChild?.dataset?.measure) this.$el.append(document.createElement('br'))
      },

      createPill(id) {
        const pill = document.createElement('span')
        pill.className = 'measure'
        pill.contentEditable = 'false'
        pill.dataset.measure = id
        // Apply this component's scoped styles
        pill.setAttribute(this.$.type.__scopeId, '')
        return pill
      },

      updatePills() {
        for(const pill of this.$el.querySelectorAll('.measure')) {
          const measure = this.measures[pill.dataset.measure]
          pill.dataset.label = Number.isFinite(measure?.value) ? Number(measure.value.toFixed(2)) : 'Lost'
          pill.style.setProperty('--color', measure?.color || '')
        }
      },

      keydown(e) {
        if(['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) this.moveCaret(e)
        else if(e.key == 'Backspace' || e.key == 'Delete') this.deletePill(e)
      },

      // Browsers stop on both sides of pills and line breaks, so the cursor is
      // moved by characters instead, where pills count as one
      moveCaret(e) {
        if(e.ctrlKey || e.metaKey || e.altKey) return
        e.preventDefault()
        const selection = window.getSelection()
        const focus = this.position(selection.focusNode, selection.focusOffset)
        const anchor = this.position(selection.anchorNode, selection.anchorOffset)
        const backwards = e.key == 'ArrowLeft'
        const end = characters(this.$el)
        let target
        if(e.key == 'Home') target = 0
        else if(e.key == 'End') target = end
        // Collapse selections to their side
        else if(focus != anchor && !e.shiftKey) target = backwards ? Math.min(focus, anchor) : Math.max(focus, anchor)
        else target = Math.max(0, Math.min(focus + (backwards ? -1 : 1), end))
        const [node, offset] = this.point(target)
        if(e.shiftKey) selection.extend(node, offset)
        else selection.collapse(node, offset)
      },

      // Pills are deleted like single characters. Browsers disagree on deleting
      // uneditable elements, Chromium refuses with a line break behind them.
      deletePill(e) {
        const selection = window.getSelection()
        if(!selection.isCollapsed) return
        const backwards = e.key == 'Backspace'
        const { anchorNode: node, anchorOffset: offset } = selection
        const neighbor = node.nodeType != Node.TEXT_NODE ?
          node.childNodes[backwards ? offset - 1 : offset] :
          backwards ? offset == 0 && node.previousSibling : offset == node.length && node.nextSibling
        if(!neighbor?.dataset?.measure) return
        e.preventDefault()
        neighbor.remove()
        this.$el.dispatchEvent(new Event('input', { bubbles: true }))
      },

      // Pills open their options instead of taking the cursor
      mousedown(e) {
        const pill = e.target.closest('.measure')
        if(!pill) return
        e.preventDefault()
        if(this.menuPill == pill) this.closeMenu()
        else this.openMenu(pill)
      },

      openMenu(pill) {
        this.closeMenu()
        const id = pill.dataset.measure
        const measure = this.measures[id]
        if(!measure?.options.length) return
        this.menuPill = pill
        pill.classList.add('open')
        // The menu teleports itself to the body
        this.menuHost = document.createElement('div')
        const vnode = h(PopupMenu, {
          anchor: pill,
          label: 'Measure',
          options: measure.options,
          value: measure.property,
          onChoose: property => {
            this.closeMenu()
            this.$emit('measure-property', id, property)
          },
          onClose: this.closeMenu,
        })
        vnode.appContext = this.$.appContext
        render(vnode, this.menuHost)
      },

      closeMenu() {
        if(!this.menuHost) return
        render(null, this.menuHost)
        this.menuPill.classList.remove('open')
        this.menuHost = null
        this.menuPill = null
      },

      // Focus without moving the cursor, which goes to the end if it was elsewhere
      focusCaret() {
        const selection = window.getSelection()
        const inside = selection.rangeCount && this.$el.contains(selection.anchorNode)
        this.$el.focus()
        if(inside) return
        selection.selectAllChildren(this.$el)
        selection.collapseToEnd()
      },

      hasSelection() {
        return !window.getSelection().isCollapsed
      },

      // Characters before a point of the content, where pills count as one
      position(node, offset) {
        // Firefox may place the cursor within a pill
        const pill = (node.nodeType == Node.TEXT_NODE ? node.parentElement : node).closest('.measure')
        if(pill) return this.position(pill.parentNode, [...pill.parentNode.childNodes].indexOf(pill) + Math.min(offset, 1))
        const range = document.createRange()
        range.setStart(this.$el, 0)
        range.setEnd(node, offset)
        return characters(range.cloneContents())
      },

      // The one point for a position, wherever the cursor came from. It lies
      // within text where possible, and before a final line break.
      point(position) {
        const children = [...this.$el.childNodes]
        for(const [index, child] of children.entries()) {
          if(child.nodeType == Node.TEXT_NODE) {
            if(position <= child.length) return [child, position]
            position -= child.length
          } else if(child.dataset?.measure) {
            if(position == 0) return [this.$el, index]
            position -= 1
          }
        }
        return [this.$el, children.at(-1)?.nodeName == 'BR' ? children.length - 1 : children.length]
      },

      // Offset within the expression text, where pills are their tokens
      offset(node, offset) {
        const range = document.createRange()
        range.setStart(this.$el, 0)
        range.setEnd(node, offset)
        return serialize(range.cloneContents()).length
      },

      recordSelection() {
        const selection = window.getSelection()
        if(!selection.rangeCount || !this.$el.contains(selection.anchorNode)) return
        // Keep the cursor at one place per position, see moveCaret
        if(selection.isCollapsed) {
          const [node, offset] = this.point(this.position(selection.anchorNode, selection.anchorOffset))
          if(node != selection.anchorNode || offset != selection.anchorOffset) return selection.collapse(node, offset)
        }
        const range = selection.getRangeAt(0)
        this.lastSelection = {
          start: this.offset(range.startContainer, range.startOffset),
          end: this.offset(range.endContainer, range.endOffset),
        }
      },

      // Text range of the latest selection, or all text if there was none
      selectionRange() {
        return this.lastSelection || { start: 0, end: this.read().length }
      },

      paste(e) {
        document.execCommand('insertText', false, e.clipboardData.getData('text/plain'))
      },
    },
  }
</script>
