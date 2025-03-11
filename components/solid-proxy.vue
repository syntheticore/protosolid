<template lang="pug">

  FaceProxy(
    v-for="face in solid.faces()"
    :key="face.tessId"
    :face="face"
    :component="component"
    :display-mode="displayMode"
    :parent-active="parentActive"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
  )

  EdgeProxy(
    v-if="parentActive"
    v-for="edge in solid.edges()"
    :key="edge.tessId"
    :edge="edge"
    :component="component"
    :display-mode="displayMode"
    :parent-active="parentActive"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
  )

</template>


<script setup>

  const props = defineProps(['document', 'component', 'solid', 'displayMode', 'parentActive', 'parentHighlighted', 'parentSelected'])
  const emit = defineEmits([])

  const highlight = inject('highlight')

  const highlighted = computed(() => props.parentHighlighted || props.solid.id == (highlight.value && highlight.value.id) )
  const selected = computed(() => props.parentSelected || props.document.selection.hasId(props.solid.id) )

  watch(() => [props.solid, props.component], () => {
    props.solid.component = props.component
  }, { immediate: true })

</script>
