<template lang="pug">

  //- Sketch constraint proxy
  Icon.constraint(
    v-if="!dimension"
    v-for="proxy in projectedProxies"
    :icon="(constraint.icon && constraint.icon()) ||constraint.constructor.icon"
    :class="{ selected }"
    :style="{ top: proxy.coords.y + 'px', left: proxy.coords.x + 'px' }"
    @click.stop="select"
  )

  //- Sketch dimension
  .dimension(
    v-else
    :class="{ selected, preview }"
    :style="{ top: projectedProxies[0].coords.y + 'px', left: projectedProxies[0].coords.x + 'px' }"
    @click.stop="select"
    @dblclick="constraint.active = true"
  )

    .dim-value(
      @mouseup="$emit('dimensionMouseUp', $event, constraint)"
      @mousedown="$emit('dimensionMouseDown', $event, constraint)"
      @mousemove="$emit('dimensionMouseMove', $event, constraint)"
    ) {{ constraint.distance.toFixed(2) }}

    Transition(name="hide-dimension")

      NumberInput(
        v-if="constraint.active"
        :component="document.top()"
        v-model:value="constraint.distance"
        @enter="constraint.active = false; constraint.sketch.profileUpdateNeeded = true"
      )

</template>


<style lang="stylus" scoped>

  .constraint
    pointer-events: auto
    position: absolute
    margin-left: -9px
    margin-top: -9px
    border-radius: 99px
    padding: 2px
    width: 16px
    height: 16px
    transition: background 0.1s
    color: #1c2127
    background: $bright1
    box-shadow: 0 1px 3px rgba(black, 0.5)

    &:hover
    &.selected
      color: white

    &.selected
      background: $highlight

  .dimension
    position: absolute
    transition: color 0.1s

    &.preview
      pointer-events: none

      .dim-value
        pointer-events: none

    &.selected .dim-value
      border-color: $highlight
      background: lighten($highlight, 73%) !important

    > *
      position: absolute

    .dim-value
      margin-top: -12px
      margin-left: -28px
      background: $bright2
      padding: 0.25rem 0.5rem
      border-radius: 99px
      font-size: 0.9rem
      font-weight: bold
      color: $dark2
      border: 2px solid $bright1
      transition: all 0.1s
      cursor: grab
      pointer-events: auto

      &:hover
        background: $bright1

    .number-input
      margin-top: -14px
      margin-left: -60px

  .hide-dimension-enter-active
  .hide-dimension-leave-active
    transition: all 0.2s

  .hide-dimension-enter-from
  .hide-dimension-leave-to
    opacity: 0
    transform: scale(90%)

</style>


<script setup>

  import { Dimension, CoincidentConstraint } from '../js/core/sketch.js'
  import DimensionControls from '../js/three/dimension-controls.js'

  const props = defineProps({
    document: Object,
    constraint: Object,
    preview: Boolean,
  })
  const emit = defineEmits(['dimensionMouseUp', 'dimensionMouseDown', 'dimensionMouseMove'])

  const bus = inject('bus')
  const frame = inject('frame')
  const renderNeeded = inject('render-needed')

  const selected = computed(() => props.document.selection.has(props.constraint) )

  let _dimension

  const dimension = computed(() => {
    if(!(props.constraint instanceof Dimension)) return
    window.alcRenderer.remove(_dimension)
    _dimension = new DimensionControls(props.constraint, window.alcRenderer)
    window.alcRenderer.add(_dimension)
    renderNeeded.value = true
    return _dimension
  })

  const proxies = computed(() => {
    if(props.constraint instanceof CoincidentConstraint) return []

    return props.constraint.items.map((item, i) => {
      const curve = item.curve()
      return {
        constraint: props.constraint,
        curve,
        pos: ((props.constraint.position && props.constraint.position.clone()) || (props.constraint.items.length == 1 ?
          curve.center()
          :
          curve.center().clone()
            .add(curve.commonHandle(props.constraint.items[1 - i].curve()) || props.constraint.items[1 - i].curve().center())
            .divideScalar(2.0)
        )).applyMatrix4(curve.sketch.workplane),
      }
    })
  })

  const projectedProxies = computed(() => {
    frame.value
    return proxies.value.map(proxy => ({ ...proxy, coords: window.alcRenderer.toScreen(proxy.pos) }))
  })

  function select() {
    props.document.selection.handle(props.constraint, bus.isCtrlPressed)
  }

  onUnmounted(() => {
    window.alcRenderer.remove(_dimension)
    renderNeeded.value = true
  })

</script>
