<template lang="pug">
  .preferences-view
    h2 Preferences
    .panes
      fieldset
        legend Standard Units
        label
          select(v-model="preferences.preferredUnit")
            option mm
            option cm
            option m
            option inch
          span Length
        label
          select
            option °
            option Radians
          span Angle
        label
          select
            option g/cm³
            option kg/m³
          span Density

      fieldset
        legend User Interface
        label
          select
            option Bright
            option Dark
            option System default
          span Theme
        label
          input(type="checkbox" v-model="preferences.antiAlias" @change="restartRequired = true")
          span Anti aliasing
        label(:disabled="!isHighDPI")
          input(type="checkbox" v-model="preferences.highDPI")
          span High DPI rendering
        label
          input(type="checkbox" v-model="preferences.shadowMaps" @change="restartRequired = true")
          span Display shadows
        label
          input(type="checkbox" v-model="preferences.blurredOverlays")
          span Blurred overlays

      fieldset
        legend Tolerances
        label
          input(type="numer" value="0.01mm" step="0.01")
          span Center of Mass Deviation
        label
          input(type="numer" value="0.0001mm" step="0.0001")
          span Curve/Surface Tesselation
        label
          input(type="numer" value="0.001mm" step="0.001")
          span Curve Fitting Accuracy

    transition(name="fade")
      .restart(v-if="restartRequired")
        | A restart is required for some changes to take effect
        button.button(@click="restart") Restart Alchemy

</template>


<style lang="stylus" scoped>
  .preferences-view
    position: relative

  .panes
    display: flex

  fieldset
    margin: 18px

  input, select
    width: 70px
    margin: 0
    margin-right: 8px
    box-shadow: none
    height: 24px

  input[type="checkbox"]
    width: unset
    height: unset

  label
    margin: 6px 0
    min-height: 24px

  .restart
    position: absolute
    bottom: 7px
    right: 7px
    white-space: nowrap
    color: $warn
    transition: all 0.2s
    .button
      margin-left: 12px

  .fade-enter
  .fade-leave-to
    opacity: 0
    transform: translateY(10px)

</style>


<script>
  import {
    default as preferences,
    savePreferences,
  } from './../preferences.js'

  export default {
    name: 'PreferencesView',

    data() {
      return {
        preferences,
        isHighDPI: window.devicePixelRatio > 1,
        restartRequired: false,
      }
    },

    watch: {
      preferences: {
        handler() {
          savePreferences()
        },
        deep: true,
      },
    },

    methods: {
      restart: function() {
        if(window.ipcRenderer) window.ipcRenderer.send('restart')
      },
    },
  }
</script>
