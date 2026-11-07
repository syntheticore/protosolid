<template lang="pug">

  Transition(name="hide-point-widget")
    .point-widget.restart-widget(
      v-if="pos"
      :style="{ top: pos.y + 'px', left: pos.x + 'px' }"
      @pointerdown.stop
      @wheel.prevent="emit('wheel', $event)"
    )
      svg.shape(viewBox="0 0 58 37")
        path.outline(:d="shape")
        path.fill(:d="shape")

      button.confirm(title="Start new chain" @click="emit('restart')")
        Icon(icon="check")

      button.cancel(title="Exit tool" @click="emit('cancel')")
        Icon(icon="times")

</template>


<style lang="stylus" scoped>
  @import '../styles/point-widget.styl'

  .restart-widget
    width: 58px
    height: 37px

  .confirm
    left: 2px
    color: var(--confirm)

  .cancel
    left: 30px
    color: var(--cancel)

</style>


<script setup>

  import { worldTransform } from '../js/core/assembly.js'

  // Offers to end the line or spline being drawn, at its last placed point
  const props = defineProps(['document', 'tool'])
  const emit = defineEmits(['restart', 'cancel', 'wheel'])

  const frame = inject('frame')

  const shape = 'M22 28.5H15A13.5 13.5 0 0 1 15 1.5H43A13.5 13.5 0 0 1 43 28.5H36L29 35.5Z'

  const target = computed(() => props.tool?.restartPoint()?.clone() || null)
  const point = shallowRef(null)
  let animation

  // Glide between placed points in sketch space,
  // so the widget still follows camera changes exactly
  watch(target, (target, previous) => {
    const start = point.value
    if(!target || !previous || !start) {
      cancelAnimationFrame(animation)
      point.value = target
      return
    }
    if(target.equals(previous)) return
    cancelAnimationFrame(animation)
    const startTime = performance.now()
    const step = time => {
      const t = Math.min((time - startTime) / 200, 1)
      point.value = start.clone().lerp(target, 1 - (1 - t) ** 3)
      if(t < 1) animation = requestAnimationFrame(step)
    }
    animation = requestAnimationFrame(step)
  }, { immediate: true })

  const pos = computed(() => {
    frame.value
    if(!point.value) return null
    const plane = worldTransform(props.document.activeComponent).multiply(props.document.activeSketch.workplane)
    return window.alcRenderer.toScreen(point.value.clone().applyMatrix4(plane))
  })

  onBeforeUnmount(() => cancelAnimationFrame(animation))

</script>
