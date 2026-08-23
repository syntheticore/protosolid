<template lang="pug">

  li.treelet-simulation(:class="{ expanded, invalid: warning }")

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

        .controls
          Icon.delete(icon="trash-alt" fixed-width title="Delete" @click.stop="remove")

      .content.form(v-if="expanded")

        fieldset

          label.picker-row(v-for="picker in configuration.pickers" :key="picker.key")
            span {{ picker.label }} ({{ simulation[picker.key].length }})
            .picker(
              :ref="element => setPickerElement(picker.key, element)"
              :class="{ active: activePicker == picker.key, filled: simulation[picker.key].length }"
              @click="pick(picker.key)"
            )

          label(v-for="input in configuration.inputs" :key="input.key")
            span {{ input.label }}
            input.input(
              type="number"
              :value="simulation[input.key]"
              @change="changed(input.key, $event)"
            )

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

  .picker-row:nth-of-type(1) .picker
    background: $blue
    border-color: lighten($blue, 75%)

  .picker-row:nth-of-type(2) .picker
    background: $purple
    border-color: lighten($purple, 75%)

  .input
    width: 64px
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
  import { ManipulationTool, StaticProbeTool, ThermalProbeTool } from '../js/tools.js'
  import { worldTransform } from '../js/core/assembly.js'

  const configurations = {
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
        { key: 'forceX', label: 'Force X' },
        { key: 'forceY', label: 'Force Y' },
        { key: 'forceZ', label: 'Force Z' },
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
        const problem = this.simulation.resolve(this.document.top())
        return problem?.warning ? problem.message : null
      },
    },

    watch: {
      expanded(expanded) {
        if(expanded) {
          this.solve()
          this.bus.emit('activate-tool', this.configuration.probeTool)
          this.$nextTick(this.updatePaths)
        } else {
          this.cancelPick()
          this.bus.emit('clear-pickers')
          this.bus.emit('activate-tool', ManipulationTool)
        }
      },
    },

    mounted() {
      this.bus.on('resize', this.updatePaths)
      this.bus.on('tree-scroll', this.updatePaths)
      this.bus.on('escape', this.onEscape)
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
      this.cancelPick()
      this.bus.off('resize', this.updatePaths)
      this.bus.off('tree-scroll', this.updatePaths)
      this.bus.off('escape', this.onEscape)
      this.document.off('regenerated', this.regenerated)
    },

    methods: {
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

      regenerated() {
        if(this.expanded) this.solve()
        this.$nextTick(this.updatePaths)
      },

      solve() {
        this.simulation.solve(this.document.top())
        if(this.configuration.resultEvent) this.bus.emit(this.configuration.resultEvent)
        this.bus.emit('render-needed')
        this.$nextTick(this.updatePaths)
      },

      changed(key, event) {
        this.simulation[key] = Number(event.target.value)
        this.document.hasChanges = true
        this.solve()
      },

      pick(key) {
        if(this.activePicker) {
          const activePicker = this.activePicker
          this.cancelPick()
          if(activePicker != key) this.pick(key)
          return
        }

        this.activePicker = key
        this.document.activeSimulationPicker = true
        this.bus.off('picked')
        this.bus.once('picked', (face, repick) => {
          const references = this.simulation[key]
          const existing = references.find(reference =>
            reference.componentId == face.solid.compound.componentId &&
            reference.solidId == face.solid.id &&
            reference.topoId == face.id
          )
          if(existing) references.splice(references.indexOf(existing), 1)
          else references.push(face.faceReference())

          this.cancelPick()
          this.document.hasChanges = true
          this.solve()
          const pickerIndex = this.configuration.pickers.findIndex(picker => picker.key == key)
          const nextPicker = this.configuration.pickers[pickerIndex + 1]
          setTimeout(() => {
            if(repick) this.pick(key)
            else if(nextPicker) this.pick(nextPicker.key)
            else this.bus.emit('activate-tool', this.configuration.probeTool)
          })
        })

        this.updatePicker = () => {
          const { pickerPos, color } = this.getPickerInfo(key)
          this.bus.emit('pick', 'face', pickerPos, color)
        }
        this.updatePicker()
      },

      cancelPick() {
        if(this.activePicker) this.bus.emit('activate-tool', this.configuration.probeTool)
        this.bus.off('picked')
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
        this.configuration.pickers.forEach(({ key }) => {
          const { pickerPos, color } = this.getPickerInfo(key)
          this.simulation[key].forEach(reference => {
            const component = this.document.top().findChild(reference.componentId)
            const solid = component && component.compound.solids().find(item => item.id == reference.solidId)
            const face = solid && solid.faces().find(item => item.id == reference.topoId)
            if(!face) return
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
