<template lang="pug">

  .box.treelet-flat-pattern(:class="{ expanded, invalid: error }")

    TreeItemHeader(
      :model-value="config.title"
      :suffix="` - ${config.format}`"
      :disabled="component.readOnly"
      label-only
      @click="toggle"
      @rename="rename"
    )

      template(#icon)
        Icon(icon="flat-pattern" :class="{ problem: error }" :title="error" fixed-width)

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

      fieldset

        h3 Flatten

        label
          ComboBox(v-model="config.format" @update:model-value="document.markChanged()" label="Format" :options="['DXF', 'STEP'].map(format => ({ value: format, label: format }))")
          span Format

        label
          ScalarInput(:value="config.kFactor" :min="0" :max="1" :step="0.01" stretch @update:value="set('kFactor', $event)")
          span K-Factor

        label
          ComboBox(:model-value="config.unit" @update:model-value="set('unit', $event)" label="Units" :options="unitOptions")
          span Units

        label(v-if="config.format == 'DXF'" title="Mark bend centerlines on their own layer")
          ToggleSwitch(:model-value="config.bendLines" @update:model-value="set('bendLines', $event)")
          span Bend Lines

        label
          button.button(title="Choose file location" @click="chooseDestination")
            Icon(icon="folder")
          span(:title="path") {{ path || 'Destination Path' }}

        label(title="Auto-export when saving document")
          ToggleSwitch(v-model="config.autoSave" @update:model-value="document.markChanged()")
          span Export on Save

    transition(
      name="message"
      @before-enter="prepareMessage"
      @enter="enterMessage"
      @after-enter="clearMessageHeight"
      @leave="leaveMessage"
    )

      .message-area(v-if="error")
        .error-msg {{ error }}

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

  .problem
    color: var(--warn) !important

  .invalid
    border-color: $warn !important

  .message-area
    overflow: hidden
    transition: height 0.25s, opacity 0.25s

  .error-msg
    min-height: 16px
    line-height: 16px
    font-size: 12px
    font-weight: bold
    padding: 6px 10px
    text-align: center
    color: lighten($warn, 65%)
    background: darken($warn, 50%)

  .message-enter-from
  .message-leave-to
    opacity: 0

</style>


<script>
  import { markRaw } from 'vue'
  import { exportFlatPattern } from './../js/core/flat-pattern.js'
  import { flatPattern } from './../js/core/sheet-metal.js'
  import { featureError } from './../js/core/errors.js'
  import { chooseSavePath } from './../js/utils.js'

  export default {
    name: 'TreeletFlatPattern',

    props: {
      config: Object,
      component: Object,
      document: Object,
    },

    data() {
      return {
        path: this.config.path || null,
        error: null,
      }
    },

    computed: {
      expanded() {
        return this.document.activeExportConfig === this.config &&
          this.document.activeExportComponentId === this.component.id
      },

      // Everything the unfolded body depends on, while the treelet is open
      flattenInputs() {
        return this.expanded && [this.component.compound, this.config.kFactor]
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
        this.path = this.path.replace(/\.(dxf|step|stp)$/i, `.${format.toLowerCase()}`)
        this.config.path = this.path
      },

      flattenInputs: {
        handler() { this.flatten() },
        immediate: true,
      },
    },

    beforeUnmount() {
      this.clearFlattened()
    },

    methods: {
      // The open treelet shows its component unfolded
      flatten() {
        this.clearFlattened()
        this.error = null
        if(!this.expanded) return
        try {
          const compound = flatPattern(this.component.compound, this.config.kFactor).unfolded
          this.flattened = markRaw({ componentId: this.component.id, compound })
          this.document.displayCompound = this.flattened
        } catch(error) {
          this.error = featureError(error, 'Flat pattern could not be computed').msg
        }
      },

      clearFlattened() {
        if(this.flattened && this.document.displayCompound === this.flattened) this.document.displayCompound = null
        this.flattened = null
      },

      prepareMessage(element) {
        element.style.height = '0px'
      },

      enterMessage(element) {
        element.style.height = `${element.scrollHeight}px`
      },

      clearMessageHeight(element) {
        element.style.height = ''
      },

      leaveMessage(element) {
        element.style.height = `${element.getBoundingClientRect().height}px`
        element.offsetHeight
        element.style.height = '0px'
      },

      rename(title) {
        this.config.title = title
        this.document.markChanged()
      },

      toggle() {
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

      async chooseDestination() {
        const path = await chooseSavePath(this.config.format.toLowerCase(), this.component.creator.title)
        if(!path) return
        this.path = path
        this.config.path = path
        this.document.markChanged()
      },

      async exportFile() {
        if(!this.path) {
          await this.chooseDestination()
          if(!this.path) return
        }
        this.config.path = this.path
        try {
          const path = await exportFlatPattern(this.component, this.config)
          if(path && path != this.config.path) {
            this.path = this.config.path = path
            this.document.markChanged()
          }
        } catch(error) {
          this.document.bus.emit('toast', { title: 'Flatten', message: `Export failed: ${error.message || error}` })
        }
      },

      remove() {
        if(this.expanded) this.document.activateExport(null)
        this.component.creator.flatPatterns =
          this.component.creator.flatPatterns.filter(config => config !== this.config)
        this.document.markChanged()
      },
    },
  }
</script>
