<template></template>

<script setup>

  import * as THREE from 'three'
  import materials from '../js/materials.js'

  const props = defineProps(['canvas', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.canvas == highlight.value )

  let mesh = new THREE.Mesh(new THREE.PlaneGeometry(50, 50), getMaterial())

  // Keep the mesh alive while the image and its dimensions are populated. The
  // renderer disposes textures when an object is removed, so rebuilding this
  // mesh for every canvas change makes subsequent updates use a dead texture.
  window.alcRenderer.add(mesh)

  watch(() => [
    props.canvas.texture,
    props.canvas.width,
    props.canvas.height,
    props.canvas.scale,
    props.canvas.x,
    props.canvas.y,
    props.canvas.plane,
  ], () => {
    if(!mesh) return

    updateTransform()
    setMaterial()
    renderNeeded.value = true
  }, { immediate: true })

  watch(() => [highlighted.value, props.parentSelected], () => {
    if(!mesh) return
    setMaterial()
    renderNeeded.value = true
  })

  watch(() => props.parentTransform, () => {
    if(!mesh) return
    updateTransform()
    renderNeeded.value = true
  }, { immediate: true })

  function updateTransform() {
    const localTransform = new THREE.Matrix4().makeTranslation(
      Number(props.canvas.x) || 0,
      Number(props.canvas.y) || 0,
      0,
    )
    mesh.matrix.copy(props.parentTransform || new THREE.Matrix4())
      .multiply(props.canvas.getTransform())
      .multiply(localTransform)
    mesh.matrix.decompose(mesh.position, mesh.quaternion, mesh.scale)

    const width = Number(props.canvas.width)
    const height = Number(props.canvas.height)
    const scale = Number(props.canvas.scale) || 1
    const aspect = width > 0 && height > 0 ? height / width : 1
    mesh.scale.set(scale, aspect * scale, 1)
  }

  function getMaterial() {
    return new THREE.MeshBasicMaterial({
      ...props.canvas.texture && { map: props.canvas.texture },
      side: THREE.DoubleSide,
      toneMapped: false,
      ...(highlighted.value || props.parentSelected) && {
        color: new THREE.Color('#0070ff') ,
        combine: THREE.MultiplyOperation,
      },
    })
  }

  function setMaterial() {
    const previous = mesh.material
    mesh.material = getMaterial()
    previous.dispose()
  }

  onUnmounted(() => {
    const material = mesh?.material
    const texture = material?.map
    window.alcRenderer.remove(mesh)
    material?.dispose()
    texture?.dispose()
    mesh = null
    renderNeeded.value = true
  })

</script>
