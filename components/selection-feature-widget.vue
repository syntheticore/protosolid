<template lang="pug">
  .selector-widget.selection-feature-widget(
    :style="{ left: position.x + 'px', top: position.y + 'px' }"
    @pointerdown.stop
    @dblclick.stop
  )
    .stacks
      //- Stacks and arrow as one outline, so the border is seamless
      svg.shape
        path.outline(:d="shape")
        path.fill(:d="shape")

      //- Clipped to the silhouette, so hover backgrounds follow its corners
      nav.columns(aria-label="Selection features" :style="{ clipPath: `path('${clip}')` }")
        .column
          button.solid(
            type="button"
            title="Select Solid"
            aria-label="Select Solid"
            :disabled="document.isRegenerating"
            @click="document.selection.only(item.solid)"
          )
            Icon(icon="layer-group" fixed-width)

        //- Each feature with the sketches it consumes stacked below
        .column(v-for="stack in stacks" :key="stack.feature.id")
          button(
            v-for="feature in [stack.feature, ...stack.sketches.map(sketch => sketch.creator)]"
            :key="feature.id"
            type="button"
            :title="feature.title"
            :aria-label="feature.title"
            :disabled="document.isRegenerating || feature.constructor.editable === false"
            @click="document.activateFeature(feature)"
            @mouseenter="highlight = feature"
            @mouseleave="clearHighlight"
          )
            Icon(:icon="feature.constructor.icon" fixed-width)
</template>

<style lang="stylus" scoped>
  .selector-widget.selection-feature-widget
    position: absolute
    // Keep the anchor and arrow transparent to a second click on the geometry.
    pointer-events: none

  // Arrow tip on the anchor
  .stacks
    position: relative
    width: max-content
    translate: -18px 9px

  .shape
    position: absolute
    left: 0
    top: 0
    width: 100%
    height: 100%
    overflow: visible

  .outline
    fill: none
    stroke: var(--ring)
    stroke-width: 3
    stroke-linejoin: round

  .fill
    fill: dark2()
    stroke: var(--rim)
    stroke-width: 1
    stroke-linejoin: round

  .columns
    position: relative
    display: flex
    align-items: flex-start
    pointer-events: auto

  button
    margin: 0
    width: 36px
    height: 24px
    padding: 0
    display: flex
    align-items: center
    justify-content: center
    background: none
    border: none
    border-radius: 0
    color: bright2()
    font-size: 12px
    &:hover
      color: bright1()
      background: dark1()
    &:active
      background: dark2(0.85)
    &:disabled
      opacity: 0.3

  // Extends the hover into the arrow, which the clip cuts to shape.
  // Stays transparent to clicks like the arrow itself.
  .solid
    position: relative
    &::before
      content: ''
      position: absolute
      left: 0
      right: 0
      bottom: 100%
      height: 7px
      background: inherit
      pointer-events: none
</style>

<script setup>
  import { computed, inject, onBeforeUnmount, shallowRef, toRaw, watch } from 'vue'
  import { worldTransform } from '../js/core/assembly.js'
  import { topologyIncludesFeature } from '../js/core/topological-naming.js'

  const props = defineProps(['document', 'item', 'renderer', 'coords'])
  const frame = inject('frame')
  const highlight = inject('highlight')
  const anchor = shallowRef()

  const columnWidth = 36
  const rowHeight = 24
  const radius = 4

  watch(() => props.item, item => {
    const transform = worldTransform(item.solid.component)
    const hit = props.coords && props.renderer.hitTest(props.coords).find(hit =>
      toRaw(hit.object.alcObject) === toRaw(item))
    anchor.value = hit
      ? (hit.pointOnLine || hit.point).clone().applyMatrix4(transform.invert())
      : item.center()
  }, { immediate: true })

  const position = computed(() => {
    frame.value
    return props.renderer.toScreen(anchor.value.clone().applyMatrix4(worldTransform(props.item.solid.component)))
  })

  const stacks = computed(() => props.document.timeline.features
    .filter(feature => topologyIncludesFeature(props.item.topology, feature))
    .toReversed()
    .map(feature => ({
      feature,
      sketches: [...new Set(feature.involvedSketches().map(toRaw))],
    }))
  )

  // Corners of the stacks' union, clockwise from the top left,
  // with the arrow on the top edge
  const corners = computed(() => {
    const heights = [1, ...stacks.value.map(stack => 1 + stack.sketches.length)].map(rows => rows * rowHeight)
    const width = heights.length * columnWidth
    const corners = [[0, 0, radius], [11, 0, 0], [18, -7, 0], [25, 0, 0], [width, 0, radius], [width, heights.at(-1), radius]]
    for(let i = heights.length - 1; i > 0; i--) {
      if(heights[i] == heights[i - 1]) continue
      corners.push([i * columnWidth, heights[i], radius], [i * columnWidth, heights[i - 1], radius])
    }
    corners.push([0, heights[0], radius])
    return corners
  })

  // Rounded outline of the corners, grown outwards by offset
  function outline(corners, offset) {
    return corners.map(([x, y, r], i) => {
      const [px, py] = corners.at(i - 1)
      const [nx, ny] = corners[(i + 1) % corners.length]
      const inLength = Math.hypot(x - px, y - py)
      const outLength = Math.hypot(nx - x, ny - y)
      const [ax, ay] = [(x - px) / inLength, (y - py) / inLength]
      const [bx, by] = [(nx - x) / outLength, (ny - y) / outLength]
      // Outward normals of both edges meet at the mitered corner
      const miter = offset / (1 + ay * by + ax * bx)
      const qx = x + (ay + by) * miter
      const qy = y - (ax + bx) * miter
      // Only convex corners are rounded
      const rounding = r && ax * by - ay * bx > 0 ? r + offset : 0
      const start = `${qx - ax * rounding} ${qy - ay * rounding}`
      if(!rounding) return (i ? 'L' : 'M') + start
      const end = `${qx + bx * rounding} ${qy + by * rounding}`
      return `${i ? 'L' : 'M'}${start}A${rounding} ${rounding} 0 0 1 ${end}`
    }).join('') + 'Z'
  }

  // Border centered half a pixel outside the buttons
  const shape = computed(() => outline(corners.value, 0.5))
  const clip = computed(() => outline(corners.value, 0))

  function clearHighlight() {
    const shown = stacks.value.flatMap(stack => [stack.feature, ...stack.sketches.map(sketch => sketch.creator)])
    if(shown.some(feature => toRaw(feature) === toRaw(highlight.value))) highlight.value = null
  }

  onBeforeUnmount(clearHighlight)
</script>
