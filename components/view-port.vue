<template lang="pug">
  .view-port(:style="{ cursor: (activeTool && activeTool.cursor) || 'auto' }")

    //- GL Viewport
    canvas(
      ref="canvas"
      @dblclick="doubleClick"
      @pointerup="mouseUp"
      @pointerdown="mouseDown"
      @mousemove="mouseMove"
    )

    svg.drawpad(ref="drawpad" viewBox="0 0 100 100" fill="transparent")

      //- Pick indicators
      path(v-for="path in allPaths", :d="path.data", :stroke="path.color")
      circle(v-for="path in allPaths", :cx="path.targetPos.x", :cy="path.targetPos.y", r="5", :fill="path.color")

      //- Snap guides
      transition-group(name="hide" tag="g")
        line(
          v-for="guide in guides"
          :key="guide.id"
          :x1="guide.start.x"
          :y1="guide.start.y"
          :x2="guide.end.x"
          :y2="guide.end.y"
        )

    Icon.constraint(
      v-for="constraint in constraints"
      :icon="constraint.constraint.constructor.icon"
      :class="{ selected: document.selection.has(constraint.constraint) }"
      :style="{ top: constraint.coords.y + 'px', left: constraint.coords.x + 'px' }"
      @click="document.selection = document.selection.handle(constraint.constraint, bus.isCtrlPressed)"
    )

    //- Snap anchor highlights active snap point
    .anchors

      div(
        v-if="snapAnchor"
        :style="{ top: snapAnchor.pos.y + 'px', left: snapAnchor.pos.x + 'px' }"
      )

    //- Draggable sketch handles
    ul.handles

      li(
        v-for="handle in allHandles"
        :key="handle.id"
        :style="{ top: handle.pos.y + 'px', left: handle.pos.x + 'px' }"
        @mouseup="mouseUp"
        @mousedown="handleMouseDown($event, handle)"
        @mousemove="handleMouseMove($event, handle)"
        @contextmenu.prevent
      )

    //- Floating UI widgets
    transition-group(name="hide2")
      SelectorWidget(
        v-for="(widget, i) in widgets"
        :key="widget.pos.x"
        :widget="widget"
        @remove="widgets.splice(i, 1)"
        @change="$emit('update:highlight', $event)"
      )

</template>


<style lang="stylus" scoped>
  .view-port
    position: relative
    overflow: hidden

  canvas
    display: block
    background: radial-gradient(farthest-corner at 50% 150%, #333333, #1c2127)

  .drawpad
  .handles
  .anchors
    position: absolute
    left: 0
    top: 0
    width: 100%
    height: 100%
    pointer-events: none

  .drawpad
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
    // cursor: grab
    &:hover
      &::before
        width: 5px
        height: 5px
    &:active
      &::before
        width:  5px
        height: 5px

  .constraint
    position: absolute
    // cursor: pointer
    margin-left: -9px
    margin-top: -9px
    border-radius: 99px
    padding: 2px
    width: 19px
    height: 19px
    transition: background 0.1s
    color: #1c2127
    background: $bright1
    box-shadow: 0 1px 3px rgba(black, 0.5)

    &:hover
    &.selected
      color: white

    &.selected
      background: $highlight

  .selector-widget
    pointer-events: auto
    position: absolute

  .hide-enter-active
  .hide-leave-active
    transition: all 0.2s
  .hide-enter-from
  .hide-leave-to
    opacity: 0

  .hide2-enter-active
  .hide2-leave-active
    transition: all 0.15s
  .hide2-enter-from
  .hide2-leave-to
    opacity: 0
    transform: translateY(6px)
</style>


<script>
  import * as THREE from 'three'

  import Snapper from './../js/snapping.js'
  import Renderer from './../js/renderer.js'
  import Transloader from './../js/transloader.js'
  import { CoincidentConstraint, Dimension } from './../js/core/kernel.js'
  import {
    DummyTool,
    ManipulationTool,
    ObjectPickTool,
    ProfilePickTool,
    EdgePickTool,
    FacePickTool,
    PlanePickTool,
    LineTool,
    SplineTool,
    CircleTool,
    ArcTool,
    PerpendicularConstraintTool,
    DimensionTool,
  } from './../js/tools.js'

  export default {
    name: 'ViewPort',

    inject: ['bus'],

    props: {
      document: Object,
      activeTool: Object,
      highlight: Object,
      activeView: Object,
      displayMode: String,
    },

    data() {
      return {
        snapAnchor: null,
        handles: {},
        paths: [],
        pickingPath: null,
        guides: [],
        widgets: [],
        constraints: [],
      }
    },

    watch: {
      document: function(document, oldDocument) {
        this.transloader.unloadTree(oldDocument.top(), true)
      },

      'document.activeComponent': function(comp) {
        this.transloader.setActiveComponent(comp)
        const tree = this.document.top()
        this.componentChanged(tree, true)
        // this.bus.emit('activate-tool', ManipulationTool)
      },

      'document.activeSketch': function(sketch) {
        // Show sketch plane
        this.transloader.setActiveSketch(sketch)
        if(sketch) {
          let plane = sketch.workplane
          this.snapper.planeTransform = plane
          // console.log('activeSketch', sketch)
          this.renderer.sketchPlane.setPlane(plane)
        }
        this.renderer.sketchPlane.visible = !!sketch
        // Display grab handles
        this.handles = {}
        this.document.activeComponent.creator.cache().curves.forEach(curve => {
          if(curve.sketch !== sketch) return
          this.onLoadElement(curve)
        })
        this.renderer.render()
      },

      'document.selection': function(selection) {
        this.transloader.setSelection(selection)
        this.renderer.render()
      },

      highlight: function(highlight) {
        this.transloader.setHighlight(highlight)
        this.renderer.render()
      },

      activeView: function(view) {
        if(!view) return
        this.renderer.setView(view.position, view.target)
      },

      displayMode: function(mode) {
        this.renderer.setDisplayMode(mode)
        this.componentChanged(this.document.top(), true)
      },
    },

    computed: {
      allHandles: function() {
        if(!this.document.activeSketch) return {}
        const handles = Object.values(this.handles).map(e => Object.values(e) ).flat()
        const set = {}
        handles.forEach(handle => set[JSON.stringify(handle.pos)] = handle )
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
      this.renderer.on('change-view', (position, target) => {
        this.document.viewChanged(position, target)
        this.$emit('update:highlight', null)
      })

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
      this.transloader.setActiveComponent(this.document.activeComponent)
      this.transloader.loadTree(this.document.top(), true)

      // Events
      this.bus.on('pick', (type, pickerCoords, color) => {
        // console.log('picking')
        this.handlePick(pickerCoords, color, {
          profile: ProfilePickTool,
          curve: ObjectPickTool,
          axis: ObjectPickTool,
          edge: EdgePickTool,
          face: FacePickTool,
          plane: PlanePickTool,
        }[type])
      })

      this.bus.on('show-picker', this.addPath)
      this.bus.on('clear-pickers', this.clearPaths)
      this.bus.on('activate-tool', this.activateTool)
      this.document.on('component-changed', this.componentChanged)
      this.document.on('component-deleted', this.componentDeleted)
      this.bus.on('render-needed', () => this.renderer.render())
      this.bus.on('preview-feature', this.transloader.previewFeature.bind(this.transloader))
      this.bus.on('unpreview-feature', this.unpreviewFeature)
      this.bus.on('resize', this.onWindowResize)
      this.bus.on('keydown', this.keyDown)
      this.bus.on('keyup', this.keyUp)

      // Window Resize
      setTimeout(() => this.onWindowResize(), 1000)
      this.onWindowResize()
    },

    beforeUnmount: function() {
      this.bus.off('resize', this.onWindowResize)
      this.renderer.dispose()
    },

    methods: {
      getMouseCoords: function(e) {
        var rect = this.$refs.canvas.getBoundingClientRect()
        return new THREE.Vector2(e.clientX, e.clientY - rect.top)
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
        this.transloader.updateRegions(this.document.activeComponent)
        this.renderer.render()
        this.regionsDirty = false
      },

      mouseDown: function(e) {
        document.activeElement.blur() // Necessary since THREE R123
        if(e.button != 0) return
        if(e.altKey) return
        const [vec, coords] = this.snap(e)
        if(vec) this.activeTool.mouseDown(vec, coords)
        this.lastCoords = coords
      },

      handleMouseDown: function(e, handle) {
        if(e.button != 0) return
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

      keyDown: function(keyCode) {
        if(keyCode == 46 || keyCode == 8) { // Del / Backspace
          // Delete Selection
          if(this.document.selection.set.size) {
            this.document.selection.set.forEach(item => {
              const type = item.typename()
              if(type != 'Solid' && type != 'Component') {
                this.deleteElement(item)
              }
            })
          }
        } else if(keyCode == 18) { // alt
          // this.guides = []
        }
      },

      keyUp: function(keyCode) {
        if(keyCode == 18) { // alt
        } else if(keyCode == 79) { // o
          this.renderer.switchCamera()
        }
      },

      snap: function(e) {
        const coords = this.getMouseCoords(e)
        let vec = this.renderer.fromScreen(coords)
        // vec.applyMatrix4(this.snapper.planeTransform.clone().invert())
        // vec.setZ(0)
        // vec.applyMatrix4(this.snapper.planeTransform)
        return [this.snapper.snap(vec, coords, this.activeTool.snapToGuides, this.activeTool.snapToPoints, this.activeTool.localSpace ), coords]
      },

      handlePick: function(pickerCoords, color, Tool) {
        if(this.activeTool) this.activeTool.dispose()
        this.pickingPath = { target: null, color, origin: pickerCoords }
        const tool = new Tool(this.document.activeComponent, this, (item, mesh) => {
          this.bus.emit('picked', item)
          this.bus.emit('activate-tool', DummyTool)
          this.pickingPath = null
        })
        this.$emit('update:active-tool', tool)
      },

      addPath: function(pickerCoords, center, color) {
        // console.log('addpath', pickerCoords, center, color)
        const path = {
          target: center,
          origin: pickerCoords,
          color,
        }
        this.updatePath(path)
        this.paths.push(path)
      },

      clearPaths: function() {
        this.paths = []
      },

      updateWidgets: function() {
        // Update Snap Anchor
        if(this.snapAnchor) this.snapAnchor.pos = this.renderer.toScreen(this.snapAnchor.vec)

        this.constraints = []

        if(this.document.activeSketch) {
          // Update Handles
          for(let elemId in this.handles) {
            const elemHandles = this.handles[elemId]
            elemHandles.forEach(handle => {
              handle.pos = this.renderer.toScreen(handle.vec)
            })
          }
          this.handles = Object.assign({}, this.handles)

          // Update constraints
          this.constraints = this.document.activeSketch.constraints.flatMap(c => {
            if(c instanceof CoincidentConstraint || c instanceof Dimension) return
            const pair = c.items()
            return pair.map((item, i) => ({
              constraint: c,
              curve: item.curve,
              coords: this.renderer.toScreen(
                (pair.length == 1 ?
                  item.curve.center()
                  :
                  item.curve.center().clone()
                    .add(item.curve.commonHandle(pair[1 - i].curve))
                    .divideScalar(2.0)
                ).applyMatrix4(item.curve.sketch.workplane)
              ),
            }))
          }).filter(Boolean)
        }

        // Update Paths
        this.paths.forEach((path, i) => {
          this.updatePath(path)
        })
        if(!this.pickingPath || !this.pickingPath.target) return
        this.updatePath(this.pickingPath)
      },

      updatePath: function(path) {
        path.targetPos = this.renderer.toScreen(path.target)
        path.data = this.buildPath(path.origin, path.targetPos)
      },

      buildPath: function(origin, pos) {
        const sign = this.document.activeFeature ? -1 : 1
        const dx = Math.min(25 + Math.abs(origin.x - pos.x) / 2.0, 200) * sign
        const dy = Math.abs(origin.y - pos.y) / 2.0 * sign
        return `M ${origin.x} ${origin.y} C ${origin.x} ${origin.y + dx} ${pos.x} ${pos.y - dy} ${pos.x} ${pos.y}`
      },

      activateTool: function(Tool) {
        console.log('activating', Tool)
        if(this.activeTool) this.activeTool.dispose()
        this.pickingPath = null
        this.snapper.reset()
        if(!Tool) return
        const tool = new Tool(this.document.activeComponent, this, this.document.activeSketch)
        this.$emit('update:active-tool', tool)
        this.$emit('update:highlight', null)
        this.renderer.render()
      },

      deleteElement: function(elem) {
        // this.renderer.removeGizmo()
        elem.sketch.remove(elem)
        this.document.selection = this.document.selection.delete(elem)
        this.componentChanged(this.document.activeComponent)
      },

      componentChanged: function(comp, recursive) {
        this.transloader.unloadTree(comp, recursive)
        this.transloader.loadTree(comp, recursive)
        this.renderer.updateShadows()
        this.renderer.render()
      },

      componentDeleted: function(comp) {
        this.transloader.unloadTree(comp, true)
        this.renderer.updateShadows()
        this.renderer.render()
      },

      elementChanged: function(elem, comp) {
        this.regionsDirty = true
        this.transloader.loadElement(elem, comp)
        this.renderer.render()
      },

      unpreviewFeature: function() {
        this.clearPaths()
        this.transloader.unpreviewFeature()
      },

      onLoadElement: function(elem) {
        if(elem.sketch !== this.document.activeSketch) return
        this.handles[elem.id] = elem.handles().map((handle, i) => {
          handle = handle.clone().applyMatrix4(elem.sketch.workplane)
          return {
            type: 'handle',
            pos: this.renderer.toScreen(handle),
            vec: handle,
            id: Math.random(),
            elem,
            index: i,
          }
        })
      },

      onUnloadElement: function(elem) {
        delete this.handles[elem.id]
        this.handles = Object.assign({}, this.handles)
      },

      onWindowResize: function() {
        const parent = this.$refs.canvas.parentElement
        this.$refs.drawpad.setAttribute(
          'viewBox',
          '0 0 ' + parent.offsetWidth + ' ' + parent.offsetHeight
        )
        setTimeout(() => this.renderer.onWindowResize() )
      },
    }
  }
</script>
