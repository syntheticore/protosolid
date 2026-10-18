<template lang="pug">

  li.tab(:class="{ active: document == activeDocument }")

    span.title(:title="document.filePath") {{ fileName }}

    span.unsaved(v-if="document.hasUnsavedChanges") *

    MenuButton.naked.natural.centered.history-btn(v-if="showHistory" icon="code-branch")

      template(v-slot:header="")
        template(v-if="branchStatus")
          Icon.branch-icon(icon="code-branch")
          | {{ branchStatus }}
        template(v-else) History
        button.format-settings(title="Document format settings" @click="openFormatSettings")
          Icon(icon="sliders-h" fixed-width)

      HistoryView(:document="document")

    button.close-btn(@click.stop="$emit('delete-document', document)")
      Icon(icon="times")

</template>


<style lang="stylus" scoped>

  .tab
    display: flex
    flex: 0 1 auto
    min-width: 0
    align-items: center
    border-left: 1px solid $dark1 * 1.2
    padding-left: 12px
    height: 100%
    font-size: 12px
    font-weight: bold
    transition: all 0.2s
    color: $bright2
    text-shadow: 0 -1px 0px black

    &:hover
      background: $dark2 * 1.2

      .close-btn
        opacity: 1

    &.active
      color: $bright1
      background: $dark1 * 1.2
      flex-shrink: 0
      max-width: 40vw

      .close-btn
      .menu-button
        opacity: 1
        pointer-events: auto

      .menu-button
        width: 56px
        margin-left: 0.75rem

  .title
    min-width: 0
    overflow: hidden
    text-overflow: ellipsis

  .unsaved
    margin-left: 0.25em

  .unsaved
  .menu-button
  .close-btn
    flex-shrink: 0

  .menu-button
    margin-right: 0
    opacity: 0
    pointer-events: none
    width: 0
    transition: all 0.2s

  .branch-icon
    margin-right: 0.5rem

  .format-settings
    position: absolute
    top: 50%
    right: 2px
    transform: translateY(-50%)
    display: flex
    align-items: center
    justify-content: center
    width: 24px
    height: 24px
    padding: 0
    border: none
    border-radius: 3px
    background: none
    color: $bright2
    font-size: 12px

    &:hover
      color: white
      background: rgba(white, 0.12)

  .close-btn
    background: none
    border: none
    padding: 6px 11px 6px 10px
    opacity: 0

    &:hover svg
      color: $bright1

    svg
      color: $bright2
      transition: all 0.2s
      filter: drop-shadow(0 1px 0px rgba(0,0,0, 0.9))

</style>


<script setup>

  import preferences, { emitter as prefEmitter, openPreferences } from './../js/preferences.js'

  const props = defineProps(['document', 'active-document'])
  const emit = defineEmits(['delete-document'])

  const bus = inject('bus')

  function openFormatSettings() {
    bus.emit('close-widgets')
    openPreferences('files')
  }

  const fileName = computed(() => props.document.filePath?.split(/[\\/]/).pop() || 'Untitled Document')

  const versioningEnabled = ref(preferences.documentFormat == 'versioned')
  const unbindPrefs = prefEmitter.on('updated', () => versioningEnabled.value = preferences.documentFormat == 'versioned' )
  onUnmounted(unbindPrefs)

  const showHistory = computed(() => {
    props.document.repositoryVersion
    return versioningEnabled.value || !!props.document.repository
  })

  const branchStatus = computed(() => {
    props.document.repositoryVersion
    const repository = props.document.repository
    if(!repository || repository.isEmpty) return null
    return repository.currentBranch ? `On branch ${repository.currentBranch}` : 'Viewing an earlier version'
  })

</script>
