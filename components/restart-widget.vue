<template lang="pug">

  Transition(name="hide-restart")
    button.restart-widget(
      v-if="pos"
      title="Start new chain"
      :style="{ top: pos.y + 'px', left: pos.x + 'px' }"
      @pointerdown.stop
      @wheel.prevent="emit('wheel', $event)"
      @click="emit('restart')"
    )
      //- Circle and arrow as one outline, so border and hover fill are seamless
      svg.shape(viewBox="0 0 30 37")
        path.outline(:d="shape")
        path.fill(:d="shape")
      span.icon
        Icon(icon="check")

</template>


<style lang="stylus" scoped>

  .restart-widget
    pointer-events: auto
    position: absolute
    width: 30px
    height: 37px
    margin: 0
    padding: 0
    border: none
    background: none
    color: $confirm
    font-size: 12px
    // Centered above the point, the arrow tip clear of its handle.
    // Uses translate, leaving transform to the hide transition.
    translate: -50% calc(-100% - 10px)

    .shape
      position: absolute
      left: 0
      top: 0
      width: 100%
      height: 100%
      overflow: visible

    .outline
      fill: none
      stroke: $dark2 * 0.5
      stroke-width: 3
      stroke-linejoin: round

    .fill
      fill: $dark2
      stroke: $dark1 * 1.1
      stroke-width: 1
      stroke-linejoin: round

    &:hover .fill
      fill: $dark1

    &:active .fill
      fill: $dark2 * 0.85

    .icon
      position: absolute
      left: 0
      top: 0
      width: 30px
      height: 30px
      display: flex
      align-items: center
      justify-content: center

  .hide-restart-enter-active
  .hide-restart-leave-active
    transition: opacity 0.2s, transform 0.2s

  .hide-restart-enter-from
  .hide-restart-leave-to
    opacity: 0
    transform: translateY(-6px)

</style>


<script setup>

  import { worldTransform } from '../js/core/assembly.js'

  // Offers to end the line or spline being drawn, at its last placed point
  const props = defineProps(['document', 'tool'])
  const emit = defineEmits(['restart', 'wheel'])

  const frame = inject('frame')

  const shape = 'M8 26.54A13.5 13.5 0 1 1 22 26.54L15 35.5Z'

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
