<template lang="pug">

  li.treelet-plane(
    @mouseenter="$emit('update:highlight', plane)"
    @mouseleave="$emit('update:highlight', null)"
    :class="{ hidden: isHidden }"
  )

    .box

      header

        Icon.eye(
          icon="eye"
          @click.stop="toggleVisibility"
        )

        Icon.icon(icon="map" fixed-width)

        h2 {{ title }}

</template>


<style lang="stylus" scoped>

  .icon
    padding-left: 3px

</style>


<script setup>

  const props = defineProps(['component', 'plane', 'index'])

  const isHidden = computed(() => props.component.isItemHidden(props.plane.id))

  const title = computed(() => `Plane ${props.index + 1}`)

  function toggleVisibility() {
    const itemId = props.component.sourceItemId(props.plane.id)
    props.component.creator.itemsHidden[itemId] = !isHidden.value
  }

</script>
