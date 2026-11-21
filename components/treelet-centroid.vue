<template lang="pug">

  li.treelet-centroid(
    v-if="helper"
    @mouseenter="$emit('update:highlight', helper)"
    @mouseleave="$emit('update:highlight', null)"
    :class="{ hidden: isHidden }"
  )

    .box(:class="{ highlighted: helper.id == (highlight && highlight.id), selected: document.selection.hasId(helper.id), invalid: problem }")

      TreeItemHeader(
        model-value="Centroid"
        :selected="document.selection.hasId(helper.id)"
        disabled
        :title="positionTitle"
        @click="document.selection.handle(helper, bus.isCtrlPressed)"
      )

        template(#icon)
          Icon.eye(
            icon="eye"
            @click.stop="toggleVisibility"
            @dblclick.stop
          )

          Icon.icon(icon="atom" :class="{ problem }" :title="problem" fixed-width)

        .controls(v-if="!component.readOnly")

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="remove"
          )

</template>


<style lang="stylus" scoped>

  .icon
    padding-left: 3px

  .problem
    color: var(--warn) !important

  .invalid
    border-color: $warn !important

</style>


<script setup>

  import { CentroidHelper } from '../js/core/helpers.js'

  const props = defineProps(['document', 'component'])
  const bus = inject('bus')
  const highlight = inject('highlight')

  const helper = computed(() => props.component.helpers.find(helper => helper instanceof CentroidHelper))
  const isHidden = computed(() => props.component.isItemHidden(helper.value.id))
  const problem = computed(() => {
    const deviation = props.document.timeline.centroids.errors[props.component.sourceId()]
    return deviation === undefined ? null : `Did not converge, ${deviation.toFixed(3)}mm off the actual center of mass`
  })

  const positionTitle = computed(() => {
    const position = props.component.creator.cogPosition
    if(!position) return 'No solid geometry'
    return ['x', 'y', 'z'].map(axis => `${axis.toUpperCase()} ${position[axis].toFixed(3)}mm`).join('  ')
  })

  function toggleVisibility() {
    const itemId = props.component.sourceItemId(helper.value.id)
    props.component.creator.itemsHidden[itemId] = !isHidden.value
    props.document.markChanged()
  }

  function remove() {
    props.document.selection.clear()
    return props.document.setCentroid(props.component, false)
  }

</script>
