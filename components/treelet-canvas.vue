<template lang="pug">

  li.treelet-canvas(
    :class="{ hidden: canvas.hidden, expanded }"
  )

    .box

      TreeItemHeader(
        :model-value="component.itemTitle(canvas.id, sourceName)"
        :title="canvas.src"
        :disabled="component.readOnly"
        label-only
        @click="expanded = !expanded"
        @rename="document.renameTreeItem(component, canvas.id, $event)"
      )

        template(#icon)
          Icon.eye(
            icon="eye"
            @click.stop="toggleVisibility"
            @dblclick.stop
          )

          Icon.icon(icon="image" fixed-width)

        Icon.expand(icon="angle-right")

        .controls

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="remove"
          )

      .content.form(v-if="expanded")

        img(v-if="canvas.data" :src="canvas.data")

        fieldset

          label
            button.button.canvas-fields(title="Choose file location" @click="chooseFile")
              Icon(icon="folder")
            span.source-path(:title="canvas.src") {{ canvas.src || 'Image Source' }}

          label
            ComboBox.canvas-fields(v-model="canvas.plane" label="Plane" :options="[{ value: 'top', label: 'Top' }, { value: 'front', label: 'Front' }, { value: 'side', label: 'Side' }]")
            span Plane

          label
            input.canvas-fields(type="range" v-model="canvas.scale" min="0.01" step="0.001" max="10.0")
            span Scale

          label
            input.canvas-fields(type="range" v-model.number="canvas.transparency" min="0" step="0.01" max="1" @input="document.markChanged()")
            span Transparency

          label
            .flex.gap.canvas-fields
              input.input(type="number" v-model.number="canvas.x" step="0.1")
              input.input(type="number" v-model.number="canvas.y" step="0.1")
            span Position

</template>


<style lang="stylus" scoped>

  .expanded .box
    width: 264px

  .icon
    padding-left: 3px

  :deep(h2.tree-item-name)
    margin: 0 !important
    text-overflow: ellipsis
    white-space: nowrap
    overflow: hidden
    max-width: 110px

  .source-path
    max-width: 90px
    overflow: hidden
    text-overflow: ellipsis
    white-space: nowrap

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: transform 0.15s

    .expanded &
      transform: rotate(90deg)

  img
    display: block
    width: 100%

  .flex > *
    min-width: 0

  .form label > :first-child
    width: 146px

</style>


<script setup>

  import * as THREE from 'three'
  import { loadFile } from '../js/utils.js'

  const props = defineProps(['document', 'component', 'canvas'])

  const bus = inject('bus')

  const expanded = computed({
    get: () => props.document.activeInteractivePanel === `canvas:${props.component.id}:${props.canvas.id}`,
    set: value => {
      if(value) props.document.activateComponent(props.component)
      props.document.activateInteractivePanel(value ? `canvas:${props.component.id}:${props.canvas.id}` : null)
    },
  })
  const sourceName = computed(() =>
    props.canvas.src?.split(/[\\/]/).pop() || 'Canvas'
  )

  function toggleVisibility() {
    props.canvas.hidden = !props.canvas.hidden
  }

  async function chooseFile() {
    const { path: filePath, data, width, height } = await loadFile('image/*', 'dataUrl')

    props.canvas.src = filePath
    props.canvas.data = data
    props.canvas.width = width
    props.canvas.height = height

    props.canvas.texture = new THREE.TextureLoader().load(data, () => bus.emit('render-needed'))
    props.canvas.texture.colorSpace = THREE.SRGBColorSpace
    props.canvas.texture.anisotropy = window.alcRenderer.renderer.capabilities.getMaxAnisotropy()
  }

  function remove() {
    if(expanded.value) props.document.activateInteractivePanel(null)
    props.component.creator.canvases = props.component.creator.canvases.filter(conf => conf !== props.canvas )
  }

</script>
