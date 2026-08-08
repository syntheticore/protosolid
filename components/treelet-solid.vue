<template lang="pug">

  li.treelet-solid(
    @mouseenter="$emit('update:highlight', solid)"
    @mouseleave="$emit('update:highlight', null)"
    @click="document.selection.handle(solid, bus.isCtrlPressed)"
    :class="{ hidden: isHidden }"
  )

    .box(:class="{ highlighted: solid.id == (highlight && highlight.id), selected: document.selection.hasId(solid.id) }")

      header

        Icon.eye(
          icon="eye"
          @click.stop="toggleVisibility"
        )

        Icon(icon="layer-group" fixed-width)

        h2 Solid {{ index + 1 }}

        .controls.wide

          Icon(
            icon="box" fixed-width
            title="Convert to Component"
            @click.stop="convertToComponent"
          )

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="removeSolid"
          )

</template>


<style lang="stylus" scoped>

</style>


<script setup>

  import { RemoveSolidsFeature } from './../js/core/features.js'

  const props = defineProps(['document', 'component', 'solid', 'index'])

  const bus = inject('bus')
  const highlight = inject('highlight')

  const isHidden = computed(() => props.component.creator.itemsHidden[props.solid.id] )

  function toggleVisibility() {
    props.component.creator.itemsHidden[props.solid.id] = !isHidden.value
  }

  function removeSolid() {
    const feature = new RemoveSolidsFeature(props.document)
    feature.solids = () => [props.solid.reference()]
    props.document.addFeature(feature)
    props.document.regenerate()
  }

  function convertToComponent() {
    props.document.convertBodyToComponent(props.component, props.solid)
  }

</script>
