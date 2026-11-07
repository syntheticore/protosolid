<template lang="pug">

  .sketch-text-input(
    ref="root"
    tabindex="-1"
    :style="{ left: Math.round(position.x) + 'px', top: Math.round(position.y) + 'px' }"
    @focusout="onFocusOut"
    @keydown.stop="onKeydown"
    @keyup="$event.key.length == 1 && $event.stopPropagation()"
  )

    .panel.bordered.tipped

      input.input(
        ref="input"
        type="text"
        spellcheck="false"
        placeholder="Text"
        :title="font ? '' : `${text.fontFamily} ${text.fontStyle} is not available`"
        :disabled="!font"
        :size="Math.max(16, text.content.length)"
        :value="text.content"
        @input="setContent($event.target.value)"
      )

      ExpressionInput(
        title="Text size"
        :value="text.size"
        @update:value="setSize"
      )

      ComboBox.family(
        label="Font"
        :disabled="!fonts.length"
        :model-value="text.fontFamily"
        :options="options(families)"
        @update:model-value="setFamily"
      )

      ComboBox(
        label="Font style"
        :disabled="!fonts.length"
        :model-value="text.fontStyle"
        :options="options(styles)"
        @update:model-value="setFont(text.fontFamily, $event)"
      )

      button.button(
        type="button"
        tabindex="-1"
        title="Convert to sketch curves"
        :disabled="!text.pieces.length"
        @mousedown.prevent
        @click="materialize"
      )
        Icon(icon="bezier-curve")

</template>


<style lang="stylus" scoped>

  // Controls and box keep the same distance all around, including at the corners
  radius = 3px
  spacing = 4px

  .sketch-text-input
    position: absolute
    pointer-events: auto
    outline: none

  // The tip of the arrow points at the start of the baseline, keeping the text above it visible
  .panel
    display: flex
    gap: spacing
    width: max-content
    margin-top: 10px
    margin-left: -23px
    padding: spacing
    border-radius: radius + spacing

  input
    margin: 0
    font-size: 12px
    border-radius: radius

  // Sets the height of the other controls
  .combo-box
    :deep(.trigger)
      border-radius: radius

    &.family
      max-width: 160px

  .button
    margin: 0
    padding: 0 9px
    border-radius: radius

</style>


<script setup>

  import { findFont, loadSystemFont, rememberFont, textPieces } from '../js/core/sketch-text.js'

  const props = defineProps(['text', 'created', 'fonts', 'transform'])
  const emit = defineEmits(['close', 'remove', 'materialize'])

  const frame = inject('frame')
  const root = ref()
  const input = ref()
  const font = shallowRef(null)
  const original = {
    content: props.text.content,
    pieces: props.text.pieces,
    size: props.text.size,
    family: props.text.fontFamily,
    style: props.text.fontStyle,
  }
  let closed = false

  const position = computed(() => {
    frame.value
    const point = props.text.position.clone().applyMatrix4(props.text.sketch.workplane)
    if(props.transform) point.applyMatrix4(props.transform)
    return window.alcRenderer.toScreen(point)
  })

  // A font missing from this system stays listed, as it describes the current outline
  const unique = values => [...new Set(values)].sort()
  const options = values => values.map(value => ({ value, label: value }))
  const stylesOf = family => props.fonts.filter(font => font.family == family).map(font => font.style)
  const families = computed(() => unique([props.text.fontFamily, ...props.fonts.map(font => font.family)]))
  const styles = computed(() => unique([props.text.fontStyle, ...stylesOf(props.text.fontFamily)]))

  onMounted(async () => {
    await loadFont()
    await nextTick()
    if(font.value) {
      input.value.focus()
      input.value.select()
    } else {
      root.value.focus()
    }
  })

  async function loadFont() {
    font.value = null
    const data = findFont(props.fonts, props.text.fontFamily, props.text.fontStyle)
    if(!data) return
    try {
      font.value = await loadSystemFont(data)
    } catch {
      window.bus.emit('toast', `The font ${data.fullName} cannot be read`)
    }
  }

  async function setFont(family, style) {
    props.text.setFont(family, style)
    await loadFont()
    if(!font.value) return
    rememberFont(family, style)
    setContent(props.text.content)
  }

  function setFamily(family) {
    const available = stylesOf(family)
    const style = [props.text.fontStyle, 'Regular'].find(style => available.includes(style))
    setFont(family, style || available[0])
  }

  function setContent(content) {
    props.text.setOutline(content, textPieces(font.value, content))
  }

  function setSize(size) {
    if(size > 0) props.text.setSize(size)
  }

  function close(event) {
    if(closed) return
    closed = true
    emit(event)
  }

  function finish() {
    close(props.text.pieces.length ? 'close' : 'remove')
  }

  function cancel() {
    if(props.created) return close('remove')
    props.text.setFont(original.family, original.style)
    props.text.setOutline(original.content, original.pieces)
    props.text.setSize(original.size)
    close('close')
  }

  function materialize() {
    close('materialize')
  }

  // The size input passes Enter on only once its value is committed, like in feature boxes
  function onKeydown(event) {
    if(event.key == 'Enter') finish()
    else if(event.key == 'Escape') cancel()
  }

  // Waits for focus to arrive. Switching to another window keeps the active element.
  function onFocusOut() {
    setTimeout(() => {
      if(!closed && !root.value.contains(document.activeElement)) finish()
    })
  }

</script>
