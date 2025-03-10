<template lang="pug">

  ElementProxy(
    v-for="elem in elements"
    :key="elem.id"
    :document="document"
    :component="component"
    :element="elem"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
  )

  ProfileProxy(
    v-for="profile in profiles"
    :key="profile.id"
    :component="component"
    :profile="profile"
    :parent-highlighted="highlighted"
    :parent-selected="selected"
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

  const props = defineProps(['document', 'component', 'sketch', 'activeHandle', 'parentHighlighted', 'parentSelected'])
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

    const handles = props.sketch.elements.flatMap(elem => {
      if(elem.projection) return []
      return elem.handles().map((p, i) => {
        p = p.clone().applyMatrix4(elem.sketch.workplane)
        return {
          type: 'handle',
          pos: window.alcRenderer.toScreen(p),
          vec: p,
          id: elem.id + i,
          elem,
          index: i,
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
    const handle = props.activeHandle
    if(handle) handle.elem.constraints().forEach(c => c.temporary = true ) //XXX make only handle-relevant constraints temporary
    props.sketch.solve(props.document.top())
    if(handle) handle.elem.constraints().forEach(c => c.temporary = false )
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
    fresh = false
  }, { immediate: true })

  watch(() => props.component, () => {
    props.sketch.component = props.component
  }, { immediate: true })

</script>
