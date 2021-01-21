<template lang="pug">
  .view-port

    canvas(
      ref="canvas"
      @click="click"
      @dblclick="doubleClick"
      @mouseup="mouseUp"
      @mousedown="mouseDown"
      @mousemove="mouseMove"
      @keydown="keyDown"
      @keyup="keyUp"
    )

    svg.drawpad(ref="drawpad" viewBox="0 0 100 100" fill="transparent")
      path(v-for="path in allPaths" :d="path.data" :stroke="path.color")
      transition-group(name="hide" tag="g")
        line(
          v-for="guide in guides"
          :key="guide.id"
          :x1="guide.start.x"
          :y1="guide.start.y"
          :x2="guide.end.x"
          :y2="guide.end.y"
        )

    .anchors
      div(
        v-if="snapAnchor"
        :style="{top: snapAnchor.pos.y + 'px', left: snapAnchor.pos.x + 'px'}"
      )

    ul.handles
      li(
        v-for="handle in allHandles"
        :key="handle.id"
        :style="{top: handle.pos.y + 'px', left: handle.pos.x + 'px'}"
        @mouseup="mouseUp"
        @mousedown="handleMouseDown($event, handle)"
        @mousemove="handleMouseMove($event, handle)"
      )
</template>


<style lang="stylus" scoped>
  .view-port
    position: relative
    overflow: hidden

  canvas
    display: block
    background: radial-gradient(50% 150%, farthest-corner, #333333, #1c2127)

  .drawpad
    position: absolute
    left: 0
    top: 0
    pointer-events: none
    width: 100%
    height: 100%
    line, path
      fill-opacity: 0
      stroke-width: 2
      stroke-linecap: round
    line
      stroke: white
      stroke-dasharray: 3, 7
    path
      opacity: 0.7
      stroke-dasharray: 4, 7

  .handles, .anchors
    position: absolute
    left: 0
    right: 0
    top: 0
    bottom: 0
    pointer-events: none
    > *
      size = 21px
      // padding: 7px
      position: absolute
      display: block
      width: size
      height: size
      margin-left: -(size / 2)
      margin-top: -(size / 2)
      display: flex
      align-items: center
      justify-content: center
      // pointer-events: auto
      &::before
        position: absolute
        display: block
        content: ''
        margin: 0
        padding: 0
        width:  7px
        height: 7px
        border-radius: 99px
        background: white
      &::after
        position: absolute
        display: block
        content: ''
        margin: 0
        padding: 0
        width:  calc(100% - 10px)
        height: calc(100% - 10px)
        border-radius: 99px
        border: 2px solid $highlight * 1.6
        pointer-events: none

  .anchors > *
    &::after
      transform: scale(1)
      opacity: 1
      width:  calc(100% - 2px)
      height: calc(100% - 2px)
      border: 2px solid $highlight * 1.6
      animation-duration: 0.15s
      animation-name: focus
    @keyframes focus {
      from {
        opacity: 0
        transform: scale(1.7)
      }

      to {
        transform: scale(1)
        opacity: 1
      }
    }

  .handles > *
    pointer-events: auto
    &:hover
      &::before
        width: 5px
        height: 5px
    &:active
      &::before
        width:  5px
        height: 5px

  .hide-enter-active
  .hide-leave-active
    transition: all 0.2s
  .hide-enter
  .hide-leave-to
    opacity: 0
</style>


<script>
  import * as THREE from 'three'

  import {
    ManipulationTool,
    ObjectSelectionTool,
    ProfileSelectionTool,
    LineTool,
    SplineTool,
    CircleTool,
    ArcTool,
    PlaneTool
  } from './../tools.js'
  import { Snapper } from './../snapping.js'
  import { Renderer } from './../renderer.js'
  import { Transloader } from './../transloader.js'

  export default {
    name: 'ViewPort',

    props: {
      document: Object,
      activeComponent: Object,
      highlightedComponent: Object,
      activeTool: Object,
      selection: Object,
      activeView: Object,
      displayMode: String,
    },

    watch: {
      document: function(document, oldDocument) {
        this.transloader.unloadTree(oldDocument.tree, true)
        this.transloader.setDocument(document)
        this.componentChanged(document.tree, true)
      },

      activeComponent: function() {
        this.componentChanged(this.document.tree, true)
        this.$root.$emit('activate-toolname', 'Manipulate')
      },

      highlightedComponent: function(comp, oldComp) {
        if(oldComp) {
          this.transloader.unhighlightComponent(oldComp.comp)
        }
        if(comp) {
          this.transloader.highlightComponent(comp.comp, comp.solidId)
        }
        this.renderer.render()
      },

      selection: function(selection) {
        this.renderer.render()
      },

      activeView: function(view) {
        if(!view) return
        this.renderer.setView(view.position, view.target)
      },

      displayMode: function(mode) {
        this.renderer.setDisplayMode(mode)
        this.componentChanged(this.document.tree, true)
      },
    },

    data() {
      return {
        snapAnchor: null,
        handles: {},
        paths: [],
        pickingPath: null,
        guides: [],
      }
    },

    computed: {
      allHandles: function() {
        const handles = Object.values(this.handles).map(e => Object.values(e)).flat().flat()
        const set = {}
        handles.forEach(handle => set[JSON.stringify(handle.pos)] = handle)
        return Object.values(set)
      },

      allPaths: function() {
        const paths = [...this.paths]
        if(this.pickingPath && this.pickingPath.target) paths.push(this.pickingPath)
        return paths
      },
    },

    mounted: function() {
      // Renderer
      this.renderer = new Renderer(this.$el.querySelector('canvas'))
      this.renderer.setDisplayMode(this.displayMode)
      this.renderer.on('render', () => this.updateWidgets() )
      this.renderer.on('change-view',
        (position, target) => this.$emit('change-view', position, target)
      )
      this.renderer.on('change-pose', () => this.$emit('change-pose') )

      // Snapping
      this.snapper = new Snapper(this, (guides, anchor) => {
        this.guides = guides
        this.snapAnchor = anchor
      })

      // Init tree
      this.transloader = new Transloader(
        this.renderer,
        this.onLoadElement.bind(this),
        this.onUnloadElement.bind(this),
      )
      this.transloader.setDocument(this.document)
      this.transloader.loadTree(this.document.tree, true)

      // Events
      this.$root.$on('pick', (type, pickerCoords, color) => {
        this.handlePick(pickerCoords, color, {
          profile: ProfileSelectionTool,
          curve: ObjectSelectionTool,
          axis: ObjectSelectionTool,
        }[type])
      })

      this.$root.$on('activate-toolname', this.activateTool)
      this.$root.$on('component-changed', this.componentChanged)
      this.$root.$on('render-needed', () => this.renderer.render())
      this.$root.$on('preview-feature', this.transloader.previewFeature.bind(this.transloader))
      this.$root.$on('unpreview-feature', this.unpreviewFeature)
      this.$root.$on('resize', this.onWindowResize)

      // Window Resize
      setTimeout(() => this.onWindowResize(), 1000)
      this.onWindowResize()
    },

    beforeDestroy: function() {
      this.$root.$off('resize', this.onWindowResize)
      this.renderer.dispose()
    },

    methods: {
      buildPath: function(origin, vec) {
        const pos = this.renderer.toScreen(vec)
        const dx = Math.min(25 + Math.abs(origin.x - pos.x) / 2.0, 200)
        const dy = Math.abs(origin.y - pos.y) / 2.0
        return `M ${origin.x} ${origin.y} C ${origin.x} ${origin.y + dx} ${pos.x} ${pos.y - dy} ${pos.x} ${pos.y}`
      },

      getMouseCoords: function(e) {
        var rect = this.$refs.canvas.getBoundingClientRect()
        return new THREE.Vector2(e.clientX, e.clientY - rect.top)
      },

      click: function(e) {
        // this.viewControls.enabled = true
        const [vec, coords] = this.snap(e)
        if(coords.x != this.lastCoords.x ||
           coords.y != this.lastCoords.y) return this.renderer.render()
        if(e.altKey) return
        this.activeTool.click(vec, coords)
      },

      doubleClick: function(e) {
        this.renderer.setPivot(this.getMouseCoords(e))
      },

      mouseUp: function(e) {
        this.activeHandle = null
        const [vec, coords] = this.snap(e)
        if(vec) this.activeTool.mouseUp(vec, coords)
        this.snapper.reset()
        this.updateRegions()
      },

      updateRegions: function() {
        if(!this.regionsDirty) return
        this.transloader.updateRegions(this.activeComponent)
        this.renderer.render()
        this.regionsDirty = false
      },

      mouseDown: function(e) {
        if(e.button != 0) return
        if(e.altKey) return
        const [vec, coords] = this.snap(e)
        if(vec) this.activeTool.mouseDown(vec, coords)
        this.lastCoords = coords
      },

      handleMouseDown: function(e, handle) {
        this.activeHandle = handle
        this.mouseDown(e)
        this.snapper.reset()
      },

      handleMouseMove: function(e, handle) {
        this.hoveredHandle = handle
        this.mouseMove(e)
        this.hoveredHandle = null
      },

      mouseMove: function(e) {
        if(e.button != 0) return
        if(this.renderer.isOrbiting) return
        if(e.altKey) return
        const [vec, coords] = this.snap(e)
        if(this.pickingPath && vec) this.pickingPath.target = vec
        if(vec) this.activeTool.mouseMove(vec, coords)
      },

      keyDown: function(e) {
        if(e.keyCode == 46 || e.keyCode == 8) { // Del / Backspace
          // Delete Selection
          if(this.selection) {
            console.log(this.selection)
            const type = this.selection.typename()
            if(type != 'Solid' && type != 'Component') {
              this.deleteElement(this.selection)
            }
          }
        } else if(e.keyCode == 18) { // alt
          // this.guides = []
        }
      },

      keyUp: function(e) {
        if(e.keyCode == 18) { // alt
        } else if(e.keyCode == 79) { // o
          this.renderer.switchCamera()
        }
      },

      snap: function(e) {
        const coords = this.getMouseCoords(e)
        let vec = this.renderer.fromScreen(coords)
        return this.snapper.snap(vec, coords)
      },

      handlePick: function(pickerCoords, color, Tool) {
        if(this.activeTool) this.activeTool.dispose()
        const mouseTarget = this.renderer
        this.pickingPath = { target: null, color, origin: pickerCoords }
        const tool = new Tool(this.activeComponent, this, (item, mesh) => {
          this.$root.$emit('picked', item)
          this.$root.$emit('activate-toolname', 'Manipulate')
          mesh.geometry.computeBoundingBox();
          const center = new THREE.Vector3()
          mesh.geometry.boundingBox.getCenter(center);
          this.paths.push({
            target: center,
            origin: pickerCoords,
            data: this.buildPath(pickerCoords, center),
            color,
          })
        })
        this.$emit('update:active-tool', tool)
      },

      updateWidgets: function() {
        // Update Snap Anchor
        if(this.snapAnchor) this.snapAnchor.pos = this.renderer.toScreen(this.snapAnchor.vec)
        // Update Handles
        for(let compId in this.handles) {
          const compHandles = this.handles[compId]
          for(let elemId in compHandles) {
            const elemHandles = compHandles[elemId]
            elemHandles.forEach(handle => {
              handle.pos = this.renderer.toScreen(handle.vec)
            })
          }
          this.handles = Object.assign({}, this.handles)
        }
        // Update Paths
        this.paths.forEach((path, i) => {
          path.data = this.buildPath(path.origin, path.target)
          this.$set(this.paths, i, path)
        })
        if(!this.pickingPath || !this.pickingPath.target) return
        this.pickingPath.data = this.buildPath(this.pickingPath.origin, this.pickingPath.target)
      },

      activateTool: function(toolName) {
        if(this.activeTool) this.activeTool.dispose()
        this.pickingPath = null
        this.snapper.reset()
        const tools = {
          Plane: PlaneTool,
          Manipulate: ManipulationTool,
          Line: LineTool,
          Spline: SplineTool,
          Circle: CircleTool,
          Arc: ArcTool,
        }
        const tool = new tools[toolName](this.activeComponent, this)
        this.$emit('update:active-tool', tool)
        this.renderer.render()
      },

      deleteElement: function(elem) {
        this.renderer.removeGizmo()
        this.activeComponent.real.get_sketch().remove_element(elem.id())
        this.componentChanged(this.activeComponent)
        this.$emit('update:selection', null)
      },

      componentChanged: function(comp, recursive) {
        this.transloader.unloadTree(comp, recursive)
        this.transloader.loadTree(comp, recursive)
        this.renderer.shadowCatcher.update()
        this.renderer.render()
      },

      elementChanged: function(elem, comp) {
        this.regionsDirty = true
        this.transloader.loadElement(elem, comp)
        this.renderer.render()
      },

      unpreviewFeature: function() {
        this.paths = []
        this.transloader.unpreviewFeature()
      },

      onLoadElement: function(elem, comp) {
        const compId = comp.real.id()
        const elemId = elem.id()
        this.handles[compId] = this.handles[compId] || {}
        this.handles[compId][elemId] = this.handles[compId][elemId] || []
        elem.get_handles().forEach((handle, i) => {
          handle = new THREE.Vector3().fromArray(handle)
          this.handles[compId][elemId].push({
            type: 'handle',
            pos: this.renderer.toScreen(handle),
            vec: handle,
            id: Math.random(),
            elem: elem,
            index: i,
          })
        })
      },

      onUnloadElement: function(elem, comp) {
        const compId = comp.real.id()
        if(this.handles[compId]) delete this.handles[compId][elem.id()]
        this.handles = Object.assign({}, this.handles)
      },

      onWindowResize: function() {
        const parent = this.$refs.canvas.parentElement
        this.$refs.drawpad.setAttribute(
          'viewBox',
          '0 0 ' + parent.offsetWidth + ' ' + parent.offsetHeight
        )
        this.renderer.onWindowResize()
      },
    }
  }
</script>
