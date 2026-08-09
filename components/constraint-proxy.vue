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
    ) {{ constraint.distance.toFixed(2) }}{{ constraint.isAngular() ? '°' : '' }}

    Transition(name="hide-dimension")

      NumberInput(
        v-if="constraint.active"
        :component="document.top()"
        v-model:value="constraint.distance"
        :focus-on-mount="true"
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
      pointer-events: auto

  .hide-dimension-enter-active
  .hide-dimension-leave-active
    transition: all 0.2s

  .hide-dimension-enter-from
  .hide-dimension-leave-to
    opacity: 0
    transform: scale(90%)

</style>


<script setup>

  import { Dimension, CoincidentConstraint, TouchConstraint } from '../js/core/sketch.js'
  import { Line } from '../js/core/geom2d.js'
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
    const sketch = props.constraint.sketch
    const point = (item, referenced=false) => {
      const curve = item.curve()
      if(referenced && item.index !== undefined && curve.handles) return curve.handles()[item.index]
      return curve.center()
    }

    if(props.constraint instanceof CoincidentConstraint) {
      // Connected line segments read as a single polyline, so a constraint icon
      // at every join would add noise rather than useful information.
      if(props.constraint.items.every(item => item.curve() instanceof Line)) return []
      return [{
        constraint: props.constraint,
        curve: props.constraint.items[0].curve(),
        pos: point(props.constraint.items[0], true).clone().applyMatrix4(sketch.workplane),
        offset: { x: 11, y: -11 },
      }]
    }

    if(props.constraint instanceof TouchConstraint) {
      const item = props.constraint.items.find(item => item.index !== undefined)
      return [{
        constraint: props.constraint,
        curve: item.curve(),
        pos: point(item, true).clone().applyMatrix4(sketch.workplane),
        offset: { x: 11, y: -11 },
      }]
    }

    return props.constraint.items.map((item, i) => {
      const curve = item.curve()
      const other = props.constraint.items[1 - i]
      const otherCurve = other && other.curve()
      const common = otherCurve && otherCurve.handles && curve.commonHandle && curve.commonHandle(otherCurve)
      return {
        constraint: props.constraint,
        curve,
        pos: ((props.constraint.position && props.constraint.position.clone()) || (props.constraint.items.length == 1 ?
          point(item)
          :
          point(item).clone()
            .add(common || point(other))
            .divideScalar(2.0)
        )).applyMatrix4(sketch.workplane),
      }
    })
  })

  const projectedProxies = computed(() => {
    frame.value
    return proxies.value.map(proxy => {
      const coords = window.alcRenderer.toScreen(proxy.pos)
      if(proxy.offset) {
        coords.x += proxy.offset.x
        coords.y += proxy.offset.y
      }
      return { ...proxy, coords }
    })
  })

  function select() {
    props.document.selection.handle(props.constraint, bus.isCtrlPressed)
  }

  onUnmounted(() => {
    window.alcRenderer.remove(_dimension)
    renderNeeded.value = true
  })

</script>
