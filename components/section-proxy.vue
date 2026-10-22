<template></template>

<script setup>

  import * as THREE from 'three'
  import { TransformGizmo } from '../js/three/gizmos.js'

  const props = defineProps(['document', 'component', 'section', 'parentTransform'])
  const renderNeeded = inject('render-needed')

  let gizmo = null

  watch(() => props.document.activeInteractivePanel === `section:${props.component.id}:${props.section.id}` &&
    !props.document.activeSketch && !props.document.activeFeature && !props.document.creatingFeature,
  visible => {
    if(visible) {
      gizmo = new TransformGizmo(getWorldTransform(), transform => {
        const local = (props.parentTransform || new THREE.Matrix4()).clone().invert()
          .multiply(transform)
        props.section.setTransform(local, gizmo.rotationControl.dragging)
        props.document.markChanged()
        renderNeeded.value = true
      })
      gizmo.rotationControl.showZ = false
      gizmo.addEventListener('dragging-changed', event => {
        if(!event.value) syncTransform()
      })
      window.alcRenderer.addGizmo(gizmo)
    } else {
      window.alcRenderer.removeGizmo(gizmo)
      gizmo = null
    }
  }, { immediate: true, flush: 'sync' })

  watch(getWorldTransform, syncTransform)

  function getWorldTransform() {
    return (props.parentTransform || new THREE.Matrix4()).clone()
      .multiply(props.section.getTransform())
  }

  function syncTransform() {
    if(gizmo && !gizmo.dragging) {
      gizmo.setTransform(getWorldTransform())
      renderNeeded.value = true
    }
  }

  onUnmounted(() => window.alcRenderer.removeGizmo(gizmo))

</script>
