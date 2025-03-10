<template lang="pug">

  FaceProxy(
    v-if="displayMode == 'shaded' || displayMode == 'wireShade'"
    v-for="face in solid.faces()"
    :key="face.tessId"
    :face="face"
    :component="component"
    :parent-active="parentActive"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
  )

  EdgeProxy(
    v-if="displayMode == 'wireframe' || (parentActive && displayMode == 'wireShade')"
    v-for="edge in solid.edges()"
    :key="edge.tessId"
    :edge="edge"
    :component="component"
    :parent-active="parentActive"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
  )

</template>


<script setup>

  const props = defineProps(['document', 'component', 'solid', 'displayMode', 'parentActive', 'parentHighlighted', 'parentSelected'])
  const emit = defineEmits([])

  const highlight = inject('highlight')

  const highlighted = computed(() => props.parentHighlighted || props.solid == highlight.value )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.solid) )

  watch(() => [props.solid, props.component], () => {
    props.solid.component = props.component
  }, { immediate: true })

</script>
