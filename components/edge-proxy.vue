<template></template>

<script setup>

  import materials from '../js/materials.js'

  const props = defineProps(['component', 'edge', 'parentActive', 'parentHighlighted', 'parentSelected'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.edge == highlight.value )

  const edgeMesh = window.alcRenderer.convertLine(props.edge.tesselate(), getMaterial())
  edgeMesh.alcTypes = ['edge', props.edge.getAxis() && 'axis'].filter(Boolean)

  watch(() => props.edge, () => {
    props.edge.mesh = () => edgeMesh
    edgeMesh.alcObject = props.edge
  }, { immediate: true })

  window.alcRenderer.add(edgeMesh, props.parentActive)
  renderNeeded.value = true

  watch(() => [highlighted.value, props.parentActive, props.parentSelected], () => {
    edgeMesh.material = getMaterial()
    renderNeeded.value = true
  })

  function getMaterial() {
    return highlighted.value || props.parentSelected ?
      materials.selectionLine
      :
      props.parentActive ? materials.wire : materials.ghostWire
  }

  onUnmounted(() => {
    window.alcRenderer.remove(edgeMesh)
    props.edge.mesh = null
    renderNeeded.value = true
  })

</script>
