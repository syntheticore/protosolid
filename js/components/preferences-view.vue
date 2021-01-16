<template lang="pug">
  .preferences-view
    h2 Preferences
    .panes
      fieldset
        legend User Interface
        label
          select
            option Bright
            option Dark
            option System default
          span Theme
        label
          input(type="checkbox" v-model="preferences.blurredOverlays")
          span Blurred overlays
        //- label(:disabled="!isHighDPI")
        label
          input(type="checkbox" v-model="preferences.highDPI")
          span High DPI rendering
        label
          input(type="checkbox" v-model="preferences.shadowMaps")
          span Render shadow maps

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
        legend Tolerances
        label
          input(type="text" value="0.01mm")
          span Center of Mass Deviation
        label
          input(type="text" value="0.0001mm")
          span Curve/Surface Tesselation
        label
          input(type="text" value="0.001mm")
          span Curve Fitting Accuracy

</template>


<style lang="stylus" scoped>
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
    min-height: 18px

  h2
    font-size: 14px
    font-weight: 600
    text-align: center
    padding: 9px
    border-bottom: 1px solid $dark1
    background: linear-gradient(0deg, $dark2 * 0.83, transparent)

</style>


<script>
  import {
    default as preferences,
    savePreferences,
    loadPreferences
  } from './../preferences.js'

  export default {
    name: 'PreferencesView',

    data() {
      return {
        open: false,
        preferences,
        isHighDPI: window.devicePixelRatio > 1,
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
  }
</script>
