<template lang="pug">

  .box.treelet-export(:class="{ expanded }")

    TreeItemHeader(
      :model-value="config.title"
      :suffix="` - ${config.format.toUpperCase()}`"
      :disabled="component.readOnly"
      label-only
      @click="toggle"
      @rename="rename"
    )

      template(#icon)
        Icon(icon="file-export" fixed-width)

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

    .content.form(v-if="expanded" @input="document.markChanged()")

      fieldset

        h3 Export Settings

        label
          ComboBox(v-model="config.format" @update:model-value="document.markChanged()" label="Format" :options="['STL', '3MF', 'STEP'].map(format => ({ value: format, label: format }))")
          span Format

        template(v-if="isMesh")

          label
            input.input(type="number" v-model="config.maxDistance" min="0.0" step="0.01" max="10.0")
            span Max Deviation

          label
            input.input(type="number" v-model="config.maxAngle" min="0.0" step="0.1" max="360.0")
            span Max Angular Deviation

        label
          button.button(title="Choose file location" @click="chooseDestination")
            Icon(icon="folder")
          span(:title="path") {{ path || 'Destination Path' }}

        template(v-if="!isMesh")

          label
            ComboBox(:model-value="config.structure || 'assembly'" @update:model-value="set('structure', $event)" label="Structure" :options="structureOptions")
            span Structure

          label
            ComboBox(:model-value="config.unit || 'mm'" @update:model-value="set('unit', $event)" label="Units" :options="unitOptions")
            span Units

          label(title="Export hidden components and bodies")
            input(type="checkbox" v-model="config.includeHidden")
            span Include Hidden

          label(title="Export component colors")
            input(type="checkbox" :checked="config.colors !== false" @change="set('colors', $event.target.checked)")
            span Colors

        label(title="Auto-export when saving document")
          input(type="checkbox" v-model="config.autoSave")
          span Export on Save

</template>


<style lang="stylus" scoped>

  header
    :deep(h2.tree-item-name)
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
  import { exportConfigured } from './../js/core/export.js'
  import { chooseSavePath } from './../js/utils.js'

  export default {
    name: 'TreeletExport',

    props: {
      config: Object,
      component: Object,
      document: Object,
    },

    data() {
      return {
        path: this.config.path || null,
      }
    },

    computed: {
      expanded() {
        return this.document.activeExportConfig === this.config &&
          this.document.activeExportComponentId === this.component.id
      },

      isMesh() {
        return this.config.format != 'STEP'
      },

      structureOptions() {
        return [
          { value: 'assembly', label: 'Assembly' },
          { value: 'flat', label: 'Flat Assembly' },
          { value: 'single', label: 'Single Part' },
        ]
      },

      unitOptions() {
        return [
          { value: 'mm', label: 'Millimeters' },
          { value: 'in', label: 'Inches' },
        ]
      },
    },

    watch: {
      'config.format': function(format) {
        if(!this.path) return
        const extension = format.toLowerCase()
        const updatedPath = this.path.replace(/\.(stl|3mf|step|stp)$/i, `.${extension}`)
        if(updatedPath == this.path) return
        this.path = updatedPath
        this.config.path = updatedPath
      },
    },

    methods: {
      rename(title) {
        this.config.title = title
        this.document.markChanged()
      },

      toggle: function() {
        if(this.expanded) {
          this.document.activateExport(null)
        } else {
          this.document.activateComponent(this.component)
          this.document.activateExport(this.config, this.component)
        }
      },

      set(key, value) {
        this.config[key] = value
        this.document.markChanged()
      },

      chooseDestination: async function() {
        const path = await chooseSavePath(
          this.config.format.toLowerCase(),
          this.component.creator.title,
        )
        if(!path) return
        this.path = path
        this.config.path = path
        this.document.markChanged()
      },

      exportFile: async function() {
        if(!this.path) {
          await this.chooseDestination()
          if(!this.path) return
        }

        this.config.path = this.path
        const path = await exportConfigured(this.component, this.config)
        if(path && path != this.config.path) {
          this.path = path
          this.config.path = path
          this.document.markChanged()
        }
      },

      remove: function() {
        if(this.expanded) this.document.activateExport(null)
        this.component.creator.exportConfigs =
          this.component.creator.exportConfigs.filter(conf => conf !== this.config )
        this.document.markChanged()
      },
    },
  }
</script>
