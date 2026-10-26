<template lang="pug">

  FaceProxy(
    v-for="face in solid.faces()"
    :key="face.tessId"
    :document="document"
    :face="face"
    :component="component"
    :display-mode="displayMode"
    :color-mode="colorMode"
    :parent-active="parentActive"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
    :parent-transform="parentTransform"
  )

  EdgeProxy(
    v-if="parentActive || projectInactive || ['wireframe', 'hiddenLines'].includes(displayMode)"
    v-for="edge in solid.edges().filter(edge => edge.isSemantic())"
    :key="edge.tessId"
    :document="document"
    :edge="edge"
    :component="component"
    :display-mode="displayMode"
    :color-mode="colorMode"
    :parent-active="parentActive"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
    :parent-transform="parentTransform"
  )

  VertexProxy(
    v-if="parentActive && pickerType == 'point'"
    v-for="vertex in solid.cornerVertices()"
    :key="vertex.tessId"
    :vertex="vertex"
    :component="component"
    :parent-transform="parentTransform"
  )

</template>


<script setup>

  const props = defineProps(['document', 'component', 'solid', 'displayMode', 'colorMode', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform', 'projectInactive'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const pickerType = inject('picker-type')

  const highlighted = computed(() => props.parentHighlighted || props.solid.id == (highlight.value && highlight.value.id) )
  const selected = computed(() => props.parentSelected || props.document.selection.hasId(props.solid.id) )

  watch(() => [props.solid, props.component], () => {
    props.solid.component = props.component
  }, { immediate: true })

</script>
