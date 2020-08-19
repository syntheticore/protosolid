<template lang="pug">
  #app(
    v-if="activeDocument"
    :class="{fullscreen: isFullscreen, maximized: isMaximized}"
    :data-platform="platform"
  )
    ToolBar(
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
      grid-template-rows: 24px 1fr

  .tool-bar
    grid-area: header

  .document-view
    grid-area: main
    position: relative
</style>


<script>
  import ToolBar from './tool-bar.vue'
  import DocumentView from './document-view.vue'
  const wasmP = import('../../rust/pkg/wasm-index.js')

  let lastId = 1;

  export default {
    name: 'app',

    components: {
      ToolBar,
      DocumentView,
    },

    data() {
      return {
        isFullscreen: false,
        isMaximized: false,
        platform: window.electronPlatform || 'browser',
        activeDocument: null,
        documents: [],
      }
    },

    created() {
      this.createDocument().then(() => this.changeDocument(this.documents[0]) )

      if(!window.ipcRenderer) {
        // this.isFullscreen = true
        return
      }

      window.ipcRenderer.on('fullscreen-changed', (e, isFullscreen) => {
        this.isFullscreen = isFullscreen
      })

      window.ipcRenderer.on('maximize-changed', (e, isMaximized) => {
        this.isMaximized = isMaximized
      })

      window.ipcRenderer.on('pong', (e, arg) => {
        console.log('pong', arg)
      })
      window.ipcRenderer.send('ping')
    },

    mounted() {
      window.addEventListener('keydown', (e) => {
        console.log(e.keyCode)
        if(e.keyCode === 83) { // S
          return
        } else if(e.keyCode === 27) {
          this.$root.$emit('escape')
        }
      });
      if(!window.ipcRenderer) return
      window.ipcRenderer.send('vue-ready')
      console.log(window.electronPlatform)
    },

    methods: {
      createDocument: function() {
        return wasmP.then((wasm) => {
          const proxy = new wasm.AlchemyProxy()
          const tree = proxy.get_main_assembly()
            const part1 = tree.create_component('Part 1')
            const assm1 = tree.create_component('Sub Assembly 1')
              const part2 = assm1.create_component('Part 2')
              const part3 = assm1.create_component('Part 3')
              const assm2 = assm1.create_component('Sub Assembly 2')
                const part4 = assm2.create_component('Part 4')
                const part5 = assm2.create_component('Part 5')
              const assm3 = assm1.create_component('Sub Assembly 3')
                const part6 = assm3.create_component('Part 6')
                const part7 = assm3.create_component('Part 7')
                const part8 = assm3.create_component('Part 8')
          // part3.create_sketch().add_segment()
          part3.add_segment()
          // part3.get_sketches()[0].add_segment()
          // console.log(part3.get_sketches()[0].get_segments())
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
