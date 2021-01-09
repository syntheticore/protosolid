<template lang="pug">
  .radio-bar(@mouseleave="$emit('unhover', chosen)")
    button(
      v-for="item in items"
      @click="$emit('update:chosen', item)"
      @mouseenter="$emit('hover', item)"
    )
      fa-icon(:icon="item.icon" :title="item.title")
</template>


<style lang="stylus" scoped>
  .radio-bar
    pointer-events: all
</style>


<script>
  export default {
    name: 'RadioBar',
    props: {
      items: Array,
      chosen: Object,
    },
  }
</script>
