<template></template>

<script setup>

  import * as THREE from 'three'
  import { PlaneHelper, AxisHelper, PointHelper } from '../js/core/helpers.js'
  import { PlaneHelperObject, AxisHelperObject, PointHelperObject } from '../js/three/helper-objects.js'

  const props = defineProps(['document', 'component', 'helper', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.helper == highlight.value )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.helper) )

  let mesh

  watch(() => props.helper, () => {
    window.alcRenderer.remove(mesh)
    const HelperObject = {
      [PlaneHelper]: PlaneHelperObject,
      [AxisHelper]: AxisHelperObject,
      [PointHelper]: PointHelperObject,
    }[props.helper.constructor]
    mesh = new HelperObject(props.helper)
    props.helper.mesh = () => mesh
    props.helper.component = props.component

    window.alcRenderer.add(mesh, true)
    renderNeeded.value = true
  }, { immediate: true })

  watch(() => [highlighted.value, selected.value], () => {
    mesh.setMaterial(highlighted.value, selected.value)
    renderNeeded.value = true
  })

  watch(() => props.parentTransform, () => {
    if(!mesh) return
    const local = props.helper.transform
    mesh.matrix.copy(props.parentTransform || new THREE.Matrix4()).multiply(local)
    mesh.matrix.decompose(mesh.position, mesh.quaternion, mesh.scale)
    renderNeeded.value = true
  }, { immediate: true })

  onUnmounted(() => {
    window.alcRenderer.remove(mesh)
    props.helper.mesh = null
    renderNeeded.value = true
  })

</script>
