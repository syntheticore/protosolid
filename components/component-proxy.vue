<template lang="pug">

  //- Solids

  template(v-for="solid in compound ? compound.solids() : []")

    SolidProxy(
      v-if="!component.isItemHidden(solid.id)"
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
    )

  //- Sketches

  template(v-for="sketch in component.sketches")

    SketchProxy(
      v-if="!component.creator.itemsHidden[sketch.id]"
      :key="sketch.id"
      :document="document"
      :component="component"
      :sketch="sketch"
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
      v-if="!component.isItemHidden(helper.id) && (component.id === document.activeComponent?.id || document.activeFeature instanceof JointFeature || (projecting && helper instanceof PointHelper))"
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
      :canvas="canvas"
      :parent-transform="transform"
      :parent-highlighted="highlighted"
      :parent-selected="selected"
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
  import { ProjectTool } from '../js/tools.js'
  import { sectionBounds, sectionPlaneRelation } from '../js/core/section-bounds.js'

  const props = defineProps(['document', 'component', 'displayMode', 'colorMode', 'activeHandle', 'activeTool', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform', 'sectionPlane', 'sectionSide'])
  const emit = defineEmits(['handleMouseDown', 'handleMouseUp', 'handleMouseMove', 'handleMouseLeave', 'handlePointerDown', 'handleWheel', 'dimensionMouseUp', 'dimensionMouseDown', 'dimensionMouseMove'])
  // defineOptions({ inheritAttrs: false })

  const highlight = inject('highlight')

  const active = computed(() => props.parentActive || props.component.id == props.document.activeComponent?.id || props.document.activeFeature instanceof JointFeature )
  const highlighted = computed(() => props.parentHighlighted || props.component == highlight.value )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.component) )
  const projecting = computed(() => props.activeTool instanceof ProjectTool )

  const transform = computed(() => {
    return (props.parentTransform || new THREE.Matrix4())
      .clone()
      .multiply(props.component.localTransform())
  })

  const bounds = computed(() => sectionBounds(props.component.compound))

  const compound = computed(() => {
    // Slice model using all visible saved sections and, while sketching,
    // the active sketch plane expressed in this component's local space.
    const sections = getSections(props.component).filter(sec => !sec.hidden )
    if(props.sectionPlane) {
      sections.push({
        getTransform: () => transform.value.clone().invert().multiply(props.sectionPlane),
        side: props.sectionSide,
      })
    }
    if(!sections.length) return props.component.compound
    const originalFaces = props.component.compound.solids().flatMap(solid => solid.faces())
    const planes = sections.map(section => section.getTransform())
    const cut = sections.reduce((shape, section, index) => {
      if(!shape) return null
      // Every subsequent cut is a subset of the original compound. Its cached
      // enclosure remains safe, without meshing intermediate Boolean results.
      const relation = sectionPlaneRelation(bounds.value, planes[index])
      if(relation) {
        const keepsShape = section.side ? relation < 0 : relation > 0
        return keepsShape ? shape : null
      }
      return shape.halfCut(planes[index], section.side)
    }, props.component.compound)
    if(!cut) return null
    if(cut !== props.component.compound) markSectionFaces(cut, planes, originalFaces)
    return cut
  })

  function markSectionFaces(compound, planes, originalFaces) {
    const sectionPlanes = planes.map(plane => ({
      origin: new THREE.Vector3().setFromMatrixPosition(plane),
      normal: new THREE.Vector3().setFromMatrixColumn(plane, 2).normalize(),
    }))
    compound.solids().flatMap(solid => solid.faces()).forEach(face => {
      // A pre-existing face may lie exactly on the cutting plane (notably when
      // sketching on a face). Only newly generated cut caps should be hatched.
      if(originalFaces.some(original => original.isSame(face))) return
      const facePlane = face.getPlane()
      if(!facePlane) return
      const origin = new THREE.Vector3().setFromMatrixPosition(facePlane)
      const normal = new THREE.Vector3().setFromMatrixColumn(facePlane, 2).normalize()
      face.sectionSurface = sectionPlanes.some(section =>
        Math.abs(normal.dot(section.normal)) > 1 - 1e-6 &&
        Math.abs(section.normal.dot(origin.clone().sub(section.origin))) < 1e-5
      )
    })
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
