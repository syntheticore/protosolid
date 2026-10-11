<template lang="pug">

  li.treelet-simulation(:class="{ expanded, invalid: warning, fluid: simulation.mode == 'fluid' }")

    .box

      header(@click="toggle")

        Icon.simulation-icon(
          :icon="configuration.icon"
          :class="{ problem: warning }"
          :title="warning || null"
          fixed-width
        )

        h2 {{ simulation.title }}

        Icon.expand(icon="angle-right")

        .controls(:class="{ wide: simulation.mode == 'static' }")

          Icon.playback(
            v-if="simulation.mode == 'static'"
            :icon="playing ? 'pause' : 'play'"
            fixed-width
            :title="playing ? 'Pause deformation' : 'Play deformation'"
            @click.stop="togglePlayback"
          )

          Icon.delete(icon="trash-alt" fixed-width title="Delete" @click.stop="remove")

      .content.form(v-if="expanded")

        fieldset

          label(v-if="simulation.mode == 'fluid'")
            span Fluid
            ComboBox.fluid-selector(
              :model-value="simulation.fluid"
              label="Fluid"
              :options="Object.entries(fluidMaterials).map(([value, material]) => ({ value, label: material.label }))"
              @update:model-value="fluidChanged"
            )

          label.picker-row(v-for="picker in configuration.pickers" :key="picker.key")
            span {{ picker.label }} ({{ simulation[picker.key].length }})
            .picker(
              :ref="element => setPickerElement(picker.key, element)"
              :class="{ active: activePicker == picker.key, filled: simulation[picker.key].length, first: picker.key == configuration.pickers[0].key }"
              @click="pick(picker.key)"
            )

          label(v-for="input in configuration.inputs" :key="input.key")
            span {{ input.label }}
            input.input(
              type="number"
              :step="input.step || 'any'"
              :min="input.min"
              :max="input.max"
              :value="simulation[input.key]"
              @change="changed(input.key, $event)"
            )

          label.quality(v-if="simulation.mode == 'static'")
            span Quality {{ simulation.quality }} / 5
            input(type="range" min="1" max="5" step="1" title="Refines the solver grid and animated display mesh" :value="simulation.quality" @change="changed('quality', $event)")

          template(v-if="simulation.mode == 'fluid'")

            label.display-option
              span Flow vectors
              input(type="checkbox" :checked="simulation.showFlowVectors" @change="displayChanged('showFlowVectors', $event)")

            label.display-option
              span Flow lines
              input(type="checkbox" :checked="simulation.showFlowLines" @change="displayChanged('showFlowLines', $event)")

        fieldset.result-stats(v-if="simulation.mode == 'static'")

          label.result-summary
            span Max von Mises
            span {{ simulation.result ? simulation.result.maxStress.toPrecision(3) + ' MPa' : '-' }}

          label.result-summary
            span Max displacement
            span {{ simulation.result ? simulation.result.maxDisplacement.toPrecision(3) + ' mm' : '-' }}

          label.result-summary
            span Solver residual
            span
              template(v-if="simulation.result")
                | {{ (simulation.result.relativeResidual * 100).toFixed(1) }}%
                template(v-if="simulation.result.relativeResidual > 0.001")  — results may be inaccurate

              template(v-else) -

        transition(
          name="message"
          mode="out-in"
          @before-enter="prepareMessage"
          @enter="enterMessage"
          @after-enter="clearMessageHeight"
          @leave="leaveMessage"
        )

          .message-area(v-if="progress != null || warning" :key="progress != null ? 'progress' : 'warning'")

            .error-msg(
              :role="progress != null ? 'progressbar' : null"
              :aria-label="progress != null ? 'Simulation progress' : null"
              :aria-valuemin="progress != null ? 0 : null"
              :aria-valuemax="progress != null ? 100 : null"
              :aria-valuenow="progress != null ? Math.round(progress) : null"
            )

              .progress-fill(v-if="progress != null" :style="{ width: progress + '%' }")

              template(v-else) {{ warning }}

</template>


<style lang="stylus" scoped>

  header h2
    margin: 0 !important

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: transform 0.15s

    .expanded &
      transform: rotate(90deg)

  .problem
    color: $warn !important

  .invalid .box
    border-color: $warn !important

  .message-area
    overflow: hidden
    transition: height 0.25s, opacity 0.25s

  .error-msg
    position: relative
    min-height: 16px
    line-height: 16px
    font-size: 12px
    font-weight: bold
    padding: 6px 10px
    text-align: center
    color: lighten($warn, 65%)
    background: darken($warn, 50%)

  .message-enter-from
  .message-leave-to
    opacity: 0

  .progress-fill
    position: absolute
    top: 0
    bottom: 0
    left: 0
    background: darken($warn, 20%)
    transition: width 0.15s linear

  .picker
    width: 24px !important
    height: 24px !important
    flex: 0 0 24px
    margin: 0 !important
    box-sizing: border-box
    border: 7px solid white
    border-radius: 50%
    transition: all 0.06s
    justify-self: end

    &:hover
    &.filled
      border-width: 2px

    &.active
      border-style: dotted
      border-width: 2px
      animation: 2s infinite linear rotate

  .picker.first
    background: $blue
    border-color: lighten($blue, 75%)

  .picker:not(.first)
    background: $purple
    border-color: lighten($purple, 75%)

  .input
    width: 64px
    justify-self: end

  .quality input
    width: 62px
    justify-self: end

  .result-stats
    background: $dark2 * 1.5
    border-top: 1px solid $dark1 * 1.4
    padding-bottom: 0

  .result-summary
    display: flex

  .result-summary span:first-child
    flex: 1
    font-weight: normal

  .result-summary span:last-child
    width: 65px
    flex: 0 0 65px
    text-align: right
    white-space: normal
    overflow-wrap: anywhere

  .fluid-selector
    width: 120px
    justify-self: end

  .fluid fieldset .display-option input[type="checkbox"]
    margin-left: 0
    justify-self: end

  fieldset label
    display: grid
    grid-template-columns: max-content 1fr
    gap: 16px
    width: 100%

    > span:first-child
      width: auto
      margin: 0
      white-space: nowrap

  @keyframes rotate
    100%
      transform: rotate(360deg)

</style>


<script>
  import { FluidProbeTool, ManipulationTool, StaticProbeTool, ThermalProbeTool } from '../js/tools.js'
  import { fluidMaterials } from '../js/core/fluid.js'
  import { worldTransform } from '../js/core/assembly.js'
  import PickerSession from '../js/picker-session.js'
  import { cancelSimulation, solveSimulation } from '../js/core/simulation-runner.js'

  const configurations = {
    fluid: {
      icon: 'wind',
      probeTool: FluidProbeTool,
      resultEvent: 'fluid-result',
      pickers: [
        { key: 'inletFaces', label: 'Inlet faces' },
        { key: 'outletFaces', label: 'Outlet faces' },
      ],
      inputs: [
        { key: 'inletSpeed', label: 'Inlet m/s', min: 0 },
        { key: 'outletPressure', label: 'Outlet Pa gauge' },
        { key: 'density', label: 'Density kg/m³', min: 0 },
        { key: 'viscosity', label: 'Viscosity Pa·s', min: 0 },
        { key: 'resolution', label: 'Grid cells', step: 1, min: 8, max: 24 },
      ],
    },
    thermal: {
      icon: 'thermometer-half',
      probeTool: ThermalProbeTool,
      resultEvent: 'thermal-result',
      pickers: [
        { key: 'coldFaces', label: 'Cold faces' },
        { key: 'hotFaces', label: 'Hot faces' },
      ],
      inputs: [
        { key: 'coldTemperature', label: 'Cold °C' },
        { key: 'hotTemperature', label: 'Hot °C' },
      ],
    },
    static: {
      icon: 'weight',
      probeTool: StaticProbeTool,
      pickers: [
        { key: 'fixedFaces', label: 'Fixed faces' },
        { key: 'loadFaces', label: 'Loaded faces' },
      ],
      inputs: [
        { key: 'forceX', label: 'Force X (N)' },
        { key: 'forceY', label: 'Force Y (N)' },
        { key: 'forceZ', label: 'Force Z (N)' },
      ],
    },
  }

  export default {
    name: 'TreeletSimulation',

    inject: ['bus'],

    props: {
      document: Object,
      component: Object,
      simulation: Object,
    },

    data() {
      return {
        activePicker: null,
        pickerElements: {},
        fluidMaterials,
        solveRevision: 0,
        progress: null,
        solveWarning: null,
        playing: false,
        playbackFrame: null,
        playbackTime: 0,
        previousFrameTime: null,
      }
    },

    computed: {
      configuration() {
        return configurations[this.simulation.mode]
      },

      expanded() {
        return this.document.activeSimulation === this.simulation
      },

      warning() {
        this.document.timeline.marker
        const problem = this.simulation.resolve(this.document.top(), false)
        return typeof problem == 'string' ? problem : problem?.warning ? problem.message : this.solveWarning
      },

    },

    watch: {
      expanded(expanded) {
        if(expanded) {
          this.solve()
          this.bus.emit('activate-tool', this.configuration.probeTool)
          this.$nextTick(this.updatePaths)
        } else {
          this.stopPlayback(true)
          this.solveRevision++
          cancelSimulation(this.simulation)
          this.progress = null
          this.cancelPick()
          this.bus.emit('clear-pickers')
          this.bus.emit('activate-tool', ManipulationTool)
        }
      },
    },

    mounted() {
      this.pickerSession = new PickerSession(this.bus, {
        available: key => this.expanded && !this.document.activeFeature &&
          this.configuration.pickers.some(picker => picker.key == key),
        defaultKey: () => this.configuration.pickers[0]?.key,
        activate: this.activatePicker,
        select: this.selectPickedFace,
        deactivate: this.deactivatePicker,
      })
      this.bus.on('resize', this.updatePaths)
      this.bus.on('tree-layout', this.updatePaths)
      this.bus.on('escape', this.onEscape)
      this.bus.on('material-changed', this.materialChanged)
      this.document.on('regenerated', this.regenerated)
      if(this.expanded) {
        this.solve()
        this.bus.emit('activate-tool', this.configuration.probeTool)
        this.$nextTick(this.updatePaths)
        if(this.simulation.activateFirstPicker) {
          this.simulation.activateFirstPicker = false
          this.$nextTick(() => this.pick(this.configuration.pickers[0].key))
        }
      }
    },

    beforeUnmount() {
      this.stopPlayback(true)
      this.solveRevision++
      cancelSimulation(this.simulation)
      this.pickerSession.dispose()
      this.bus.off('resize', this.updatePaths)
      this.bus.off('tree-layout', this.updatePaths)
      this.bus.off('escape', this.onEscape)
      this.bus.off('material-changed', this.materialChanged)
      this.document.off('regenerated', this.regenerated)
    },

    methods: {
      prepareMessage(element) {
        element.style.height = '0px'
      },

      enterMessage(element) {
        element.style.height = `${element.scrollHeight}px`
      },

      clearMessageHeight(element) {
        element.style.height = ''
      },

      leaveMessage(element) {
        element.style.height = `${element.getBoundingClientRect().height}px`
        element.offsetHeight
        element.style.height = '0px'
      },

      setPickerElement(key, element) {
        this.pickerElements[key] = element
      },

      toggle() {
        if(this.expanded) {
          this.document.activateSimulation(null)
        } else {
          this.document.activateComponent(this.component)
          this.document.activateSimulation(this.simulation)
        }
      },

      togglePlayback() {
        if(this.playing) {
          this.stopPlayback(false)
          return
        }
        if(!this.expanded) {
          this.document.activateComponent(this.component)
          this.document.activateSimulation(this.simulation)
          this.$nextTick(this.startPlayback)
        } else {
          this.startPlayback()
        }
      },

      startPlayback() {
        if(!this.expanded || !this.simulation.result) return
        this.playing = true
        this.previousFrameTime = null
        this.playbackFrame = requestAnimationFrame(this.advancePlayback)
      },

      advancePlayback(time) {
        if(!this.playing) return
        this.playbackFrame = requestAnimationFrame(this.advancePlayback)
        if(this.previousFrameTime !== null && time - this.previousFrameTime < 30) return
        if(this.previousFrameTime !== null) this.playbackTime += Math.min(time - this.previousFrameTime, 50)
        this.previousFrameTime = time
        this.simulation.deformationFactor = (1 + Math.cos(2 * Math.PI * this.playbackTime / 2400)) / 2
        this.simulation.animationRevision++
      },

      stopPlayback(reset) {
        if(this.playbackFrame !== null) cancelAnimationFrame(this.playbackFrame)
        this.playbackFrame = null
        this.previousFrameTime = null
        this.playing = false
        if(reset && this.simulation.mode == 'static') {
          this.playbackTime = 0
          this.simulation.deformationFactor = 1
          this.simulation.animationRevision++
        }
      },

      regenerated() {
        if(this.expanded) this.solve()
        this.$nextTick(this.updatePaths)
      },

      materialChanged() {
        if(this.simulation.mode != 'static') return
        this.$nextTick(() => {
          if(this.expanded) this.solve()
        })
      },

      async solve() {
        const revision = ++this.solveRevision

        cancelSimulation(this.simulation)
        this.solveWarning = null

        if(this.warning) {
          this.progress = null
        } else {
          this.progress = 0
          await new Promise(resolve => requestAnimationFrame(() => setTimeout(resolve, 0)))
          if(revision != this.solveRevision) return
        }

        try {
          const pending = solveSimulation(this.simulation, this.document.top(), progress => {
            if(revision == this.solveRevision) this.progress = progress
          })

          if(this.configuration.resultEvent) this.bus.emit(this.configuration.resultEvent)
          const warning = await pending
          if(revision == this.solveRevision) this.solveWarning = warning || null
        } catch {
          if(revision == this.solveRevision) {
            this.solveWarning = 'The simulation solver failed; check the geometry and settings'
            cancelSimulation(this.simulation)
          }
        }

        if(revision != this.solveRevision) return

        this.progress = null
        if(this.simulation.mode == 'static' && !this.simulation.result && this.playing) this.stopPlayback(true)
        if(this.configuration.resultEvent) this.bus.emit(this.configuration.resultEvent)
        this.bus.emit('render-needed')
        this.$nextTick(this.updatePaths)
      },

      changed(key, event) {
        this.simulation[key] = Number(event.target.value)
        if(this.simulation.mode == 'fluid' && (key == 'density' || key == 'viscosity')) this.simulation.fluid = 'custom'
        this.document.hasChanges = true
        this.solve()
      },

      fluidChanged(fluid) {
        this.simulation.setFluid(fluid)
        this.document.hasChanges = true
        this.solve()
      },

      displayChanged(key, event) {
        this.simulation[key] = event.target.checked
        this.document.hasChanges = true
        this.bus.emit('fluid-result')
      },

      async pick(key) {
        if(this.activePicker == key) {
          this.cancelPick()
          return
        }
        if(!await this.pickerSession.start(key)) return
        const pickerIndex = this.configuration.pickers.findIndex(picker => picker.key == key)
        const nextPicker = this.configuration.pickers[pickerIndex + 1]
        if(nextPicker && !this.simulation[nextPicker.key].length) this.pick(nextPicker.key)
      },

      activatePicker(key) {
        this.activePicker = key
        this.document.activeSimulationPicker = true
        this.updatePicker = () => {
          const { pickerPos, color } = this.getPickerInfo(key)
          this.bus.emit('pick', 'face', pickerPos, color)
        }
        this.updatePicker()
      },

      selectPickedFace(key, face) {
        const references = this.simulation[key]
        const tree = this.document.top()
        const existing = references.find(reference => {
          const resolved = reference.clone()
          return !resolved.update(tree) && resolved.getItem() === face
        })
        if(existing) references.splice(references.indexOf(existing), 1)
        else references.push(face.faceReference())
        this.document.hasChanges = true
        this.solve()
      },

      cancelPick() {
        this.pickerSession.cancel()
      },

      deactivatePicker() {
        this.bus.emit('activate-tool', this.configuration.probeTool)
        this.document.activeSimulationPicker = false
        this.activePicker = null
        this.updatePicker = null
      },

      onEscape() {
        if(this.activePicker) this.cancelPick()
      },

      updatePaths() {
        if(!this.expanded) return
        this.bus.emit('clear-pickers')
        const tree = this.document.top()
        this.configuration.pickers.forEach(({ key }) => {
          const { pickerPos, color } = this.getPickerInfo(key)
          this.simulation[key].forEach(reference => {
            const resolved = reference.clone()
            if(resolved.update(tree)) return
            const component = tree.findChild(reference.componentId)
            const face = resolved.getItem()
            const center = face.center().applyMatrix4(worldTransform(component))
            this.bus.emit('show-picker', pickerPos, center, color)
          })
        })
        if(this.updatePicker) this.updatePicker()
      },

      getPickerInfo(key) {
        const picker = this.pickerElements[key]
        const rectangle = picker.getBoundingClientRect()
        return {
          pickerPos: { x: rectangle.left + rectangle.width / 2, y: rectangle.top + rectangle.height / 2 },
          color: window.getComputedStyle(picker).backgroundColor,
        }
      },

      remove() {
        this.document.activateSimulation(null)
        this.bus.emit('clear-pickers')
        const simulations = this.component.creator.simulations
        simulations.splice(simulations.indexOf(this.simulation), 1)
        this.document.hasChanges = true
        this.bus.emit('activate-tool', ManipulationTool)
      },
    },
  }
</script>
