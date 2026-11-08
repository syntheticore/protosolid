<template lang="pug">

  footer.cam-bar.bordered(v-pixel-snap)

    nav.controls

      button.toolpaths(
        title="Show toolpaths"
        :class="{ active: !player.simulation }"
        :disabled="!player.canPlay"
        @click="player.stop()"
      )
        Icon(icon="route" fixed-width)

      button(title="Start of program" :disabled="!player.canPlay" @click="player.pause(); player.seek(0)")
        Icon(icon="angle-double-left")

      button(title="Previous operation" :disabled="!player.canPlay" @click="player.step(-1)")
        Icon(icon="angle-left")

      button(
        :title="player.playing ? 'Pause' : 'Simulate machining'"
        :disabled="!player.canPlay"
        @click="player.toggle()"
      )
        Icon(:icon="player.playing ? 'pause' : 'play'" fixed-width)

      button(title="Next operation" :disabled="!player.canPlay" @click="player.step(1)")
        Icon(icon="angle-right")

      button(title="End of program" :disabled="!player.canPlay" @click="player.pause(); player.seek(player.duration)")
        Icon(icon="angle-double-right")

    ol.operations(
      ref="track"
      @pointerdown="startScrub"
      @pointermove="scrub"
      @pointerup="scrubbing = false"
      @pointercancel="scrubbing = false"
    )

      li(
        v-for="(operation, index) in operations"
        :key="index"
        :class="{ current: player.simulation && index == player.operationIndex }"
        :style="{ flexGrow: operation.end - operation.start, '--progress': progress(operation) }"
        :title="`${operationTypes[operation.operation.type].label}: ${formatTime(operation.end - operation.start)}`"
      )
        Icon(:icon="operationTypes[operation.operation.type].icon" fixed-width)
        span {{ operationTypes[operation.operation.type].label }}

        .marker(v-if="player.simulation && index == player.operationIndex")

      li.empty(v-if="!operations.length") No operations

    .time

      .values
        //- With as many digits as the total, so scrubbing doesn't shift it
        span {{ formatTime(player.time, player.duration) }}
        span.total &nbsp;/ {{ formatTime(player.duration) }}

      label.speed(title="Simulation speed relative to the real machine")
        RangeSlider(:min="1" :max="playbackSpeeds.length" :step="1" v-model="player.speed")
        span.factor {{ playbackSpeeds[player.speed - 1] }}×

</template>


<style lang="stylus" scoped>

  .cam-bar
    font-size: 13px
    color: bright2()
    display: flex
    width: 760px
    user-select: none

  .controls
    border-right: 1px solid dark1()
    flex: 0 0 auto
    display: flex

    button
      background: none
      border: none
      color: bright2()
      padding: 20px 13px
      transition: all 0.15s

      &:first-child
        border-top-left-radius: 4px
        border-bottom-left-radius: 4px

      &:hover
        background: dark1()
        transition: none

      &:active
        background: dark2(0.85)

      &:disabled
        opacity: 0.3

      // Toolpaths instead of the simulated stock
      &.toolpaths
        border-right: 1px solid dark1()

        &.active
          color: bright1()
          background: dark1()

  .operations
    flex: 1 1 auto
    display: flex
    align-items: stretch
    margin: 0
    padding: 2px
    cursor: ew-resize
    min-width: 0

    li
      position: relative
      flex-basis: 0
      min-width: 34px
      display: flex
      align-items: center
      gap: 6px
      padding: 0 8px
      overflow: hidden
      white-space: nowrap
      border-bottom: 2px solid dark1(1.4)
      background: linear-gradient(to right, bright1(1, 0.07) var(--progress), transparent var(--progress))
      transition: border-color 0.15s

      &:not(:last-child)
        margin-right: 2px

      &.current
        color: bright1()
        border-bottom-color: bright2()

      &.empty
        flex-grow: 1
        cursor: default
        background: none
        border: none

      span
        overflow: hidden
        text-overflow: ellipsis

    .marker
      position: absolute
      top: 0
      bottom: -2px
      left: var(--progress)
      width: 2px
      margin-left: -1px
      background: $highlight
      pointer-events: none

  .time
    flex: 0 0 auto
    width: 170px
    box-sizing: border-box
    display: flex
    flex-direction: column
    justify-content: center
    gap: 5px
    padding: 0 14px
    border-left: 1px solid dark1()
    font-variant-numeric: tabular-nums
    color: bright1()

    .values
      display: flex
      justify-content: center

    .total
      color: bright2()

    .speed
      display: flex
      align-items: center
      gap: 6px
      margin: 0
      font-size: 11px
      color: bright2()

      .range-slider
        flex: 1 1 0
        min-width: 0

      .factor
        min-width: 5ch
        text-align: right

</style>


<script setup>

  import { operationTypes } from '../js/core/cam.js'
  import { playbackSpeeds } from '../js/cam-player.js'

  const props = defineProps(['player'])

  const track = ref(null)
  const scrubbing = ref(false)

  const operations = computed(() => props.player.result?.operations || [])

  function progress(operation) {
    const duration = operation.end - operation.start
    const fraction = duration > 0 ? (props.player.time - operation.start) / duration : 1
    return `${Math.min(1, Math.max(0, props.player.simulation ? fraction : 0)) * 100}%`
  }

  // Padded to the digits of the reference time
  function formatTime(minutes, reference = minutes) {
    const seconds = Math.round(minutes * 60)
    const total = Math.round(reference * 60)
    const pad = (value, digits = 2) => String(value).padStart(digits, '0')
    const digits = value => String(value).length
    const h = Math.floor(seconds / 3600)
    const m = Math.floor(seconds / 60) % 60
    if(total >= 3600) return `${pad(h, digits(Math.floor(total / 3600)))}:${pad(m)}:${pad(seconds % 60)}`
    return `${pad(m, digits(Math.floor(total / 60)))}:${pad(seconds % 60)}`
  }

  // Operations take space by their duration, but at least room for their
  // icon, so time maps to the track piecewise.
  function timeAt(x) {
    const items = [...track.value.querySelectorAll('li')]
    for(const [index, item] of items.entries()) {
      const { left, right, width } = item.getBoundingClientRect()
      if(x > right && index < items.length - 1) continue
      const { start, end } = operations.value[index]
      return start + (end - start) * Math.min(1, Math.max(0, (x - left) / width))
    }
  }

  function startScrub(event) {
    if(!props.player.canPlay) return
    scrubbing.value = true
    track.value.setPointerCapture(event.pointerId)
    props.player.pause()
    props.player.seek(timeAt(event.clientX))
  }

  function scrub(event) {
    if(scrubbing.value) props.player.seek(timeAt(event.clientX))
  }

</script>
