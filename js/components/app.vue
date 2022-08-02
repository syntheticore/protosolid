<template lang="pug">
  #app(
    v-if="activeDocument"
    :class="{fullscreen: isFullscreen, maximized: isMaximized}"
  )
    TabBar(
      :documents="documents"
      :active-document.sync="activeDocument"
      :is-maximized="isMaximized"
      @create-document="createDocument"
      @open-document="loadDocument"
      @save-document="saveDocument"
      @save-document-as="saveDocumentAs"
      @delete-document="closeDocument"
    )
    DocumentView(
      :document="activeDocument"
    )
</template>


<style lang="stylus" scoped>
  #app
    width: 100%
    height: 100%
    display: grid
    grid-template-rows: 38px 1fr
    // grid-gap: 1px
    // grid-auto-rows: minmax(100px, auto)
    grid-template-areas:
      "header"\
      "main"
    user-select: none
    cursor: default
    overflow: hidden
    color: $bright1
    &.fullscreen
    &.maximized
    [data-platform="browser"] &
      grid-template-rows: 33px 1fr

  .tool-bar
    grid-area: header

  .document-view
    grid-area: main
    position: relative
</style>


<script>
  import TabBar from './tab-bar.vue'
  import DocumentView from './document-view.vue'

  import { loadPreferences } from './../preferences.js'
  import Document from './../document.js'
  const wasmP = import('../../rust/pkg/wasm-index.js')

  const ipc = window.electron && window.electron.ipc

  export default {
    name: 'app',

    components: {
      TabBar,
      DocumentView,
    },

    data() {
      return {
        isFullscreen: false,
        isMaximized: false,
        activeDocument: null,
        documents: [],
      }
    },

    created() {
      loadPreferences()

      window.addEventListener('resize', () => {
        this.$root.$emit('resize')
      }, false)

      this.createDocument()

      if(!window.electron) return

      ipc.on('fullscreen-changed', (e, isFullscreen) => {
        this.isFullscreen = isFullscreen
      })

      ipc.on('maximize-changed', (e, isMaximized) => {
        this.isMaximized = isMaximized
      })

      ipc.on('dark-mode', (e, darkMode) => {
        if(darkMode) {
          document.body.setAttribute('data-dark-mode', true)
        } else {
          document.body.removeAttribute('data-dark-mode')
        }
        this.$root.$emit('resize')
      })
    },

    mounted() {
      window.addEventListener('keydown', (e) => {
        // console.log(e.keyCode)
        if(e.keyCode === 27) {
          this.$root.$emit('escape')
        } else if(e.keyCode === 13) {
          this.$root.$emit('enter-pressed')
        } else if(e.keyCode === 16) {
          this.$root.$emit('shift-pressed')
          this.$root.isShiftPressed = true
        } else if(e.keyCode === 17) {
          this.$root.$emit('ctrl-pressed')
          this.$root.isCtrlPressed = true
        } else {
          this.$root.$emit('keydown', e.keyCode)
        }
      });

      window.addEventListener('keyup', (e) => {
        this.$root.$emit('keyup', e.keyCode)
        if(e.keyCode === 16) {
          this.$root.isShiftPressed = false
        } else if(e.keyCode === 17) {
          this.$root.isCtrlPressed = false
        }
      });

      this.$root.$on('component-changed', () => {
        this.activeDocument.hasChanges = true
        this.activeDocument.isFresh = false
      })

      if(!window.electron) return
      setTimeout(() => ipc.send('vue-ready'), 0)
    },

    methods: {
      createDocument: function() {
        return wasmP.then((wasm) => {
          window.alcWasm = wasm
          this.activeDocument = new Document()
          this.documents.push(this.activeDocument)
        })
      },

      loadDocument: function(path) {
        const doc = new Document()
        doc.load(path).then(() => {
          // Close untouched documents on load
          if(this.activeDocument.isFresh) this.deleteDocument(this.activeDocument)
          this.activeDocument = doc
          this.documents.push(doc)
          setTimeout(() => {
            doc.real.marker = doc.features.length
            this.$root.$emit('regenerate')
            doc.hasChanges = false
          }, 0)
        })
      },

      saveDocument: async function() {
        this.activeDocument.save()
      },

      saveDocumentAs: async function() {
        this.activeDocument.save(true)
      },

      closeDocument: function(doc) {
        const name = doc.filePath || 'Untitled Document'
        if(doc.hasChanges &&
          !window.confirm(name + ' has unsaved changes. Close anyway?')
        ) return
        const index = this.documents.indexOf(doc)
        this.deleteDocument(doc)
        if(!this.documents.length) {
          this.createDocument()
        } else if(this.activeDocument === doc) {
          this.activeDocument = this.documents[Math.min(index, this.documents.length - 1)]
        }
      },

      deleteDocument: function(doc) {
        const index = this.documents.indexOf(doc)
        this.documents = this.documents.filter(d => d !== doc)
        // Free Rust memory when old doc has been removed by viewport
        setTimeout(() => doc.dispose() )
      },
    },
  }
</script>
