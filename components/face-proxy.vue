<template></template>

<script setup>

  import * as THREE from 'three'
  import { TessellateModifier } from 'three/examples/jsm/modifiers/TessellateModifier.js'
  import materials from '../js/materials.js'

  const props = defineProps(['document', 'component', 'face', 'displayMode', 'colorMode', 'parentActive', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.face == highlight.value )

  const tessellation = props.face.tesselate()
  const faceMesh = window.alcRenderer.convertMesh(tessellation, getMaterial())
  const originalGeometry = faceMesh.geometry
  const originalPositions = originalGeometry.getAttribute('position').array.slice()
  const originalNormals = originalGeometry.getAttribute('normal').array.slice()
  let analysisGeometry = null
  let analysisResult = null
  let analysisPositions = null
  let analysisNormals = null
  let analysisDisplacements = null
  let analysisStresses = null
  const coldColor = new THREE.Color(0x003cff)
  const hotColor = new THREE.Color(0xff0000)
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

  watch(() => [props.face, highlighted.value, props.parentActive, props.parentSelected, props.displayMode, props.colorMode, props.component.getMaterial(), props.document.activeSimulation?.revision, props.document.activeSimulationPicker], () => {
    updateAnalysisGeometry()
    faceMesh.visible = props.displayMode != 'exportPreview'
    faceMesh.alcPathTracingMaterial = getSurfaceMaterial()
    const material = getMaterial()
    faceMesh.material = material
    renderNeeded.value = true
  }, { immediate: true })

  watch(() => props.document.activeSimulation?.animationRevision, () => {
    if(props.colorMode != 'static') return
    updateDeformation()
    renderNeeded.value = true
  })

  function getSurfaceMaterial() {
    if(props.face.sectionSurface) return props.component.creator.compSectionMaterial
    if(props.colorMode == 'fluid') return materials.fluidSurface
    if(props.colorMode == 'thermal') return materials.thermalSurface
    if(props.colorMode == 'static') return materials.staticSurface
    if(props.colorMode == 'interference') return materials.interferenceSurface
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

  function updateAnalysisGeometry() {
    const simulation = props.document.activeSimulation
    const simulatedFace = simulation?.result &&
      simulation.result.componentId == props.component.id &&
      simulation.result.solidId == props.face.solid.id
    if(props.colorMode == 'static' && simulatedFace) {
      if(analysisResult != simulation.result) {
        analysisGeometry?.dispose()
        const quality = simulation.quality - 1
        const edgeLength = simulation.result.step * (3 - quality * 0.5)
        const angularTolerance = 1.75 - quality * 0.175
        const tessellation = props.face.tesselateAt(edgeLength * 0.25, angularTolerance)
        const baseGeometry = window.alcRenderer.convertBufferGeometry(tessellation)
        analysisGeometry = new TessellateModifier(edgeLength, 10).modify(baseGeometry)
        baseGeometry.dispose()
        analysisResult = simulation.result
        analysisPositions = analysisGeometry.getAttribute('position').array.slice()
        analysisNormals = analysisGeometry.getAttribute('normal').array.slice()
      }
      faceMesh.geometry = analysisGeometry
    } else {
      faceMesh.geometry = originalGeometry
    }
    const basePositions = faceMesh.geometry == analysisGeometry ? analysisPositions : originalPositions
    const baseNormals = faceMesh.geometry == analysisGeometry ? analysisNormals : originalNormals
    const position = faceMesh.geometry.getAttribute('position')
    const normal = faceMesh.geometry.getAttribute('normal')
    position.array.set(basePositions)
    normal.array.set(baseNormals)
    position.needsUpdate = true
    normal.needsUpdate = true
    if(props.colorMode != 'thermal' && props.colorMode != 'static') return

    const values = new Float32Array(position.count * 3)
    const color = new THREE.Color()
    analysisDisplacements = props.colorMode == 'static' && simulatedFace ? new Float32Array(basePositions.length) : null
    analysisStresses = analysisDisplacements ? new Float32Array(position.count).fill(-1) : null
    for(let i = 0; i < position.count; i++) {
      const point = new THREE.Vector3(basePositions[i * 3], basePositions[i * 3 + 1], basePositions[i * 3 + 2])
      const staticSample = props.colorMode == 'static' && simulatedFace && simulation.sampleAt(point)
      const value = props.colorMode == 'thermal'
        ? simulatedFace && simulation.temperatureAt(point)
        : staticSample?.relativeStress
      if(value === undefined || value === false) {
        color.setRGB(0.18, 0.18, 0.18)
      } else {
        const ratio = props.colorMode == 'thermal'
          ? (value - simulation.coldTemperature) / (simulation.hotTemperature - simulation.coldTemperature)
          : Math.sqrt(Math.min(1, value))
        color.copy(coldColor).lerp(hotColor, ratio)
        if(props.colorMode == 'static') {
          analysisStresses[i] = value
          const displacement = staticSample.displacement
          analysisDisplacements[i * 3] = displacement.x * simulation.result.displayScale
          analysisDisplacements[i * 3 + 1] = displacement.y * simulation.result.displayScale
          analysisDisplacements[i * 3 + 2] = displacement.z * simulation.result.displayScale
        }
      }
      values.set(color.toArray(), i * 3)
    }
    faceMesh.geometry.setAttribute('color', new THREE.BufferAttribute(values, 3))

    if(analysisDisplacements) {
      const bounds = new THREE.Box3()
      const point = new THREE.Vector3()
      for(let i = 0; i < analysisDisplacements.length; i += 3) {
        const x = analysisPositions[i]
        const y = analysisPositions[i + 1]
        const z = analysisPositions[i + 2]
        bounds.expandByPoint(point.set(x, y, z))
        bounds.expandByPoint(point.set(
          x + analysisDisplacements[i],
          y + analysisDisplacements[i + 1],
          z + analysisDisplacements[i + 2],
        ))
      }
      analysisGeometry.boundingSphere = bounds.getBoundingSphere(new THREE.Sphere())
      updateDeformation()
    }
  }

  function updateDeformation() {
    if(!analysisDisplacements || faceMesh.geometry != analysisGeometry) return
    const position = analysisGeometry.getAttribute('position')
    const colors = analysisGeometry.getAttribute('color')
    const factor = props.document.activeSimulation.deformationFactor
    for(let i = 0; i < analysisDisplacements.length; i++) {
      position.array[i] = analysisPositions[i] + analysisDisplacements[i] * factor
    }
    for(let i = 0; i < analysisStresses.length; i++) {
      if(analysisStresses[i] < 0) continue
      const ratio = Math.sqrt(Math.min(1, analysisStresses[i] * factor))
      const offset = i * 3
      colors.array[offset] = coldColor.r + (hotColor.r - coldColor.r) * ratio
      colors.array[offset + 1] = coldColor.g + (hotColor.g - coldColor.g) * ratio
      colors.array[offset + 2] = coldColor.b + (hotColor.b - coldColor.b) * ratio
    }
    position.needsUpdate = true
    colors.needsUpdate = true
    analysisGeometry.computeVertexNormals()
  }

  function getMaterial() {
    if(props.displayMode == 'exportPreview') return materials.invisibleSurface
    if(props.face.sectionSurface) return props.component.creator.compSectionMaterial
    if(props.displayMode == 'pathTraced') return getSurfaceMaterial()
    if(props.colorMode == 'fluid' && !props.document.activeSimulationPicker) {
      return props.parentActive ? materials.fluidSurface : materials.ghostSurface
    }
    if((props.colorMode == 'thermal' || props.colorMode == 'static') && !props.document.activeSimulationPicker) {
      return props.parentActive ? getSurfaceMaterial() : materials.ghostSurface
    }
    if(props.colorMode == 'interference') {
      return props.parentActive ? materials.interferenceSurface : materials.ghostSurface
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
    analysisGeometry?.dispose()
    props.face.mesh = null
    renderNeeded.value = true
  })

</script>
