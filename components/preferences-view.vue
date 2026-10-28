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
      section(v-if="activeSection === 'files'")
        header
          h1 Files
          p Choose how documents are stored when saving.

        .settings-group
          label
            span.setting-label
              strong Document format
              small Versioned documents keep every save as a version with a message, and support branching and merging. Existing history is only dropped by an explicit Save As.
            ComboBox(v-model="preferences.documentFormat" label="Document format" :options="[{ value: 'plain', label: 'Unversioned' }, { value: 'versioned', label: 'Versioned' }]")

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
          p Control how the interface and the modeling viewport are displayed.

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
            ToggleSwitch(v-model="preferences.antiAlias" @update:model-value="restartRequired = true")

          label(:disabled="!isHighDPI")
            span.setting-label
              strong High-DPI rendering
              small Render the viewport at the display's native resolution
            ToggleSwitch(v-model="preferences.highDPI")

          label
            span.setting-label
              strong Display shadows
              small Show a soft shadow below the model
            ToggleSwitch(v-model="preferences.shadowMaps" @update:model-value="restartRequired = true")

          label
            span.setting-label
              strong Blurred overlays
              small Blur content behind floating controls
            ToggleSwitch(v-model="preferences.blurredOverlays")

          label
            span.setting-label
              strong Always show side bar labels
              small Display tooltips for view & render settings constantly, instead of on hover.
            ToggleSwitch(v-model="preferences.alwaysShowSideLabels")

      section(v-else-if="activeSection === 'behaviour'")
        header
          h1 Behaviour
          p Control how the interface responds while you work.

        .settings-group
          label
            span.setting-label
              strong Preview on hover
              small Preview zoom, projection, shading and material options while hovering them
            ToggleSwitch(v-model="preferences.previewOnHover")

          label
            span.setting-label
              strong Sketch camera
              small Move the camera when entering or leaving sketches
            ComboBox(v-model="preferences.sketchCameraMovement" label="Sketch camera" :options="[{ value: 'off', label: 'Off' }, { value: 'enter', label: 'Entering only' }, { value: 'both', label: 'Entering and leaving' }]")

          label
            span.setting-label
              strong Timeline features
              small Choose which features remain visible in the timeline
            ComboBox.wide(v-model="preferences.timelineFeatureVisibility" label="Timeline features" :options="[{ value: 'active-component', label: 'Active component and children only' }, { value: 'all', label: 'All features (dim unrelated)' }]")

          label
            span.setting-label
              strong Debug mode
              small Show detailed internal error messages when features fail
            ToggleSwitch(v-model="preferences.debugMode")

      section(v-else)
        header
          h1 Tolerances
          p Set the precision used for geometry calculations and display meshes.

        .settings-group
          label
            span.setting-label
              strong Center of mass deviation
              small Allowed deviation when calculating mass properties
            ExpressionInput.measurement(
              stretch
              :expression="preferences.centerOfMassTolerance"
              :step="0.01"
              @expression="preferences.centerOfMassTolerance = $event"
            )

          label
            span.setting-label
              strong Linear tessellation
              small Maximum deviation of the display mesh
            ExpressionInput.measurement(
              stretch
              :expression="preferences.tessellationTolerance"
              :step="0.01"
              @expression="setRestartPreference('tessellationTolerance', $event)"
            )

          label
            span.setting-label
              strong Angular tessellation
              small Maximum angular deviation of the display mesh
            ExpressionInput.measurement(
              stretch
              :expression="preferences.tessellationAngularTolerance"
              dimension="angle"
              :step="0.01"
              @expression="setRestartPreference('tessellationAngularTolerance', $event)"
            )

          label
            span.setting-label
              strong Sketch tessellation
              small Maximum deviation of drawn sketch curves
            ExpressionInput.measurement(
              stretch
              :expression="preferences.sketchTessellationTolerance"
              :step="0.01"
              @expression="setRestartPreference('sketchTessellationTolerance', $event)"
            )

      transition(name="fade")
        .restart(v-if="restartRequired")
          span Some settings changes require a restart.
          button.button(@click="restart") Restart now
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
      width: 112px
      height: 28px

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
        activeSection: new URLSearchParams(window.location.search).get('section') || 'appearance',
        isHighDPI: window.devicePixelRatio > 1,
        restartRequired: false,
        sections: [
          { id: 'appearance', title: 'Appearance', icon: 'desktop' },
          { id: 'behaviour', title: 'Behaviour', icon: 'sliders-h' },
          { id: 'files', title: 'Files', icon: 'save' },
          { id: 'units', title: 'Units', icon: 'ruler' },
          { id: 'tolerances', title: 'Tolerances', icon: 'crosshairs' },
        ],
      }
    },

    mounted() {
      if(window.ipc) window.ipc.on('show-preferences-section', (e, section) => this.activeSection = section )
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

      setRestartPreference(key, expression) {
        if(preferences[key] == expression) return
        preferences[key] = expression
        this.restartRequired = true
      },

      restart: function() {
        if(window.ipc) window.ipc.send('restart')
      },
    },
  }
</script>
