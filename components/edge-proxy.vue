<template></template>

<script setup>

  import { toRaw } from 'vue'
  import * as THREE from 'three'
  import { Line2 } from 'three/examples/jsm/lines/Line2.js'
  import materials from '../js/materials.js'

  const props = defineProps(['component', 'edge', 'displayMode', 'colorMode', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const edgeMesh = window.alcRenderer.convertLine(props.edge.tesselate(), getMaterial())
  edgeMesh.alcTypes = ['edge', props.edge.getAxis() && 'axis'].filter(Boolean)

  const hiddenMaterial = materials.hiddenLine.clone()
  const hiddenMesh = edgeMesh.isLine2 ? new Line2(edgeMesh.geometry, hiddenMaterial) : new THREE.Object3D()
  hiddenMesh.visible = false
  // Draw hidden edges before the visible edges and sketch overlays. They must
  // compare against face depth, not depth written by another edge.
  hiddenMesh.renderOrder = -1
  hiddenMesh.raycast = () => {}
  edgeMesh.add(hiddenMesh)

  const dashPosition = new THREE.Vector4()
  hiddenMesh.onBeforeRender = function(renderer, scene, camera) {
    Line2.prototype.onBeforeRender.call(this, renderer)
    hiddenMaterial.color.copy(edgeMesh.material.color)
    hiddenMaterial.toneMapped = edgeMesh.material.toneMapped
    // Keep dashes in screen pixels as the camera zooms or changes projection.
    const center = this.geometry.boundingSphere.center
    dashPosition.set(center.x, center.y, center.z, 1)
      .applyMatrix4(this.matrixWorld)
      .applyMatrix4(camera.matrixWorldInverse)
      .applyMatrix4(camera.projectionMatrix)
    hiddenMaterial.dashScale = hiddenMaterial.resolution.y * camera.projectionMatrix.elements[5] / (2 * Math.abs(dashPosition.w))
  }

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
    hiddenMaterial.dispose()
    window.alcRenderer.remove(edgeMesh)
    props.edge.mesh = null
    renderNeeded.value = true
  })

</script>
