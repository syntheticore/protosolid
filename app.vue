<template lang="pug">

  #app(
    v-if="activeDocument"
    :class="{ fullscreen, maximized, blurry }"
  )

    DocumentView(:document="activeDocument")

    TabBar(
      :documents="documents"
      v-model:active-document="activeDocument"
      :maximized="maximized"
      @create-document="createDocument"
      @open-document="loadDocument"
      @save-document="saveDocument"
      @save-document-as="saveDocumentAs"
      @delete-document="closeDocument"
    )

</template>


<style lang="stylus" scoped>

  #app
    width: 100%
    height: 100%
    user-select: none
    cursor: default
    overflow: hidden
    color: $bright1

  .tab-bar
    position: absolute
    top: 1px
    left: 1px
    right: 1px
    height: 38px

    .fullscreen &
    .maximized[data-platform="win32"] &
    [data-platform="browser"] &
      height: 33px

  .document-view
    grid-area: main
    position: absolute
    left: 1px
    right: 1px
    top: 1px
    bottom: 1px

</style>


<style lang="stylus">

  @import 'styles/main.styl'

</style>


<script setup>

  import { provide } from 'vue'

  import { default as preferences, loadPreferences, emitter as prefEmitter } from './js/preferences.js'
  import Document from './js/core/document.js'
  import Emitter from './js/emitter.js'
  // const wasmP = import('../../rust/pkg/wasm-index.js')

  loadPreferences()
  const blurry = ref(preferences.blurredOverlays)
  prefEmitter.on('updated', () => blurry.value = preferences.blurredOverlays )

  document.body.setAttribute('data-platform', window.platform || 'browser')

  const oc = await useOC()
  window.oc = oc

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

  watch(renderNeeded, () => {
    if(!renderNeeded.value) return
    window.alcRenderer && window.alcRenderer.render()
    renderNeeded.value = false
  })

  // const store = useMainStore()

  const fullscreen = ref(false)
  const maximized = ref(false)
  const activeDocument = ref(null)
  const documents = ref([])

  window.addEventListener('resize', () => {
    bus.emit('resize')
  }, false)

  createDocument()

  if(window.ipc) {
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
    window.ipc.on('closeFile', () => closeDocument(activeDocument.value) )

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
      const isModifierKey = ['Shift', 'Control', 'Meta', 'Alt'].includes(e.key)
      if(!isModifierKey && (e.ctrlKey || e.metaKey || e.altKey)) return

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


  function createDocument() {
    activeDocument.value = new Document(bus)
    documents.value.push(activeDocument.value)
  }

  function loadDocument() {
    const doc = new Document()
    doc.load().then(() => {
      // Close untouched documents on load
      if(activeDocument.value.isFresh) deleteDocument(activeDocument.value)
      activeDocument.value = doc
      documents.value.push(doc)
      nextTick().then(() => {
        doc.treeletsExpandedByDefault = true
        bus.emit('zoom-all')
      })
    })
  }

  async function saveDocument() {
    activeDocument.value.save()
  }

  async function saveDocumentAs() {
    activeDocument.value.save(true)
  }

  function closeDocument(doc) {
    const name = doc.filePath || 'Untitled Document'
    if(doc.hasChanges &&
      !window.confirm(name + ' has unsaved changes. Close anyway?')
    ) return
    const index = documents.value.indexOf(doc)
    deleteDocument(doc)
    if(!documents.value.length) {
      createDocument()
    } else if(activeDocument.value === doc) {
      activeDocument.value = documents.value[Math.min(index, documents.value.length - 1)]
    }
  }

  function deleteDocument(doc) {
    const index = documents.value.indexOf(doc)
    documents.value = documents.value.filter(d => d !== doc)
  }

</script>
