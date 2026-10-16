<template></template>

<script setup>

  import * as THREE from 'three'
  import materials from '../js/materials.js'
  import { SketchPoint, Spline } from '../js/core/geom2d.js'
  import { Constraint } from '../js/core/sketch.js'
  import { ScreenPointObject } from '../js/three/helper-objects.js'
  import HiddenLine from '../js/three/hidden-line.js'

  const props = defineProps(['document', 'component', 'element', 'displayMode', 'activeTool', 'showSplineControls', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.element == highlight.value ||
    (highlight.value instanceof Constraint && highlight.value.items.some(item => item.curve() == props.element))
  )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.element) )
  const toolSelected = computed(() => props.activeTool && props.activeTool.items && props.activeTool.items.some(item =>
    (item.curve ? item.curve() : item) == props.element
  ))
  const fullyConstrained = computed(() =>
    !props.element.projection && props.element.fullyConstrained
  )

  let mesh
  let hiddenMesh
  let controlMesh

  const getMesh = () => mesh

  function removeMeshes() {
    if(hiddenMesh) {
      mesh.remove(hiddenMesh)
      hiddenMesh.material.dispose()
      hiddenMesh = null
    }
    window.alcRenderer.remove(mesh)
    window.alcRenderer.remove(controlMesh)
    mesh = null
    controlMesh = null
  }

  function renderElement() {
    removeMeshes()

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
    if(!(props.element instanceof SketchPoint)) {
      window.alcRenderer.offsetSketchObject(mesh)
      if(mesh.isLine2) {
        hiddenMesh = new HiddenLine(mesh)
        hiddenMesh.visible = props.displayMode == 'hiddenLines'
        window.alcRenderer.offsetSketchObject(hiddenMesh)
        mesh.add(hiddenMesh)
      }
    }

    props.element.mesh = getMesh
    props.element.component = props.component

    window.alcRenderer.add(mesh, true)
    renderNeeded.value = true
  }

  watch(() => props.element, renderElement, { immediate: true, deep: 1 })
  watch(() => props.showSplineControls, renderElement)

  watch([highlighted, selected, toolSelected, fullyConstrained, () => props.displayMode], () => {
    // Zero-length drawing elements have no mesh, but their reactive selection
    // state can still change before Vue unmounts their proxy.
    if(!mesh) return
    mesh.material = getMaterial()
    if(hiddenMesh) hiddenMesh.visible = props.displayMode == 'hiddenLines'
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
      if(selected.value || toolSelected.value || highlighted.value) return materials.highlightUiPoint
      return fullyConstrained.value ? materials.constrainedUiPoint : materials.uiPoint
    }
    const material = materials.table.curve[
      highlighted.value ? 'highlighted' :
        ((selected.value || toolSelected.value) ? 'selected' :
          (fullyConstrained.value ? 'constrained' : 'unselected'))
    ][
      props.element.projection ? 'projected' : 'regular'
    ][
      props.element.isReference ? 'reference' : 'actual'
    ]
    return props.displayMode == 'hiddenLines' && material == materials.highlightLine
      ? materials.visibleHighlightLine
      : material
  }

  onUnmounted(() => {
    removeMeshes()
    props.element.mesh = null
    renderNeeded.value = true
  })

</script>
