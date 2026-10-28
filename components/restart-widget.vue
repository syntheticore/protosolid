<template lang="pug">

  Transition(name="hide-restart")
    .restart-widget(
      v-if="pos"
      :style="{ top: pos.y + 'px', left: pos.x + 'px' }"
      @pointerdown.stop
      @wheel.prevent="emit('wheel', $event)"
    )
      //- Pill and arrow as one outline, so the border is seamless
      svg.shape(viewBox="0 0 58 37")
        path.outline(:d="shape")
        path.fill(:d="shape")

      button.confirm(title="Start new chain" @click="emit('restart')")
        Icon(icon="check")

      button.cancel(title="Exit tool" @click="emit('cancel')")
        Icon(icon="times")

</template>


<style lang="stylus" scoped>

  .restart-widget
    pointer-events: auto
    position: absolute
    width: 58px
    height: 37px
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

    // Concentric with the pill's rounded ends
    button
      position: absolute
      top: 2px
      width: 26px
      height: 26px
      margin: 0
      padding: 0
      display: flex
      align-items: center
      justify-content: center
      border: none
      border-radius: 99px
      background: none

      &:hover
        background: $dark1

      &:active
        background: $dark2 * 0.85

    .confirm
      left: 2px
      color: $confirm

    .cancel
      left: 30px
      color: $cancel

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
