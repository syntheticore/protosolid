<template lang="pug">
  .bordered.feature-box(:class="{'has-header': this.showHeader}")

    header(v-if="showHeader")
      fa-icon(:icon="activeFeature.icon" title="Confirm")
      .title {{ activeFeature.title }}

    .main
      .settings
        .setting(v-for="(setting, key) in activeFeature.settings")
          span {{ setting.title }}
          .picker(
            v-if="needsPicker(setting, true)"
            :ref="key"
            :class="{active: activePicker == key, filled: activeFeature[key]}"
            @click="pick(setting.type, key)"
          )
          NumberInput(
            v-if="setting.type == 'length' || setting.type == 'angle'"
            :component="document.tree"
            :value.sync="activeFeature[key]"
            @update:value="update"
            @error="error = $event"
          )
          input(
            type="text"
            v-if="setting.type == 'text'"
            :value.sync="activeFeature[key]"
            spellcheck="false"
          )
          IconToggle(
            v-if="setting.type == 'bool'"
            :icons="setting.icons"
            :active.sync="activeFeature[key]"
            @update:active="update"
          )
          RadioBar(
            v-if="setting.type == 'select'"
            :items="setting.options"
            :chosen.sync="activeFeature[key]"
            @update:chosen="update"
          )
          MaterialSelector(
            v-if="setting.type == 'material'"
            :chosen.sync="activeFeature[key]"
            @update:chosen="update"
          )
      transition(name="fade")
        .error-msg(v-if="error", :class="errorStyle") {{ error[0] }}

    .confirmation
      button.ok(
        title="Confirm"
        v-if="!isSketchFeature || showHeader"
        :disabled="!canConfirm"
        @click="confirm"
      )
        fa-icon(icon="check-circle")
      button.cancel(
        title="Cancel"
        v-if="!isSketchFeature || !showHeader"
        @click="cancel"
      )
        fa-icon(icon="times-circle")

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
    &:not(.has-header)
      margin-left: 0
      border-bottom-left-radius: 4px
    &.error
      color: lighten($red, 65%)
      background: darken($red, 50%)
    &.warning
      color: lighten($warn, 65%)
      background: darken($warn, 50%)

  .fade-enter
  .fade-leave-to
    opacity: 0
    padding-top: 0
    padding-bottom: 0
    margin-bottom: -12px
</style>


<script>
  import IconToggle from './icon-toggle.vue'
  import RadioBar from './radio-bar.vue'
  import NumberInput from './number-input.vue'
  import MaterialSelector from './material-selector.vue'

  import * as THREE from 'three'

  import { DummyTool, ManipulationTool } from './../tools.js'
  import { CreateSketchFeature } from './../features.js'
  import { vec2three } from './../utils.js'

  export default {
    name: 'FeatureBox',

    components: {
      IconToggle,
      RadioBar,
      NumberInput,
      MaterialSelector,
    },

    props: {
      document: Object,
      activeTool: Object,
      activeFeature: Object,
      showHeader: Boolean,
    },

    watch: {
      activeTool: function(tool) {
        if(tool.constructor !== DummyTool) return
        this.activePicker = null
        this.$root.$off('picked')
      },
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
        return this.activeFeature.isComplete() && !(this.error && this.error[1] == 'error')
      },

      isSketchFeature: function() {
        return this.activeFeature && this.activeFeature.constructor === CreateSketchFeature
      },

      errorStyle: function() {
        const style = { 'has-header': this.showHeader }
        if(this.error) style[this.error[1]] = true
        return style
      },
    },

    mounted: function() {
      this.$root.$on('enter-pressed', this.confirm)
      this.$root.$on('escape', this.onEscape)
      this.$root.$on('resize', this.updatePaths)
      this.$root.$on('deactivate-feature', this.deactivateFeature)
      this.startValues = this.activeFeature.getValues()
      this.activateBaseTool()
      setTimeout(() => {
        this.updatePaths()
        this.pickAll()
      }, 0)
    },

    beforeDestroy: function() {
      // Remove temporary feature when feature creation was not completed
      if(this.status !== 'confirmed' && !this.showHeader) {
        this.$emit('remove-feature', this.activeFeature)
      }
      this.activeFeature.dispose()
      this.$root.$off('enter-pressed', this.confirm)
      this.$root.$off('escape', this.onEscape)
      this.$root.$off('resize', this.updatePaths)
      this.$root.$off('deactivate-feature', this.deactivateFeature)
      this.$root.$emit('unpreview-feature')
      this.$root.$emit('activate-toolname', 'Manipulate')
      this.destroyed = true
    },

    methods: {
      pickAll: function() {
        const pickerKeys = Object.keys(this.activeFeature.settings).filter(key =>
          !this.activeFeature[key] && this.needsPicker(this.activeFeature.settings[key])
        )
        let chain = Promise.resolve()
        for(const key of pickerKeys) {
          const setting = this.activeFeature.settings[key]
          chain = chain.then(() => this.pick(setting.type, key) )
        }
      },

      needsPicker: function(setting, includeOptionals) {
        return ['profile', 'curve', 'axis', 'plane', 'face', 'edge'].some(type =>
          type == setting.type && (!setting.optional || includeOptionals)
        )
      },

      pick: function(type, key) {
        return new Promise((resolve) => {
          this.$root.$off('picked')
          this.$root.$once('picked', (item) => {
            let itemRef;
            if(this.activeFeature.settings[key].type == 'profile') {
              itemRef = item.make_reference()
            } else if(this.activeFeature.settings[key].type == 'face') {
              itemRef = item.make_face_reference()
            } else if(this.activeFeature.settings[key].type == 'plane') {
              itemRef = item.make_planar_reference()
            } else if(this.activeFeature.settings[key].type == 'axis') {
              itemRef = item.make_axial_reference()
            }
            if(this.activeFeature.settings[key].multi) {
              const currentItems = (this.activeFeature[key] && this.activeFeature[key]()) || []
              this.activeFeature[key] = () => currentItems
              const oldItem = currentItems.find(otherRef => {
                return otherRef.item_id() == item.id()
              })
              if(oldItem) {
                oldItem.free()
                currentItems.splice(currentItems.indexOf(oldItem), 1)
              } else {
                currentItems.push(itemRef)
              }
            } else {
              const oldRef = this.activeFeature[key]
              if(oldRef) oldRef().free()
              // Hide heavy data from Vue in a closure
              this.activeFeature[key] = () => itemRef
            }
            this.update()
            this.updatePaths()
            this.activePicker = null
            this.updatePicker = null
            resolve()
            if(this.activeFeature.settings[key].autoConfirm) this.confirm()
            if(this.activeFeature.settings[key].autoMulti) setTimeout(() => this.pick(type, key), 0)
          })
          this.activePicker = key
          this.updatePicker = () => {
            const { pickerPos, color } = this.getPickerInfo(key)
            this.$root.$emit('pick', type, pickerPos, color)
          }
          this.updatePicker()
        })
      },

      updatePaths: function() {
        setTimeout(() => {
          if(this.destroyed) return
          this.$root.$emit('clear-pickers')
          for(const key in this.activeFeature.settings) {
            const data = this.activeFeature[key]
            if(data && this.needsPicker(this.activeFeature.settings[key], true) && data()) {
              const { pickerPos, color } = this.getPickerInfo(key)
              const refs = Array.isArray(data()) ? data() : [data()]
              refs.forEach(ref => {
                const item = ref.item()
                this.$root.$emit('show-picker', pickerPos, vec2three(item.center()), color)
                item.free()
              })
            }
          }
          if(this.updatePicker) this.updatePicker()
        })
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
        this.activeFeature.update()
        this.$root.$emit('regenerate')
        this.error = this.activeFeature.real.error()
        const preview = this.activeFeature.real.preview()
        if(preview) this.$root.$emit('preview-feature', this.activeFeature.component, preview)
      },

      confirm: function() {
        if(!this.canConfirm) return
        this.status = 'confirmed'
        if(this.error) this.activeFeature.real.repair()
        this.activeFeature.confirm(this)
        this.close()
      },

      cancel: function() {
        this.close()
      },

      close: function() {
        if(this.isSketchFeature) this.activeFeature.real.invalidate()
        this.$emit('close')
      },

      deactivateFeature() {
        // Restore feature state on cancel
        if(this.status !== 'confirmed' && this.showHeader && !this.isSketchFeature) {
          this.activeFeature.setValues(this.startValues)
          this.update()
        }
      },

      onEscape: function() {
        if(this.activeTool.constructor === DummyTool || this.activeTool.constructor === ManipulationTool) {
          this.cancel()
        } else {
          this.activateBaseTool()
        }
      },

      activateBaseTool() {
        this.$root.$emit('activate-toolname', this.isSketchFeature ? 'Manipulate' : 'Dummy')
      }
    },
  }
</script>
