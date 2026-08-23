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

        h2(:title="canvas.src") {{ sourceName }}

        Icon.expand(icon="angle-right")

        .controls

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="remove"
          )

      .content.form(v-if="expanded")

        fieldset

          img(v-if="canvas.data" :src="canvas.data")

          label
            button.button.canvas-fields(title="Choose file location" @click="chooseFile")
              Icon(icon="folder")
            span(:title="path") {{ path || 'Image Source'}}

          label
            select.input.canvas-fields(v-model="canvas.plane")
              option(value="top") Top
              option(value="front") Front
              option(value="side") Side
            span Plane

          label
            input.canvas-fields(type="range" v-model="canvas.scale" min="0.01" step="0.001" max="10.0")
            span Scale

          label
            .flex.gap.canvas-fields
              input.input(type="number" v-model.number="canvas.x" step="1")
              input.input(type="number" v-model.number="canvas.y" step="1")
            span Position

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
    max-width: 240px
    margin-inline: -12px

  .flex > *
    min-width: 0

  .form label > :first-child
    width: 122px

</style>


<script setup>

  import * as THREE from 'three'
  import { loadFile } from '../../js/utils.js'

  const props = defineProps(['document', 'component', 'canvas'])

  const bus = inject('bus')

  const expanded = ref(props.document.treeletsExpandedByDefault)
  const path = ref(null)
  const sourceName = computed(() =>
    props.canvas.src?.split(/[\\/]/).pop() || 'Canvas'
  )

  function toggleVisibility() {
    props.canvas.hidden = !props.canvas.hidden
  }

  async function chooseFile() {
    const { path: filePath, data, width, height } = await loadFile('image/*', 'dataUrl')

    path.value = filePath
    props.canvas.src = filePath
    props.canvas.data = data
    props.canvas.width = width
    props.canvas.height = height

    const textureLoader = new THREE.TextureLoader()
    props.canvas.texture = textureLoader.load(data, texture => {
      texture.colorSpace = THREE.SRGBColorSpace
      texture.needsUpdate = true
      bus.emit('render-needed')
    })
    props.canvas.texture.colorSpace = THREE.SRGBColorSpace

    // Render again, as image takes one frame to load
    setTimeout(() => bus.emit('render-needed') )
  }

  function remove() {
    props.component.creator.canvases = props.component.creator.canvases.filter(conf => conf !== props.canvas )
  }

</script>
