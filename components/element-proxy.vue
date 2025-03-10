<template></template>

<script setup>

  import materials from '../js/materials.js'

  const props = defineProps(['document', 'component', 'element', 'parentHighlighted', 'parentSelected'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.element == highlight.value )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.element) )

  let mesh

  const getMesh = () => mesh

  watch(() => props.element, () => {
    window.alcRenderer.remove(mesh)

    const vertices = props.element.tesselate()
    if(!vertices) return

    mesh = window.alcRenderer.convertLine(vertices, materials.line)
    mesh.applyMatrix4(props.element.sketch.workplane)
    mesh.alcTypes = ['curve', props.element.getAxis && 'axis']
    mesh.material = getMaterial()
    mesh.alcObject = props.element

    props.element.mesh = getMesh
    props.element.component = props.component

    window.alcRenderer.add(mesh, true)
    renderNeeded.value = true
  }, { immediate: true, deep: 1 })

  watch(() => [highlighted.value, selected.value], () => {
    mesh.material = getMaterial()
    renderNeeded.value = true
  })

  function getMaterial() {
    return materials.table.curve[
      selected.value ? 'selected' : (highlighted.value ? 'highlighted' : 'unselected')
    ][
      props.element.projection ? 'projected' : 'regular'
    ][
      props.element.isReference ? 'reference' : 'actual'
    ]
  }

  onUnmounted(() => {
    window.alcRenderer.remove(mesh)
    props.element.mesh = null
    renderNeeded.value = true
  })

</script>
