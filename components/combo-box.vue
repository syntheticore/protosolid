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
      :aria-activedescendant="open ? `${menuId}-${activeIndex}` : undefined"
      :disabled="disabled"
      @click="open ? close() : show()"
      @blur="close"
    )

      span.value {{ selected?.label || placeholder }}

      Icon.expander(icon="angle-down")

    Teleport(to="body")

      .combo-menu.bordered(
        v-if="open"
        ref="menu"
        :id="menuId"
        role="listbox"
        :aria-label="label"
        :style="menuStyle"
        @pointerdown.prevent.stop
        @click.stop
      )

        .combo-option(
          v-for="(option, index) in options"
          :key="index"
          :id="`${menuId}-${index}`"
          role="option"
          :aria-selected="option.value === modelValue"
          :class="{ active: index === activeIndex, selected: option.value === modelValue }"
          @pointermove="activeIndex = index"
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
  const activeIndex = ref(0)
  const menuStyle = ref({})
  const selected = computed(() => props.options.find(option => option.value === props.modelValue))
  let search = ''
  let searchTimer

  async function show() {
    if(props.disabled || !props.options.length) return
    activeIndex.value = Math.max(0, props.options.findIndex(option => option.value === props.modelValue))
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
    nextTick(() => menu.value?.children[activeIndex.value]?.scrollIntoView({ block: 'nearest' }))
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
      if(open.value) choose(activeIndex.value)
      else show()
    } else if(['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault()
      if(!open.value) show()
      else if(event.key === 'Home') activeIndex.value = 0
      else if(event.key === 'End') activeIndex.value = props.options.length - 1
      else activeIndex.value = (activeIndex.value + (event.key === 'ArrowDown' ? 1 : -1) + props.options.length) % props.options.length
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
    font-size: inherit
    font-weight: inherit
    cursor: pointer

    &:disabled
      opacity: 0.5

  .value
    overflow: hidden
    white-space: nowrap
    text-overflow: ellipsis

  .expander
    flex-shrink: 0
    font-size: 10px

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

  .combo-option
    display: flex
    align-items: center
    justify-content: space-between
    gap: 16px
    min-height: 30px
    padding: 0 8px
    border-radius: optionRadius
    cursor: pointer

    &.selected
      color: white

    &.active
      color: white
      background: $highlight

    svg
      flex-shrink: 0
</style>
