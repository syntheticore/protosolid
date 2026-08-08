<template></template>

<script setup>

  import * as THREE from 'three'
  import materials from '../js/materials.js'

  const props = defineProps(['component', 'profile', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.parentSelected || props.profile == highlight.value )

  const mesh = window.alcRenderer.convertMesh(props.profile.tesselate(), getMaterial())
  mesh.applyMatrix4(props.profile.sketch.workplane)
  mesh.alcTypes = ['profile']
  mesh.alcObject = props.profile
  props.profile.mesh = () => mesh
  props.profile.component = props.component
  window.alcRenderer.add(mesh, true)

  renderNeeded.value = true

  watch(() => [highlighted.value], () => {
    mesh.material = getMaterial()
    renderNeeded.value = true
  })

  watch(() => props.parentTransform, () => {
    mesh.matrix.copy(props.parentTransform || new THREE.Matrix4()).multiply(props.profile.sketch.workplane)
    mesh.matrix.decompose(mesh.position, mesh.quaternion, mesh.scale)
    renderNeeded.value = true
  }, { immediate: true })

  function getMaterial() {
    return highlighted.value ?
      materials.highlightRegion
      :
      materials.region
  }

  onUnmounted(() => {
    window.alcRenderer.remove(mesh)
    props.profile.mesh = null
    renderNeeded.value = true
  })

</script>
