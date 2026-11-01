<template lang="pug">

  .combo-box(@pointerdown.stop @click.stop @keydown.stop="onKeydown")

    button.bordered.trigger(
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

    PopupMenu(
      v-if="open"
      :id="menuId"
      :anchor="trigger"
      :label="label"
      :options="options"
      :value="modelValue"
      :active-index="activeIndex"
      @choose="choose(options.findIndex(option => option.value === $event))"
      @hover="activeIndex = -1"
      @close="close"
    )

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
  const open = ref(false)
  // Option highlighted by keyboard navigation, the mouse uses :hover
  const activeIndex = ref(-1)
  const selectedIndex = computed(() => props.options.findIndex(option => !option.heading && option.value === props.modelValue))
  // Indices of the options that can be chosen, without section headings
  const choosable = computed(() => props.options.flatMap((option, index) => option.heading ? [] : [index]))
  const selected = computed(() => props.options[selectedIndex.value])
  let search = ''
  let searchTimer

  function show() {
    if(props.disabled || !choosable.value.length) return
    activeIndex.value = -1
    open.value = true
  }

  function close() {
    open.value = false
    search = ''
    clearTimeout(searchTimer)
  }

  function choose(index) {
    emit('update:modelValue', props.options[index].value)
    close()
    trigger.value.focus()
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
      else if(event.key === 'Home') activeIndex.value = choosable.value[0]
      else if(event.key === 'End') activeIndex.value = choosable.value.at(-1)
      else {
        // Continue from the selected option when nothing is highlighted yet
        const position = choosable.value.indexOf(activeIndex.value >= 0 ? activeIndex.value : selectedIndex.value)
        const step = event.key === 'ArrowDown' ? 1 : -1
        const count = choosable.value.length
        activeIndex.value = choosable.value[position < 0 ? (step > 0 ? 0 : count - 1) : (position + step + count) % count]
      }
    } else if(event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault()
      if(!open.value) show()
      search += event.key.toLowerCase()
      clearTimeout(searchTimer)
      searchTimer = setTimeout(() => search = '', 700)
      const index = choosable.value.find(index => props.options[index].label.toLowerCase().startsWith(search))
      if(index !== undefined) activeIndex.value = index
    }
  }

  onBeforeUnmount(close)
</script>


<style lang="stylus" scoped>

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
    padding: 0 8px 0 10px
    color: $bright1
    font-size: inherit
    font-weight: inherit
    transition: background 0.25s

    &:hover
    &:focus-visible
    &[aria-expanded="true"]
      color: white
      background: $dark1
      transition: none

    &:active
      background: $dark1 * 0.9

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
</style>
