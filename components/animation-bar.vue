<template lang="pug">

  footer.animation-bar.bordered(v-pixel-snap :style="{ '--rows': Math.max(1, rows.length) }")

    .labels

      nav.controls

        button(title="Start" @click="player.pause(); player.seek(0)")
          Icon(icon="angle-double-left")

        button(title="Previous keyframe" @click="player.step(-1)")
          Icon(icon="angle-left")

        button(:title="player.playing ? 'Pause' : 'Play animation'" @click="player.toggle()")
          Icon(:icon="player.playing ? 'pause' : 'play'" fixed-width)

        button(title="Next keyframe" @click="player.step(1)")
          Icon(icon="angle-right")

        button(title="End" @click="player.pause(); player.seek(player.duration)")
          Icon(icon="angle-double-right")

      .label(
        v-for="row in rows"
        :key="row.componentId"
        :class="{ offered: !row.track, moved: player.posed.includes(row.componentId) }"
        :title="row.track ? title(row) : `${title(row)} was moved. Set a keyframe to animate it.`"
      )
        span.name {{ title(row) }}
        template(v-if="row.track")
          span.count(title="Keyframes") {{ row.track.keyframes.length }}
          Icon.remove(icon="trash-alt" fixed-width title="Delete track" @click="removeTrack(row.track)")
        Icon.remove(v-else icon="trash-alt" fixed-width title="Put back where it was" @click="player.discard(row.componentId)")

    .lanes(
      @pointerdown="startScrub"
      @pointermove="scrub"
      @pointerup="scrubbing = false"
      @pointercancel="scrubbing = false"
    )

      .area(ref="area")

        .ruler
          .tick(v-for="second in ticks" :key="second" :style="{ left: percent(second) }")
            span {{ second }}

        .lane(v-for="row in rows" :key="row.componentId" :class="{ offered: !row.track }")
          .keyframe(
            v-for="keyframe in row.track?.keyframes || []"
            :style="{ left: percent(keyframe.time) }"
            :class="[keyframe.easing || 'linear', { current: Math.abs(keyframe.time - player.time) < 1e-3, selected: selected?.keyframe === keyframe }]"
            :title="`${easingTitles[keyframe.easing || 'linear']}. Double-click to change.`"
            @pointerdown.stop="startDrag(row.track, keyframe, $event)"
            @pointermove.stop="drag"
            @pointerup.stop="endDrag"
            @dblclick.stop="cycleEasing(keyframe)"
          )

        .hint(v-if="!rows.length") Move components in the viewport to animate them

        .playhead(:style="{ left: percent(player.time) }")

    .side

      .time
        span {{ formatDuration(player.time) }}
        span.total &nbsp;/ {{ formatDuration(player.duration) }}

      .actions

        button(
          :title="player.posed.length ? 'Keyframe moved components' : 'Keyframe all tracks at this time'"
          :disabled="!rows.length"
          @click="keyframe"
        )
          Icon(icon="key" fixed-width)

</template>


<style lang="stylus" scoped>

  $header = 36px
  $row = 24px

  // Grows and shrinks with the tracks, at least the placeholder's row
  .animation-bar
    font-size: 13px
    color: bright2()
    display: flex
    width: 760px
    height: s('calc(%s + var(--rows) * %s + 2px)', $header, $row)
    overflow: hidden
    transition: height 0.15s ease-out
    user-select: none

  button
    background: none
    border: none
    color: bright2()
    transition: all 0.15s

    &:hover
      background: dark1()
      transition: none

    &:active
      background: dark2(0.85)

    &:disabled
      opacity: 0.3

  .labels
    flex: 0 0 170px
    border-right: 1px solid dark1()

  .controls
    display: flex
    height: $header
    border-bottom: 1px solid dark1()

    button
      flex: 1 1 0
      padding: 0

      &:first-child
        border-top-left-radius: 4px

  .label
    height: $row
    display: flex
    align-items: center
    gap: 6px
    padding: 0 8px 0 10px
    white-space: nowrap
    color: bright1()

    .name
      flex: 1 1 auto
      min-width: 0
      overflow: hidden
      text-overflow: ellipsis

    &.offered
      color: bright2()
      font-style: italic

    // Not keyframed yet
    &.moved .name::after
      content: ' •'
      color: $highlight

    .count
      font-size: 11px
      color: bright2()
      font-variant-numeric: tabular-nums

    .remove
      color: bright2()
      cursor: pointer

      &:hover
        color: var(--cancel)

  .lanes
    flex: 1 1 auto
    min-width: 0
    padding: 0 12px
    cursor: ew-resize

  .area
    position: relative
    height: 100%

  .ruler
    position: relative
    height: $header
    border-bottom: 1px solid dark1()

    .tick
      position: absolute
      bottom: 0
      height: 6px
      border-left: 1px solid bright2(1, 0.5)

      span
        position: absolute
        bottom: 8px
        transform: translateX(-50%)
        font-size: 11px
        font-variant-numeric: tabular-nums

  // Dividers only between rows, within the row height to line up with the labels
  .lane
    position: relative
    height: $row
    box-sizing: border-box

    & + .lane
      border-top: 1px solid dark1(1, 0.5)

    & + .lane.offered
      border-top-style: dashed

  .keyframe
    position: absolute
    top: 50%
    width: 9px
    height: 9px
    margin: -5px 0 0 -5px
    transform: rotate(45deg)
    background: bright2()
    cursor: grab

    // Grabbable beyond the small shape, about as tall as the row
    &::before
      content: ''
      position: absolute
      inset: -7px

    &:hover
      background: bright1()

    &.current
      background: $highlight

    &.selected
      background: bright1()
      box-shadow: 0 0 0 2px $highlight

    // Linear keyframes are diamonds, smooth ones round, stepping ones square
    &.smooth
      width: 10px
      height: 10px
      border-radius: 50%
      transform: none

    &.step
      width: 8px
      height: 8px
      margin: -4px 0 0 -4px
      transform: none

  // As tall as a track, so the first one doesn't change the height
  .hint
    height: $row
    line-height: $row
    overflow: hidden
    white-space: nowrap
    text-overflow: ellipsis
    color: bright2()
    cursor: default

  .playhead
    position: absolute
    top: 0
    bottom: 0
    width: 2px
    margin-left: -1px
    background: $highlight
    pointer-events: none

  .side
    flex: 0 0 110px
    display: flex
    flex-direction: column
    border-left: 1px solid dark1()

  .time
    height: $header
    display: flex
    align-items: center
    justify-content: center
    border-bottom: 1px solid dark1()
    font-variant-numeric: tabular-nums
    color: bright1()

    .total
      color: bright2()

  // As tall as the track rows, not taller
  .actions
    flex: 1 1 auto
    min-height: $row
    display: flex

    button
      flex: 1 1 0
      padding: 0

      &:last-child
        border-bottom-right-radius: 4px

</style>


<script setup>

  import { formatDuration } from '../js/animation-player.js'

  const props = defineProps(['document', 'player'])
  const bus = inject('bus')

  const area = ref(null)
  const scrubbing = ref(false)
  // { track, keyframe }
  const selected = ref(null)
  let dragging = null

  const animation = computed(() => props.player.animation)

  // Tracks, then moved components that could get one
  const rows = computed(() => [
    ...animation.value.tracks.map(track => ({ componentId: track.componentId, track })),
    ...props.player.offered.map(componentId => ({ componentId, track: null })),
  ])

  // At most six labeled steps
  const ticks = computed(() => {
    const duration = props.player.duration
    const step = [0.5, 1, 2, 5, 10, 30, 60].find(step => duration / step <= 6) || 120
    return Array.from({ length: Math.floor(duration / step + 1e-6) + 1 }, (_, index) => index * step)
  })

  function title(row) {
    return props.document.top().findChild(row.componentId)?.creator.title || 'Missing component'
  }

  function percent(time) {
    return `${time / props.player.duration * 100}%`
  }

  // Scrubbing and keyframes snap to tenths of a second
  function timeAt(x) {
    const { left, width } = area.value.getBoundingClientRect()
    return Math.round(Math.min(1, Math.max(0, (x - left) / width)) * props.player.duration * 10) / 10
  }

  // Keyframes and the document share one selection, so Delete removes only one of them
  watch(() => props.document.selection.items.length, count => {
    if(count) selected.value = null
  })

  onMounted(() => {
    bus.on('keydown', keydown)
    bus.on('escape', deselect)
  })

  onUnmounted(() => {
    bus.off('keydown', keydown)
    bus.off('escape', deselect)
  })

  function deselect() {
    selected.value = null
  }

  function keydown(key) {
    if(key != 'Delete' && key != 'Backspace' || !selected.value) return
    const { track, keyframe } = selected.value
    if(track.keyframes.includes(keyframe)) removeKeyframe(track, keyframe)
    selected.value = null
  }

  function startScrub(event) {
    selected.value = null
    scrubbing.value = true
    event.currentTarget.setPointerCapture(event.pointerId)
    props.player.pause()
    props.player.seek(timeAt(event.clientX))
  }

  function scrub(event) {
    if(scrubbing.value) props.player.seek(timeAt(event.clientX))
  }

  function startDrag(track, keyframe, event) {
    event.currentTarget.setPointerCapture(event.pointerId)
    props.player.pause()
    dragging = { track, keyframe, x: event.clientX, moved: false }
    selected.value = { track, keyframe }
    props.document.selection.clear()
  }

  function drag(event) {
    if(!dragging) return
    if(Math.abs(event.clientX - dragging.x) > 3) dragging.moved = true
    if(dragging.moved) props.player.moveKeyframe(dragging.track, dragging.keyframe, timeAt(event.clientX))
  }

  // A click jumps to the keyframe
  function endDrag() {
    if(!dragging) return
    if(dragging.moved) props.document.markChanged()
    else props.player.seek(dragging.keyframe.time)
    dragging = null
  }

  function removeTrack(track) {
    props.player.removeTrack(track)
    props.document.markChanged()
  }

  function removeKeyframe(track, keyframe) {
    props.player.removeKeyframe(track, keyframe)
    props.document.markChanged()
  }

  const easingTitles = {
    linear: 'Linear',
    smooth: 'Smooth',
    step: 'Step',
  }

  function cycleEasing(keyframe) {
    props.player.cycleEasing(keyframe)
    props.document.markChanged()
  }

  function keyframe() {
    props.player.keyframe()
    props.document.markChanged()
  }

</script>
