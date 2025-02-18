<template lang="pug">

  li.treelet-section(
    :class="{ hidden: section.hidden, expanded }"
  )

    .box

      header(@click="expanded = !expanded")

        Icon.eye(
          icon="eye"
          @click.stop="toggleVisibility"
        )

        Icon.icon(icon="object-group" fixed-width)

        h2 Section {{ index + 1 }}

        Icon.expand(icon="angle-right")

        .controls

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="deleteSection"
          )

      .content.form(v-if="expanded")

        fieldset

          label
            input(type="number" v-model="section.orientation.x" min="0.0" step="1.0" max="360.0")
            input(type="number" v-model="section.orientation.y" min="0.0" step="1.0" max="360.0")
            span Orientation

</template>


<style lang="stylus" scoped>

  .icon
    padding-left: 3px
    width: auto

  h2
    margin: 0 !important

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: transform 0.15s

    .expanded &
      transform: rotate(90deg)

  input[type="number"]
    width: 3.75rem

</style>


<script setup>

  import * as THREE from 'three'

  const props = defineProps(['document', 'component', 'section', 'index'])

  const bus = inject('bus')

  const expanded = ref(true)

  watch(() => props.section.orientation, () => {
    props.document.emit('component-changed', props.component)
  }, { deep: true })

  function toggleVisibility() {
    props.section.hidden = !props.section.hidden
    props.document.emit('component-changed', props.component)
  }

  function deleteSection() {
    const creator = props.component.creator
    creator.sectionViews = creator.sectionViews.filter(section => section != props.section )
    props.document.emit('component-changed', props.component)
  }

</script>
