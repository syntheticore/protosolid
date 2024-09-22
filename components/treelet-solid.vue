<template lang="pug">

  li.treelet-solid(
    @mouseenter="$emit('update:highlight', solid)"
    @mouseleave="$emit('update:highlight', null)"
    @click="document.selection = document.selection.handle(solid, bus.isCtrlPressed)"
  )
    .box(:class="{ selected: document.selection.has(solid) }")
      header
        Icon(icon="layer-group" fixed-width)
        h2 Solid {{ index + 1 }}
        .controls
          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="document.deleteSolid(solid)"
          )

</template>


<style lang="stylus" scoped>

</style>


<script>
  export default {
    name: 'TreeletSolid',

    inject: ['bus'],

    props: {
      document: Object,
      solid: Object,
      index: Number,
      // component: Object,
      // selection: Object,
    },

    data() {
      return {}
    },
  }
</script>
