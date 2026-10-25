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

      .content.form(v-if="expanded")

        fieldset

          label
            VectorInput(
              :value="section.position"
              :step="0.1"
              @update="(axis, value) => set(section.position, axis, value)"
            )
            span Position

          label
            VectorInput(
              :value="section.orientation"
              :axes="['x', 'y']"
              :min="0"
              :max="360"
              @update="(axis, value) => set(section.orientation, axis, value)"
            )
            span Orientation

          label
            ToggleSwitch(v-model="section.side" @update:model-value="document.markChanged()")
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

  .form label > :first-child
    width: 204px

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

  function set(vector, axis, value) {
    vector[axis] = value
    props.document.markChanged()
  }

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
