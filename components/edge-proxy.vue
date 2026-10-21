<template></template>

<script setup>

  import { toRaw } from 'vue'
  import * as THREE from 'three'
  import { Face } from '../js/core/geom3d.js'
  import { Feature } from '../js/core/features.js'
  import { topologyIncludesFeature } from '../js/core/topological-naming.js'
  import materials from '../js/materials.js'
  import HiddenLine from '../js/three/hidden-line.js'

  const props = defineProps(['document', 'component', 'edge', 'displayMode', 'colorMode', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const pickerSelection = inject('picker-selection')
  const renderNeeded = inject('render-needed')
  const selected = computed(() => props.document.selection.has(props.edge) ||
    pickerSelection.value.get(props.component.id)?.has(props.edge.id))
  const directlyHighlighted = computed(() => toRaw(props.edge) == toRaw(highlight.value))
  const highlightedFeature = computed(() =>
    [highlight.value, props.document.activeFeature || props.document.creatingFeature].find(feature =>
      feature instanceof Feature && props.edge.connectedFaces()
        .some(face => face.topology && topologyIncludesFeature(face.topology, feature))))
  const highlighted = computed(() => {
    const hovered = toRaw(highlight.value)
    return directlyHighlighted.value || !!highlightedFeature.value ||
      hovered instanceof Face &&
      toRaw(props.edge.solid) == toRaw(hovered.solid) &&
      props.edge.connectedFaces().some(face => toRaw(face) == hovered)
  })

  const edgeMesh = window.alcRenderer.convertLine(props.edge.tesselate(), getMaterial())
  edgeMesh.alcTypes = ['edge', 'path', props.edge.getAxis() && 'axis'].filter(Boolean)

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

  watch(() => [props.edge, highlighted.value, highlightedFeature.value?.operation, directlyHighlighted.value, selected.value, props.parentActive, props.parentHighlighted, props.parentSelected, props.displayMode, props.colorMode], () => {
    edgeMesh.visible = props.displayMode != 'exportPreview' && props.colorMode != 'interference'
    edgeMesh.material = getMaterial()
    hiddenMesh.visible = props.displayMode == 'hiddenLines'
    nextTick().then(() => renderNeeded.value = true )
  }, { immediate: true })

  function getMaterial() {
    if(props.displayMode == 'exportPreview' || props.colorMode == 'interference') return materials.invisibleLine
    if(highlightedFeature.value?.operation == 'cut' && !directlyHighlighted.value) return materials.visibleCutHighlightLine
    return highlighted.value ?
      !directlyHighlighted.value || props.displayMode == 'hiddenLines' ? materials.visibleHighlightLine : materials.highlightLine
      :
      selected.value ? materials.selectionLine
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
