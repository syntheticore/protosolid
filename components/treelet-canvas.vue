<template lang="pug">

  li.treelet-canvas(
    :class="{ hidden: canvas.hidden, expanded }"
  )

    .box

      header(@click="expanded = !expanded")

        Icon.eye(
          icon="eye"
          @click.stop="toggleVisibility"
        )

        Icon.icon(icon="image" fixed-width)

        h2 {{ canvas.src || 'Canvas' }}

        Icon.expand(icon="angle-right")

        .controls

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="remove"
          )

      .content.form(v-if="expanded")

        fieldset

          label
            button.button(title="Choose file location" @click="chooseFile")
              Icon(icon="folder")
            span(:title="path") {{ path || 'Image Source'}}

          label
            input(type="range" v-model="canvas.scale" min="0.01" step="0.001" max="10.0")
            span Scale

        img(:src="canvas.data")

</template>


<style lang="stylus" scoped>

  .icon
    padding-left: 3px

  h2
    margin: 0 !important
    text-overflow: ellipsis
    white-space: nowrap
    overflow: hidden
    max-width: 110px

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: transform 0.15s

    .expanded &
      transform: rotate(90deg)

  img
    display: block
    width: 198px

</style>


<script setup>

  import * as THREE from 'three'
  import { loadFile } from '../../js/utils.js'

  const props = defineProps(['document', 'component', 'canvas'])

  const bus = inject('bus')

  const expanded = ref(true)
  const path = ref(null)

  function update() {
    props.document.emit('component-changed', props.component)
  }

  watch(() => props.canvas, update, { deep: true })

  function toggleVisibility() {
    props.canvas.hidden = !props.canvas.hidden
    update()
  }

  async function chooseFile() {
    const { path, data, width, height } = await loadFile('image/*', 'dataUrl')

    props.canvas.src = path
    props.canvas.data = data
    props.canvas.width = width
    props.canvas.height = height

    const textureLoader = new THREE.TextureLoader()
    props.canvas.texture = textureLoader.load(data)

    update()

    // Render again, as image takes one frame to load
    setTimeout(() => bus.emit('render-needed') )
  }

  function remove() {
    props.component.creator.canvases = props.component.creator.canvases.filter(conf => conf !== props.canvas )
    update()
  }

</script>
