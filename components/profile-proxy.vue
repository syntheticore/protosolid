<template></template>

<script setup>

  import materials from '../js/materials.js'

  const props = defineProps(['component', 'profile', 'parentHighlighted', 'parentSelected'])
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
