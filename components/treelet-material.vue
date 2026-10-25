<template lang="pug">

  li.treelet-material(:class="{ expanded }")

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

      .content.form(v-if="expanded" @input="document.markChanged()")

        fieldset.preset

          label

            ComboBox(v-model="component.creator.material" @update:model-value="document.markChanged()" label="Material" :options="document.materials.map(material => ({ value: material, label: material.title }))")

            IconButton.clone(icon="plus" @click="cloneMaterial")

        fieldset.physical

          h3 Properties

          label

            input.input(type="text" v-model="material.title")

            span Title

          label
            ScalarInput(:value="material.density" :min="0" :max="10" :step="0.01" stretch @update:value="set('density', $event)")
            span Density (g/cm³)

          label
            ScalarInput(:value="material.elasticModulus" :min="0" :step="100" stretch @update:value="set('elasticModulus', $event)")
            span Elastic modulus (MPa)

          label
            ScalarInput(:value="material.poissonRatio" :min="0" :max="0.449" :step="0.01" stretch @update:value="set('poissonRatio', $event)")
            span Poisson ratio

        fieldset.visual

          h3 Visual Properties

          label
            input(type="color" v-model="material.color")
            span Color

          label
            ToggleSwitch(v-model="material.metal")
            span Metal

          label
            RangeSlider(v-model="material.roughness" :step="0.001")
            span Roughness

          label
            RangeSlider(v-model="material.emission" :step="0.001")
            span Emission

          label
            RangeSlider(v-model="material.transparency" :step="0.001" @update:model-value="bus.emit('render-needed')")
            span Transparency

          label(v-if="material.transparency > 0" title="Controls how strongly transmitted light bends; glass is typically 1.5.")
            ScalarInput(:value="material.refractiveIndex" :min="1" :max="2.333" :step="0.01" stretch @update:value="set('refractiveIndex', $event)")
            span Refractive Index

          label
            ToggleSwitch(v-model="material.clearcoat")
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

    watch: {
      'component.creator.material': {
        handler() {
          this.bus.emit('render-needed')
          this.bus.emit('material-changed')
        },
        deep: true
      },
    },

    computed: {
      expanded: {
        get() {
          return this.document.isTreeletExpanded(`material:${this.component.id}`)
        },
        set(value) {
          this.document.setTreeletExpanded(`material:${this.component.id}`, value)
        },
      },
    },

    methods: {
      set(key, value) {
        this.material[key] = value
        this.document.markChanged()
        this.bus.emit('render-needed')
      },

      cloneMaterial: function() {
        this.component.creator.material = this.material.clone()
        this.document.materials.push(this.component.creator.material)
        this.document.markChanged()
      },

      remove: function() {
        this.component.creator.material = null
        this.document.markChanged()
      },
    },
  }
</script>
