<template lang="pug">
  .material-treelet(:class="{expanded: expanded}")

    header(@click="expanded = !expanded")
      fa-icon(icon="volleyball-ball" fixed-width)
      h3 {{ material.title }}
      fa-icon.expand(icon="angle-right")

    .content(v-if="expanded")
      fieldset
        legend Physical Properties
        label
          input(type="number" v-model="material.density" min="0.0" max="10.0")
          | Density
      fieldset
        legend Visual Properties
        label
          input(type="color" v-model="material.color")
          | Color
        label
          input(type="range" v-model="material.roughness" min="0.0" max="1.0")
          | Roughness
        label
          input(type="range" v-model="material.metalness" min="0.0" max="1.0")
          | Metalness
        label
          input(type="range" v-model="material.opacity" min="0.0" max="1.0")
          | Opacity
        label
          input(type="range" v-model="material.translucency" min="0.0" max="1.0")
          | Translucency
</template>


<style lang="stylus" scoped>
  header
    display: flex
    align-items: center
    h3
      margin: 0 !important

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: all 0.15s
    .expanded &
      transform: rotate(90deg)

  .content
    padding: 0 12px
</style>


<script>
  export default {
    name: 'MaterialTreelet',

    components: {},

    props: {
      material: Object,
    },

    data() {
      return {
        expanded: false,
      }
    },

    mounted() {},
    methods: {},
  }
</script>
