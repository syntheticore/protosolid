<template lang="pug">

  ElementProxy(
    v-for="elem in elements"
    :key="elem.id"
    :document="document"
    :component="component"
    :element="elem"
    :active-tool="activeTool"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
    :parent-transform="parentTransform"
  )

  ProfileProxy(
    v-for="profile in profiles"
    :key="profile.id"
    :component="component"
    :profile="profile"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
    :parent-transform="parentTransform"
  )

  template(v-if="active")

    ConstraintProxy(
      v-for="constraint in sketch.constraints"
      :key="constraint.id"
      :document="document"
      :constraint="constraint"
      @dimensionMouseUp="(...args) => emit('dimensionMouseUp', ...args)"
      @dimensionMouseDown="(...args) => emit('dimensionMouseDown', ...args)"
      @dimensionMouseMove="(...args) => emit('dimensionMouseMove', ...args)"
    )

    .drag-handle.handle(
      v-for="{ handle, pos } in projectedHandles"
      :key="handle.id"
      :style="{ top: pos.y + 'px', left: pos.x + 'px' }"
      @mouseup="emit('handleMouseUp', $event, handle)"
      @mousedown="emit('handleMouseDown', $event, handle)"
      @mousemove="emit('handleMouseMove', $event, handle)"
      @mouseleave="emit('handleMouseLeave', $event, handle)"
    )

</template>


<style lang="stylus" scoped>

  .drag-handle
    pointer-events: auto
    // cursor: grab

    &:hover

      &::before
        width: 5px
        height: 5px

    &:active

      &::before
        width:  5px
        height: 5px

    // .orbiting &
    //   pointer-events: none

</style>


<script setup>

  import { Circle } from '../js/core/geom2d.js'

  const props = defineProps(['document', 'component', 'sketch', 'activeHandle', 'activeTool', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits(['handleMouseDown', 'handleMouseUp', 'handleMouseMove', 'handleMouseLeave', 'dimensionMouseUp', 'dimensionMouseDown', 'dimensionMouseMove'])
  // defineOptions({ inheritAttrs: false })

  const highlight = inject('highlight')
  const frame = inject('frame')

  const highlighted = computed(() => props.parentHighlighted || props.sketch == highlight.value )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.sketch) )
  const active = computed(() => props.sketch == props.document.activeSketch )

  const elements = computed(() =>
    [...props.sketch.elements, ...props.sketch.projections.map(proj => proj.geometry() )].filter(Boolean)
  )

  const allHandles = computed(() => {
    if(!active.value) return []

    const projectedElements = props.sketch.projections
      .map(projection => projection.geometry())
      .filter(elem => elem instanceof Circle)
    const handles = [...props.sketch.elements, ...projectedElements].flatMap(elem => {
      const elemHandles = elem.projection ? elem.handles().slice(0, 1) : elem.handles()
      return elemHandles.map((p, i) => {
        p = p.clone().applyMatrix4(elem.sketch.workplane)
        if(props.parentTransform) p.applyMatrix4(props.parentTransform)
        return {
          type: 'handle',
          pos: window.alcRenderer.toScreen(p),
          vec: p,
          id: elem.id + i,
          elem,
          index: i,
          readonly: !!elem.projection,
        }
      })
    })

    const set = {}
    handles.forEach(handle => set[JSON.stringify(handle.pos)] = handle )
    return Object.values(set)
  })

  const projectedHandles = computed(() => {
    frame.value
    return allHandles.value.map(handle => ({ handle, pos: window.alcRenderer.toScreen(handle.vec) }))
  })

  function solve() {
    const handle = props.activeHandle || props.sketch.solveHandle
    props.sketch.solve(props.document.top(), handle)
  }

  watch(() => props.sketch.solveNeeded, () => {
    if(!props.sketch.solveNeeded) return
    solve()
    props.sketch.solveNeeded = false
  })

  const profiles = ref([])
  let fresh = true

  watch(() => props.sketch.profileUpdateNeeded, () => {
    if(!props.sketch.profileUpdateNeeded && !fresh) return
    solve()
    profiles.value = props.sketch.profiles(props.component)
    props.sketch.profileUpdateNeeded = false
    props.sketch.solveHandle = null
    fresh = false
  }, { immediate: true })

  watch(() => props.component, () => {
    props.sketch.component = props.component
  }, { immediate: true })

</script>
