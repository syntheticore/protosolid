<template lang="pug">

  li.treelet-helper(
    @mouseenter="$emit('update:highlight', helper)"
    @mouseleave="$emit('update:highlight', null)"
    :class="{ hidden: isHidden }"
  )

    .box(:class="{ highlighted: helper.id == (highlight && highlight.id), selected: document.selection.hasId(helper.id) }")

      TreeItemHeader(
        :model-value="component.itemTitle(helper.id, `${helper.typename()} ${index + 1}`)"
        :selected="document.selection.hasId(helper.id)"
        :disabled="component.readOnly"
        @click="document.selection.handle(helper, bus.isCtrlPressed)"
        @rename="document.renameTreeItem(component, helper.id, $event)"
      )

        template(#icon)
          Icon.eye(
            icon="eye"
            @click.stop="toggleVisibility"
            @dblclick.stop
          )

          Icon.icon(:icon="icons[helper.typename()]" fixed-width)

</template>


<style lang="stylus" scoped>

  .icon
    padding-left: 3px

</style>


<script setup>

  const props = defineProps(['document', 'component', 'helper', 'index'])
  const bus = inject('bus')
  const highlight = inject('highlight')

  const icons = { Plane: 'map', Axis: 'construction-axis', Point: 'construction-point' }

  const isHidden = computed(() => props.component.isItemHidden(props.helper.id))

  function toggleVisibility() {
    const itemId = props.component.sourceItemId(props.helper.id)
    props.component.creator.itemsHidden[itemId] = !isHidden.value
    props.document.markChanged()
  }

</script>
