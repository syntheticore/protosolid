<template lang="pug">
  .preferences-view
    button.window-close(
      title="Close settings"
      aria-label="Close settings"
      @click="close"
    )
      Icon(icon="window-close")

    aside.sidebar
      nav(aria-label="Settings sections")
        button(
          v-for="section in sections"
          :key="section.id"
          :class="{ active: activeSection === section.id }"
          @click="activeSection = section.id"
        )
          Icon(:icon="section.icon" fixed-width)
          span {{ section.title }}

    main.settings-content(:class="{ 'has-restart': restartRequired }")
      section(v-if="activeSection === 'modeling'")
        header
          h1 Modeling
          p Choose the geometry engine used to build and modify solid bodies.

        .settings-group
          label
            span.setting-label
              strong Geometry kernel
              small A restart is required after changing the modeling kernel
            ComboBox(v-model="preferences.modelingKernel" label="Geometry kernel" :options="[{ value: 'occ', label: 'OpenCascade' }, { value: 'shapex', label: 'Shapex' }]" @update:model-value="restartRequired = true")

      section(v-else-if="activeSection === 'units'")
        header
          h1 Units
          p Choose the standard units used for new values and measurements.

        .settings-group
          label
            span.setting-label
              strong Length
              small Default unit for distances and dimensions
            ComboBox(v-model="preferences.preferredUnit" label="Length unit" :options="[{ value: 'mm', label: 'Millimeters' }, { value: 'cm', label: 'Centimeters' }, { value: 'm', label: 'Meters' }, { value: 'inch', label: 'Inches' }]")

          label
            span.setting-label
              strong Angle
              small Default unit for angular measurements
            ComboBox(v-model="preferences.angleUnit" label="Angle unit" :options="[{ value: 'degrees', label: 'Degrees' }, { value: 'radians', label: 'Radians' }]")

          label
            span.setting-label
              strong Density
              small Default unit for material density
            ComboBox(v-model="preferences.densityUnit" label="Density unit" :options="[{ value: 'g/cm3', label: 'g/cm³' }, { value: 'kg/m3', label: 'kg/m³' }]")

      section(v-else-if="activeSection === 'appearance'")
        header
          h1 Appearance
          p Control how ProtoSolid and the modeling viewport are displayed.

        .settings-group
          label
            span.setting-label
              strong Theme
              small Match the system or choose a fixed appearance
            ComboBox(v-model="preferences.theme" label="Theme" :options="[{ value: 'system', label: 'System default' }, { value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]")

          label
            span.setting-label
              strong Anti-aliasing
              small Smooth jagged edges in the viewport
            input.input(type="checkbox" v-model="preferences.antiAlias" @change="restartRequired = true")

          label(:disabled="!isHighDPI")
            span.setting-label
              strong High-DPI rendering
              small Render the viewport at the display's native resolution
            input.input(type="checkbox" v-model="preferences.highDPI")

          label
            span.setting-label
              strong Display shadows
              small Show real-time shadows in the viewport
            input.input(type="checkbox" v-model="preferences.shadowMaps" @change="restartRequired = true")

          label
            span.setting-label
              strong Blurred overlays
              small Blur content behind floating controls
            input.input(type="checkbox" v-model="preferences.blurredOverlays")

          label
            span.setting-label
              strong Timeline features
              small Choose which features remain visible in the timeline
            ComboBox.wide(v-model="preferences.timelineFeatureVisibility" label="Timeline features" :options="[{ value: 'active-component', label: 'Active component and children only' }, { value: 'all', label: 'All features (dim unrelated)' }]")

      section(v-else)
        header
          h1 Tolerances
          p Set the precision used for geometry calculations and display meshes.

        .settings-group
          label
            span.setting-label
              strong Center of mass deviation
              small Allowed deviation when calculating mass properties
            input.input.measurement(type="text" v-model="preferences.centerOfMassTolerance")

          label
            span.setting-label
              strong Linear tessellation
              small Maximum deviation of the display mesh
            input.input.measurement(
              type="text"
              v-model="preferences.tessellationTolerance"
              @change="restartRequired = true"
            )

          label
            span.setting-label
              strong Angular tessellation
              small Maximum angular deviation of the display mesh
            input.input.measurement(
              type="text"
              v-model="preferences.tessellationAngularTolerance"
              @change="restartRequired = true"
            )

      transition(name="fade")
        .restart(v-if="restartRequired")
          span Some settings changes require a restart.
          button.button(@click="restart") Restart ProtoSolid
</template>


<style lang="stylus" scoped>
  contentInset = 40px
  sidebarInset = 12px

  .preferences-view
    position: relative
    display: grid
    grid-template-columns: 190px minmax(0, 1fr)
    grid-template-rows: minmax(0, 1fr)
    width: 100%
    height: 100%
    color: $bright1
    background: $dark2 * 0.8

    &::before
      content: ''
      position: absolute
      z-index: 2
      top: 0
      right: 0
      left: 0
      height: 28px
      -webkit-app-region: drag

  .window-close
    position: absolute
    z-index: 3
    top: 0
    right: 0
    width: 44px
    height: 32px
    padding: 0
    border: 0
    color: $bright2
    background: transparent
    font-size: 12px
    -webkit-app-region: no-drag

    &:hover
      color: white
      background: #b70f0f

  .sidebar
    background: $dark2 * 1.25
    border-right: 1px solid $dark1
    padding: 12px sidebarInset

    [data-platform="darwin"] &
      padding-top: 34px

  nav
    display: flex
    flex-direction: column
    gap: 4px

    button
      display: flex
      align-items: center
      gap: 10px
      width: 100%
      height: 36px
      padding: 0 12px
      border: 0
      border-radius: 5px
      color: $bright2
      background: transparent
      font-size: 13px
      font-weight: 600
      text-align: left

      svg
        color: $bright2 * 0.9

      &:hover
        color: $bright1
        background: rgba(white, 0.06)

      &.active
        color: white
        background: rgba(white, 0.1)

        svg
          color: white

  .settings-content
    position: relative
    min-width: 0
    padding: contentInset
    overflow: auto

    &.has-restart
      padding-bottom: 88px

    .input:focus
      border-color: $bright2 * 0.7

    section
      max-width: 560px

    header
      margin-bottom: 32px

      h1
        margin: 0 0 8px
        font-size: 22px
        font-weight: 650

      p
        margin: 0
        color: $bright2
        font-size: 12px
        line-height: 1.5

  .settings-group
    label
      display: grid
      grid-template-columns: minmax(0, 1fr) auto
      align-items: center
      gap: 32px
      min-height: 56px
      margin: 0
      padding: 12px 0

      > :first-child
        width: auto
        margin: 0

    .combo-box
      width: 168px

      &.wide
        width: 224px

    .measurement
      box-sizing: border-box
      width: 112px
      height: 28px
      text-align: right

    input[type="checkbox"]
      width: 16px
      height: 16px
      margin: 0
      accent-color: $highlight

  .setting-label
    display: flex
    flex-direction: column
    gap: 4px

    strong
      color: $bright1
      font-size: 13px
      font-weight: 600

    small
      color: $bright2
      font-size: 11px
      line-height: 1.35

  .restart
    position: absolute
    right: contentInset
    bottom: 24px
    left: contentInset
    display: flex
    align-items: center
    justify-content: space-between
    gap: 16px
    padding: 12px 16px
    border: 1px solid rgba($warn, 0.42)
    border-radius: 5px
    color: $warn
    background: $dark2 * 0.84
    font-size: 11px
    transition: all 0.2s

    .button
      white-space: nowrap

  .fade-enter-from
  .fade-leave-to
    opacity: 0
    transform: translateY(8px)
</style>


<script>
  import {
    default as preferences,
    savePreferences,
  } from './../js/preferences.js'

  export default {
    name: 'PreferencesView',

    data() {
      return {
        preferences,
        activeSection: 'modeling',
        isHighDPI: window.devicePixelRatio > 1,
        restartRequired: false,
        sections: [
          { id: 'modeling', title: 'Modeling', icon: 'cube' },
          { id: 'units', title: 'Units', icon: 'ruler' },
          { id: 'appearance', title: 'Appearance', icon: 'desktop' },
          { id: 'tolerances', title: 'Tolerances', icon: 'crosshairs' },
        ],
      }
    },

    watch: {
      preferences: {
        handler() {
          savePreferences()
          if(window.ipc) window.ipc.send('set-theme', preferences.theme)
        },
        deep: true,
      },
    },

    methods: {
      close: function() {
        if(window.ipc) {
          window.ipc.send('close-preferences')
        } else {
          window.close()
        }
      },

      restart: function() {
        if(window.ipc) window.ipc.send('restart')
      },
    },
  }
</script>
