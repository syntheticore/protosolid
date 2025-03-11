<template></template>

<script setup>

  import materials from '../js/materials.js'

  const props = defineProps(['component', 'face', 'displayMode', 'parentActive', 'parentHighlighted', 'parentSelected'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.face == highlight.value )

  const faceMesh = window.alcRenderer.convertMesh(props.face.tesselate(), getMaterial())
  faceMesh.alcTypes = ['face', props.face.getPlane() && 'plane'].filter(Boolean)

  watch(() => props.face, () => {
    props.face.mesh = () => faceMesh
    faceMesh.alcObject = props.face
  }, { immediate: true })

  watch(() => props.parentActive, () => {
    faceMesh.alcProjectable = props.parentActive
    faceMesh.castShadow = props.parentActive
    faceMesh.receiveShadow = props.parentActive
    window.alcRenderer.remove(faceMesh)
    window.alcRenderer.add(faceMesh, props.parentActive)
  }, { immediate: true })

  renderNeeded.value = true

  watch(() => [highlighted.value, props.parentActive, props.parentSelected, props.displayMode, props.component.creator.material], () => {
    faceMesh.material = getMaterial()
    renderNeeded.value = true
  })

  function getMaterial() {
    const material = props.component.getMaterial()

    const surfaceMaterial = material ?
      material.displayMaterial
      :
      materials.surface

    return highlighted.value || props.parentSelected ?
      materials.highlightSurface
      :
      props.displayMode == 'wireframe' ?
        materials.invisibleSurface
        :
        props.parentActive ? surfaceMaterial : materials.ghostSurface
  }

  onUnmounted(() => {
    window.alcRenderer.remove(faceMesh)
    props.face.mesh = null
    renderNeeded.value = true
  })

</script>
