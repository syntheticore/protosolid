<template lang="pug">
  .bordered.feature-box

    header(v-if="showHeader")
      fa-icon(:icon="activeFeature.icon" title="Confirm")
      .title {{ activeFeature.title }}

    .main
      .settings
        .setting(v-for="(setting, key) in activeFeature.settings")
          | {{ setting.title }}
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
            @error="showError"
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
        .error(v-if="error") {{ error }}

    .confirmation
      button.ok(
        title="Confirm"
        :disabled="!canConfirm"
        @click="confirm"
      )
        fa-icon(icon="check-circle")
      button.cancel(title="Cancel" @click="cancel")
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
    > *
      margin-top: 6px
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

  .error
    background: darken($red, 50%)
    font-size: 12px
    font-weight: bold
    padding: 5px 10px
    margin-right: 1px
    transition: all 0.25s
    color: lighten($red, 65%)
    text-align: center
    border-bottom-left-radius: 4px

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

  import { ManipulationTool } from './../tools.js'
  import { CreateSketchFeature } from './../features.js'

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
        if(tool.constructor !== ManipulationTool) return
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
        return this.activeFeature.isComplete() && !this.error
      },
    },

    mounted: function() {
      this.pickAll()
      this.$root.$on('enter-pressed', () => this.confirm() )
      this.$root.$on('escape', () => {
        if(this.activeTool.constructor === ManipulationTool) {
          this.cancel()
        } else {
          this.$root.$emit('activate-toolname', 'Manipulate')
        }
      })
    },

    beforeDestroy: function() {
      this.$root.$emit('unpreview-feature')
      this.$root.$emit('activate-toolname', 'Manipulate')
      this.activeFeature.dispose()
      if(this.activeFeature.constructor === CreateSketchFeature) this.activeFeature.real.invalidate()
      if(this.status == 'confirmed') {
        this.activeFeature.confirm(this)
      } else {
        this.$emit('remove-feature', this.activeFeature)
      }
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
        return ['profile', 'curve', 'axis', 'plane'].some(type =>
          type == setting.type && (!setting.optional || includeOptionals)
        )
      },

      pick: function(type, key) {
        return new Promise((resolve) => {
          this.$root.$off('picked')
          this.$root.$once('picked', (item) => {
            // Copy profiles before they get destroyed by transloader
            if(this.activeFeature.settings[key].type == 'profile') {
              const oldProfile = this.activeFeature[key]
              if(oldProfile) oldProfile().free()
              item = item.duplicate()
            }
            // Hide heavy data from Vue in a closure
            this.activeFeature[key] = () => item
            this.update()
            this.activePicker = null
            resolve()
            if(this.activeFeature.settings[key].autoConfirm) this.confirm()
          })
          this.activePicker = key
          const picker = this.$refs[key][0]
          const pickerRect = picker.getBoundingClientRect()
          const pickerPos = {
            x: pickerRect.left + (pickerRect.width / 2),
            y: pickerRect.top + (pickerRect.height / 2) - 38,
          }
          const style = window.getComputedStyle(picker)
          const color = style.getPropertyValue('background-color')
          this.$root.$emit('pick', type, pickerPos, color)
        })
      },

      update: function() {
        this.activeFeature.update()
        this.$root.$emit('regenerate')
        this.error = this.activeFeature.real.error()
        const preview = this.activeFeature.real.preview()
        if(preview) this.$root.$emit('preview-feature', this.activeFeature.component, preview)
      },

      showError: function(error) {
        this.error = error
      },

      confirm: function(e) {
        if(!this.canConfirm) return
        this.status = 'confirmed'
        this.$emit('close')
      },

      cancel: function(e) {
        this.status = 'canceled'
        this.$emit('close')
      },
    },
  }
</script>
