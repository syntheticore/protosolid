<template lang="pug">

  li.treelet-animation(:class="{ expanded }")

    .box

      TreeItemHeader(
        :model-value="animation.title"
        :suffix="` - ${formatDuration(animation.duration)}`"
        :disabled="component.readOnly"
        label-only
        @click="expanded = !expanded"
        @rename="rename"
      )

        template(#icon)
          Icon(icon="film" fixed-width)

        Icon.expand(icon="angle-right")

        .controls

          Icon.delete(icon="trash-alt" fixed-width title="Delete" @click.stop="remove")

      .content.form(v-if="expanded")

        fieldset

          label
            ScalarInput(:value="animation.duration" :min="0.1" stretch @update:value="set(animation, 'duration', $event)")
            span Duration (s)

          label(title="Start over at the end")
            ToggleSwitch(:model-value="animation.loop" @update:model-value="set(animation, 'loop', $event)")
            span Loop

</template>


<style lang="stylus" scoped>

  :deep(h2.tree-item-name)
    margin: 0 !important

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: transform 0.15s

    .expanded &
      transform: rotate(90deg)

</style>


<script setup>

  import AnimationPlayer, { formatDuration } from '../js/animation-player.js'

  const props = defineProps(['document', 'component', 'animation'])
  const bus = inject('bus')

  const player = reactive(new AnimationPlayer(props.document, props.component, props.animation))

  const panelKey = computed(() => `animation:${props.component.id}:${props.animation.id}`)

  const expanded = computed({
    get: () => props.document.activeInteractivePanel === panelKey.value,
    set: value => {
      if(value) props.document.activateComponent(props.component)
      props.document.activateInteractivePanel(value ? panelKey.value : null)
    },
  })

  watch(expanded, value => value ? open() : close())

  // Replay with the edited keyframes
  watch(() => JSON.stringify([props.animation.tracks, props.animation.duration]), () => {
    if(!expanded.value) return
    player.clearTrails()
    player.refresh()
  })

  onMounted(() => {
    props.document.on('regenerated', reset)
    if(expanded.value) open()
  })

  onUnmounted(() => {
    props.document.off('regenerated', reset)
    if(expanded.value) close()
  })

  function open() {
    player.capture()
    props.document.animationPlayer = player
    bus.on('components-moved', moved)
  }

  function close() {
    bus.off('components-moved', moved)
    player.close()
    if(props.document.animationPlayer === player) props.document.animationPlayer = null
  }

  function moved(componentIds) {
    const tree = props.document.top()
    player.moved(componentIds.filter(id => player.animates(tree.findChild(id))))
  }

  // The regenerated assembly starts over from its pose
  function reset() {
    if(!expanded.value) return
    player.restore()
    player.capture()
  }

  function rename(title) {
    props.animation.title = title
    props.document.markChanged()
  }

  function set(target, key, value) {
    target[key] = value
    props.document.markChanged()
  }

  function remove() {
    // Restored before unmounting, which would skip the expansion watcher
    if(expanded.value) {
      close()
      props.document.activateInteractivePanel(null)
    }
    const creator = props.component.creator
    creator.animations = creator.animations.filter(animation => animation != props.animation)
    props.document.markChanged()
  }

</script>
