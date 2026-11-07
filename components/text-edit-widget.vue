<template lang="pug">

  TransitionGroup(name="hide-point-widget")
    .point-widget.text-edit-widget(
      v-for="text in texts"
      :key="text.id"
      :style="placement(text)"
      @pointerdown.stop
      @wheel.prevent="emit('wheel', $event)"
    )
      svg.shape(viewBox="0 0 30 36")
        path.outline(:d="shape")
        path.fill(:d="shape")

      button(title="Edit text" @click="emit('edit', text)")
        Icon(icon="font")

</template>


<style lang="stylus" scoped>
  @import '../styles/point-widget.styl'

  // Below the handle, leaving the text above it uncovered
  .text-edit-widget
    width: 30px
    height: 36px
    translate: -50% 10px

  button
    top: 8px
    left: 2px
    color: bright1()

</style>


<script setup>

  import { worldTransform } from '../js/core/assembly.js'
  import { SketchText } from '../js/core/geom2d.js'

  // Offers to edit selected texts of the active sketch, below their handles
  const props = defineProps(['document', 'editing'])
  const emit = defineEmits(['edit', 'wheel'])

  const frame = inject('frame')

  // A circle around the button, with the arrow above it
  const shape = 'M21 8.91A13.5 13.5 0 1 1 9 8.91L15 1.91Z'

  const texts = computed(() => props.document.selection.items.filter(item =>
    item instanceof SketchText &&
    props.document.activeSketch.elements.includes(item) &&
    toRaw(item) != toRaw(props.editing)
  ))

  function placement(text) {
    frame.value
    const plane = worldTransform(props.document.activeComponent).multiply(text.sketch.workplane)
    const { x, y } = window.alcRenderer.toScreen(text.position.clone().applyMatrix4(plane))
    return { left: x + 'px', top: y + 'px' }
  }

</script>
