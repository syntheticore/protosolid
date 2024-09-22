<template lang="pug">

  .box.treelet-export(:class="{expanded: expanded}")
    header(@click="expanded = !expanded")
      Icon(icon="file-export" fixed-width)
      h2 {{ config.title }} - {{ config.format.toUpperCase() }}
      Icon.expand(icon="angle-right")
      .controls.wide
        Icon(
          icon="file-export" fixed-width
          title="Export"
          :disabled="!path"
          @click.stop="exportFile"
        )
        Icon.delete(
          icon="trash-alt" fixed-width
          title="Delete"
          @click.stop="remove"
        )

    .content.form(v-if="expanded")
      fieldset.physical
        h3 Export Settings

        label
          select(v-model="config.format")
            option STL
            option 3MF
            option STEP
          span Format

        label
          input(type="number" v-model="config.maxDistance" min="0.0" step="0.01" max="10.0")
          span Max Deviation

        label
          input(type="number" v-model="config.maxAngle" min="0.0" step="0.1" max="360.0")
          span Max Angular Deviation

        label
          button.button(title="Choose file location" @click="exportFile")
            Icon(icon="folder")
          span(:title="path") {{ path || 'Destination Path'}}

        label(title="Auto-export when saving document")
          input(type="checkbox" v-model="config.autoSave")
          span Export on Save

</template>


<style lang="stylus" scoped>

  header
    h2
      margin: 0 !important

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: transform 0.15s
    .expanded &
      transform: rotate(90deg)

  .form
    .button
      padding: 0
      svg
        padding: 3px

    label span
      text-overflow: ellipsis
      overflow: hidden
      max-width: 120px
      line-height: 1.2

</style>


<script>
  import { export3mf, exportStl } from './../js/core/export.js'

  export default {
    name: 'TreeletExport',

    props: {
      config: Object,
      component: Object,
    },

    data() {
      return {
        expanded: false,
        path: null,
      }
    },

    methods: {
      exportFile: async function() {
        const exporter = {
          'STL': exportStl,
          '3MF': export3mf,
        }[this.config.format]
        this.path = await exporter(this.component, this.path)
      },

      remove: function() {
        this.component.exportConfigs =
          this.component.exportConfigs.filter(conf => conf !== this.config )
      },
    },
  }
</script>
