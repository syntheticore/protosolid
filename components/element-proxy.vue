<template></template>

<script setup>

  import * as THREE from 'three'
  import materials from '../js/materials.js'
  import { SketchPoint, Spline } from '../js/core/geom2d.js'
  import { ScreenPointObject } from '../js/three/helper-objects.js'

  const props = defineProps(['document', 'component', 'element', 'activeTool', 'showSplineControls', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.element == highlight.value )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.element) )
  const toolSelected = computed(() => props.activeTool && props.activeTool.items && props.activeTool.items.some(item =>
    (item.curve ? item.curve() : item) == props.element
  ))

  let mesh
  let controlMesh

  const getMesh = () => mesh

  function renderElement() {
    window.alcRenderer.remove(mesh)
    window.alcRenderer.remove(controlMesh)
    mesh = null
    controlMesh = null

    if(props.element instanceof SketchPoint) {
      mesh = new ScreenPointObject(22)
    } else {
      const vertices = props.element.tesselate()
      if(!vertices) return
      mesh = window.alcRenderer.convertLine(vertices, materials.line)
      if(props.showSplineControls && props.element instanceof Spline) {
        controlMesh = window.alcRenderer.convertLine(
          props.element.handles().map(point => point.toArray()),
          materials.splineControlLine,
        )
        controlMesh.renderOrder = -1
        window.alcRenderer.add(controlMesh, true)
      }
    }
    updateTransform()
    mesh.alcTypes = props.element instanceof SketchPoint ?
      ['point']
      :
      ['curve', props.element.getAxis && 'axis']
    mesh.material = getMaterial()
    mesh.alcObject = props.element

    props.element.mesh = getMesh
    props.element.component = props.component

    window.alcRenderer.add(mesh, true)
    renderNeeded.value = true
  }

  watch(() => props.element, renderElement, { immediate: true, deep: 1 })
  watch(() => props.showSplineControls, renderElement)

  watch([highlighted, selected, toolSelected], () => {
    // Zero-length drawing elements have no mesh, but their reactive selection
    // state can still change before Vue unmounts their proxy.
    if(!mesh) return
    mesh.material = getMaterial()
    renderNeeded.value = true
  })

  watch(() => props.parentTransform, () => {
    if(!mesh) return
    updateTransform()
    renderNeeded.value = true
  }, { immediate: true })

  function updateTransform() {
    mesh.matrix.copy(props.parentTransform || new THREE.Matrix4()).multiply(props.element.sketch.workplane)
    if(controlMesh) {
      controlMesh.matrix.copy(mesh.matrix)
      controlMesh.matrix.decompose(controlMesh.position, controlMesh.quaternion, controlMesh.scale)
    }
    if(props.element instanceof SketchPoint) {
      mesh.matrix.multiply(new THREE.Matrix4().makeTranslation(
        props.element.point.x,
        props.element.point.y,
        props.element.point.z,
      ))
    }
    mesh.matrix.decompose(mesh.position, mesh.quaternion, mesh.scale)
  }

  function getMaterial() {
    if(props.element instanceof SketchPoint) {
      return (selected.value || toolSelected.value || highlighted.value) ? materials.highlightUiPoint : materials.uiPoint
    }
    return materials.table.curve[
      (selected.value || toolSelected.value) ? 'selected' : (highlighted.value ? 'highlighted' : 'unselected')
    ][
      props.element.projection ? 'projected' : 'regular'
    ][
      props.element.isReference ? 'reference' : 'actual'
    ]
  }

  onUnmounted(() => {
    window.alcRenderer.remove(mesh)
    window.alcRenderer.remove(controlMesh)
    props.element.mesh = null
    renderNeeded.value = true
  })

</script>
