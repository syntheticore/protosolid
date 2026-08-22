<template lang="pug">

  li.treelet-thermal(:class="{ expanded, invalid: problem }")

    .box

      header(@click="toggle")

        Icon(icon="thermometer-half" fixed-width)

        h2 {{ simulation.title }}

        Icon.problem(v-if="problem" icon="exclamation-triangle" :title="problem")

        Icon.expand(icon="angle-right")

        .controls

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="remove"
          )

      .content.form(v-if="expanded")

        fieldset

          label
            span Cold faces ({{ simulation.coldFaces.length }})
            .picker.cold(
              ref="coldFaces"
              :class="{ active: activePicker == 'coldFaces', filled: simulation.coldFaces.length }"
              @click="pick('coldFaces')"
            )

          label
            span Hot faces ({{ simulation.hotFaces.length }})
            .picker.hot(
              ref="hotFaces"
              :class="{ active: activePicker == 'hotFaces', filled: simulation.hotFaces.length }"
              @click="pick('hotFaces')"
            )

          label
            span Cold temperature °C
            input.input(type="number" v-model.number="simulation.coldTemperature" @change="changed")

          label
            span Hot temperature °C
            input.input(type="number" v-model.number="simulation.hotTemperature" @change="changed")

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

  .cold
    background: #168cff
    border-color: lighten(#168cff, 75%)

  .hot
    background: #ff3b24
    border-color: lighten(#ff3b24, 75%)

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
  import { ManipulationTool, ThermalProbeTool } from '../js/tools.js'
  import { worldTransform } from '../js/core/assembly.js'

  export default {
    name: 'TreeletThermal',

    inject: ['bus'],

    props: {
      document: Object,
      component: Object,
      simulation: Object,
    },

    data() {
      return {
        activePicker: null,
      }
    },

    computed: {
      expanded() {
        return this.document.activeSimulation === this.simulation
      },

      problem() {
        this.document.timeline.marker
        return this.simulation.resolve(this.document.top())
      },

    },

    watch: {
      expanded(expanded) {
        if(expanded) {
          this.solve()
          this.bus.emit('activate-tool', ThermalProbeTool)
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
      this.bus.on('escape', this.onEscape)
      this.document.on('regenerated', this.regenerated)
      if(this.expanded) {
        this.solve()
        this.bus.emit('activate-tool', ThermalProbeTool)
        this.$nextTick(this.updatePaths)
      }
    },

    beforeUnmount() {
      this.cancelPick()
      this.bus.off('resize', this.updatePaths)
      this.bus.off('escape', this.onEscape)
      this.document.off('regenerated', this.regenerated)
    },

    methods: {
      toggle() {
        this.document.activateSimulation(this.expanded ? null : this.simulation)
      },

      regenerated() {
        if(this.expanded) this.solve()
        this.$nextTick(this.updatePaths)
      },

      solve() {
        this.simulation.solve(this.document.top())
        this.bus.emit('thermal-result')
        this.bus.emit('render-needed')
        this.$nextTick(this.updatePaths)
      },

      changed() {
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
          if(existing) {
            references.splice(references.indexOf(existing), 1)
          } else {
            references.push(face.faceReference())
          }

          this.cancelPick()
          this.document.hasChanges = true
          this.solve()
          setTimeout(() => repick ? this.pick(key) : this.bus.emit('activate-tool', ThermalProbeTool))
        })

        this.updatePicker = () => {
          const { pickerPos, color } = this.getPickerInfo(key)
          this.bus.emit('pick', 'face', pickerPos, color)
        }
        this.updatePicker()
      },

      cancelPick() {
        if(this.activePicker) this.bus.emit('activate-tool', ThermalProbeTool)
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
        setTimeout(() => {
          if(!this.expanded) return
          this.bus.emit('clear-pickers')
          for(const key of ['coldFaces', 'hotFaces']) {
            const { pickerPos, color } = this.getPickerInfo(key)
            this.simulation[key].forEach(reference => {
              const component = this.document.top().findChild(reference.componentId)
              const solid = component && component.compound.solids().find(item => item.id == reference.solidId)
              const face = solid && solid.faces().find(item => item.id == reference.topoId)
              if(!face) return
              const center = face.center().applyMatrix4(worldTransform(component))
              this.bus.emit('show-picker', pickerPos, center, color)
            })
          }
          if(this.updatePicker) this.updatePicker()
        })
      },

      getPickerInfo(key) {
        const picker = this.$refs[key]
        const rectangle = picker.getBoundingClientRect()
        return {
          pickerPos: {
            x: rectangle.left + rectangle.width / 2,
            y: rectangle.top + rectangle.height / 2,
          },
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
