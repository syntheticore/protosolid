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

            .flex.gap.section-fields
              input.input(type="number" v-model.number="section.position.x" step="1")
              input.input(type="number" v-model.number="section.position.y" step="1")
              input.input(type="number" v-model.number="section.position.z" step="1")

            span Position

          label

            .flex.gap.section-fields

              input.input(type="number" v-model="section.orientation.x" min="0.0" step="1.0" max="360.0")

              input.input(type="number" v-model="section.orientation.y" min="0.0" step="1.0" max="360.0")

            span Orientation

          label

            .section-fields.side-field
              input(type="checkbox" v-model="section.side")

            span Side

</template>


<style lang="stylus" scoped>

  .icon
    padding-left: 3px

  h2
    margin: 0 !important

  .expand
    padding: 3px !important
    margin: 0 2px
    transition: transform 0.15s

    .expanded &
      transform: rotate(90deg)

  .flex > *
    width: 0 !important

  .side-field
    display: flex
    justify-content: flex-end

  .form label > :first-child
    width: 184px

</style>


<script setup>

  import * as THREE from 'three'

  const props = defineProps(['document', 'component', 'section', 'index'])

  const expanded = ref(props.document.treeletsExpandedByDefault)

  function toggleVisibility() {
    props.section.hidden = !props.section.hidden
  }

  function deleteSection() {
    const creator = props.component.creator
    creator.sectionViews = creator.sectionViews.filter(section => section != props.section )
  }

</script>
