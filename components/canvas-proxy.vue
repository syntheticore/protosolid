<template></template>

<script setup>

  import * as THREE from 'three'
  import materials from '../js/materials.js'

  const props = defineProps(['canvas', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.canvas == highlight.value )

  let mesh

  watch(() => props.canvas, () => {
    window.alcRenderer.remove(mesh)

    mesh = new THREE.Mesh(new THREE.PlaneGeometry(50, 50), getMaterial())
    const aspect = props.canvas.height / props.canvas.width
    mesh.scale.set(props.canvas.scale, props.canvas.height * props.canvas.scale / props.canvas.width)

    window.alcRenderer.add(mesh)
    renderNeeded.value = true
  }, { immediate: true, deep: 1 })

  watch(() => [highlighted.value, props.parentSelected], () => {
    mesh.material = getMaterial()
    renderNeeded.value = true
  })

  watch(() => props.parentTransform, () => {
    if(!mesh) return
    mesh.matrix.copy(props.parentTransform || new THREE.Matrix4())
    mesh.matrix.decompose(mesh.position, mesh.quaternion, mesh.scale)
    renderNeeded.value = true
  }, { immediate: true })

  function getMaterial() {
    return new THREE.MeshBasicMaterial({
      ...props.canvas.texture && { map: props.canvas.texture },
      side: THREE.DoubleSide,
      ...(highlighted.value || props.parentSelected) && {
        color: new THREE.Color('#0070ff') ,
        combine: THREE.MultiplyOperation,
      },
    })
  }

  onUnmounted(() => {
    window.alcRenderer.remove(mesh)
    renderNeeded.value = true
  })

</script>
