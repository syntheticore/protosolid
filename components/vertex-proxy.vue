<template></template>

<script setup>

  import { toRaw } from 'vue'
  import * as THREE from 'three'
  import materials from '../js/materials.js'
  import { ScreenPointObject } from '../js/three/helper-objects.js'

  const props = defineProps(['component', 'vertex', 'parentTransform'])

  const highlight = inject('highlight')
  const pickerSelection = inject('picker-selection')
  const renderNeeded = inject('render-needed')
  const hovered = computed(() => toRaw(props.vertex) == toRaw(highlight.value))
  const highlighted = computed(() => hovered.value ||
    pickerSelection.value.get(props.component.id)?.has(props.vertex.id))

  const mesh = new ScreenPointObject(6)
  mesh.alcTypes = ['point']

  // Invisible disc keeping the hit area at the hovered size
  const hitArea = new THREE.Mesh(mesh.geometry, materials.invisibleSurface)
  hitArea.alcTypes = mesh.alcTypes
  mesh.add(hitArea)

  watch(() => [props.vertex, props.parentTransform], () => {
    mesh.alcObject = hitArea.alcObject = props.vertex
    mesh.position.copy(props.vertex.point).applyMatrix4(props.parentTransform)
    renderNeeded.value = true
  }, { immediate: true })

  watch([hovered, highlighted], () => {
    mesh.pixelDiameter = hovered.value ? 18 : 6
    hitArea.scale.setScalar(18 / mesh.pixelDiameter)
    mesh.material = highlighted.value ? materials.highlightUiPoint : materials.vertexPoint
    renderNeeded.value = true
  }, { immediate: true })

  window.alcRenderer.add(mesh, true)

  onUnmounted(() => {
    window.alcRenderer.remove(mesh)
    mesh.geometry.dispose()
    renderNeeded.value = true
  })

</script>
