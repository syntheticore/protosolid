<template lang="pug">

  li.treelet-solid(
    @mouseenter="$emit('update:highlight', solid)"
    @mouseleave="$emit('update:highlight', null)"
    :class="{ hidden: isHidden }"
  )

    .box(:class="{ highlighted: solid.id == (highlight && (highlight.solid || highlight).id) || isDescendantSelected, selected: document.selection.hasId(solid.id) }")

      TreeItemHeader(
        :model-value="component.itemTitle(solid.id, `Solid ${index + 1}`)"
        :selected="document.selection.hasId(solid.id)"
        :disabled="component.readOnly"
        @click="document.selection.handle(solid, bus.isCtrlPressed)"
        @rename="document.renameTreeItem(component, solid.id, $event)"
      )

        template(#icon)
          Icon.eye(
            icon="eye"
            @click.stop="toggleVisibility"
            @dblclick.stop
          )

          Icon(icon="layer-group" fixed-width)

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


<script setup>

  import { RemoveSolidsFeature } from './../js/core/features.js'

  const props = defineProps(['document', 'component', 'solid', 'index'])

  const bus = inject('bus')
  const highlight = inject('highlight')

  const isHidden = computed(() => props.component.isItemHidden(props.solid.id) )

  const isDescendantSelected = computed(() =>
    props.document.selection.items.some(item => item.solid?.id == props.solid.id)
  )

  function toggleVisibility() {
    props.component.creator.itemsHidden[props.component.sourceItemId(props.solid.id)] = !isHidden.value
    props.document.updateFeatureVisibility(props.component, props.solid.id)
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
