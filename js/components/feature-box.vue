<template lang="pug">
  form.bordered.tipped.feature-box

    .options
      label(v-for="(fields, key) in activeFeature.settings()")
        | {{ fields.title }}
        .picker(
          v-if="fields.type == 'profile' || fields.type == 'curve'" @click="pick(fields.type, key)"
          :ref="key"
          :class="{active: activePicker == key, filled: activeFeature[key]}"
          :data-color="fields.color"
        )
        input(
          v-if="fields.type == 'length'"
          type="number"
          v-model="activeFeature[key]"
        )
        input(
          v-if="fields.type == 'bool'"
          type="checkbox"
          v-model="activeFeature[key]"
        )
        select(v-if="fields.type == 'select'")
          option(v-for="option in fields.options") {{ option }}

    .confirmation
      button.ok
        fa-icon(icon="check-circle" title="Confirm")
      button.cancel
        fa-icon(icon="times-circle" title="Cancel")
</template>


<style lang="stylus" scoped>
  .feature-box
    font-size: 12px
    display: flex

  .options
    margin: 10px
    display: flex

  .confirmation
    display: flex
    flex-direction: column
    justify-content: space-around
    background: $dark1

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

    mounted() {
      console.log(this.$refs)
    },

    methods: {
      pick: function(type, name) {
        this.$root.$once('picked', (item) => {
          this.data[name] = item
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
          this.$root.$emit('pick-profile', pickerPos)
        } else if(type == 'curve') {
          this.$root.$emit('pick-curve', pickerPos)
        }
      },
    },

    beforeDestroy() {
      console.log('Feature destroy')
    },
  }
</script>
