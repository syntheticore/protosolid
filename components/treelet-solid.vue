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

        .controls.wide(v-if="!component.readOnly")

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

  const isHidden = computed(() => props.component.isItemHidden(props.solid.id) )

  function toggleVisibility() {
    props.component.creator.itemsHidden[props.component.sourceItemId(props.solid.id)] = !isHidden.value
    props.document.updateFeatureVisibility()
  }

  async function removeSolid() {
    if(props.component.readOnly) return
    const feature = new RemoveSolidsFeature(props.document)
    feature.solids = () => [props.solid.reference()]
    await props.document.addFeature(feature)
  }

  function convertToComponent() {
    if(props.component.readOnly) return
    props.document.convertBodyToComponent(props.component, props.solid)
  }

</script>
