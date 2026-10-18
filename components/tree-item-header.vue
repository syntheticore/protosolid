<template lang="pug">

  header(@click="click" @dblclick="dblclick")

    slot(name="icon")

    input.input.tree-item-name(
      v-if="renaming"
      ref="input"
      v-model="draft"
      type="text"
      aria-label="Item name"
      :size="Math.max(8, draft.length)"
      @click.stop
      @dblclick.stop
      @keydown.stop
      @keydown.enter.prevent="commit"
      @keydown.esc.prevent="cancel"
      @blur="commit"
    )
    component.tree-item-name(v-else :is="labelTag") {{ modelValue }}{{ suffix }}

    slot

</template>


<style lang="stylus" scoped>

  .tree-item-name
    white-space: nowrap
    margin-right: 8px

  span.tree-item-name
    margin-right: 6px

  input.tree-item-name
    width: auto
    min-width: 50px
    max-width: 240px
    margin-right: 0
    padding: 1px 3px
    font: inherit
    line-height: normal

</style>


<script setup>

  const props = defineProps({
    modelValue: String,
    selected: Boolean,
    disabled: Boolean,
    labelOnly: Boolean,
    labelTag: { type: String, default: 'h2' },
    suffix: { type: String, default: '' },
  })
  const emit = defineEmits(['click', 'dblclick', 'rename'])
  const bus = inject('bus')
  const renaming = ref(false)
  const draft = ref('')
  const input = ref(null)

  function canRename(event) {
    return !props.disabled && !bus.componentPickerActive &&
      !event.ctrlKey && !event.metaKey && !event.shiftKey && !event.altKey
  }

  function click(event) {
    if(event.detail > 1) return
    if(props.labelOnly || !props.selected || !canRename(event)) {
      emit('click', event)
      return
    }
    startRename()
  }

  function dblclick(event) {
    if(props.labelOnly && event.target.closest('.tree-item-name') && canRename(event)) {
      startRename()
      return
    }
    emit('dblclick', event)
  }

  async function startRename() {
    draft.value = props.modelValue
    renaming.value = true
    await nextTick()
    input.value.focus()
    input.value.select()
  }

  function commit() {
    if(!renaming.value) return
    renaming.value = false
    const title = draft.value.trim()
    if(title && title !== props.modelValue) emit('rename', title)
  }

  function cancel() {
    renaming.value = false
  }

</script>
