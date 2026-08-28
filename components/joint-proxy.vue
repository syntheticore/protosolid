<template lang="pug">

  Icon.joint-proxy(
    :icon="icon"
    :class="{ selected, picking: bus.jointPickerActive }"
    :style="{ top: coords.y + 'px', left: coords.x + 'px' }"
    :title="title"
    @click.stop="pick"
    @dblclick.stop="openFeature"
  )

</template>

<style lang="stylus" scoped>

  .joint-proxy
    pointer-events: auto
    position: absolute
    width: 18px
    height: 18px
    padding: 3px
    margin-left: -12px
    margin-top: -12px
    border-radius: 99px
    color: $dark2
    background: $bright1
    border: 1px solid rgba(white, 0.7)
    box-shadow: 0 1px 4px rgba(black, 0.65)
    transition: color 0.1s, background 0.1s, transform 0.1s

    &:hover
      color: white
      transform: scale(1.12)

    &.selected
      color: white
      background: $highlight

    &.picking
      box-shadow: 0 0 0 2px rgba($highlight, 0.65), 0 1px 4px rgba(black, 0.65)

</style>

<script setup>

  import * as THREE from 'three'
  import { worldTransform } from '../js/core/assembly.js'

  const props = defineProps({
    document: Object,
    joint: Object,
    activeTool: Object,
  })
  const bus = inject('bus')
  const frame = inject('frame')

  const icons = {
    axis: 'sync-alt',
    coplanar: 'arrows-alt',
    ball: 'dot-circle',
    fix: 'lock',
    group: 'object-group',
  }
  const names = {
    axis: 'Axis Joint',
    coplanar: 'Planar Joint',
    ball: 'Ball Joint',
    fix: 'Fixed Joint',
    group: 'Rigid Group',
  }

  const icon = computed(() => icons[props.joint.type] || 'code-branch')
  const title = computed(() => names[props.joint.type] || 'Joint')
  const selected = computed(() => props.document.selection.has(props.joint))
  const coords = computed(() => {
    frame.value
    const component = props.document.top().findChild(props.joint.componentA)
    if(!component) return new THREE.Vector2(-100, -100)
    const attachment = worldTransform(component).multiply(props.joint.frameA || new THREE.Matrix4())
    return window.alcRenderer.toScreen(new THREE.Vector3().setFromMatrixPosition(attachment))
  })

  function pick() {
    if(bus.jointPickerActive) {
      if(props.activeTool?.acceptsInput && !props.activeTool.acceptsInput(props.joint)) return
      bus.emit('joint-picked', props.joint)
      return
    }
    props.joint.typename ||= () => names[props.joint.type] || 'Joint'
    props.document.selection.handle(props.joint, bus.isCtrlPressed)
  }

  function openFeature() {
    if(bus.jointPickerActive || props.document.activeFeature) return
    const feature = props.document.timeline.features.find(feature => feature.id == props.joint.id)
    if(!feature || feature.constructor.editable === false) return
    bus.emit('close-feature')
    setTimeout(() => props.document.activateFeature(feature))
  }

</script>
