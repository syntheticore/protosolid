<template lang="pug">

  .combo-box(@pointerdown.stop @click.stop @keydown.stop="onKeydown")

    button.input.trigger(
      ref="trigger"
      type="button"
      role="combobox"
      aria-haspopup="listbox"
      :aria-label="label"
      :aria-expanded="open"
      :aria-controls="menuId"
      :aria-activedescendant="open && activeIndex >= 0 ? `${menuId}-${activeIndex}` : undefined"
      :disabled="disabled"
      @click="open ? close() : show()"
      @blur="close"
    )

      span.value {{ selected?.label || placeholder }}

      Icon.expander(icon="angle-down" :class="{ open }")

    Teleport(to="body")

      .combo-menu.bordered(
        v-if="open"
        ref="menu"
        :id="menuId"
        role="listbox"
        :aria-label="label"
        :style="menuStyle"
        @pointerdown.prevent.stop
        @pointermove="activeIndex = -1"
        @click.stop
      )

        .combo-option(
          v-for="(option, index) in options"
          :key="index"
          :id="`${menuId}-${index}`"
          role="option"
          :aria-selected="option.value === modelValue"
          :class="{ active: index === activeIndex, selected: option.value === modelValue }"
          @click="choose(index)"
        )

          span {{ option.label }}

          Icon(v-if="option.value === modelValue" icon="check")

</template>


<script setup>

  const props = defineProps({
    modelValue: {},
    options: { type: Array, required: true },
    label: String,
    placeholder: { type: String, default: 'Choose…' },
    disabled: Boolean,
  })
  const emit = defineEmits(['update:modelValue'])
  const menuId = `combo-${useId()}`
  const trigger = ref(null)
  const menu = ref(null)
  const open = ref(false)
  // Option highlighted by keyboard navigation, the mouse uses :hover
  const activeIndex = ref(-1)
  const menuStyle = ref({})
  const selectedIndex = computed(() => props.options.findIndex(option => option.value === props.modelValue))
  const selected = computed(() => props.options[selectedIndex.value])
  let search = ''
  let searchTimer

  async function show() {
    if(props.disabled || !props.options.length) return
    activeIndex.value = -1
    const rect = trigger.value.getBoundingClientRect()
    const below = window.innerHeight - rect.bottom - 8
    const above = rect.top - 8
    const upwards = below < Math.min(props.options.length * 30 + 8, 240) && above > below
    menuStyle.value = {
      minWidth: `${rect.width}px`,
      maxWidth: `${window.innerWidth - 16}px`,
      maxHeight: `${Math.min(240, upwards ? above : below)}px`,
      top: upwards ? undefined : `${rect.bottom + 4}px`,
      bottom: upwards ? `${window.innerHeight - rect.top + 4}px` : undefined,
      left: `${rect.left}px`,
      visibility: 'hidden',
    }
    open.value = true
    window.addEventListener('pointerdown', outside, true)
    window.addEventListener('resize', close)
    window.addEventListener('scroll', onScroll, true)
    await nextTick()
    if(!open.value) return
    menuStyle.value.left = `${Math.max(8, Math.min(rect.left, window.innerWidth - menu.value.offsetWidth - 8))}px`
    menuStyle.value.visibility = 'visible'
    revealActive()
  }

  function close() {
    open.value = false
    search = ''
    clearTimeout(searchTimer)
    window.removeEventListener('pointerdown', outside, true)
    window.removeEventListener('resize', close)
    window.removeEventListener('scroll', onScroll, true)
  }

  function outside(event) {
    if(!trigger.value.contains(event.target) && !menu.value?.contains(event.target)) close()
  }

  function onScroll(event) {
    if(!menu.value?.contains(event.target)) close()
  }

  function choose(index) {
    emit('update:modelValue', props.options[index].value)
    close()
    trigger.value.focus()
  }

  function revealActive() {
    const index = activeIndex.value >= 0 ? activeIndex.value : selectedIndex.value
    nextTick(() => menu.value?.children[index]?.scrollIntoView({ block: 'nearest' }))
  }

  function onKeydown(event) {
    if(event.key === 'Tab') {
      close()
      return
    }
    if(event.key === 'Escape') {
      event.preventDefault()
      close()
    } else if(['Enter', ' '].includes(event.key)) {
      event.preventDefault()
      if(!open.value) show()
      else if(activeIndex.value >= 0) choose(activeIndex.value)
      else close()
    } else if(['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault()
      if(!open.value) show()
      else if(event.key === 'Home') activeIndex.value = 0
      else if(event.key === 'End') activeIndex.value = props.options.length - 1
      else {
        // Continue from the selected option when nothing is highlighted yet
        const index = activeIndex.value >= 0 ? activeIndex.value : selectedIndex.value
        const step = event.key === 'ArrowDown' ? 1 : -1
        activeIndex.value = index < 0 ? (step > 0 ? 0 : props.options.length - 1) :
          (index + step + props.options.length) % props.options.length
      }
      revealActive()
    } else if(event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault()
      if(!open.value) show()
      search += event.key.toLowerCase()
      clearTimeout(searchTimer)
      searchTimer = setTimeout(() => search = '', 700)
      const index = props.options.findIndex(option => option.label.toLowerCase().startsWith(search))
      if(index !== -1) activeIndex.value = index
      revealActive()
    }
  }

  onBeforeUnmount(close)
</script>


<style lang="stylus" scoped>

  optionRadius = 3px
  menuPadding = 4px

  .combo-box
    display: inline-flex
    min-width: 68px
    height: 28px
    font-size: 11px
    font-weight: 700

  .trigger
    display: flex
    align-items: center
    justify-content: space-between
    gap: 12px
    width: 100%
    min-width: 0
    height: 100%
    border-radius: 3px
    font-size: inherit
    font-weight: inherit

    &:focus
      border-color: transparent

    &:disabled
      opacity: 0.5

  .value
    overflow: hidden
    white-space: nowrap
    text-overflow: ellipsis

  .expander
    flex-shrink: 0
    font-size: 12px
    transition: transform 0.15s

    &.open
      transform: rotate(180deg)

  .combo-menu
    position: fixed
    z-index: 10000
    box-sizing: border-box
    overflow: auto
    padding: menuPadding
    border-radius: optionRadius + menuPadding + 1px
    color: $bright1
    font-size: 11px
    font-weight: 600
    // Teleported outside #app, so restore its defaults
    user-select: none
    cursor: default

  .combo-option
    display: flex
    align-items: center
    justify-content: space-between
    gap: 16px
    min-height: 30px
    padding: 0 8px
    border-radius: optionRadius

    &.selected
      color: white

    &:hover
    &.active
      color: white
      background: $highlight

    svg
      flex-shrink: 0
</style>
