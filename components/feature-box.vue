<template lang="pug">

  .bordered.feature-box(:class="{'has-header': this.showHeader}")

    header(v-if="showHeader")

      Icon(:icon="activeFeature.constructor.icon" title="Confirm")

      .title {{ activeFeature.title }}

    .main

      .settings

        .setting(
          v-for="(setting, key) in orderedSettings"
          v-show="activeFeature.isSettingVisible(setting)"
        )

          span {{ setting.title }}

          .picker(
            v-if="activeFeature.needsPicker(key, true)"
            :ref="key"
            :class="{ active: activePicker == key, filled: activeFeature[key] }"
            @click="pick(key)"
          )

          NumberInput(
            v-if="setting.type == 'length' || setting.type == 'angle'"
            :component="document.top()"
            :dimension="setting.type"
            :allow-negative="setting.allowNegative"
            v-model:value="activeFeature[key]"
            :expression="activeFeature.expressions[key]"
            :auto-focus="key === autoFocusSetting"
            @focusin="setting.autoFocus !== false && (focusedSetting = key)"
            @update:value="update"
            @expression="activeFeature.setExpression(key, $event)"
            @error="error = $event"
          )

          IntegerInput(
            v-if="setting.type == 'integer'"
            v-model:value="activeFeature[key]"
            :min="setting.min"
            :max="setting.max"
            @update:value="update"
            @error="error = $event"
          )

          ScalarInput(
            v-if="setting.type == 'number'"
            v-model:value="activeFeature[key]"
            :min="setting.min"
            :max="setting.max"
            :step="setting.step || 1"
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

          EnumInput(
            v-if="setting.type == 'enum'"
            v-model:value="activeFeature[key]"
            :options="settingOptions(key, setting)"
            @update:value="updateSetting"
          )

          RadioBar(
            v-if="setting.type == 'select'"
            :items="setting.options"
            v-model:chosen="activeFeature[key]"
            @update:chosen="updateSetting(key, $event)"
          )

          button.button.file-input(
            v-if="setting.type == 'file'"
            :title="activeFeature[key] || 'Choose a ProtoSolid document'"
            @click="chooseFile(key)"
          )
            Icon(icon="file-import")
            span {{ activeFeature.fileLabel?.(key) || 'Choose file…' }}

          //- MaterialSelector(
          //-   v-if="setting.type == 'material'"
          //-   v-model:chosen="activeFeature[key]"
          //-   @update:chosen="update"
          //- )

      transition(name="fade")
        .error-msg(v-if="error", :class="errorStyle") {{ error.msg || error }}

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

  .main
    overflow: hidden

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
      white-space: nowrap

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
    white-space: nowrap

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

    > .picker
      margin-top: 8px

    .has-header &
      flex-direction: column-reverse

      > :not(span)
        margin-top: 0
        margin-bottom: 8px
    input
    .radio-bar
    .icon-toggle
      flex: 1 1 auto

  .file-input
    align-items: center
    display: flex
    max-width: 180px
    min-height: 28px

    span
      overflow: hidden
      text-overflow: ellipsis
      white-space: nowrap

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
  import * as THREE from 'three'

  import { DummyTool, ManipulationTool } from './../js/tools.js'
  import { shallowEqual } from './../js/utils.js'
  import { CreateSketchFeature } from './../js/core/features.js'
  import { CurveReference, JointReference, PatternInputReference } from './../js/core/references.js'
  import { referenceComponentId, worldTransform } from './../js/core/assembly.js'
  import PickerSession from './../js/picker-session.js'

  export default {
    name: 'FeatureBox',

    inject: ['bus'],

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
        focusedSetting: null,
        status: null,
        pendingUpdates: 0,
      }
    },

    computed: {
      busy() {
        return this.document.isRegenerating || this.pendingUpdates > 0
      },
      autoFocusSetting() {
        const keys = Object.keys(this.orderedSettings).filter(key => {
          const setting = this.orderedSettings[key]
          return ['length', 'angle'].includes(setting.type) &&
            setting.autoFocus !== false && this.activeFeature.isSettingVisible(setting)
        })
        return keys.includes(this.focusedSetting) ? this.focusedSetting : keys[0]
      },

      orderedSettings: function() {
        return Object.fromEntries(
          Object.entries(this.activeFeature.settings).sort(([, left], [, right]) =>
            Number(this.activeFeature.isPickerSetting(right)) - Number(this.activeFeature.isPickerSetting(left))
          )
        )
      },

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
      this.bus.on('keydown', this.onKeydown)
      this.pickerSession = new PickerSession(this.bus, {
        available: key => !this.destroyed && this.activeFeature.needsPicker(key, true),
        defaultKey: () => Object.keys(this.orderedSettings).find(key =>
          this.activeFeature.needsPicker(key, true)
        ),
        activate: this.activatePicker,
        select: this.selectPickedItem,
        deactivate: this.deactivatePicker,
        complete: async (key, changed, current) => {
          await this.update()
          if(!current() || this.destroyed) return
          if(changed && this.activeFeature.settings[key].autoConfirm) this.confirm()
        },
      })
      this.document.on('deactivate-feature', this.deactivateFeature)
      this.document.on('regenerated', this.refreshError)
      this.startValues = this.activeFeature.getValues()
      this.startExpressions = { ...this.activeFeature.expressions }
      this.activateBaseTool()
      setTimeout(async () => {
        const activation = this.activeFeature.activate?.()
        this.updatePaths()
        // if(this.activeFeature.error) this.update()
        this.refreshError()
        this.activeFeature.updateGizmos()
        this.previewFeature()
        this.pickAll()
        await activation
        if(this.destroyed) return
        this.refreshError()
      }, 0)
    },

    beforeUnmount: function() {
      this.pickerSession.dispose()
      // Remove temporary feature when feature creation was not completed
      if(this.status !== 'confirmed' && !this.showHeader) {
        this.document.removeFeature(this.activeFeature)
      }
      this.activeFeature.dispose()
      this.bus.off('enter-pressed', this.confirm)
      this.bus.off('escape', this.onEscape)
      this.bus.off('resize', this.updatePaths)
      this.bus.off('keydown', this.onKeydown)
      this.document.off('deactivate-feature', this.deactivateFeature)
      this.document.off('regenerated', this.refreshError)
      this.bus.emit('unpreview-feature')
      this.bus.emit('clear-pickers')
      this.bus.emit('activate-tool', ManipulationTool)
      this.destroyed = true
    },

    methods: {
      pickAll: async function() {
        const inputs = Object.entries(this.activeFeature.settings).filter(([key, setting]) =>
          !this.activeFeature[key] && (
            this.activeFeature.needsPicker(key) ||
            (!this.showHeader && setting.type == 'file' && setting.autoActivate)
          )
        )
        for(const [key, setting] of inputs) {
          if(this.destroyed) return
          if(setting.type == 'file') await this.chooseFile(key)
          else if(!await this.pick(key)) return
        }
      },

      settingOptions: function(key, setting) {
        return this.activeFeature.settingOptions?.(key) || setting.options
      },

      chooseFile: async function(key) {
        try {
          await this.activeFeature.chooseFile?.(key)
          this.update()
        } catch(error) {
          if(error == 'canceled') return
          this.activeFeature.activationError = {
            type: 'error',
            msg: error?.message || error?.msg || String(error),
          }
          this.refreshError()
        }
      },

      refreshError: function() {
        const errors = [this.activeFeature.error, this.activeFeature.activationError].filter(Boolean)
        this.error = errors.find(error => error.type == 'error') || errors[0] || null
      },

      pick: function(key, restart) {
        if(this.activePicker == key && !restart) {
          this.cancelPick()
          this.update()
          return Promise.resolve(false)
        }
        return this.pickerSession.start(key)
      },

      selectPickedItem: function(key, item) {
        const setting = this.activeFeature.settings[key]
        const type = setting.type
        // Build reference of requested type from item
        let itemRef
        if(type == 'profile' || type == 'solid') {
          itemRef = item.reference()
        } else if(type == 'patternInput') {
          itemRef = PatternInputReference.fromItem(item)
        } else if(type == 'point') {
          itemRef = item.pointReference()
        } else if(type == 'componentRef') {
          itemRef = item.componentReference()
        } else if(type == 'face') {
          itemRef = item.faceReference()
        } else if(type == 'edge') {
          itemRef = item.edgeReference()
        } else if(type == 'plane') {
          itemRef = item.planarReference()
        } else if(type == 'axis') {
          itemRef = item.axialReference()
        } else if(type == 'curve') {
          itemRef = new CurveReference(item)
        } else if(type == 'joint') {
          itemRef = new JointReference(item)
        }

        // Add item reference to feature
        if(setting.multi) {
          // Toggle items in multi select mode
          const currentItems = (this.activeFeature[key] && [...this.activeFeature[key]()]) || []
          this.activeFeature[key] = () => currentItems
          const oldItem = currentItems.find(otherRef => {
            return otherRef.matches ? otherRef.matches(item) : otherRef.item.id == item.id
          })
          if(oldItem) {
            currentItems.splice(currentItems.indexOf(oldItem), 1)
          } else {
            currentItems.push(itemRef)
          }
        } else {
          // Hide heavy data from Vue in a closure
          this.activeFeature[key] = () => itemRef
        }
        this.activeFeature.pickedInput?.(key, itemRef, item)
        this.updatePaths()
      },

      activatePicker: function(key) {
        const type = this.activeFeature.settings[key].type
        // Show old model state while picker is active
        this.activeFeature.suppressUpdate = true
        if(this.activeFeature.isComplete()) this.update() // Don't regenerate for fresh features

        this.activePicker = key
        this.bus.featurePickerActive = type == 'patternInput'
        this.bus.componentPickerActive = type == 'componentRef' || type == 'patternInput'
        this.bus.jointPickerActive = type == 'joint'
        this.updatePicker = () => {
          const { pickerPos, color } = this.getPickerInfo(key)
          this.bus.emit('pick', type, pickerPos, color, item => this.activeFeature.acceptsInput(item))
        }
        this.updatePicker()
      },

      cancelPick: function() {
        this.pickerSession.cancel()
      },

      deactivatePicker: function() {
        this.activePicker = null
        this.updatePicker = null
        this.activeFeature.suppressUpdate = false
        this.bus.featurePickerActive = false
        this.bus.componentPickerActive = false
        this.bus.jointPickerActive = false
        this.activateBaseTool()
      },

      // Activate pickers using number keys
      onKeydown: function(key) {
        key = Number(key)
        if(isNaN(key)) return
        const entry = Object
          .entries(this.activeFeature.settings)
          .filter(([key, _]) => this.activeFeature.needsPicker(key, true) )
          [key - 1]
        if(!entry) return
        this.pick(entry[0], true)
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
                if(!item) return // Lost references stay editable, but have no picker position.
                this.bus.emit('show-picker', pickerPos, this.getCenter(item, ref), color)
                // item.free()
              })
            }
          }
          if(this.updatePicker) this.updatePicker()
        })
      },

      getCenter(item, reference) {
        let center
        if(item instanceof THREE.Matrix4) {
          center = new THREE.Vector3().setFromMatrixPosition(item)
        } else if(item.componentA && item.frameA) {
          const component = this.document.top().findChild(item.componentA)
          return component
            ? new THREE.Vector3().setFromMatrixPosition(worldTransform(component).multiply(item.frameA))
            : new THREE.Vector3()
        } else {
          center = item.center()
        }
        const componentId = referenceComponentId(reference) ||
          item.sketch?.component?.id ||
          item.sketch?.creator?.componentOccurrenceId ||
          item.sketch?.creator?.componentId
        const component = componentId && this.document.top().findChild(componentId)
        return component ? center.applyMatrix4(worldTransform(component)) : center
      },

      getPickerInfo: function(key) {
        const picker = this.$refs[key][0]
        const pickerRect = picker.getBoundingClientRect()
        const pickerPos = {
          x: pickerRect.left + (pickerRect.width / 2),
          y: pickerRect.top + (pickerRect.height / 2),
        }
        const style = window.getComputedStyle(picker)
        const color = style.getPropertyValue('background-color')
        return { pickerPos, color }
      },

      update: async function() {
        this.pendingUpdates++
        try {
          this.document.timeline.invalidateFeature(this.activeFeature)
          await this.document.regenerate()
          if(this.destroyed) return
          this.activeFeature.updateGizmos()
          this.refreshError()
          this.previewFeature()
          this.updatePaths()
        } finally {
          this.pendingUpdates--
        }
      },

      updateSetting: async function(key, value) {
        this.activeFeature.settingChanged?.(key, value)
        if(this.activePicker && !this.activeFeature.needsPicker(this.activePicker, true)) {
          this.cancelPick()
        }
        await this.update()
        if(this.destroyed) return
        if(!this.activePicker) this.$nextTick(() => this.pickAll())
      },

      previewFeature: function() {
        const preview = this.activeFeature.preview()
        if(preview) this.bus.emit('preview-feature', preview, this.activeFeature.operation == 'cut')
        else this.bus.emit('unpreview-feature')
      },

      confirm: async function() {
        if(this.status === 'confirmed') return
        // Enter can commit a focused input and start regeneration before the
        // global enter handler reaches the feature box. Preserve that confirm
        // request until the committed value has finished updating.
        if(this.busy) {
          await this.document.regenerationQueue.catch(() => {})
          if(this.destroyed || this.status === 'confirmed') return
        }
        if(!this.canConfirm) return
        this.status = 'confirmed'
        if(this.error) this.activeFeature.repair()
        if(!this.showHeader) this.activeFeature.confirm()
        this.activeFeature.commit?.()
        this.close()
      },

      close: function() {
        if(this.busy && this.status !== 'confirmed') return
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
          (!shallowEqual(this.activeFeature.getValues(), this.startValues) ||
            !shallowEqual(this.activeFeature.expressions, this.startExpressions)) // Only if feature actually changed
        ) {
          this.activeFeature.setValues(this.startValues)
          this.activeFeature.expressions = { ...this.startExpressions }
          this.update()
        }
      },

      onEscape: function() {
        if(this.busy) return
        if(this.activePicker) {
          this.cancelPick()
          this.update()
        } else if(this.activeTool.constructor === DummyTool || this.activeTool.constructor === ManipulationTool) {
          this.close()
        } else {
          this.activateBaseTool()
        }
      },

      activateBaseTool() {
        this.bus.emit('activate-tool', this.isSketchFeature ? ManipulationTool : DummyTool)
      },
    },
  }
</script>
