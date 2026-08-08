<template></template>

<script setup>

  import materials from '../js/materials.js'

  const props = defineProps(['component', 'face', 'displayMode', 'colorMode', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.face == highlight.value )

  const faceMesh = window.alcRenderer.convertMesh(props.face.tesselate(), getMaterial())
  faceMesh.alcTypes = [
    'face',
    props.face.getPlane() && 'plane',
    props.face.getAxis() && 'axis',
    props.face.getPoint() && 'point',
  ].filter(Boolean)

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

  watch(() => props.parentTransform, () => {
    faceMesh.matrix.copy(props.parentTransform)
    faceMesh.matrix.decompose(faceMesh.position, faceMesh.quaternion, faceMesh.scale)
    renderNeeded.value = true
  }, { immediate: true })

  renderNeeded.value = true

  watch(() => [highlighted.value, props.parentActive, props.parentSelected, props.displayMode, props.colorMode, props.component.creator.material], () => {
    faceMesh.material = getMaterial()
    renderNeeded.value = true
  })

  function getSurfaceMaterial() {
    if(props.colorMode == 'component') return props.component.creator.compMaterial
    const material = props.component.getMaterial()
    return material ? material.displayMaterial : materials.surface
  }

  function getMaterial() {
    return highlighted.value || props.parentSelected ?
      materials.highlightSurface
      :
      props.displayMode == 'wireframe' ?
        materials.invisibleSurface
        :
        props.parentActive ? getSurfaceMaterial() : materials.ghostSurface
  }

  onUnmounted(() => {
    window.alcRenderer.remove(faceMesh)
    props.face.mesh = null
    renderNeeded.value = true
  })

</script>
