<template lang="pug">

  li.treelet-material(:class="{expanded: expanded}")
    .box
      header(@click="expanded = !expanded")
        Icon(icon="volleyball-ball" fixed-width)
        h2 {{ material.title }}
        Icon.expand(icon="angle-right")
        .controls
          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="remove"
          )

      .content.form(v-if="expanded")
        fieldset.physical
          h3 Physical Properties
          label
            input(type="number" v-model="material.density" min="0.0" step="0.01" max="10.0")
            span Density

        fieldset.visual
          h3 Visual Properties
          label
            input(type="color" v-model="material.color")
            span Color
          label
            input(type="checkbox" v-model="material.metal")
            span Metal
          label
            input(type="range" v-model="material.roughness" min="0.0" step="0.001" max="1.0")
            span Roughness
          label
            input(type="range" v-model="material.transparency" min="0.0" step="0.001" max="1.0")
            span Transparency
          label
            input(type="range" v-model="material.translucency" min="0.0" step="0.001" max="1.0")
            span Translucency
          label
            input(type="checkbox" v-model="material.clearcoat")
            span Clear Coat
</template>


<style lang="stylus" scoped>

  header h2
    margin: 0 !important

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: transform 0.15s
    .expanded &
      transform: rotate(90deg)

</style>


<script>
  export default {
    name: 'TreeletMaterial',

    inject: ['bus'],

    props: {
      document: Object,
      material: Object,
      component: Object,
    },

    data() {
      return {
        expanded: false,
      }
    },

    watch: {
      material: {
        handler(material) {
          this.bus.emit('render-needed')
        },
        deep: true
      },
    },

    methods: {
      remove: function() {
        const mat = this.material
        this.component.material = null
        this.document.emit('component-changed', this.component, true)
        mat.dispose()
      },
    },
  }
</script>
