<template></template>

<script setup>

  import { toRaw } from 'vue'
  import * as THREE from 'three'
  import materials from '../js/materials.js'
  import HiddenLine from '../js/three/hidden-line.js'

  const props = defineProps(['component', 'edge', 'displayMode', 'colorMode', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const edgeMesh = window.alcRenderer.convertLine(props.edge.tesselate(), getMaterial())
  edgeMesh.alcTypes = ['edge', props.edge.getAxis() && 'axis'].filter(Boolean)

  const hiddenMesh = edgeMesh.isLine2 ? new HiddenLine(edgeMesh) : new THREE.Object3D()
  edgeMesh.add(hiddenMesh)

  watch(() => props.edge, () => {
    props.edge.mesh = () => edgeMesh
    edgeMesh.alcObject = props.edge
  }, { immediate: true })

  watch(() => props.parentTransform, () => {
    edgeMesh.matrix.copy(props.parentTransform)
    edgeMesh.matrix.decompose(edgeMesh.position, edgeMesh.quaternion, edgeMesh.scale)
    renderNeeded.value = true
  }, { immediate: true })

  window.alcRenderer.add(edgeMesh, props.parentActive)
  renderNeeded.value = true

  watch(() => [props.edge, highlight.value, props.parentActive, props.parentHighlighted, props.parentSelected, props.displayMode, props.colorMode], () => {
    edgeMesh.visible = props.displayMode != 'exportPreview' && props.colorMode != 'interference'
    edgeMesh.material = getMaterial()
    hiddenMesh.visible = props.displayMode == 'hiddenLines'
    nextTick().then(() => renderNeeded.value = true )
  }, { immediate: true })

  function getMaterial() {
    if(props.displayMode == 'exportPreview' || props.colorMode == 'interference') return materials.invisibleLine
    return toRaw(props.edge) == toRaw(highlight.value) ?
      props.displayMode == 'hiddenLines' ? materials.visibleHighlightLine : materials.highlightLine
      :
      ['shaded', 'pathTraced'].includes(props.displayMode) ?
        materials.invisibleLine
        :
        props.parentHighlighted || props.parentSelected ?
          materials.selectionLine
          :
          props.parentActive ?
            props.colorMode == 'component' ? props.component.creator.compLineMaterial : materials.wire
            :
            materials.ghostWire
  }

  onUnmounted(() => {
    edgeMesh.remove(hiddenMesh)
    if(edgeMesh.isLine2) hiddenMesh.material.dispose()
    window.alcRenderer.remove(edgeMesh)
    props.edge.mesh = null
    renderNeeded.value = true
  })

</script>
