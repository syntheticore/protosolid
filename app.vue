<template lang="pug">

  #app(
    v-if="activeDocument"
    :class="{ fullscreen: isFullscreen, maximized: isMaximized }"
  )

    DocumentView(
      :document="activeDocument"
    )

    TabBar(
      :documents="documents"
      v-model:active-document="activeDocument"
      :is-maximized="isMaximized"
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
    .maximized &
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

  import { loadPreferences } from './../js/preferences.js'
  import Document from './../js/core/document.js'
  import Emitter from './../js/emitter.js'
  // const wasmP = import('../../rust/pkg/wasm-index.js')

  document.body.setAttribute('data-platform', window.platform || 'browser')

  const oc = await useOC()
  window.oc = oc

  const bus = new Emitter()
  provide('bus', bus)
  window.bus = bus

  const store = useMainStore()

  const isFullscreen = ref(false)
  const isMaximized = ref(false)
  const activeDocument = ref(null)
  const documents = ref([])

  loadPreferences()

  window.addEventListener('resize', () => {
    bus.emit('resize')
  }, false)

  createDocument()

  if(window.ipc) {
    window.ipc.on('fullscreen-changed', (e, isFullscreen) => {
      isFullscreen.value = isFullscreen
    })

    window.ipc.on('maximize-changed', (e, isMaximized) => {
      isMaximized.value = isMaximized
    })

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
    window.addEventListener('keydown', (e) => {
      // console.log(e.keyCode)
      if(e.key === 'Escape') {
        bus.emit('escape')
      } else if(e.key === 'Enter') {
        bus.emit('enter-pressed')
      } else if(e.key === 'Shift') {
        bus.emit('shift-pressed')
        bus.isShiftPressed = true
      } else if(e.key === 'Control') {
        bus.emit('ctrl-pressed')
        bus.isCtrlPressed = true
      } else {
        bus.emit('keydown', e.key)
      }
    })

    window.addEventListener('keyup', (e) => {
      bus.emit('keyup', e.key)
      if(e.key === 'Shift') {
        bus.isShiftPressed = false
      } else if(e.key === 'Control') {
        bus.isCtrlPressed = false
      }
    })

    // bus.on('component-changed', () => {
    //   activeDocument.value.hasChanges = true
    //   activeDocument.value.isFresh = false
    // })

    if(!window.ipc) return
    setTimeout(() => window.ipc.send('vue-ready'), 0)
  })


  function createDocument() {
    // return wasmP.then((wasm) => {
      // window.alcWasm = wasm
      activeDocument.value = new Document()
      documents.value.push(activeDocument.value)
    // })
  }

  function loadDocument(path) {
    const doc = new Document()
    doc.load(path).then(() => {
      // Close untouched documents on load
      if(activeDocument.value.isFresh) deleteDocument(activeDocument.value)
      activeDocument.value = doc
      documents.value.push(doc)
      setTimeout(() => {
        doc.real.marker = doc.features.length
        bus.emit('regenerate')
        doc.hasChanges = false
      }, 0)
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
    // Free Rust memory when old doc has been removed by viewport
    setTimeout(() => doc.dispose() )
  }

</script>
