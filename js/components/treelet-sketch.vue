<template lang="pug">
  li.sketch-treelet(
    @dblclick="$emit('update:active-sketch', sketch)"
  )
    //- @click="$emit('update:selection', sketch)"
    //- @mouseenter="$emit('update:highlight', sketch)"
    //- @mouseleave="$emit('update:highlight', null)"
    .box
      header
        fa-icon(icon="edit" fixed-width)
        h2 Sketch {{ index + 1 }}
        //- .controls
        //-   fa-icon.delete(
        //-     icon="trash-alt" fixed-width
        //-     title="Delete"
        //-     @click.stop="$emit('delete-sketch', sketch)"
          )
</template>


<style lang="stylus" scoped>

</style>


<script>
  export default {
    name: 'SketchTreelet',

    props: {
      sketch: Object,
      index: Number,
    },

    data() {
      return {}
    },
  }
</script>
