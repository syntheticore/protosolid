<template lang="pug">
  .material-treelet
    header
      fa-icon(icon="volleyball-ball" fixed-width)
      span {{ material.title }}
      fa-icon.expand(icon="angle-right")
</template>


<style lang="stylus" scoped>
  header
    display: flex
    align-items: center
    .expand
      padding: 2px !important
      margin-left: 4px
</style>


<script>
  export default {
    name: 'MaterialTreelet',

    components: {},

    props: {
      material: Object,
    },

    data() {
      return {}
    },

    mounted() {},
    methods: {},
  }
</script>
