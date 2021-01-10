<template lang="pug">
  button.button.icon-toggle(
    :class="{active: active}"
    @click.prevent="$emit('update:active', !active)"
  )
    fa-icon(:icon="icon" fixed-width)
</template>


<style lang="stylus" scoped>
  .icon-toggle
    flex: 1 1 auto
    margin-left: 0
    margin-right: 0
    margin-bottom: 0
    text-shadow: none
    border-radius: 4px
    &.active
      color: lighten($highlight, 20%)

</style>


<script>
  export default {
    name: 'IconToggle',

    props: {
      title: String,
      icons: Array,
      active: Boolean,
    },

    computed: {
      icon: function() {
        return this.icons[this.active ? 0 : 1]
      },
    },
  }
</script>
