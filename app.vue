<template lang="pug">

  #preferences-app(v-if="isPreferencesWindow")
    PreferencesView

  #app(
    v-else-if="activeDocument"
    :class="{ fullscreen, maximized, blurry }"
  )

    TabBar(
      :documents="documents"
      :active-document="activeDocument"
      :inert="documentBusy"
      @update:active-document="activateDocument"
      :maximized="maximized"
      :can-save="activeDocument.canSave"
      @create-document="createDocument"
      @open-document="loadDocument"
      @save-document="saveDocument"
      @save-document-as="saveDocumentAs"
      @import-file="importFile"
      @export-file="exportFile"
      @delete-document="closeDocument"
    )

    DocumentView(:document="activeDocument" :inert="documentBusy")

    DialogView

</template>


<style lang="stylus" scoped>

  #app
  #preferences-app
    width: 100%
    height: 100%
    user-select: none
    cursor: default
    overflow: hidden
    color: $bright1
    display: flex
    flex-direction: column

  .tab-bar
    height: 38px

    .fullscreen &
    [data-platform="win32"] .maximized &
    [data-platform="browser"] &
      height: 33px

  .document-view
    grid-area: main
    height: 100%

</style>


<style lang="stylus">

  @import 'styles/main.styl'

</style>


<script setup>

  import './styles/fonts.css'
  import { provide } from 'vue'
  import { init_planegcs_module } from '@salusoft89/planegcs'
  import sketchSolverWasm from '@salusoft89/planegcs/dist/planegcs_dist/planegcs.wasm?url'
  import { initializeShapex } from './js/core/shapex.js'
  import { initializeSketchSolver } from './js/core/sketch-solver.js'

  import {
    default as preferences,
    loadPreferences,
    applyPreferences,
    emitter as prefEmitter,
  } from './js/preferences.js'
  import Document from './js/core/document.js'
  import { stepFiletype } from './js/core/features.js'
  import { loadFile } from './js/utils.js'
  import Emitter from './js/emitter.js'
  import { ManipulationTool } from './js/tools.js'

  loadPreferences()
  const isPreferencesWindow = new URLSearchParams(window.location.search).get('window') === 'preferences'
  const blurry = ref(preferences.blurredOverlays)
  prefEmitter.on('updated', () => blurry.value = preferences.blurredOverlays )

  document.body.setAttribute('data-platform', window.platform || 'browser')

  if(!isPreferencesWindow) {
    await Promise.all([
      initializeShapex(),
      init_planegcs_module({ locateFile: () => sketchSolverWasm }).then(initializeSketchSolver),
    ])
  }

  const bus = new Emitter()
  bus.isShiftPressed = false
  bus.isCtrlPressed = false
  bus.isAltPressed = false
  provide('bus', bus)
  window.bus = bus

  provide('highlight', ref(null))
  provide('frame', ref(1))

  const renderNeeded = ref(false)
  provide('render-needed', renderNeeded)

  // Observe every flag transition immediately, then render after mesh updates.
  watch(renderNeeded, needed => {
    if(!needed) return
    nextTick(() => {
      renderNeeded.value = false
      window.alcRenderer && window.alcRenderer.render()
    })
  }, { flush: 'sync' })

  // const store = useMainStore()

  const fullscreen = ref(false)
  const maximized = ref(false)
  const activeDocument = ref(null)
  const documents = ref([])
  const switchingDocument = ref(false)
  const documentBusy = computed(() => switchingDocument.value || activeDocument.value?.isRegenerating)
  provide('documentBusy', documentBusy)
  let documentSwitchQueue = Promise.resolve()

  window.addEventListener('resize', () => {
    bus.emit('resize')
  }, false)

  if(!isPreferencesWindow) createDocument()

  if(window.ipc) {
    window.ipc.on('preferences-updated', (e, value) => applyPreferences(value))
    window.ipc.send('set-theme', preferences.theme)

    if(!isPreferencesWindow) {
      window.ipc.on('fullscreen-changed', (e, value) => {
        fullscreen.value = value
      })

      window.ipc.on('maximize-changed', (e, value) => {
        maximized.value = value
      })

      window.ipc.on('createFile', createDocument)
      window.ipc.on('openFile', loadDocument)
      window.ipc.on('saveFile', saveDocument)
      window.ipc.on('saveFileAs', saveDocumentAs)
      window.ipc.on('importFile', importFile)
      window.ipc.on('exportFile', exportFile)
      window.ipc.on('closeFile', () => closeDocument(activeDocument.value) )

      watch(() => activeDocument.value?.canSave ?? false, canSave => {
        window.ipc.send('set-save-enabled', canSave)
      }, { immediate: true })
    }

    window.ipc.on('dark-mode', (e, darkMode) => {
      if(darkMode) {
        document.body.setAttribute('data-dark-mode', true)
      } else {
        document.body.removeAttribute('data-dark-mode')
      }
      bus.emit('resize')
    })
  }

  onMounted(() => {
    if(isPreferencesWindow) return

    const setModifier = (property, event, pressed) => {
      if(bus[property] === pressed) return
      bus[property] = pressed
      bus.emit(event, pressed)
    }

    const syncModifiers = (e) => {
      setModifier('isShiftPressed', 'shift-pressed', e.shiftKey)
      setModifier('isCtrlPressed', 'ctrl-pressed', e.ctrlKey || e.metaKey)
      setModifier('isAltPressed', 'alt-pressed', e.altKey)
    }

    const releaseModifiers = () => {
      setModifier('isShiftPressed', 'shift-pressed', false)
      setModifier('isCtrlPressed', 'ctrl-pressed', false)
      setModifier('isAltPressed', 'alt-pressed', false)
    }

    window.addEventListener('keydown', (e) => {
      // console.log(e.keyCode, e.key)
      syncModifiers(e)
      const key = e.key.toLowerCase()
      const editingText = e.target.closest?.('input, textarea, [contenteditable="true"]')
      if(!editingText && !switchingDocument.value && !documentBusy.value &&
        activeDocument.value?.activeSketch && !e.altKey && (e.ctrlKey || e.metaKey) &&
        (key == 'c' || key == 'v')) {
        e.preventDefault()
        bus.emit('sketch-clipboard', key == 'c' ? 'copy' : 'paste')
        return
      }
      if(!editingText && !switchingDocument.value && !documentBusy.value &&
        activeDocument.value?.activeSketch && !e.altKey && (e.ctrlKey || e.metaKey) &&
        (key == 'z' || key == 'y')) {
        e.preventDefault()
        bus.emit('sketch-history', key == 'y' || e.shiftKey ? 1 : -1)
        return
      }
      const isModifierKey = ['Shift', 'Control', 'Meta', 'Alt'].includes(e.key)
      if(!isModifierKey && (e.ctrlKey || e.metaKey || e.altKey)) return

      if(switchingDocument.value || (documentBusy.value && e.key !== 'Enter')) return

      if(e.key === 'Escape') {
        bus.emit('escape')

      } else if(e.key === 'Enter') {
        bus.emit('enter-pressed')

      } else if(!isModifierKey) {
        bus.emit('keydown', e.key)
      }
    })

    window.addEventListener('keyup', (e) => {
      bus.emit('keyup', e.key)
      syncModifiers(e)
    })

    // Native dialogs and application switches can swallow modifier keyup events.
    window.addEventListener('blur', releaseModifiers)
    window.addEventListener('pointerdown', syncModifiers, true)

    if(!window.ipc) return
    setTimeout(() => window.ipc.send('vue-ready'), 200)
  })


  function activateDocument(doc) {
    switchingDocument.value = true
    return documentSwitchQueue = documentSwitchQueue.catch(() => {}).then(async () => {
      if(activeDocument.value === doc || !documents.value.includes(doc)) return
      if(activeDocument.value) {
        activeDocument.value.activateSimulation(null)
        bus.emit('activate-tool', ManipulationTool)
        await activeDocument.value.cancelFeature()
      }
      activeDocument.value = doc
      await nextTick()
    }).finally(() => switchingDocument.value = false)
  }

  async function createDocument() {
    if(documentBusy.value) return
    const doc = new Document(bus)
    documents.value.push(doc)
    await activateDocument(doc)
  }

  function loadDocument() {
    if(documentBusy.value) return
    const doc = new Document(bus)
    doc.load().then(async () => {
      // Close untouched documents on load
      const previousDocument = activeDocument.value
      documents.value.push(doc)
      await activateDocument(doc)
      if(previousDocument.isFresh) deleteDocument(previousDocument)
      doc.treeletsExpandedByDefault = true
      bus.emit('zoom-all')
    })
  }

  function saveDocument() {
    return activeDocument.value.save()
  }

  function saveDocumentAs() {
    return activeDocument.value.save(true)
  }

  // The menu imports STEP files only, each into a new document. Documents
  // are placed from the toolbar.
  async function importFile() {
    if(documentBusy.value) return
    try {
      const file = await loadFile(stepFiletype, 'text')
      await createDocument()
      await activeDocument.value.placeFile(file)
      bus.emit('zoom-all')
    } catch(error) {
      if(error == 'canceled') return
      console.error(error)
      alert('The file could not be imported.')
    }
  }

  async function exportFile() {
    if(documentBusy.value) return
    try {
      await activeDocument.value.exportComponent()
    } catch(error) {
      if(error == 'canceled') return
      console.error(error)
      alert(`The component could not be exported: ${error.message || error}`)
    }
  }

  async function closeDocument(doc) {
    if(documentBusy.value) return
    const name = doc.filePath || 'Untitled Document'
    if(doc.hasUnsavedChanges &&
      !window.confirm(name + ' has unsaved changes. Close anyway?')
    ) return
    const index = documents.value.indexOf(doc)
    if(activeDocument.value === doc) {
      if(documents.value.length === 1) {
        await createDocument()
      } else {
        await activateDocument(documents.value[index + 1] || documents.value[index - 1])
      }
    }
    deleteDocument(doc)
  }

  function deleteDocument(doc) {
    documents.value = documents.value.filter(d => d !== doc)
  }

</script>
