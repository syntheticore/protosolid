<template lang="pug">
  .bordered.tipped.feature-box

    //- header
    //-   fa-icon(icon="box" title="Confirm")
    //-   .title Extrude

    form.options
      label(v-for="(fields, key) in activeFeature.settings")
        | {{ fields.title }}
        .picker(
          v-if="fields.type == 'profile' || fields.type == 'curve'"
          :ref="key"
          :class="{active: activePicker == key, filled: activeFeature[key]}"
          @click="pick(fields.type, key)"
        )
        input(
          v-if="fields.type == 'length'"
          type="number"
          v-model="activeFeature[key]"
          @change="update"
        )
        input(
          v-if="fields.type == 'bool'"
          type="checkbox"
          v-model="activeFeature[key]"
          @change="update"
        )
        select(v-if="fields.type == 'select'")
          option(v-for="option in fields.options") {{ option }}

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
    input, select
      max-width: 65px

  .picker
    width: 24px
    height: 24px
    border: 7px solid white
    border-radius: 40px
    cursor: pointer
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
    50%
      transform: rotate(180deg)
    100%
      transform: rotate(360deg)

</style>


<script>
  import { ManipulationTool } from './../tools.js'

  export default {
    name: 'FeatureBox',

    components: {},

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
      if(!this.activeFeature.defaultSetting) return
      const setting = this.activeFeature.settings[this.activeFeature.defaultSetting]
      this.pick(setting.type, this.activeFeature.defaultSetting)
    },

    beforeDestroy: function() {
      this.cancel()
    },

    methods: {
      pick: function(type, name) {
        this.$root.$once('picked', (item) => {
          this.activeFeature[name] = item
          this.update()
          this.activePicker = null
        })
        this.activePicker = name
        const picker = this.$refs[name][0]
        const pickerRect = picker.getBoundingClientRect()
        const pickerPos = {
          x: pickerRect.left + (pickerRect.width / 2),
          y: pickerRect.top + (pickerRect.height / 2) - 38,
        }
        const style = window.getComputedStyle(picker)
        const color = style.getPropertyValue('background-color')
        if(type == 'profile') {
          this.$root.$emit('pick-profile', pickerPos, color)
        } else if(type == 'curve') {
          this.$root.$emit('pick-curve', pickerPos, color)
        }
      },

      update: function() {
        const mesh = this.activeFeature.update()
        this.$root.$emit('preview-feature', this.activeFeature.component, mesh)
      },

      confirm: function(e) {
        this.activeFeature.confirm()
        this.$root.$emit('component-changed', this.activeFeature.component)
        this.$emit('close')
      },

      cancel: function(e) {
        this.activeFeature.cancel()
        this.$root.$emit('component-changed', this.activeFeature.component)
        this.$emit('close')
      },
    },
  }
</script>
