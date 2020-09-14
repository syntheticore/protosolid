<template lang="pug">
  form.bordered.tipped.feature-box
    //- h1.header Extrude
    .options
      label(v-for="(fields, key) in settings")
        | {{ fields.title }}
        .picker(
          v-if="fields.type == 'region' || fields.type == 'edge'" @click="pick(fields.type, key)"
          :ref="key"
          :class="{active: activePicker == key, filled: data[key]}"
          :data-color="fields.color"
        )
        input(
          v-if="fields.type == 'length'"
          type="number"
          v-model="data[key]"
        )
        input(
          v-if="fields.type == 'bool'"
          type="checkbox"
          v-model="data[key]"
        )
        select(v-if="fields.type == 'select'")
          option(v-for="option in fields.options") {{ option }}
</template>


<style lang="stylus" scoped>
  .feature-box
    font-size: 12px
    padding: 10px 14px
    &::before
      // left: 96px

  .options
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
      // box-shadow: 0 0 5px rgba(white, 0.7)
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
    },

    data() {
      return {
        settings: {
          profile: {
            title: 'Profile',
            type: 'region',
            color: 'pink',
          },
          rail: {
            title: 'Rail',
            type: 'region',
            color: 'pink',
          },
          distance: {
            title: 'Distance',
            type: 'length',
          },
          direction: {
            title: 'Direction',
            type: 'bool',
          },
          operation: {
            title: 'Operation',
            type: 'select',
            options: ['join', 'cut', 'intersect', 'create'],
          },
        },
        data: {
          profile: null,
          rail: null,
          distance: 0,
          direction: true,
          operation: 'join',
        },
        activePicker: null,
      }
    },

    mounted() {
      console.log(this.$refs)
    },

    methods: {
      pick: function(type, name) {
        this.$root.$once('picked-profile', (profile) => {
          console.log('Picked profile', profile)
          this.data[name] = profile
          this.activePicker = null
        })
        this.activePicker = name
        const picker = this.$refs[name][0]
        const rect = picker.getBoundingClientRect()
        this.$root.$emit('pick-profile', {
          x: rect.left + (rect.width / 2),
          y: rect.top + (rect.height / 2) - 38,
        })
      },
    },

    beforeDestroy() {
      console.log('Feature destroy')
    },
  }
</script>
