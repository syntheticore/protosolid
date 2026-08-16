<template lang="pug">

  .view-port(
    :style="{ cursor: (activeTool && activeTool.cursor) || 'auto' }"
    :class="{ orbiting: isOrbiting }"
    @contextmenu.prevent
  )

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
      TransitionGroup(name="hide-guides" tag="g")
        line(
          v-for="guide in guides"
          :key="guide.id"
          :x1="guide.start.x"
          :y1="guide.start.y"
          :x2="guide.end.x"
          :y2="guide.end.y"
        )

    .floaters

      ConstraintProxy(
        v-if="dimensionPreview"
        :document="document"
        :constraint="dimensionPreview"
        :preview="true"
        :parent-transform="activeComponentTransform"
      )

      ComponentProxy(
        v-if="isReady"
        :document="document"
        :component="document.top()"
        :display-mode="displayMode"
        :color-mode="colorMode"
        :active-handle="activeHandle"
        :active-tool="activeTool"
        @handleMouseUp="mouseUp"
        @handleMouseDown="handleMouseDown"
        @handleMouseMove="handleMouseMove"
        @handleMouseLeave="handleMouseLeave"
        @dimensionMouseUp="mouseUp"
        @dimensionMouseDown="dimensionMouseDown"
        @dimensionMouseMove="dimensionMouseMove"
      )

      //- Snap anchor highlights active snap point
      .anchor.handle(
        v-if="snapAnchor"
        :style="{ top: snapAnchor.pos.y + 'px', left: snapAnchor.pos.x + 'px' }"
      )

      //- Floating UI widgets
      TransitionGroup(name="hide-selector")
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
  .floaters
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

  .anchor

    &::after
      transform: scale(1)
      opacity: 1
      width:  calc(100% - 2px)
      height: calc(100% - 2px)
      border: 2px solid $highlight * 1.6
      animation-duration: 0.15s
      animation-name: focus

    @keyframes focus
      from
        opacity: 0
        transform: scale(1.7)

      to
        transform: scale(1)
        opacity: 1

  .selector-widget
    pointer-events: auto
    position: absolute

  .hide-guides-enter-active
  .hide-guides-leave-active
    transition: all 0.2s
  .hide-guides-enter-from
  .hide-guides-leave-to
    opacity: 0

  .hide-selector-enter-active
  .hide-selector-leave-active
    transition: all 0.15s
  .hide-selector-enter-from
  .hide-selector-leave-to
    opacity: 0
    transform: translateY(6px)

</style>


<script>

  import * as THREE from 'three'

  import Snapper from './../js/snapping.js'
  import Renderer from './../js/renderer.js'
  import { SketchElement } from './../js/core/geom2d.js'
  import { Dimension, CoincidentConstraint } from './../js/core/sketch.js'
  import { Solid, Face } from './../js/core/geom3d.js'
  import { worldTransform } from './../js/core/assembly.js'
  import {
    DummyTool,
    ManipulationTool,
    CurvePickTool,
    ProfilePickTool,
    EdgePickTool,
    FacePickTool,
    PlanePickTool,
    AxisPickTool,
    SolidPickTool,
    PointPickTool,
    ComponentPickTool,
    PatternInputPickTool,
    LineTool,
    SplineTool,
    CircleTool,
    ArcTool,
    PerpendicularConstraintTool,
    DimensionTool,
  } from './../js/tools.js'
  import materials from '../js/materials.js'

  export default {
    name: 'ViewPort',

    inject: ['bus', 'frame'],

    props: {
      document: Object,
      activeTool: Object,
      highlight: Object,
      activeView: Object,
      displayMode: String,
      colorMode: String,
    },

    data() {
      return {
        isReady: false,
        snapAnchor: null,
        paths: [],
        pickingPath: null,
        guides: [],
        widgets: [],
        isOrbiting: false,
        isPanning: false,
        activeHandle: null,
        hoveredHandle: null,
        activeDimension: null,
        hoveredDimension: null,
        dimensionPreview: null,
        cameraPreview: null,
      }
    },

    watch: {
      document: function(document, oldDocument) {
        this.cameraPreview = null
        this.registerDocument(this.document, oldDocument)
      },

      'document.activeSketch': function(sketch) {
        // Show sketch plane
        if(sketch) {
          const plane = worldTransform(this.document.activeComponent).multiply(sketch.workplane)
          this.snapper.planeTransform = plane
          this.renderer.sketchPlane.setPlane(plane)
        }
        this.renderer.sketchPlane.visible = !!sketch
        this.renderer.render()
      },

      activeView: function(view) {
        if(!view) return
        this.renderer.setView(view.position, view.target)
      },

      displayMode: function(mode) {
        this.renderer.setDisplayMode(mode)
      },
    },

    computed: {
      activeComponentTransform: function() {
        return this.document.activeSketch ? worldTransform(this.document.activeComponent) : null
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
      this.renderer.on('render', () => {
        this.frame++
        this.updateWidgets()
      })
      this.renderer.on('change-view', (position, target) => {
        this.document.viewChanged(position, target)
        if(!this.widgets.length) this.$emit('update:highlight', null)
      })
      this.isReady = true

      // Snapping
      this.snapper = new Snapper(this, (guides, anchor) => {
        this.guides = guides
        this.snapAnchor = anchor
      })

      // Events
      this.bus.on('pick', (type, pickerCoords, color, acceptsInput) => {
        this.handlePick(pickerCoords, color, {
          profile: ProfilePickTool,
          curve: CurvePickTool,
          axis: AxisPickTool,
          edge: EdgePickTool,
          face: FacePickTool,
          plane: PlanePickTool,
          solid: SolidPickTool,
          patternInput: PatternInputPickTool,
          point: PointPickTool,
          componentRef: ComponentPickTool,
        }[type], acceptsInput)
      })

      this.bus.on('show-picker', this.addPath)
      this.bus.on('clear-pickers', this.clearPaths)
      this.bus.on('activate-tool', this.activateTool)
      this.bus.on('zoom-all', () => {
        this.commitCameraPreview()
        this.zoomToFit()
      })
      this.bus.on('zoom-active', () => {
        this.commitCameraPreview()
        this.zoomToFit(this.document.activeComponent.compound.solids())
      })
      this.bus.on('zoom-selection', () => {
        this.commitCameraPreview()
        this.zoomToFit(this.document.selection.items)
      })
      this.bus.on('preview-zoom-all', () => this.previewCamera(() => this.zoomToFit([], true)) )
      this.bus.on('preview-zoom-active', () => this.previewCamera(() =>
        this.zoomToFit(this.document.activeComponent.compound.solids(), true)
      ))
      this.bus.on('preview-zoom-selection', () => this.previewCamera(() =>
        this.zoomToFit(this.document.selection.items, true)
      ))
      this.bus.on('preview-look-at', plane => this.previewCamera(() => {
        this.renderer.lookAt(plane, false)
        const sketch = this.document.activeSketch
        if(sketch && sketch.elements.length) this.zoomToFit(sketch.elements, true)
      }))
      this.bus.on('commit-camera-preview', this.commitCameraPreview)
      this.bus.on('unpreview-camera', this.unpreviewCamera)
      this.bus.on('render-needed', () => this.renderer.render() )
      this.bus.on('preview-feature', this.previewFeature)
      this.bus.on('unpreview-feature', this.unpreviewFeature)
      this.bus.on('resize', this.onWindowResize)
      this.bus.on('keydown', this.keyDown)
      this.bus.on('keyup', this.keyUp)

      bus.on('alt-pressed', (pressed) => {
        this.renderer.viewControls.enableRotate = pressed
        this.isOrbiting = pressed
      })

      this.registerDocument(this.document)

      // Window Resize
      setTimeout(() => this.onWindowResize(), 1000)
      this.onWindowResize()
    },

    beforeUnmount: function() {
      this.bus.off('resize', this.onWindowResize)
      this.renderer.dispose()
    },

    methods: {
      registerDocument: function(doc, oldDoc) {
        if(oldDoc) {
          oldDoc.off('force-view')
          oldDoc.off('look-at')
        }
        doc.on('force-view', (view) => this.renderer.setView(view.position, view.target) )
        doc.on('look-at', (plane) => setTimeout(() => {
          this.renderer.lookAt(plane)
          const sketch = this.document.activeSketch
          if(sketch && sketch.elements.length) this.zoomToFit(sketch.elements)
        }))
      },

      getMouseCoords: function(e) {
        var rect = this.$refs.canvas.getBoundingClientRect()
        return new THREE.Vector2(e.clientX, e.clientY - rect.top)
      },

      doubleClick: function(e) {
        this.renderer.setPivot(this.getMouseCoords(e))
      },

      mouseUp: function(e) {
        if(this.isPanning) {
          this.isPanning = false
          this.activeHandle = null
          this.activeDimension = null
          this.snapper.reset()
          return
        }
        const [vec, coords] = this.snap(e)
        const draggedHandle = this.activeHandle
        if(vec) this.activeTool.mouseUp(vec, coords)
        this.activeHandle = null
        this.activeDimension = null
        this.snapper.reset()
        this.updateRegions(false, draggedHandle)
      },

      updateRegions: function(force, solveHandle) {
        if(!this.regionsDirty && !force) return
        this.updateSketch()
        if(this.document.activeSketch) {
          // Vue processes the final solve after activeHandle is cleared. Keep
          // the just-dragged handle available so releasing the pointer does
          // not let under-constrained geometry drift to a different solution.
          this.document.activeSketch.solveHandle = solveHandle
          this.document.activeSketch.profileUpdateNeeded = true
        }
        this.regionsDirty = false
      },

      updateSketch: async function(temporary) {
        const sketch = this.document.activeSketch
        if(!sketch) return
        this.regionsDirty = true
        if(temporary) {
          // Keep constraints visually satisfied during a drag. Deferring this
          // through Vue lets pointer events outrun the solver and leaves the
          // geometry showing stale, invalid intermediate positions.
          sketch.solve(this.document.top(), this.activeHandle)
          sketch.solveNeeded = false
        } else {
          sketch.solveNeeded = true
        }
      },

      mouseDown: function(e) {
        document.activeElement.blur() // Necessary since THREE R123
        if(e.button != 0) return
        this.isPanning = e.shiftKey
        if(this.isPanning) {
          // Shift gives the complete pointer gesture to OrbitControls. This
          // also clears proxy state when the pan starts over a sketch handle.
          this.activeHandle = null
          this.activeDimension = null
          this.snapper.reset()
          return
        }
        if(e.altKey) return
        const [vec, coords] = this.snap(e)
        if(vec) this.activeTool.mouseDown(vec, coords, e)
      },

      handleMouseDown: function(e, handle) {
        if(e.button != 0) return
        this.activeHandle = handle.readonly ? null : handle
        this.mouseDown(e)
        this.snapper.reset()
      },

      handleMouseMove: function(e, handle) {
        this.hoveredHandle = handle
        this.mouseMove(e)
      },

      handleMouseLeave: function(e, handle) {
        this.hoveredHandle = null
      },

      dimensionMouseDown: function(e, dimension) {
        if(e.button != 0) return
        this.activeDimension = dimension
        this.mouseDown(e)
      },

      dimensionMouseMove: function(e, dimension) {
        this.hoveredDimension = dimension
        this.mouseMove(e)
        this.hoveredDimension = null
      },

      mouseMove: function(e) {
        if(e.button != 0) return
        if(this.isPanning) return
        if(this.isOrbiting) return
        if(e.altKey) return
        const [vec, coords] = this.snap(e)
        if(this.pickingPath && vec) this.pickingPath.target = vec
        if(vec) this.activeTool.mouseMove(vec, coords)
      },

      keyDown: function(key) {
        if(key == 'Delete' || key == 'Backspace') {
          // Delete Selection
          if(this.document.selection.items.length) {
            [...this.document.selection.items].forEach(item => {
              const type = item.typename()
              if(type != 'Solid' && type != 'Component') {
                this.deleteElement(item)
              }
            })
          }
        }
      },

      keyUp: function(key) {
        if(key == 'Alt') {
        } else if(key == 'o') {
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

      handlePick: function(pickerCoords, color, Tool, acceptsInput) {
        if(this.activeTool) this.activeTool.dispose()
        this.resetProxyInteraction()
        this.pickingPath = { target: null, color, origin: pickerCoords }
        const tool = new Tool(this.document.activeComponent, this, (item, repick) => {
          this.bus.emit('picked', item, repick)
          this.bus.emit('activate-tool', DummyTool)
          this.pickingPath = null
        })
        tool.acceptsInput = acceptsInput
        this.$emit('update:active-tool', tool)
      },

      addPath: function(pickerCoords, center, color) {
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
        if(this.activeTool) this.activeTool.dispose()
        this.resetProxyInteraction()
        this.pickingPath = null
        this.snapper.reset()
        if(!Tool) return
        const tool = new Tool(this.document.activeComponent, this, this.document.activeSketch)
        this.$emit('update:active-tool', tool)
        this.$emit('update:highlight', null)
        // this.renderer.render()
      },

      resetProxyInteraction: function() {
        // A proxy can disappear without receiving mouseleave (for example the
        // line tool's zero-length next segment). Never carry its handles or
        // dimensions into the next tool's picking state.
        this.activeHandle = null
        this.hoveredHandle = null
        this.activeDimension = null
        this.hoveredDimension = null
      },

      deleteElement: function(elem) {
        elem.sketch.remove(elem)
        this.document.selection.delete(elem)
        this.updateRegions(true)
      },

      previewCamera: function(action) {
        if(!this.cameraPreview) {
          this.cameraPreview = {
            position: (this.renderer.cameraTarget || this.renderer.camera.position).clone(),
            target: (this.renderer.viewControlsTarget || this.renderer.viewControls.target).clone(),
          }
        }
        action()
      },

      commitCameraPreview: function() {
        this.cameraPreview = null
      },

      unpreviewCamera: function() {
        if(!this.cameraPreview) return
        const view = this.cameraPreview
        this.cameraPreview = null
        this.renderer.setView(view.position, view.target)
      },

      zoomToFit: function(objects=[], preview=false) {
        const solidFaces = objects.filter(sel => sel instanceof Solid ).flatMap(solid => solid.faces() )
        const rest = objects.filter(sel => sel instanceof Face || sel instanceof SketchElement )
        const meshes = solidFaces.concat(rest).map(obj => obj.mesh && obj.mesh() ).filter(Boolean)
        this.renderer.zoomToFit(meshes.length && meshes, !preview)
      },

      elementChanged: function() {
        this.regionsDirty = true
      },

      previewFeature: function(compound, subtracting) {
        this.renderer.remove(this.previewMesh)
        this.previewMesh = this.renderer.convertMesh(
          compound.tesselate(),
          subtracting ? materials.previewSubtractSurface : materials.previewAddSurface,
        )
        this.previewMesh.applyMatrix4(worldTransform(this.document.activeComponent))
        this.renderer.add(this.previewMesh)
        this.renderer.render()
      },

      unpreviewFeature: function() {
        this.clearPaths()
        this.renderer.remove(this.previewMesh)
        this.renderer.render()
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
