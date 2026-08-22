<template></template>

<script setup>

  import * as THREE from 'three'
  import materials from '../js/materials.js'

  const props = defineProps(['document', 'component', 'face', 'displayMode', 'colorMode', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.face == highlight.value )

  const faceMesh = window.alcRenderer.convertMesh(props.face.tesselate(), getMaterial())
  faceMesh.alcTypes = [
    'face',
    props.face.getPlane() && 'plane',
    props.face.getAxis() && 'axis',
    props.face.getPoint() && 'point',
  ].filter(Boolean)

  watch(() => props.face, () => {
    props.face.mesh = () => faceMesh
    faceMesh.alcObject = props.face
  }, { immediate: true })

  watch(() => props.parentActive, () => {
    faceMesh.alcProjectable = props.parentActive
    faceMesh.castShadow = props.parentActive
    faceMesh.receiveShadow = props.parentActive
    window.alcRenderer.remove(faceMesh)
    window.alcRenderer.add(faceMesh, props.parentActive)
  }, { immediate: true })

  watch(() => props.parentTransform, () => {
    faceMesh.matrix.copy(props.parentTransform)
    faceMesh.matrix.decompose(faceMesh.position, faceMesh.quaternion, faceMesh.scale)
    renderNeeded.value = true
  }, { immediate: true })

  renderNeeded.value = true

  watch(() => [highlighted.value, props.parentActive, props.parentSelected, props.displayMode, props.colorMode, props.component.creator.material, props.document.activeSimulation?.revision, props.document.activeSimulationPicker], () => {
    updateThermalColors()
    const material = getMaterial()
    faceMesh.material = material
    renderNeeded.value = true
  }, { immediate: true })

  function getSurfaceMaterial() {
    if(props.colorMode == 'thermal') return materials.thermalSurface
    if(props.colorMode.startsWith('diagnostic:')) {
      const [mode, direction, frequency] = props.colorMode.slice('diagnostic:'.length).split(':')
      if(mode == 'zebra') {
        materials.diagnostic.zebra.setFrequency(Number(frequency))
        return materials.diagnostic.zebra
      }
      return materials.diagnostic.draft[direction]
    }
    if(props.colorMode == 'component') return props.component.creator.compMaterial
    const material = props.component.getMaterial()
    return material ? material.displayMaterial : materials.surface
  }

  function updateThermalColors() {
    if(props.colorMode != 'thermal') return
    const simulation = props.document.activeSimulation
    const position = faceMesh.geometry.getAttribute('position')
    const values = new Float32Array(position.count * 3)
    const color = new THREE.Color()
    const coldColor = new THREE.Color(0x003cff)
    const hotColor = new THREE.Color(0xff0000)
    const simulatedFace = simulation?.result &&
      simulation.result.componentId == props.component.id &&
      simulation.result.solidId == props.face.solid.id

    for(let i = 0; i < position.count; i++) {
      const temperature = simulatedFace ? simulation.temperatureAt(
        new THREE.Vector3(position.getX(i), position.getY(i), position.getZ(i))
      ) : undefined
      if(temperature === undefined) {
        color.setRGB(0.18, 0.18, 0.18)
      } else {
        const ratio = (temperature - simulation.coldTemperature) /
          (simulation.hotTemperature - simulation.coldTemperature)
        color.copy(coldColor).lerp(hotColor, ratio)
      }
      values.set(color.toArray(), i * 3)
    }
    faceMesh.geometry.setAttribute('color', new THREE.BufferAttribute(values, 3))
  }

  function getMaterial() {
    if(props.colorMode == 'thermal' && !props.document.activeSimulationPicker) {
      return props.parentActive ? materials.thermalSurface : materials.ghostSurface
    }
    const diagnostic = props.colorMode.startsWith('diagnostic:')
    return highlighted.value || (props.parentSelected && !diagnostic) ?
      materials.highlightSurface
      :
      props.displayMode == 'wireframe' ?
        materials.invisibleSurface
        :
        props.parentActive ? getSurfaceMaterial() : materials.ghostSurface
  }

  onUnmounted(() => {
    window.alcRenderer.remove(faceMesh)
    props.face.mesh = null
    renderNeeded.value = true
  })

</script>
