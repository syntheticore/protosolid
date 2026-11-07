<template lang="pug">

  span.toggle-switch

    input(
      type="checkbox"
      role="switch"
      :checked="modelValue"
      @change="$emit('update:modelValue', $event.target.checked)"
    )

</template>


<style lang="stylus" scoped>

  @import '../styles/switch.styl'

  trackWidth = 36px

  // Fills its slot in forms and keeps the switch right aligned
  .toggle-switch
    display: flex
    justify-content: flex-end

  input
    switch-track()
    appearance: none
    position: relative
    flex: none
    width: trackWidth
    margin: 0
    box-shadow: $switchInsetShadow
    transition: background-color 0.15s

    &::before
      switch-knob()
      content: ''
      position: absolute
      top: $switchKnobOffset
      left: $switchKnobOffset
      transition: transform 0.15s

    &::after
      switch-inner-border()
      transition: border-color 0.15s

    &:checked
      background-color: $highlight

      &::after
        border-color: $highlight

        // The light theme keeps the light inner border around the highlight
        :where([data-theme="light"]) &
          border-color: var(--switch-rim)

      &::before
        background: $switchKnobOn
        transform: translateX(trackWidth - $switchHeight)

    &:focus-visible
      switch-focus()

</style>


<script setup>

  defineProps({
    modelValue: Boolean,
  })

  defineEmits(['update:modelValue'])

</script>
