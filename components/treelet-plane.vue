<template lang="pug">

  li.treelet-plane(
    @mouseenter="$emit('update:highlight', plane)"
    @mouseleave="$emit('update:highlight', null)"
    :class="{ hidden: isHidden }"
  )

    .box(:class="{ highlighted: plane.id == (highlight && highlight.id), selected: document.selection.hasId(plane.id) }")

      TreeItemHeader(
        :model-value="component.itemTitle(plane.id, `Plane ${index + 1}`)"
        :selected="document.selection.hasId(plane.id)"
        :disabled="component.readOnly"
        @click="document.selection.handle(plane, bus.isCtrlPressed)"
        @rename="document.renameTreeItem(component, plane.id, $event)"
      )

        template(#icon)
          Icon.eye(
            icon="eye"
            @click.stop="toggleVisibility"
            @dblclick.stop
          )

          Icon.icon(icon="map" fixed-width)

</template>


<style lang="stylus" scoped>

  .icon
    padding-left: 3px

</style>


<script setup>

  const props = defineProps(['document', 'component', 'plane', 'index'])
  const bus = inject('bus')
  const highlight = inject('highlight')

  const isHidden = computed(() => props.component.isItemHidden(props.plane.id))

  function toggleVisibility() {
    const itemId = props.component.sourceItemId(props.plane.id)
    props.component.creator.itemsHidden[itemId] = !isHidden.value
  }

</script>
