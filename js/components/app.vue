<template lang="pug">
  #app(
    v-if="activeDocument"
    :class="{fullscreen: isFullscreen, maximized: isMaximized}"
  )
    TabBar(
      :documents="documents"
      :active-document.sync="activeDocument"
      :is-maximized="isMaximized"
      @delete-document="deleteDocument"
      @create-document="createDocument"
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

  import Document from './../document.js'
  const wasmP = import('../../rust/pkg/wasm-index.js')

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
      window.addEventListener('resize', () => {
        this.$root.$emit('resize')
      }, false)

      this.createDocument()

      if(!window.ipcRenderer) return

      window.ipcRenderer.on('fullscreen-changed', (e, isFullscreen) => {
        this.isFullscreen = isFullscreen
      })

      window.ipcRenderer.on('maximize-changed', (e, isMaximized) => {
        this.isMaximized = isMaximized
      })

      window.ipcRenderer.on('dark-mode', (e, darkMode) => {
        if(darkMode) {
          document.body.setAttribute('data-dark-mode', true)
        } else {
          document.body.removeAttribute('data-dark-mode')
        }
        this.$root.$emit('resize')
      })
    },

    mounted() {
      if(!window.ipcRenderer) return
      window.ipcRenderer.send('vue-ready')
    },

    methods: {
      createDocument: function() {
        return wasmP.then((wasm) => {
          this.activeDocument = new Document(wasm)
          this.documents.push(this.activeDocument)
        })
      },

      deleteDocument: function(doc) {
        const index = this.documents.indexOf(doc)
        this.documents = this.documents.filter(d => d !== doc)
        if(!this.documents.length) this.createDocument()
        if(this.activeDocument === doc) {
          this.activeDocument = this.documents[Math.min(index, this.documents.length - 1)]
        }
        // Free Rust memory when old doc has been removed by viewport
        setTimeout(() => {
          if(doc.tree) doc.tree.free()
        })
      },
    },
  }
</script>
