<template></template>

<script setup>

  import * as THREE from 'three'
  import materials from '../js/materials.js'

  const props = defineProps(['component', 'profile', 'flat', 'parentHighlighted', 'parentSelected', 'parentTransform'])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.parentSelected || props.profile == highlight.value )

  const wrapped = props.profile.sketch.surfaceSketch && !props.flat
  const mesh = window.alcRenderer.convertMesh(props.profile.tesselate(props.flat), getMaterial())
  mesh.alcTypes = ['profile']
  mesh.alcObject = props.profile
  if(!wrapped) window.alcRenderer.offsetSketchObject(mesh)
  props.profile.mesh = () => mesh
  props.profile.component = props.component
  window.alcRenderer.add(mesh, true)

  renderNeeded.value = true

  watch(() => [highlighted.value], () => {
    mesh.material = getMaterial()
    renderNeeded.value = true
  })

  watch([() => props.parentTransform, () => props.profile.sketch.workplane], () => {
    mesh.matrix.copy(props.parentTransform || new THREE.Matrix4())
    if(!wrapped) mesh.matrix.multiply(props.profile.sketch.workplane)
    mesh.matrix.decompose(mesh.position, mesh.quaternion, mesh.scale)
    renderNeeded.value = true
  }, { immediate: true })

  function getMaterial() {
    return highlighted.value ?
      materials.highlightRegion
      :
      materials.region
  }

  onBeforeUnmount(() => {
    window.alcRenderer.remove(mesh)
    props.profile.mesh = null
    renderNeeded.value = true
  })

</script>
