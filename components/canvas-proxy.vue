<template></template>

<script setup>

  import * as THREE from 'three'
  import { CanvasPositionGizmo } from '../js/three/gizmos.js'

  const props = defineProps(['document', 'canvas', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits([])

  const highlight = inject('highlight')
  const renderNeeded = inject('render-needed')

  const highlighted = computed(() => props.parentHighlighted || props.canvas == highlight.value )

  let mesh = new THREE.Mesh(new THREE.PlaneGeometry(50, 50), getMaterial())
  let positionGizmo = null

  // Keep the mesh alive while the image and its dimensions are populated. The
  // renderer disposes textures when an object is removed, so rebuilding this
  // mesh for every canvas change makes subsequent updates use a dead texture.
  window.alcRenderer.add(mesh)

  watch(() => !!props.canvas.texture &&
    props.document.isTreeletExpanded(`canvas:${props.canvas.id}`) &&
    !props.document.activeSketch && !props.document.activeFeature &&
    !props.document.activeSimulation && !props.document.activeExportConfig,
  visible => {
    if(visible) {
      positionGizmo = new CanvasPositionGizmo(getPlaneTransform(), props.canvas.x, props.canvas.y, (x, y) => {
        props.canvas.x = x
        props.canvas.y = y
        updateTransform()
        props.document.markChanged()
        renderNeeded.value = true
      })
      window.alcRenderer.addGizmo(positionGizmo)
    } else {
      window.alcRenderer.removeGizmo(positionGizmo)
      positionGizmo = null
    }
  }, { immediate: true })

  watch(() => [
    props.canvas.texture,
    props.canvas.width,
    props.canvas.height,
    props.canvas.scale,
    props.canvas.transparency,
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
    const planeTransform = getPlaneTransform()
    mesh.matrix.copy(planeTransform)
      .multiply(localTransform)
    mesh.matrix.decompose(mesh.position, mesh.quaternion, mesh.scale)

    const width = Number(props.canvas.width)
    const height = Number(props.canvas.height)
    const scale = Number(props.canvas.scale) || 1
    const aspect = width > 0 && height > 0 ? height / width : 1
    mesh.scale.set(scale, aspect * scale, 1)

    if(positionGizmo && !positionGizmo.dragging) {
      positionGizmo.setFrame(planeTransform)
      positionGizmo.setPosition(props.canvas.x, props.canvas.y)
    }
  }

  function getPlaneTransform() {
    return (props.parentTransform || new THREE.Matrix4()).clone()
      .multiply(props.canvas.getTransform())
  }

  function getMaterial() {
    return new THREE.MeshBasicMaterial({
      ...props.canvas.texture && { map: props.canvas.texture },
      side: THREE.DoubleSide,
      toneMapped: false,
      transparent: props.canvas.transparency > 0,
      opacity: 1 - props.canvas.transparency,
      depthWrite: props.canvas.transparency === 0,
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
    window.alcRenderer.removeGizmo(positionGizmo)
    const material = mesh?.material
    const texture = material?.map
    window.alcRenderer.remove(mesh)
    material?.dispose()
    texture?.dispose()
    mesh = null
    renderNeeded.value = true
  })

</script>
