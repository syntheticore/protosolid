<template lang="pug">

  //- Solids

  template(v-for="solid in compound && !editingSurfaceChart ? compound.solids() : []")

    SolidProxy(
      v-if="!component.isItemHidden(solid.id) || solid.id == highlight?.id"
      :solid="solid"
      :key="solid.id"
      :document="document"
      :component="component"
      :display-mode="displayMode"
      :color-mode="colorMode"
      :parent-active="active"
      :parent-highlighted="highlighted"
      :parent-selected="selected"
      :parent-transform="transform"
      :project-inactive="projecting"
      :project-points="projectingPoints"
    )

  //- Sketches

  template(v-for="sketch in component.sketches")

    SketchProxy(
      v-if="(!editingSurfaceChart || sketch == document.activeSketch) && (!component.creator.itemsHidden[sketch.id] || sketch == highlight || sketch.creator == highlight)"
      :key="sketch.id"
      :document="document"
      :component="component"
      :sketch="sketch"
      :display-mode="displayMode"
      :active-handle="activeHandle"
      :active-tool="activeTool"
      :parent-transform="transform"
      :parent-highlighted="highlighted"
      :parent-selected="selected"
      @handleMouseUp="handleMouseUp"
      @handleMouseDown="handleMouseDown"
      @handleMouseMove="handleMouseMove"
      @handleMouseLeave="handleMouseLeave"
      @handlePointerDown="handlePointerDown"
      @handleWheel="handleWheel"
      @dimensionMouseUp="dimensionMouseUp"
      @dimensionMouseDown="dimensionMouseDown"
      @dimensionMouseMove="dimensionMouseMove"
    )

  //- Helpers

  template(v-for="helper in component.helpers")

    HelperProxy(
      v-if="!((document.activeMerge || document.activeSimulation || document.camMode) && helper.isStandard()) &&(isHelperHighlighted(helper) || !component.isItemHidden(helper.id) && (component.id === document.activeComponent?.id || referencedItemIds.has(helper.id) || document.activeFeature instanceof JointFeature || (projectingPoints && helper instanceof PointHelper)))"
      :key="helper.id"
      :document="document"
      :component="component"
      :helper="helper"
      :parent-transform="transform"
      :parent-highlighted="highlighted"
      :parent-selected="selected"
    )

  //- Canvases

  template(v-for="canvas in component.creator.canvases")

    CanvasProxy(
      v-if="!canvas.hidden"
      :key="canvas.id"
      :document="document"
      :component="component"
      :canvas="canvas"
      :parent-transform="transform"
      :parent-highlighted="highlighted"
      :parent-selected="selected"
    )

  //- Section Views

  template(v-for="section in component.creator.sectionViews")

    SectionProxy(
      v-if="!section.hidden"
      :key="section.id"
      :document="document"
      :component="component"
      :section="section"
      :parent-transform="transform"
    )

  //- Child Components

  template(v-for="child in component.children")

    ComponentProxy(
      v-if="!child.creator.hidden"
      :key="child.id"
      :document="document"
      :component="child"
      :display-mode="displayMode"
      :color-mode="colorMode"
      :parent-active="active"
      :parent-highlighted="highlighted"
      :parent-selected="selected"
      :parent-transform="transform"
      :active-tool="activeTool"
      :section-plane="sectionPlane"
      :section-side="sectionSide"
      @handleMouseUp="handleMouseUp"
      @handleMouseDown="handleMouseDown"
      @handleMouseMove="handleMouseMove"
      @handleMouseLeave="handleMouseLeave"
      @handlePointerDown="handlePointerDown"
      @handleWheel="handleWheel"
      @dimensionMouseUp="dimensionMouseUp"
      @dimensionMouseDown="dimensionMouseDown"
      @dimensionMouseMove="dimensionMouseMove"
    )

</template>


<script setup>

  import * as THREE from 'three'
  import { JointFeature } from '../js/core/features.js'
  import { PointHelper } from '../js/core/helpers.js'
  import { ProjectionTool, ProjectTool } from '../js/tools.js'
  import { sectionBounds, sectionPlaneRelation } from '../js/core/section-bounds.js'
  import SectionCaps from '../js/three/section-caps.js'
  import materials from '../js/materials.js'

  const props = defineProps(['document', 'component', 'displayMode', 'colorMode', 'activeHandle', 'activeTool', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform', 'sectionPlane', 'sectionSide'])
  const emit = defineEmits(['handleMouseDown', 'handleMouseUp', 'handleMouseMove', 'handleMouseLeave', 'handlePointerDown', 'handleWheel', 'dimensionMouseUp', 'dimensionMouseDown', 'dimensionMouseMove'])
  // defineOptions({ inheritAttrs: false })

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const active = computed(() => props.parentActive || props.component.id == props.document.activeComponent?.id || props.document.activeFeature instanceof JointFeature )
  const highlighted = computed(() => props.parentHighlighted || props.component == highlight.value )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.component) )
  const projecting = computed(() => props.activeTool instanceof ProjectionTool )
  const projectingPoints = computed(() => props.activeTool instanceof ProjectTool )
  const editingSurfaceChart = computed(() => !!props.document.activeSketch?.surfaceSketch && !projecting.value)
  const referencedItemIds = computed(() => new Set((props.document.activeFeature?.referencedItems() || [])
    .filter(item => item.componentId == props.component.id)
    .map(item => item.itemId)))

  const transform = computed(() => {
    return (props.parentTransform || new THREE.Matrix4())
      .clone()
      .multiply(props.component.localTransform())
  })

  // Another body may temporarily take the place of the component's own
  const model = computed(() => props.document.displayCompound?.componentId == props.component.id ?
    props.document.displayCompound.compound : props.component.compound)

  const bounds = computed(() => sectionBounds(model.value))

  // All visible saved sections and, while sketching, the active sketch plane,
  // as world space planes. Tools picking model topology reference the real
  // model, so they see it unsectioned.
  const sections = computed(() => {
    if(props.activeTool?.showsUnsectioned) return []
    const sections = getSections(props.component).filter(sec => !sec.hidden )
      .map(section => ({ matrix: transform.value.clone().multiply(section.getTransform()), side: section.side }))
    if(props.sectionPlane) sections.push({ matrix: props.sectionPlane, side: props.sectionSide })
    return sections
  })

  // Sections are clipped on the GPU, only the path tracer needs the model cut
  const cutsModel = computed(() => props.displayMode == 'pathTraced')

  const compound = computed(() => {
    if(!cutsModel.value || !sections.value.length) return model.value
    try {
      const originalFaces = model.value.solids().flatMap(solid => solid.faces())
      const inverse = transform.value.clone().invert()
      const planes = sections.value.map(section => inverse.clone().multiply(section.matrix))
      const cut = sections.value.reduce((shape, section, index) => {
        if(!shape) return null
        // Every subsequent cut is a subset of the original compound. Its cached
        // enclosure remains safe, without meshing intermediate Boolean results.
        const relation = sectionPlaneRelation(bounds.value, planes[index])
        if(relation) {
          const keepsShape = section.side ? relation < 0 : relation > 0
          return keepsShape ? shape : null
        }
        return shape.halfCut(planes[index], section.side)
      }, model.value)
      if(!cut) return null
      if(cut !== model.value) markSectionFaces(cut, planes, originalFaces)
      return cut
    } catch(error) {
      console.error(`Section view failed for component ${props.component.id}:`, error)
      return model.value
    }
  })

  // Clipping planes of the component's faces and edges
  const sectionPlanes = shallowRef(null)
  provide('section-planes', sectionPlanes)
  let sectionCaps = null

  const sectionFaces = computed(() => (model.value?.solids() || [])
    .filter(solid => !props.component.isItemHidden(solid.id))
    .flatMap(solid => solid.faces()))

  // The cut's outline looks like the component's other edges
  const outlineMaterial = computed(() => {
    if(['shaded', 'pathTraced', 'exportPreview'].includes(props.displayMode) || props.colorMode == 'interference') return null
    if(!active.value && !['wireframe', 'hiddenLines'].includes(props.displayMode)) return null
    if(highlighted.value || selected.value) return materials.selectionLine
    if(!active.value) return materials.ghostWire
    return props.colorMode == 'component' ? props.component.creator.compLineMaterial : materials.wire
  })

  watch(() => [
    cutsModel.value || props.displayMode == 'exportPreview' ? [] : sections.value,
    sectionFaces.value,
    outlineMaterial.value,
    props.displayMode,
    transform.value,
  ], ([sections, faces, outlineMaterial]) => {
    if(sections.length) {
      sectionCaps ||= new SectionCaps()
      sectionCaps.update({
        sections,
        transform: transform.value,
        faces,
        color: props.component.creator.compSectionMaterial.color,
        outlineMaterial,
        hiddenLines: props.displayMode == 'hiddenLines',
      })
      sectionPlanes.value = sectionCaps.planes
    } else {
      sectionCaps?.dispose()
      sectionCaps = null
      sectionPlanes.value = null
    }
    window.alcRenderer.invalidateShadows()
    renderNeeded.value = true
  }, { immediate: true })

  onUnmounted(() => sectionCaps?.dispose())

  function markSectionFaces(compound, planes, originalFaces) {
    const unchangedFaces = compound.preservedFaces(originalFaces)
    const sectionPlanes = planes.map(plane => ({
      origin: new THREE.Vector3().setFromMatrixPosition(plane),
      normal: new THREE.Vector3().setFromMatrixColumn(plane, 2).normalize(),
    }))
    compound.solids().flatMap(solid => solid.faces()).forEach(face => {
      const facePlane = face.getPlane()
      if(!facePlane) return
      const origin = new THREE.Vector3().setFromMatrixPosition(facePlane)
      const normal = new THREE.Vector3().setFromMatrixColumn(facePlane, 2).normalize()
      const onSectionPlane = sectionPlanes.some(section =>
        Math.abs(normal.dot(section.normal)) > 1 - 1e-6 &&
        Math.abs(section.normal.dot(origin.clone().sub(section.origin))) < 1e-5
      )
      if(!onSectionPlane) return
      face.sectionSurface = !unchangedFaces.has(face)
    })
  }

  // Hovered helpers show even when hidden or outside the active component
  function isHelperHighlighted(helper) {
    return helper == highlight.value || helper.id == highlight.value?.helperId
  }

  function getSections(comp) {
    if(!comp) return []
    return [...comp.creator.sectionViews].concat(getSections(comp.parent))
  }

  const handleMouseUp = (...args) => emit('handleMouseUp', ...args)
  const handleMouseDown = (...args) => emit('handleMouseDown', ...args)
  const handleMouseMove = (...args) => emit('handleMouseMove', ...args)
  const handleMouseLeave = (...args) => emit('handleMouseLeave', ...args)
  const handlePointerDown = (...args) => emit('handlePointerDown', ...args)
  const handleWheel = (...args) => emit('handleWheel', ...args)

  const dimensionMouseUp = (...args) => emit('dimensionMouseUp', ...args)
  const dimensionMouseDown = (...args) => emit('dimensionMouseDown', ...args)
  const dimensionMouseMove = (...args) => emit('dimensionMouseMove', ...args)

</script>
