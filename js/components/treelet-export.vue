<template lang="pug">
  .box.export-treelet(:class="{expanded: expanded}")
    header(@click="expanded = !expanded")
      fa-icon(icon="file-export" fixed-width)
      h2 {{ config.title }} - {{ config.format.toUpperCase() }}
      fa-icon.expand(icon="angle-right")
      .controls.wide
        fa-icon(
          icon="file-export" fixed-width
          title="Export"
          :disabled="!path"
          @click.stop="exportFile"
        )
        fa-icon.delete(
          icon="trash-alt" fixed-width
          title="Delete"
          @click.stop="$emit('delete')"
        )

    .content.form(v-if="expanded")
      fieldset.physical
        h3 Export Settings

        label
          select(v-model="format")
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
          button.button(title="Choose file location" @click="pathChanged")
            fa-icon(icon="folder")
          span(:title="path") {{ path || 'Destination Path'}}
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
  import { export3mf, exportStl } from './../export.js'

  export default {
    name: 'ExportTreelet',

    props: {
      config: Object,
      component: Object,
    },

    data() {
      return {
        expanded: false,
        format: 'STL',
        path: null,
      }
    },

    methods: {
      pathChanged: function(e) {
        if(!window.electron) return
        window.electron.ipcRenderer.invoke('get-save-path', this.format).then(path => {
          this.path = path || this.path
        })
      },

      exportFile: function() {
        const stl = this.component.real.export_stl(this.component.title)
        if(!window.electron) return
        window.electron.ipcRenderer.invoke('save-file', this.path, stl).then(error => {
          alert(error || 'Saved as ' + this.path)
        })
      },
    },
  }
</script>
