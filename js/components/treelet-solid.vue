<template lang="pug">
  li.solid-treelet(
    @mouseenter="$emit('update:highlight', solid)"
    @mouseleave="$emit('update:highlight', null)"
    @click="$emit('update:selection', selection.handle(solid, $root.isCtrlPressed))"
  )
    .box(:class="{selected: selection.has(solid)}")
      header
        fa-icon(icon="layer-group" fixed-width)
        h2 Solid {{ index + 1 }}
        .controls
          fa-icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="$emit('delete-solid', solid)"
          )
</template>


<style lang="stylus" scoped>

</style>


<script>
  export default {
    name: 'SolidTreelet',

    props: {
      solid: Object,
      index: Number,
      component: Object,
      selection: Object,
    },

    data() {
      return {}
    },
  }
</script>
