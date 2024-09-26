<template lang="pug">
  .bordered.feature-box(:class="{'has-header': this.showHeader}")

    header(v-if="showHeader")
      Icon(:icon="activeFeature.constructor.icon" title="Confirm")
      .title {{ activeFeature.title }}

    .main
      .settings
        .setting(v-for="(setting, key) in activeFeature.settings")
          span {{ setting.title }}
          .picker(
            v-if="activeFeature.needsPicker(key, true)"
            :ref="key"
            :class="{ active: activePicker == key, filled: activeFeature[key] }"
            @click="pick(setting.type, key)"
          )
          NumberInput(
            v-if="setting.type == 'length' || setting.type == 'angle'"
            :component="document.top()"
            v-model:value="activeFeature[key]"
            @update:value="update"
            @error="error = $event"
          )
          input(
            type="text"
            v-if="setting.type == 'text'"
            v-model="activeFeature[key]"
            spellcheck="false"
          )
          IconToggle(
            v-if="setting.type == 'bool'"
            :icons="setting.icons"
            v-model:active="activeFeature[key]"
            @update:active="update"
          )
          RadioBar(
            v-if="setting.type == 'select'"
            :items="setting.options"
            v-model:chosen="activeFeature[key]"
            @update:chosen="update"
          )
          MaterialSelector(
            v-if="setting.type == 'material'"
            v-model:chosen="activeFeature[key]"
            @update:chosen="update"
          )
      transition(name="fade")
        .error-msg(v-if="error", :class="errorStyle") {{ error.msg }}

    .confirmation
      button.ok(
        title="Confirm"
        v-if="!isSketchFeature || showHeader"
        :disabled="!canConfirm"
        @click="confirm"
      )
        Icon(icon="check-circle")
      button.cancel(
        title="Cancel"
        v-if="!isSketchFeature || !showHeader"
        @click="close"
      )
        Icon(icon="times-circle")

</template>


<style lang="stylus" scoped>
  .feature-box
    font-size: 12px
    display: flex
    &::before
      left: 24px

  header
    display: flex
    flex-direction: column
    justify-content: center
    align-items: center
    background: $dark1
    padding: 0 10px
    min-width: 60px
    svg
      font-size: 21px
      color: $bright1
      // transition: all 0.15s
      filter: none
    .title
      color: $bright1
      font-size: 11px
      margin-top: 6px
      font-weight: bold

  .settings
    margin: 10px
    display: flex

  .setting
    display: flex
    flex-direction: column
    align-items: center
    font-size: 12px
    color: $bright1
    font-weight: bold
    & + .setting
      margin-left: 12px
    &:nth-of-type(1) .picker
      background: $blue
      border-color: lighten($blue, 75%)
    &:nth-of-type(2) .picker
      background: $purple
      border-color: lighten($purple, 75%)
    &:nth-of-type(3) .picker
      background: $pink
      border-color: lighten($pink, 75%)
    &:nth-of-type(4) .picker
      background: $orange
      border-color: lighten($orange, 75%)
    &:nth-of-type(5) .picker
      background: $green
      border-color: lighten($green, 75%)
    > :not(span)
      margin-top: 6px
    .has-header &
      flex-direction: column-reverse
      > :not(span)
        margin-top: 0
        margin-bottom: 8px
    input
    .radio-bar
    .icon-toggle
      flex: 1 1 auto

  .picker
    width: 24px
    height: 24px
    border: 7px solid white
    border-radius: 99px
    transition: all 0.06s
    &:hover
      border-width: 0px
    &.active
      animation: 2s infinite linear rotate
      border-style: dotted
      border-width: 2px !important
    &.filled
      border-width: 2px
    @keyframes rotate
      100%
        transform: rotate(360deg)

  .confirmation
    display: flex
    flex-direction: column
    justify-content: space-around
    background: $dark1
    border-radius: 0 4px 4px 0

    button
      height: 100%
      background: none
      border: none
      color: $bright1
      font-size: 16px
      transition: all 0.15s
      padding: 0px 9px
      padding-left: 11px
      transition: all 0.15s
      &:hover
        &.ok
          color: $confirm
        &.cancel
          color: $cancel
      &:active
        &.ok
          color: darken($confirm, 40%)
        &.cancel
          color: darken($cancel, 15%)
        transition: none
      &:disabled
        color: $bright1 * 0.5 !important

  .error-msg
    font-size: 12px
    font-weight: bold
    padding: 5px 10px
    margin: 0 1px
    transition: all 0.25s
    text-align: center
    white-space: nowrap
    &:not(.has-header)
      margin-left: 0
      border-bottom-left-radius: 4px
    &.error
      color: lighten($red, 65%)
      background: darken($red, 50%)
    &.warning
      color: lighten($warn, 65%)
      background: darken($warn, 50%)

  .fade-enter-from
  .fade-leave-to
    opacity: 0
    padding-top: 0
    padding-bottom: 0
    margin-bottom: -12px
</style>


<script>
  // import IconToggle from './icon-toggle.vue'
  // import RadioBar from './radio-bar.vue'
  // import NumberInput from './number-input.vue'
  // import MaterialSelector from './material-selector.vue'

  import * as THREE from 'three'

  import { DummyTool, ManipulationTool } from './../js/tools.js'
  import { shallowEqual } from './../js/utils.js'
  import { CreateSketchFeature } from './../js/core/features.js'

  export default {
    name: 'FeatureBox',

    inject: ['bus'],

    // components: {
    //   IconToggle,
    //   RadioBar,
    //   NumberInput,
    //   MaterialSelector,
    // },

    props: {
      document: Object,
      activeTool: Object,
      activeFeature: Object,
      showHeader: Boolean,
    },

    data() {
      return {
        error: null,
        activePicker: null,
        status: null,
      }
    },

    computed: {
      canConfirm: function() {
        return this.activeFeature.isComplete() && !(this.error && this.error.type == 'error')
      },

      isSketchFeature: function() {
        return this.activeFeature && this.activeFeature.constructor === CreateSketchFeature
      },

      errorStyle: function() {
        const style = { 'has-header': this.showHeader }
        if(this.error) style[this.error.type] = true
        return style
      },
    },

    mounted: function() {
      this.bus.on('enter-pressed', this.confirm)
      this.bus.on('escape', this.onEscape)
      this.bus.on('resize', this.updatePaths)
      this.document.on('deactivate-feature', this.deactivateFeature)
      this.startValues = this.activeFeature.getValues()
      this.activateBaseTool()
      setTimeout(() => {
        this.updatePaths()
        // if(this.activeFeature.error) this.update()
        this.error = this.activeFeature.error
        this.activeFeature.updateGizmos()
        this.pickAll()
      }, 0)
    },

    beforeUnmount: function() {
      // Remove temporary feature when feature creation was not completed
      if(this.status !== 'confirmed' && !this.showHeader) {
        this.document.removeFeature(this.activeFeature)
      }
      this.activeFeature.dispose()
      this.bus.off('enter-pressed', this.confirm)
      this.bus.off('escape', this.onEscape)
      this.bus.off('resize', this.updatePaths)
      this.document.off('deactivate-feature', this.deactivateFeature)
      this.bus.emit('unpreview-feature')
      this.bus.emit('activate-toolname', 'Manipulate')
      this.destroyed = true
    },

    methods: {
      pickAll: function() {
        const pickerKeys = Object.keys(this.activeFeature.settings).filter(key =>
          !this.activeFeature[key] && this.activeFeature.needsPicker(key)
        )
        let chain = Promise.resolve()
        for(const key of pickerKeys) {
          const setting = this.activeFeature.settings[key]
          chain = chain.then(() => this.pick(setting.type, key) )
        }
      },

      pick: function(type, key) {
        // Toggle picker off if active
        if(this.activePicker) {
          this.onEscape()
          return
        }
        const setting = this.activeFeature.settings[key]

        return new Promise((resolve) => {
          this.bus.off('picked')

          this.bus.once('picked', async (item) => {
            // Build reference of requested type from item
            let itemRef;
            if(type == 'profile') {
              itemRef = item.reference()
            } else if(type == 'face') {
              itemRef = item.faceReference()
            } else if(type == 'edge') {
              itemRef = item.edgeReference()
            } else if(type == 'plane') {
              itemRef = item.planarReference()
            } else if(type == 'axis') {
              itemRef = item.axialReference()
            }

            // Add item reference to feature
            if(setting.multi || setting.autoMulti) {
              // Toggle items in multi select mode
              const currentItems = (this.activeFeature[key] && this.activeFeature[key]()) || []
              this.activeFeature[key] = () => currentItems
              const oldItem = currentItems.find(otherRef => {
                return otherRef.item.id == item.id
              })
              if(oldItem) {
                // oldItem.free()
                currentItems.splice(currentItems.indexOf(oldItem), 1)
              } else {
                currentItems.push(itemRef)
              }
            } else {
              // const oldRef = this.activeFeature[key]
              // if(oldRef) oldRef().free()

              // Hide heavy data from Vue in a closure
              this.activeFeature[key] = () => itemRef
            }

            // Regenerate feature once unsuppressed
            this.cancelPick()
            if(!setting.autoMulti) this.update()
            this.updatePaths()

            // Auto-close or auto-repick
            if(setting.autoConfirm) this.confirm()
            if(setting.autoMulti) await this.pick(type, key)

            resolve()
          })

          // Show old model state while picker is active
          this.activeFeature.suppressUpdate = true
          if(this.activeFeature.isComplete()) this.update() // Don't regenerate for fresh features

          // Generate picker curve
          this.activePicker = key
          this.updatePicker = () => {
            const { pickerPos, color } = this.getPickerInfo(key)
            this.bus.emit('pick', type, pickerPos, color)
          }
          this.updatePicker()
        })
      },

      cancelPick: function() {
        this.bus.off('picked')
        this.activePicker = null
        this.updatePicker = null
        this.activeFeature.suppressUpdate = false
      },

      updatePaths: function() {
        setTimeout(() => {
          if(this.destroyed) return
          this.bus.emit('clear-pickers')
          for(const key in this.activeFeature.settings) {
            const data = this.activeFeature[key]
            if(data && this.activeFeature.needsPicker(key, true) && data()) {
              const { pickerPos, color } = this.getPickerInfo(key)
              const refs = Array.isArray(data()) ? data() : [data()]
              refs.forEach(ref => {
                const item = ref.getItem()
                this.bus.emit('show-picker', pickerPos, this.getCenter(item), color)
                // item.free()
              })
            }
          }
          if(this.updatePicker) this.updatePicker()
        })
      },

      getCenter(item) {
        if(item instanceof THREE.Matrix4) {
          return new THREE.Vector3().setFromMatrixPosition(item)
        } else {
          return item.center()
        }
      },

      getPickerInfo: function(key) {
        const picker = this.$refs[key][0]
        const pickerRect = picker.getBoundingClientRect()
        const pickerPos = {
          x: pickerRect.left + (pickerRect.width / 2),
          y: pickerRect.top + (pickerRect.height / 2) - 38,
        }
        const style = window.getComputedStyle(picker)
        const color = style.getPropertyValue('background-color')
        return { pickerPos, color }
      },

      update: function() {
        this.document.timeline.invalidateFeature(this.activeFeature)
        this.document.regenerate()
        this.activeFeature.updateGizmos()
        this.error = this.activeFeature.error
        const preview = this.activeFeature.preview()
        if(preview) this.bus.emit('preview-feature', preview)
      },

      confirm: function() {
        if(!this.canConfirm) return
        this.status = 'confirmed'
        // if(this.error) this.activeFeature.repair()
        this.activeFeature.repair()
        if(!this.showHeader) this.activeFeature.confirm()
        this.close()
      },

      close: function() {
        this.$emit('close')
      },

      deactivateFeature() {
        // Always consider sketches as modified
        if(this.isSketchFeature) this.document.timeline.invalidateFeature(this.activeFeature)
        // Restore feature state on cancel
        if(
          this.status !== 'confirmed' &&
          this.showHeader && // Only for existing features
          !this.isSketchFeature && // Never reset sketches
          !shallowEqual(this.activeFeature.getValues(), this.startValues) // Only if feature actually changed
        ) {
          this.activeFeature.setValues(this.startValues)
          this.update()
        }
      },

      onEscape: function() {
        const wasPicking = !!this.activePicker
        this.cancelPick()
        if(this.activeTool.constructor === DummyTool || this.activeTool.constructor === ManipulationTool) {
          this.close()
        } else {
          this.activateBaseTool()
          if(wasPicking) this.update() // Regenerate with full feature applied
        }
      },

      activateBaseTool() {
        this.bus.emit('activate-toolname', this.isSketchFeature ? 'Manipulate' : 'Dummy')
      },
    },
  }
</script>
