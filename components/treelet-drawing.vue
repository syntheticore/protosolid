<template lang="pug">

  .box.treelet-drawing(:class="{ expanded }")

    TreeItemHeader(
      :model-value="drawing.title"
      :suffix="` - ${drawing.sheet}`"
      :disabled="component.readOnly"
      label-only
      @click="toggle"
      @rename="rename"
    )

      template(#icon)
        Icon(icon="drafting-compass" fixed-width)

      Icon.expand(icon="angle-right")

      .controls.wide

        Icon(
          icon="file-export" fixed-width
          title="Export"
          @click.stop="exportFile"
        )

        Icon.delete(
          icon="trash-alt" fixed-width
          title="Delete"
          @click.stop="remove"
        )

    .content.form(v-if="expanded")

      fieldset

        h3 Drawing Settings

        label
          ComboBox(:model-value="drawing.sheet" @update:model-value="set('sheet', $event)" label="Sheet" :options="sheetOptions")
          span Sheet

        label
          ComboBox(:model-value="drawing.scale" @update:model-value="set('scale', $event)" label="Scale" :options="scaleOptions")
          span Scale

        label
          ComboBox(:model-value="drawing.projection" @update:model-value="set('projection', $event)" label="Projection" :options="projectionOptions")
          span Projection

        label
          ToggleSwitch(:model-value="drawing.hiddenLines" @update:model-value="set('hiddenLines', $event)")
          span Hidden Lines

        label
          ComboBox(:model-value="drawing.format" @update:model-value="set('format', $event)" label="Format" :options="['PDF', 'SVG'].map(format => ({ value: format, label: format }))")
          span Format

        label
          button.button(title="Choose file location" @click="chooseDestination")
            Icon(icon="folder")
          span(:title="drawing.path") {{ drawing.path || 'Destination Path' }}

        label(title="Auto-export when saving document")
          ToggleSwitch(:model-value="drawing.autoSave" @update:model-value="set('autoSave', $event)")
          span Export on Save

      fieldset

        h3 Title Block

        label(v-for="field in infoFields" :key="field.key")
          input.input(
            type="text"
            :value="drawing.info[field.key]"
            :placeholder="field.placeholder"
            @keydown.stop
            @change="setInfo(field.key, $event.target.value.trim())"
          )
          span {{ field.label }}

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
  import { drawingScales, exportDrawing, sheetSizes } from './../js/core/drawing.js'
  import { toRaw } from 'vue'
  import { chooseSavePath, exportWithToast } from './../js/utils.js'

  export default {
    name: 'TreeletDrawing',

    props: {
      drawing: Object,
      component: Object,
      document: Object,
    },

    computed: {
      expanded() {
        return this.document.activeDrawing === this.drawing &&
          this.document.activeDrawingComponentId === this.component.id
      },

      sheetOptions() {
        return Object.keys(sheetSizes).map(sheet => ({ value: sheet, label: sheet }))
      },

      scaleOptions() {
        return drawingScales.map(scale => ({ value: scale, label: scale }))
      },

      infoFields() {
        return [
          { key: 'partNumber', label: 'Part No.', placeholder: 'Part number...' },
          { key: 'material', label: 'Material', placeholder: `${this.component.getMaterial()?.title || 'Material'}...` },
          { key: 'drawnBy', label: 'Drawn by', placeholder: 'Name...' },
          { key: 'revision', label: 'Revision', placeholder: 'Revision...' },
        ]
      },

      projectionOptions() {
        return [
          { value: 'first', label: 'First Angle' },
          { value: 'third', label: 'Third Angle' },
        ]
      },
    },

    watch: {
      'drawing.format': function(format) {
        if(!this.drawing.path) return
        this.drawing.path = this.drawing.path.replace(/\.(pdf|svg)$/i, `.${format.toLowerCase()}`)
      },
    },

    methods: {
      rename(title) {
        this.drawing.title = title
        this.document.markChanged()
      },

      toggle() {
        if(this.expanded) {
          this.document.activateDrawing(null)
        } else {
          this.document.activateComponent(this.component)
          this.document.activateDrawing(this.drawing, this.component)
        }
      },

      set(key, value) {
        this.drawing[key] = value
        this.drawing.revision++
        this.document.markChanged()
      },

      setInfo(key, value) {
        this.drawing.info[key] = value
        this.drawing.revision++
        this.document.markChanged()
      },

      async chooseDestination() {
        const path = await chooseSavePath(this.drawing.format.toLowerCase(), this.component.creator.title)
        if(!path) return
        this.set('path', path)
      },

      async exportFile() {
        if(!this.drawing.path) {
          await this.chooseDestination()
          if(!this.drawing.path) return
        }
        const path = await exportWithToast(this.document.bus, 'Drawing', () =>
          exportDrawing(toRaw(this.component), toRaw(this.drawing), toRaw(this.document.top())))
        if(path && path != this.drawing.path) this.set('path', path)
      },

      remove() {
        if(this.expanded) this.document.activateDrawing(null)
        const drawings = this.component.creator.drawings
        drawings.splice(drawings.indexOf(this.drawing), 1)
        this.document.markChanged()
      },
    },
  }
</script>
