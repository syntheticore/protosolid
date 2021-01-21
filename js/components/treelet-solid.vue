<template lang="pug">
  li.solid-treelet(
    @mouseenter="$emit('highlight-component', component, solid.get_id())"
    @mouseleave="$emit('highlight-component', null)"
    @click="$emit('update:selection', solid)"
  )
    .box
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
    },

    data() {
      return {}
    },
  }
</script>
