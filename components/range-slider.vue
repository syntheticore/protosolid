<template lang="pug">

  span.range-slider

    //- Wraps the knob like a checked toggle and disappears at the minimum
    .fill(v-show="ratio > 0" :style="{ '--ratio': ratio }")

    input(
      :class="{ filled: ratio > 0 }"
      type="range"
      :min="min"
      :max="max"
      :step="step"
      :value="current"
      @input="input"
      @change="$emit('change', Number($event.target.value))"
    )

</template>


<style lang="stylus" scoped>

  @import '../styles/switch.styl'

  // Knob with the gap around it
  knobArea = $switchKnobSize + $switchKnobOffset * 2

  track()
    height: 100%
    background: none

  .range-slider
    switch-track()
    position: relative
    display: flex

    // Inset shadow of toggle switches, above the fill. Toggles draw their inner
    // border on top of the shadow, so leave the shadow out of the border here too
    &::before
      content: ''
      position: absolute
      inset: 0
      z-index: 2
      border-radius: $switchInnerRadius
      box-shadow: $switchInsetShadow
      clip-path: inset($switchBorder round ($switchInnerRadius - $switchBorder))
      pointer-events: none

    &::after
      switch-inner-border()

    &:has(input:focus-visible)
      switch-focus()

  // Covers the inner border like the background of a checked toggle
  .fill
    position: absolute
    top: 0
    bottom: 0
    left: 0
    width: unquote('calc(%s + var(--ratio) * (100% - %s))' % (knobArea knobArea))
    z-index: 1
    border-radius: $switchInnerRadius
    background: $highlight
    pointer-events: none

  input
    appearance: none
    position: absolute
    inset: 0
    z-index: 3
    box-sizing: border-box
    width: 100%
    height: 100%
    margin: 0
    padding: 0 $switchKnobOffset
    background: none
    outline: none

    // Chrome and Firefox need separate rules, as either drops rules with
    // pseudo elements it doesn't know
    &::-webkit-slider-runnable-track
      track()

    &::-moz-range-track
      track()

    &::-webkit-slider-thumb
      switch-knob()
      // Chrome aligns the knob to the top of the track
      margin-top: $switchKnobOffset

    &::-moz-range-thumb
      switch-knob()

    &.filled::-webkit-slider-thumb
      background: $switchKnobOn

    &.filled::-moz-range-thumb
      background: $switchKnobOn

</style>


<script setup>

  const props = defineProps({
    // Documents may still hold values stored as strings by plain range inputs
    modelValue: [Number, String],
    min: { type: Number, default: 0 },
    max: { type: Number, default: 1 },
    step: { type: Number, default: 0.01 },
  })

  const emit = defineEmits(['update:modelValue', 'change'])

  // Follows dragging also when the owner only listens to change
  const current = ref(Number(props.modelValue))
  watch(() => props.modelValue, value => current.value = Number(value))

  const ratio = computed(() => (current.value - props.min) / (props.max - props.min))

  function input(event) {
    current.value = Number(event.target.value)
    emit('update:modelValue', current.value)
  }

</script>
