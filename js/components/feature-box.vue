<template lang="pug">
  .bordered.tipped.feature-box

    //- header
    //-   fa-icon(icon="box" title="Confirm")
    //-   .title Extrude

    form.options
      label(v-for="(setting, key) in activeFeature.settings")
        | {{ setting.title }}
        .picker(
          v-if="needsPicker(setting)"
          :ref="key"
          :class="{active: activePicker == key, filled: activeFeature[key]}"
          @click="pick(setting.type, key)"
        )
        NumberInput(
          v-if="setting.type == 'length' || setting.type == 'angle'"
          :value.sync="activeFeature[key]"
          @update:value="update"
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

    .confirmation
      button.ok(
        title="Confirm"
        :disabled="!activeFeature.isComplete()"
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

  // header
  //   display: flex
  //   flex-direction: column
  //   justify-content: center
  //   align-items: center
  //   background: $dark1
  //   padding: 0 10px
  //   svg
  //     font-size: 21px
  //     color: $bright1
  //     // transition: all 0.15s
  //     filter: none
  //   .title
  //     color: $bright1
  //     font-size: 11px
  //     margin-top: 6px
  //     font-weight: bold

  .options
    margin: 10px
    display: flex

  label
    display: flex
    flex-direction: column
    align-items: center
    font-size: 12px
    color: $bright1
    font-weight: bold
    & + label
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
    select
    .radio-bar
    .icon-toggle
      flex: 1 1 auto

  .picker
    width: 24px
    height: 24px
    border: 7px solid white
    border-radius: 99px
    // cursor: pointer
    transition: all 0.06s
    &:hover
      border-width: 0px
    &.active
      animation: 2s infinite linear rotate
      border-style: dotted
      border-width: 2px !important
    &.filled
      border-width: 2px

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
        color: $dark2 !important
        transition: none
      &:disabled
        color: $bright1 * 0.5 !important

  @keyframes rotate
    50%
      transform: rotate(180deg)
    100%
      transform: rotate(360deg)

</style>


<script>
  import IconToggle from './icon-toggle.vue'
  import RadioBar from './radio-bar.vue'
  import NumberInput from './number-input.vue'

  import { ManipulationTool } from './../tools.js'

  export default {
    name: 'FeatureBox',

    components: {
      IconToggle,
      RadioBar,
      NumberInput,
    },

    props: {
      activeTool: Object,
      activeFeature: Object,
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
        activePicker: null,
      }
    },

    mounted: function() {
      this.pickAll()
    },

    beforeDestroy: function() {
      this.cancel()
    },

    methods: {
      pick: function(type, key) {
        return new Promise((resolve) => {
          this.$root.$off('picked')
          this.$root.$once('picked', (item) => {
            this.activeFeature[key] = item
            this.update()
            this.activePicker = null
            resolve()
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

      needsPicker: function(setting) {
        return ['profile', 'curve', 'axis'].some(type => type == setting.type )
      },

      pickAll: function() {
        const pickerKeys = Object.keys(this.activeFeature.settings).filter(key =>
          this.needsPicker(this.activeFeature.settings[key])
        )
        let chain = Promise.resolve()
        for(const key of pickerKeys) {
          const setting = this.activeFeature.settings[key]
          chain = chain.then(() => this.pick(setting.type, key) )
        }
      },

      update: function() {
        const mesh = this.activeFeature.update()
        if(mesh) this.$root.$emit('preview-feature', this.activeFeature.component, mesh)
      },

      confirm: function(e) {
        this.activeFeature.confirm()
        this.$root.$emit('component-changed', this.activeFeature.component)
        this.$emit('close')
      },

      cancel: function(e) {
        this.activeFeature.cancel()
        this.$root.$emit('unpreview-feature')
        this.$root.$emit('activate-toolname', 'Manipulate')
        this.$emit('close')
      },
    },
  }
</script>
