<template lang="pug">
  .selector-widget.selection-feature-widget(
    :style="{ left: position.x + 'px', top: position.y + 'px' }"
    @pointerdown.stop
    @dblclick.stop
  )
    .panel
      nav.actions.bordered.tipped(aria-label="Selection features")
        button(
          type="button"
          title="Select Solid"
          aria-label="Select Solid"
          :disabled="document.isRegenerating"
          @click="document.selection.only(item.solid)"
        )
          Icon(icon="layer-group" fixed-width)

        button(
          v-for="feature in features"
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
  @import '../styles/selector-widget.styl'

  .selector-widget.selection-feature-widget
    position: absolute
    // Keep the anchor and arrow transparent to a second click on the geometry.
    pointer-events: none

  .panel
    transform: translateX(-19px)

  .actions.bordered
    pointer-events: auto
    &::before
      left: 11px
</style>

<script setup>
  import { computed, inject, onBeforeUnmount, shallowRef, toRaw, watch } from 'vue'
  import { worldTransform } from '../js/core/assembly.js'
  import { topologyIncludesFeature } from '../js/core/topological-naming.js'

  const props = defineProps(['document', 'item', 'renderer', 'coords'])
  const frame = inject('frame')
  const highlight = inject('highlight')
  const anchor = shallowRef()

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

  const features = computed(() => props.document.timeline.features.filter(feature =>
    topologyIncludesFeature(props.item.topology, feature)
  ).toReversed())

  function clearHighlight() {
    if(features.value.some(feature => toRaw(feature) === toRaw(highlight.value))) highlight.value = null
  }

  onBeforeUnmount(clearHighlight)
</script>
