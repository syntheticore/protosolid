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
          :data-color="fields.color"
          @click="pick(fields.type, key, fields.color)"
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
      button.ok
        fa-icon(icon="check-circle" title="Confirm" @click="confirm")
      button.cancel
        fa-icon(icon="times-circle" title="Cancel" @click="cancel")
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
    padding-left: 9px
    &:hover
      &.ok
        color: #b9ff64
      &.cancel
        color: #ff6f6f
    &:active
      color: $dark2 !important
      transition: none

  label
    display: flex
    flex-direction: column
    align-items: center
    font-size: 12px
    color: $bright1
    font-weight: bold
    & + label
      margin-left: 12px
    > *
      margin-top: 6px
    input, select
      max-width: 65px

  .picker
    width: 23px
    height: 23px
    border: 6px solid white
    border-radius: 40px
    cursor: pointer
    transition: all 0.1s
    &:hover
      border-width: 1px
    &.active
      animation: 1s infinite linear rotate
      border-style: dashed
      border-width: 2px !important
    &.filled
      border-width: 0px
    &[data-color="pink"]
      background: #ee3367
      border-color: lighten(#ee3367, 87%)
    &[data-color="purple"]
      background: #dd18dd
      border-color: lighten(#dd18dd, 87%)
    &[data-color="orange"]
      background: #ea6a43
      border-color: lighten(#ea6a43, 87%)
    &[data-color="green"]
      background: #15c115
      border-color: lighten(#15c115, 87%)
    &[data-color="blue"]
      background: #4b8ee3
      border-color: lighten(#4b8ee3, 87%)

  @keyframes rotate
    50%
      transform: rotate(-180deg)
    100%
      transform: rotate(-360deg)

</style>


<script>
  export default {
    name: 'FeatureBox',

    components: {},

    props: {
      activeTool: Object,
      activeFeature: Object,
    },

    data() {
      return {
        activePicker: null,
      }
    },

    methods: {
      pick: function(type, name, color) {
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
        this.$emit('confirm')
      },

      cancel: function(e) {
        this.activeFeature.cancel()
        this.$root.$emit('component-changed', this.activeFeature.component)
        this.$emit('cancel')
      },
    },
  }
</script>
