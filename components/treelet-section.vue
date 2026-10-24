<template lang="pug">

  li.treelet-section(
    :class="{ hidden: section.hidden, expanded }"
  )

    .box

      TreeItemHeader(
        :model-value="component.itemTitle(section.id, `Section ${index + 1}`)"
        :disabled="component.readOnly"
        label-only
        @click="expanded = !expanded"
        @rename="document.renameTreeItem(component, section.id, $event)"
      )

        template(#icon)
          Icon.eye(
            icon="eye"
            @click.stop="toggleVisibility"
            @dblclick.stop
          )

          Icon.icon(icon="object-group" fixed-width)

        Icon.expand(icon="angle-right")

        .controls

          Icon.delete(
            icon="trash-alt" fixed-width
            title="Delete"
            @click.stop="deleteSection"
          )

      .content.form(v-if="expanded" @input="document.markChanged()")

        fieldset

          label

            .flex.gap.section-fields
              input.input(type="number" v-model.number="section.position.x" step="0.1")
              input.input(type="number" v-model.number="section.position.y" step="0.1")
              input.input(type="number" v-model.number="section.position.z" step="0.1")

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

  :deep(h2.tree-item-name)
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

  const props = defineProps(['document', 'component', 'section', 'index'])

  const expanded = computed({
    get: () => props.document.activeInteractivePanel === `section:${props.component.id}:${props.section.id}`,
    set: value => {
      if(value) props.document.activateComponent(props.component)
      props.document.activateInteractivePanel(value ? `section:${props.component.id}:${props.section.id}` : null)
    },
  })

  function toggleVisibility() {
    props.section.hidden = !props.section.hidden
    props.document.markChanged()
  }

  function deleteSection() {
    if(expanded.value) props.document.activateInteractivePanel(null)
    const creator = props.component.creator
    creator.sectionViews = creator.sectionViews.filter(section => section != props.section )
    props.document.markChanged()
  }

</script>
