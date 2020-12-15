<template lang="pug">
  #app(
    v-if="activeDocument"
    :class="{fullscreen: isFullscreen, maximized: isMaximized}"
  )
    TabBar(
      :documents="documents"
      :active-document="activeDocument"
      :is-maximized="isMaximized"
      @create-document="createDocument"
      @change-document="changeDocument"
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
  const wasmP = import('../../rust/pkg/wasm-index.js')

  let lastId = 1;

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
      this.createDocument().then(() => this.changeDocument(this.documents[0]) )

      window.addEventListener('resize', () => {
        this.$root.$emit('resize')
      }, false)

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
      console.log(window.electronPlatform)
      console.log(window.electronPlatformVersion)
    },

    methods: {
      createDocument: function() {
        return wasmP.then((wasm) => {
          const proxy = new wasm.AlchemyProxy()
          const tree = proxy.get_main_assembly()
          this.documents.push({
            title: 'Untitled Document',
            proxy: proxy,
            views: [
              { title: 'Top', id: lastId++ },
              { title: 'Left', id: lastId++ },
              { title: 'Front', id: lastId++ },
              { title: 'Perspective', id: lastId++ },
            ],
            poses: [
              { title: 'Base', id: lastId++ },
              { title: 'Activated', id: lastId++ },
            ],
            sets: [
              { title: 'Filet 14', id: lastId++ },
              { title: 'Extrude 2', id: lastId++ },
            ],
            tree: tree,
            data: {
              [tree.id()]: {},
            },
            activeView: null,
            activePose: null,
            isViewDirty: false,
            isPoseDirty: false,
            isSetDirty: true,
          })
        })
      },

      changeDocument: function(doc) {
        this.activeDocument = doc
        this.activeDocument.activeView = this.activeDocument.views[0]
        this.activeDocument.activePose = this.activeDocument.poses[0]
      }
    },
  }
</script>
