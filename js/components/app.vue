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
  import * as THREE from 'three'

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
      this.createDocument().then(() => this.activeDocument = this.documents[0] )

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
          this.activeDocument = {
            title: 'Untitled Document',
            proxy: proxy,
            views: [
              {
                id: lastId++,
                title: 'Top',
                position: new THREE.Vector3(0.0, 0.0, 9.0),
                target: new THREE.Vector3(0.0, 0.0, 0.0),
              },
              {
                id: lastId++,
                title: 'Front',
                position: new THREE.Vector3(0.0, 9.0, 0.0),
                target: new THREE.Vector3(0.0, 0.0, 0.0),
              },
              {
                id: lastId++,
                title: 'Side',
                position: new THREE.Vector3(9.0, 0.0, 0.0),
                target: new THREE.Vector3(0.0, 0.0, 0.0),
              },
              {
                id: lastId++,
                title: 'Perspective',
                position: new THREE.Vector3(9.0, 9.0, 9.0),
                target: new THREE.Vector3(0.0, 0.0, 0.0),
              },
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
            activeComponent: tree,
            data: {},
            activeView: null,
            activePose: null,
            isPoseDirty: false,
            isSetDirty: true,
          }
          this.documents.push(this.activeDocument)
        })
      },

      deleteDocument: function(doc) {
        this.documents = this.documents.filter(d => d !== doc)
        if(!this.documents.length) this.createDocument()
        this.activeDocument = this.documents[0]
        // Free Rust memory when old doc has been removed by viewport
        setTimeout(() => {
          doc.tree.free()
          doc.proxy.free()
        })
      },
    },
  }
</script>
