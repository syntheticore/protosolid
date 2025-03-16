<template lang="pug">

  //- Solids

  template(v-for="solid in compound.solids()")

    SolidProxy(
      v-if="!component.creator.itemsHidden[solid.id]"
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
      :parent-highlighted="highlighted"
      :parent-selected="selected"
      @handleMouseUp="handleMouseUp"
      @handleMouseDown="handleMouseDown"
      @handleMouseMove="handleMouseMove"
      @handleMouseLeave="handleMouseLeave"
      @dimensionMouseUp="dimensionMouseUp"
      @dimensionMouseDown="dimensionMouseDown"
      @dimensionMouseMove="dimensionMouseMove"
    )

  //- Helpers

  HelperProxy(
    v-if="component === document.activeComponent"
    v-for="helper in component.helpers"
    :key="helper.id"
    :document="document"
    :component="component"
    :helper="helper"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
  )

  //- Canvases

  template(v-for="canvas in component.creator.canvases")

    CanvasProxy(
      v-if="!canvas.hidden"
      :key="canvas.id"
      :canvas="canvas"
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
      @handleMouseUp="handleMouseUp"
      @handleMouseDown="handleMouseDown"
      @handleMouseMove="handleMouseMove"
      @handleMouseLeave="handleMouseLeave"
      @dimensionMouseUp="dimensionMouseUp"
      @dimensionMouseDown="dimensionMouseDown"
      @dimensionMouseMove="dimensionMouseMove"
    )

</template>


<script setup>

  import * as THREE from 'three'

  const props = defineProps(['document', 'component', 'displayMode', 'colorMode', 'activeHandle', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits(['handleMouseDown', 'handleMouseUp', 'handleMouseMove', 'handleMouseLeave', 'dimensionMouseUp', 'dimensionMouseDown', 'dimensionMouseMove'])
  // defineOptions({ inheritAttrs: false })

  const highlight = inject('highlight')

  const active = computed(() => props.parentActive || props.component == props.document.activeComponent )
  const highlighted = computed(() => props.parentHighlighted || props.component == highlight.value )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.component) )

  const transform = computed(() => {
    return (props.parentTransform || new THREE.Matrix4())
      .clone()
      .multiply(props.component.transform || new THREE.Matrix4())
  })

  const compound = computed(() => {
    // Slice model using visible section views
    const sections = getSections(props.component).filter(sec => !sec.hidden )
    if(!sections.length) return props.component.compound
    const section = sections[0]
    const cut = props.component.compound.halfCut(section.getTransform(), section.side)
    return cut
  })

  function getSections(comp) {
    if(!comp) return []
    return [...comp.creator.sectionViews].concat(getSections(comp.parent))
  }

  const handleMouseUp = (...args) => emit('handleMouseUp', ...args)
  const handleMouseDown = (...args) => emit('handleMouseDown', ...args)
  const handleMouseMove = (...args) => emit('handleMouseMove', ...args)
  const handleMouseLeave = (...args) => emit('handleMouseLeave', ...args)

  const dimensionMouseUp = (...args) => emit('dimensionMouseUp', ...args)
  const dimensionMouseDown = (...args) => emit('dimensionMouseDown', ...args)
  const dimensionMouseMove = (...args) => emit('dimensionMouseMove', ...args)

</script>
