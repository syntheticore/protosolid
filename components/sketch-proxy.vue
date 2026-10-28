<template lang="pug">

  ElementProxy(
    v-for="elem in elements"
    :key="elem.id"
    :document="document"
    :component="component"
    :element="elem"
    :display-mode="displayMode"
    :active-tool="activeTool"
    :show-spline-controls="active"
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
      :active-tool="activeTool"
      :parent-transform="parentTransform"
      @dimensionMouseUp="(...args) => emit('dimensionMouseUp', ...args)"
      @dimensionMouseDown="(...args) => emit('dimensionMouseDown', ...args)"
      @dimensionMouseMove="(...args) => emit('dimensionMouseMove', ...args)"
    )

    .drag-handle.handle(
      v-for="{ handle, pos } in projectedHandles"
      :key="handle.id"
      :class="{ highlighted: highlight instanceof ElemRef && highlight.curve() == handle.elem && highlight.index == handle.index, projected: !!handle.elem.projection, broken: !!handle.elem.projection?.error }"
      :style="{ top: pos.y + 'px', left: pos.x + 'px' }"
      @mouseup="emit('handleMouseUp', $event, handle)"
      @mousedown="emit('handleMouseDown', $event, handle)"
      @mousemove="emit('handleMouseMove', $event, handle)"
      @mouseleave="emit('handleMouseLeave', $event, handle)"
      @pointerdown="emit('handlePointerDown', $event)"
      @wheel.prevent="emit('handleWheel', $event)"
    )

</template>


<style lang="stylus" scoped>

  .drag-handle
    pointer-events: auto

    &:hover, &:active, &.highlighted
      &::before
        width:  calc(100% - 10px)
        height: calc(100% - 10px)

    &.highlighted::before
      background: $highlight

    // Projected points are fixed references, unlike draggable handles,
    // so their dot keeps its size on hover. Follows the hover rule above.
    // Spanning the whole handle, the ring frames a coincident handle's ring.
    &.projected
      &::before
        width: 8px
        height: 8px
        background: $projected

      &::after
        box-sizing: border-box
        width: 100%
        height: 100%
        border-color: $projected

      &.broken
        &::before
          background: $brokenProjected

        &::after
          border-color: $brokenProjected

      &.highlighted
        &::before
          background: $highlight

        &::after
          border-color: $highlight

</style>


<script setup>

  import { Circle, OffsetCurve } from '../js/core/geom2d.js'
  import { ProjectedPoint, ElemRef } from '../js/core/sketch.js'
  import { layoutConstraintIcons } from '../js/three/constraint-icons.js'

  const props = defineProps(['document', 'component', 'sketch', 'displayMode', 'activeHandle', 'activeTool', 'parentHighlighted', 'parentSelected', 'parentTransform'])
  const emit = defineEmits(['handleMouseDown', 'handleMouseUp', 'handleMouseMove', 'handleMouseLeave', 'handlePointerDown', 'handleWheel', 'dimensionMouseUp', 'dimensionMouseDown', 'dimensionMouseMove'])
  // defineOptions({ inheritAttrs: false })

  const highlight = inject('highlight')
  const frame = inject('frame')

  const constraintIcons = shallowReactive(new Map())
  // A removed constraint stays registered until its proxy unmounts, while
  // the curves it referenced may already be cleared.
  const constraintIconLayout = computed(() => {
    const ids = new Set(props.sketch.constraints.map(constraint => constraint.id))
    return layoutConstraintIcons([...constraintIcons]
      .filter(([id]) => ids.has(id))
      .map(([id, proxies]) => [id, proxies.value]))
  })
  provide('constraint-icons', constraintIcons)
  provide('constraint-icon-layout', constraintIconLayout)

  const highlighted = computed(() => props.parentHighlighted || props.sketch == highlight.value || props.sketch.creator == highlight.value )
  const selected = computed(() => props.parentSelected || props.document.selection.has(props.sketch) )
  const active = computed(() => props.sketch == props.document.activeSketch )

  const elements = computed(() =>
    [...props.sketch.elements, ...props.sketch.projectedElements()]
      .filter(elem => !(elem instanceof ProjectedPoint))
  )

  const allHandles = computed(() => {
    if(!active.value) return []

    const projectedElements = props.sketch.projectedElements()
      .filter(elem => elem instanceof Circle || elem instanceof ProjectedPoint)
    const origin = props.sketch.origin()
    const elements = [...props.sketch.elements, ...projectedElements, origin]
      .filter(elem => props.activeTool?.showsHandles?.(elem) !== false)
    const handles = elements.flatMap(elem => {
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
          readonly: elem == origin || !!elem.projection || elem instanceof OffsetCurve,
        }
      })
    })

    const set = {}
    const projected = {}
    handles.forEach(handle => {
      const key = JSON.stringify(handle.pos)
      // Projected points stay visible around coincident handles
      if(handle.elem.projection) return projected[key] = handle
      // Prefer draggable geometry over the origin and other readonly handles.
      if(set[key] && !set[key].readonly && handle.readonly) return
      set[key] = handle
    })
    // Later handles are on top and receive the pointer
    return [...Object.values(projected), ...Object.values(set)]
  })

  const projectedHandles = computed(() => {
    frame.value
    return allHandles.value.map(handle => ({ handle, pos: window.alcRenderer.toScreen(handle.vec) }))
  })

  function solve() {
    const activeHandle = props.activeHandle
    const releasedHandle = !activeHandle && props.sketch.solveHandle
    // Hidden sketches may be revealed while the marker is past them. Their
    // projections refer to geometry as it was at the sketch's own position.
    props.sketch.solve(props.document.top(props.sketch.creator), activeHandle, {
      applyResults: !releasedHandle && !props.sketch.restoringHistory,
    })
  }

  watch(() => props.sketch.solveNeeded, () => {
    if(!props.sketch.solveNeeded) return
    solve()
    props.sketch.solveNeeded = false
  })

  const profiles = ref([])
  let fresh = true

  watch([() => props.sketch, () => props.sketch.profileUpdateNeeded], ([sketch, updateNeeded], [oldSketch] = []) => {
    if(!updateNeeded && sketch == oldSketch && !fresh) return
    solve()
    profiles.value = props.sketch.profiles(props.component)
    props.sketch.profileUpdateNeeded = false
    props.sketch.solveHandle = null
    if(active.value && !props.activeHandle) {
      props.sketch.recordHistory(props.activeTool?.historyState?.())
    }
    props.sketch.restoringHistory = false
    fresh = false
  }, { immediate: true })

  watch(() => props.component, () => {
    props.sketch.component = props.component
  }, { immediate: true })

</script>
