<template lang="pug">

  li.treelet-cam(:class="{ expanded }")

    .box

      TreeItemHeader(
        :model-value="setup.title"
        :suffix="` - ${postProcessors[setup.postProcessor].label}`"
        :disabled="component.readOnly"
        label-only
        @click="expanded = !expanded"
        @rename="rename"
      )

        template(#icon)
          Icon(icon="industry" fixed-width)

        Icon.expand(icon="angle-right")

        .controls.wide

          Icon(
            icon="file-export" fixed-width
            title="Export G-code"
            @click.stop="exportFile"
          )

          Icon.delete(icon="trash-alt" fixed-width title="Delete" @click.stop="remove")

      .content.form(v-if="expanded")

        fieldset(:class="{ collapsed: collapsed.has('stock') }")

          h3.section(@click="toggleSection('stock')")
            Icon.fold(icon="angle-right" fixed-width :class="{ open: !collapsed.has('stock') }")
            span Stock

          template(v-if="!collapsed.has('stock')")

            label(title="Side of the part facing the spindle")
              ComboBox(:model-value="setup.up" label="Up" :options="Object.keys(orientations).map(value => ({ value, label: value }))" @update:model-value="set(setup, 'up', $event)")
              span Facing Spindle

            label(title="Point the machine is zeroed on")
              ComboBox(:model-value="setup.origin" label="Work Zero" :options="choices(origins)" @update:model-value="set(setup, 'origin', $event)")
              span Work Zero

            label(title="Material around the part")
              ScalarInput(:value="setup.stockOffset" :min="0" stretch @update:value="set(setup, 'stockOffset', $event)")
              span Offset (mm)

            label(title="Material below the part, held by the vise. The tool stays above it.")
              ScalarInput(:value="setup.clampHeight" :min="0" stretch @update:value="set(setup, 'clampHeight', $event)")
              span Clamp Height (mm)

            label.readout(v-if="setup.result" title="Size of the block to clamp into the vise")
              span.value {{ stockSize }}
              span Stock Size (mm)

            label
              RangeSlider(:min="1" :max="5" :step="1" title="Refines the toolpaths and the simulated stock" :model-value="setup.quality" @change="set(setup, 'quality', $event)")
              span Quality {{ setup.quality }} / 5

        fieldset(
          v-for="(operation, index) in setup.operations"
          :key="operation.id"
          :class="{ collapsed: collapsed.has(operation.id), current: player.simulation && index == player.operationIndex }"
        )

          h3.section(@click="toggleSection(operation.id)")
            Icon.fold(icon="angle-right" fixed-width :class="{ open: !collapsed.has(operation.id) }")
            Icon(:icon="operationTypes[operation.type].icon" fixed-width)
            span {{ operationTypes[operation.type].label }}
            Icon.remove(icon="trash-alt" fixed-width title="Delete operation" @click.stop="removeOperation(index)")

          template(v-if="!collapsed.has(operation.id)")

            label(v-for="field in operationFields.filter(field => field.key in operation)" :key="field.key")
              ComboBox(v-if="field.options" :model-value="operation[field.key]" :label="field.label" :options="choices(field.options)" @update:model-value="set(operation, field.key, $event)")
              ScalarInput(v-else :value="operation[field.key]" :min="field.min" stretch @update:value="set(operation, field.key, $event)")
              span {{ field.label }}

            label.readout(v-if="operation.type == 'drill' && setup.result" title="Holes along the spindle axis that fit the drill")
              span.value {{ setup.result.holes.filter(hole => drillFits(hole, operation)).length }} of {{ setup.result.holes.length }}
              span Holes

        fieldset(v-if="!setup.operations.length")
          p.hint Add operations from the Milling tab

        fieldset(:class="{ collapsed: collapsed.has('post') }")

          h3.section(@click="toggleSection('post')")
            Icon.fold(icon="angle-right" fixed-width :class="{ open: !collapsed.has('post') }")
            span Post Processing

          template(v-if="!collapsed.has('post')")

            label
              ComboBox(
                :model-value="setup.postProcessor"
                label="Controller"
                :options="Object.entries(postProcessors).map(([value, post]) => ({ value, label: post.label }))"
                @update:model-value="set(setup, 'postProcessor', $event)"
              )
              span Controller

            label
              button.button(title="Choose file location" @click="chooseDestination")
                Icon(icon="folder")
              span(:title="setup.path") {{ setup.path || 'Destination Path' }}

            label(title="Export G-code when saving document")
              ToggleSwitch(:model-value="setup.autoSave" @update:model-value="set(setup, 'autoSave', $event)")
              span Export on Save

</template>


<style lang="stylus" scoped>

  :deep(h2.tree-item-name)
    margin: 0 !important

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: transform 0.15s

    .expanded &
      transform: rotate(90deg)

  .form
    .button
      padding: 0
      svg
        padding: 3px

    label span
      text-overflow: ellipsis
      overflow: hidden
      max-width: 120px
      line-height: 1.2

  // Headings sit in the middle of collapsed sections
  fieldset.collapsed
    padding-bottom: 0

  // The operation being simulated
  fieldset
    transition: background 0.15s

    &.current
      background: bright1(1, 0.05)

  .section
    display: flex
    align-items: center
    gap: 6px
    cursor: pointer
    user-select: none

    .fold
      margin-left: -3px
      color: bright2()
      transition: transform 0.15s

      &.open
        transform: rotate(90deg)

    .remove
      margin-left: auto

      &:hover
        color: var(--cancel)

  .readout .value
    text-align: right
    color: bright1()

  .hint
    font-size: 12px
    color: bright2()

</style>


<script setup>

  import {
    cutDirections, drillFits, exportGcode, generateToolpaths, operationTypes, orientations, origins,
    postProcessors, toolShapes,
  } from '../js/core/cam.js'
  import CamPlayer from '../js/cam-player.js'
  import { chooseSavePath, exportWithToast } from '../js/utils.js'

  const operationFields = [
    { key: 'shape', label: 'Tool', options: toolShapes },
    { key: 'toolDiameter', label: 'Tool Ø (mm)', min: 0.1 },
    { key: 'direction', label: 'Direction', options: cutDirections },
    { key: 'stepover', label: 'Stepover (mm)', min: 0.05 },
    { key: 'stepdown', label: 'Stepdown (mm)', min: 0.05 },
    { key: 'peck', label: 'Peck Depth (mm)', min: 0.1 },
    { key: 'leave', label: 'Stock to Leave (mm)', min: 0 },
    { key: 'feed', label: 'Feed (mm/min)', min: 1 },
    { key: 'plungeFeed', label: 'Plunge (mm/min)', min: 1 },
    { key: 'spindleSpeed', label: 'Spindle (rpm)', min: 1 },
  ]

  const props = defineProps(['document', 'component', 'setup'])
  const bus = inject('bus')

  const player = reactive(new CamPlayer(simulation => bus.emit('cam-frame', simulation)))
  const collapsed = reactive(new Set())

  const panelKey = computed(() => `cam:${props.component.id}:${props.setup.id}`)

  const expanded = computed({
    get: () => props.document.activeInteractivePanel === panelKey.value,
    set: value => {
      if(value) props.document.activateComponent(props.component)
      props.document.activateInteractivePanel(value ? panelKey.value : null)
    },
  })

  const choices = map => Object.entries(map).map(([value, label]) => ({ value, label }))

  const stockSize = computed(() => {
    const { width, depth, height } = props.setup.result.grid
    return [width, depth, height].map(value => Number(value.toFixed(1))).join(' × ')
  })

  let shown = false

  watch(expanded, value => value ? generate() : hide())
  watch(() => JSON.stringify([
    props.setup.up, props.setup.origin, props.setup.stockOffset, props.setup.clampHeight,
    props.setup.quality, props.setup.operations,
  ]), generate)

  onMounted(() => {
    props.document.on('regenerated', generate)
    generate()
  })

  onUnmounted(() => {
    props.document.off('regenerated', generate)
    hide()
  })

  function generate() {
    if(!expanded.value) return
    const result = generateToolpaths(props.setup, props.component)
    props.setup.result = result && markRaw(result)
    player.load(props.setup.result)
    props.document.camPlayer = player
    shown = true
    bus.emit('cam-result', { setup: props.setup, component: props.component })
  }

  function hide() {
    player.load(null)
    if(props.document.camPlayer === player) props.document.camPlayer = null
    props.setup.result = null
    if(shown) bus.emit('cam-result', { setup: props.setup, component: props.component })
    shown = false
  }

  function toggleSection(key) {
    collapsed.has(key) ? collapsed.delete(key) : collapsed.add(key)
  }

  function rename(title) {
    props.setup.title = title
    props.document.markChanged()
  }

  function set(target, key, value) {
    target[key] = value
    props.document.markChanged()
  }

  function removeOperation(index) {
    props.setup.operations.splice(index, 1)
    props.document.markChanged()
  }

  async function chooseDestination() {
    const path = await chooseSavePath('nc', props.setup.title)
    if(path) set(props.setup, 'path', path)
  }

  async function exportFile() {
    const path = await exportWithToast(props.document.bus, 'CAM', () => exportGcode(props.setup, props.component))
    if(path && path != props.setup.path) set(props.setup, 'path', path)
  }

  function remove() {
    if(expanded.value) props.document.activateInteractivePanel(null)
    const creator = props.component.creator
    creator.camSetups = creator.camSetups.filter(setup => setup != props.setup)
    props.document.markChanged()
  }

</script>
