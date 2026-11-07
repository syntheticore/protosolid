<template lang="pug">

  //- Submenus stay within their parent, so clicks in them are inside the menu
  Teleport(to="body" :disabled="nested")

    .popup-menu.bordered(
      ref="menu"
      role="listbox"
      :id="id"
      :aria-label="label"
      :style="style"
      @pointerdown.prevent.stop
      @pointermove="$emit('hover')"
      @click.stop
    )

      template(v-for="(option, index) in options" :key="index")

        .popup-heading(v-if="option.heading" ref="rows" role="presentation") {{ option.heading }}

        .popup-option(
          v-else
          :id="id && `${id}-${index}`"
          ref="rows"
          role="option"
          :aria-selected="isSelected(option)"
          :class="{ active: index === activeIndex || index === openIndex, selected: isSelected(option) }"
          @pointerenter="openIndex = option.children ? index : -1"
          @click="option.children ? openIndex = index : $emit('choose', option.value)"
        )

          span {{ option.label }}

          span.detail(v-if="option.detail") {{ option.detail }}

          Icon(v-if="option.children" icon="angle-right")

          Icon(v-else-if="option.value === value" icon="check")

      PopupMenu(
        v-if="openIndex >= 0"
        nested
        :key="openIndex"
        :anchor="rows[openIndex]"
        :options="options[openIndex].children"
        :value="value"
        @choose="$emit('choose', $event)"
      )

</template>


<script setup>

  const props = defineProps({
    // Element the menu opens from, clicks on it are left to its owner
    anchor: Object,
    // As [{ value, label, detail, children }], where options with children open a
    // submenu. Entries as { heading } start a section.
    options: Array,
    value: {},
    // Option highlighted by keyboard navigation of the owner
    activeIndex: { type: Number, default: -1 },
    id: String,
    label: String,
    // Opens beside an option of a parent menu
    nested: Boolean,
  })
  const emit = defineEmits(['choose', 'close', 'hover'])
  const menu = ref(null)
  const rows = ref([])
  const openIndex = ref(-1)
  const style = ref({ visibility: 'hidden' })
  let upwards = false

  // Slides in from the anchor, like the transitions of dialogs and panels
  const timing = { duration: 150, easing: 'ease-out' }
  const hidden = () => ({ opacity: 0, transform: `translateY(${upwards ? 6 : -6}px)` })
  const shown = { opacity: 1, transform: 'none' }

  function isSelected(option) {
    if(option.heading) return false
    return option.children ? option.children.some(isSelected) : option.value === props.value
  }

  function place() {
    const rect = props.anchor.getBoundingClientRect()
    const width = menu.value.offsetWidth
    const height = menu.value.scrollHeight
    if(props.nested) {
      // Beside the option, on the left if there is no room on the right
      const right = rect.right + width + 8 <= window.innerWidth
      style.value = {
        top: `${Math.max(8, Math.min(rect.top - 1, window.innerHeight - height - 8))}px`,
        left: `${right ? rect.right + 5 : rect.left - width - 5}px`,
        maxHeight: `${window.innerHeight - 16}px`,
      }
      return
    }
    const below = window.innerHeight - rect.bottom - 8
    const above = rect.top - 8
    upwards = below < Math.min(height, 240) && above > below
    style.value = {
      minWidth: `${rect.width}px`,
      maxWidth: `${window.innerWidth - 16}px`,
      maxHeight: `${Math.min(240, upwards ? above : below)}px`,
      top: upwards ? undefined : `${rect.bottom + 4}px`,
      bottom: upwards ? `${window.innerHeight - rect.top + 4}px` : undefined,
      left: `${Math.max(8, Math.min(rect.left, window.innerWidth - width - 8))}px`,
    }
  }

  function reveal() {
    const index = props.activeIndex >= 0 ? props.activeIndex : props.options.findIndex(isSelected)
    rows.value[index]?.scrollIntoView({ block: 'nearest' })
  }

  watch(() => props.activeIndex, () => nextTick(reveal))

  function outside(event) {
    if(!menu.value.contains(event.target) && !props.anchor.contains(event.target)) emit('close')
  }

  function keydown(event) {
    if(event.key != 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    emit('close')
  }

  function scroll(event) {
    if(!menu.value.contains(event.target)) emit('close')
  }

  function close() {
    emit('close')
  }

  onMounted(() => {
    place()
    nextTick(reveal)
    if(props.nested) return
    menu.value.animate([hidden(), shown], timing)
    window.addEventListener('pointerdown', outside, true)
    window.addEventListener('keydown', keydown, true)
    window.addEventListener('resize', close)
    window.addEventListener('scroll', scroll, true)
  })

  // Owners remove the menu at once, so a copy of it fades out instead
  function fadeOut() {
    const copy = menu.value.cloneNode(true)
    copy.style.pointerEvents = 'none'
    document.body.append(copy)
    copy.scrollTop = menu.value.scrollTop
    copy.animate([shown, hidden()], timing).finished.then(() => copy.remove())
  }

  onBeforeUnmount(() => {
    if(!props.nested) fadeOut()
    window.removeEventListener('pointerdown', outside, true)
    window.removeEventListener('keydown', keydown, true)
    window.removeEventListener('resize', close)
    window.removeEventListener('scroll', scroll, true)
  })
</script>


<style lang="stylus" scoped>

  .popup-menu
    position: fixed
    z-index: 10000
    box-sizing: border-box
    overflow: auto
    // Like the tree view. The track is square, so a colored one would cover the rounded border.
    scrollbar-color: $dark1 * 1.15 transparent
    scrollbar-width: thin
    color: $bright1
    font-size: 11px
    font-weight: 600
    // Teleported outside #app, so restore its defaults
    user-select: none
    cursor: default

    // Highlights follow the pointer instantly and fade out when it leaves
    &:hover .popup-option
      transition: none

  .popup-option
    display: flex
    align-items: center
    gap: 16px
    min-height: 30px
    padding: 0 12px
    white-space: nowrap
    transition: all 0.25s

    > :first-child
      flex: 1 1 auto

    &.selected
      color: white

    &:hover
    &.active
      color: white
      background: $dark1

    &:active
      background: $dark1 * 0.9

    svg
      flex-shrink: 0

  .popup-heading
    padding: 8px 12px 4px
    color: $bright2
    white-space: nowrap

    &:not(:first-child)
      margin-top: 4px
      padding-top: 10px
      border-top: 1px solid $dark1

  .detail
    font-weight: 400
    opacity: 0.75
</style>
