<template lang="pug">

  .view-port(
    :style="{ cursor: (activeTool && activeTool.cursor) || 'auto' }"
    :class="{ orbiting: isOrbiting, merging: !!document.activeMerge }"
    @contextmenu.prevent
    @mouseleave="$emit('update:highlight', null)"
  )

    //- GL Viewport
    canvas(
      ref="canvas"
      @click="click"
      @dblclick="doubleClick"
      @pointerup="mouseUp"
      @pointerdown="mouseDown"
      @pointercancel="pointerCancel"
      @lostpointercapture="pointerCancel"
      @wheel="mouseWheel"
      @mousemove="mouseMove"
      @mouseleave="mouseLeave"
    )

    svg.drawpad(ref="drawpad" viewBox="0 0 100 100" fill="transparent")

      //- Pick indicators
      path(v-for="path in allPaths", :d="path.data", :stroke="path.color")
      circle(v-for="path in allPaths", :cx="path.targetPos.x", :cy="path.targetPos.y", r="5", :fill="path.color")

      g.thermal-probe(v-if="thermalProbe")
        path(:d="probePath")
        circle(:cx="thermalProbe.position.x" :cy="thermalProbe.position.y" r="4")

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

      rect.selection-box(
        v-if="selectionBox"
        :x="selectionBox.x"
        :y="selectionBox.y"
        :width="selectionBox.width"
        :height="selectionBox.height"
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
        :section-plane="sketchSectionPlane"
        :section-side="sketchSectionSide"
        :active-handle="activeHandle"
        :active-tool="activeTool"
        @handleMouseUp="mouseUp"
        @handleMouseDown="handleMouseDown"
        @handleMouseMove="handleMouseMove"
        @handleMouseLeave="handleMouseLeave"
        @handlePointerDown="forwardCameraPointerDown"
        @handleWheel="forwardWheelToCanvas"
        @dimensionMouseUp="mouseUp"
        @dimensionMouseDown="dimensionMouseDown"
        @dimensionMouseMove="dimensionMouseMove"
      )

      JointProxy(
        v-if="isReady && displayMode != 'pathTraced'"
        v-for="joint in visibleJoints"
        :key="joint.id"
        :document="document"
        :joint="joint"
        :active-tool="activeTool"
      )

      .thermal-probe-box(
        v-if="thermalProbe"
        :style="{ left: thermalProbe.position.x + 22 + 'px', top: thermalProbe.position.y - 31 + 'px' }"
      ) {{ thermalProbe.label }}

      //- Snap anchor highlights active snap point
      .anchor.handle(
        v-if="snapAnchor"
        :style="{ top: snapAnchor.pos.y + 'px', left: snapAnchor.pos.x + 'px' }"
      )

      //- Numeric input for the shape being drawn
      DrawingInput(
        v-if="hasDrawingInput"
        :key="activeTool.inputStage"
        :class="{ hidden: drawingInputHidden }"
        :tool="activeTool"
        :component="document.top()"
        :position="pointerCoords"
      )

      //- Ends the line or spline being drawn to start a new one
      RestartWidget(
        v-if="isReady && document.activeSketch"
        :document="document"
        :tool="activeTool"
        @restart="restartTool"
        @wheel="forwardWheelToCanvas"
      )

      //- Floating UI widgets
      Transition(name="hide-selector")
        SelectionFeatureWidget(
          v-if="isReady && selectionWidgetItem"
          :document="document"
          :item="selectionWidgetItem"
          :renderer="renderer"
          :coords="pointerCoords"
        )

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

  // Registered, so the gradient colors can transition
  @property --viewport-inner
    syntax: '<color>'
    inherits: false
    initial-value: #333333

  @property --viewport-outer
    syntax: '<color>'
    inherits: false
    initial-value: #1c2127

  canvas
    display: block
    // Keep the active tool cursor when OrbitControls writes an inline cursor.
    cursor: inherit !important
    --viewport-inner: #333333
    --viewport-outer: #1c2127
    background: radial-gradient(farthest-corner at 50% 150%, var(--viewport-inner), var(--viewport-outer))
    transition: --viewport-inner 0.4s, --viewport-outer 0.4s

    // Tinted while previewing a merge
    .merging &
      --viewport-inner: #1d2729
      --viewport-outer: #0f1517

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

    .selection-box
      fill: rgba($highlight, 0.12)
      stroke: $highlight
      stroke-width: 1
      stroke-dasharray: 4, 3

    .thermal-probe

      path
        stroke: darkgray
        stroke-dasharray: none
        opacity: 1

      circle
        fill: darkgray
        stroke: none

  .thermal-probe-box
    position: absolute
    padding: 6px 8px
    color: white
    background: rgba($dark1, 0.6)
    backdrop-filter: blur(32px)
    border: 2px solid darkgray
    border-radius: 99px
    font-size: 12px
    font-weight: bold
    white-space: nowrap

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

  // Stays mounted, keeping focus and typed values
  .drawing-input.hidden
    opacity: 0

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
  import { computed, markRaw } from 'vue'

  import Snapper from './../js/snapping.js'
  import Renderer from './../js/renderer.js'
  import { SketchElement } from './../js/core/geom2d.js'
  import { Dimension, CoincidentConstraint } from './../js/core/sketch.js'
  import { Solid, Face, Edge } from './../js/core/geom3d.js'
  import { Reference } from './../js/core/references.js'
  import { setWorldTransform, solveAssembly, worldTransform } from './../js/core/assembly.js'
  import { TransformGizmo } from './../js/three/gizmos.js'
  import SimulationFlow from '../js/three/simulation-flow.js'
  import { componentTriangles } from './../js/core/export.js'
  import {
    DummyTool,
    ManipulationTool,
    CurvePickTool,
    PathPickTool,
    ProfilePickTool,
    EdgePickTool,
    FacePickTool,
    PlanePickTool,
    AxisPickTool,
    SolidPickTool,
    PointPickTool,
    ComponentPickTool,
    JointPickTool,
    PatternInputPickTool,
    LineTool,
    SplineTool,
    CircleTool,
    ArcTool,
    PerpendicularConstraintTool,
    DimensionTool,
  } from './../js/tools.js'
  import materials from '../js/materials.js'
  import { copySketchSelection, hasSketchClipboard } from '../js/sketch-clipboard.js'
  import { PasteSketchTool } from '../js/tools.js'

  export default {
    name: 'ViewPort',

    inject: ['bus', 'frame'],

    provide() {
      const pickerSelection = computed(() => {
        const selection = new Map()
        for(let ref of this.pickerReferences) {
          while(ref.item instanceof Reference) ref = ref.item
          if(!selection.has(ref.componentId)) selection.set(ref.componentId, new Set())
          for(const item of ref.getItems()) selection.get(ref.componentId).add(item.id)
        }
        return selection
      })
      return { 'picker-selection': pickerSelection, 'picker-type': computed(() => this.pickerType) }
    },

    props: {
      document: Object,
      activeTool: Object,
      highlight: Object,
      activeView: Object,
      displayMode: String,
      colorMode: String,
      projectionMode: String,
      sketchSection: Boolean,
    },

    data() {
      return {
        isReady: false,
        snapAnchor: null,
        paths: [],
        pickingPath: null,
        pickerReferences: [],
        pickerType: null,
        guides: [],
        widgets: [],
        isOrbiting: false,
        isZooming: false,
        zoomEndTimer: null,
        isPanning: false,
        activeHandle: null,
        hoveredHandle: null,
        activeDimension: null,
        hoveredDimension: null,
        selectionBox: null,
        dimensionPreview: null,
        cameraPreview: null,
        cameraUnpreviewFrame: null,
        thermalProbe: null,
        exportPreviewMesh: null,
        sketchSectionSide: false,
        pointerCoords: null,
        drawingInputHidden: false,
      }
    },

    watch: {
      document: function(document, oldDocument) {
        this.cancelCameraUnpreview()
        this.cameraPreview = null
        this.registerDocument(this.document, oldDocument)
        this.activateTool(ManipulationTool)
      },

      'document.activeSketch': function(sketch) {
        // Show sketch plane
        if(sketch) {
          const plane = worldTransform(this.document.activeComponent).multiply(sketch.workplane)
          this.snapper.planeTransform = plane
          this.renderer.sketchPlane.setPlane(plane)
        }
        this.renderer.sketchPlane.setSketch(sketch)
        this.renderer.sketchPlane.visible = !!sketch
        this.updateSketchSectionSide()
        this.renderer.render()
      },

      sketchSection: function() {
        this.updateSketchSectionSide()
      },

      cameraView: function({ document, view }, previous) {
        const cameraView = document !== previous.document ? document.currentCameraView : view
        if(!cameraView) return
        this.$nextTick(() => {
          if(this.document === document && this.activeView === view) this.applyView(cameraView)
        })
      },

      displayMode: function(mode) {
        this.renderer.setDisplayMode(mode)
      },

      projectionMode: function(mode) {
        this.renderer.setProjectionMode(mode)
      },

      'document.activeSimulation': function() {
        this.thermalProbe = null
        this.$nextTick(this.updateSimulationFlow)
      },

      exportPreview: function() {
        if(this.renderer) this.updateExportPreview()
      },

      transformSelectionKey: function() {
        if(this.renderer) this.updateSelectionTransformGizmo()
      },
    },

    computed: {
      cameraView: function() {
        return { document: this.document, view: this.activeView }
      },

      activeComponentTransform: function() {
        return this.document.activeSketch ? worldTransform(this.document.activeComponent) : null
      },

      sketchSectionPlane: function() {
        if(!this.sketchSection || !this.document.activeSketch) return null
        return worldTransform(this.document.activeComponent).multiply(this.document.activeSketch.workplane)
      },

      exportPreview: function() {
        const config = this.document.activeExportConfig
        // STEP keeps the exact BREP; only mesh formats preview a tessellation.
        if(!config || config.format == 'STEP') return null
        return {
          config,
          componentId: this.document.activeExportComponentId,
          maxDistance: config.maxDistance,
          maxAngle: config.maxAngle,
          marker: this.document.timeline.marker,
        }
      },

      transformSelectionKey: function() {
        const feature = this.document.activeFeature
        const creatingFeature = this.document.creatingFeature
        const sketch = this.document.activeSketch
        const panel = this.document.activeInteractivePanel
        const items = this.document.selection.items.map(item => {
          const component = item?.typename?.() == 'Component' ? item : item?.component
          return `${item?.id || ''}:${component?.id || ''}`
        })
        return `${feature?.id || ''}|${creatingFeature?.id || ''}|${sketch?.id || ''}|${panel || ''}|${items.join(',')}`
      },

      selectionWidgetItem: function() {
        if(this.widgets.length || !(this.activeTool instanceof ManipulationTool) || this.document.activeSketch || this.document.activeMerge) return null
        const item = this.document.selection.items.at(-1)
        return item instanceof Face || item instanceof Edge ? item : null
      },

      hasDrawingInput: function() {
        return this.pointerCoords && this.activeTool?.inputFields().length > 0
      },

      allPaths: function() {
        const paths = [...this.paths]
        if(this.pickingPath && this.pickingPath.target) paths.push(this.pickingPath)
        return paths
      },

      probePath: function() {
        const position = this.thermalProbe.position
        return `M ${position.x} ${position.y} L ${position.x + 12} ${position.y - 18} L ${position.x + 22} ${position.y - 18}`
      },

      visibleJoints: function() {
        const seen = new Set()
        return (this.document.top().assemblyJoints || []).filter(joint => {
          if(seen.has(joint.id)) return false
          seen.add(joint.id)
          return true
        })
      },
    },

    mounted: function() {
      // Renderer
      this.renderer = new Renderer(this.$el.querySelector('canvas'))
      this.renderer.sketchPlane.setSketch(this.document.activeSketch)
      this.renderer.setDisplayMode(this.displayMode)
      this.renderer.setProjectionMode(this.projectionMode)
      this.renderer.on('before-render', () => this.syncSelectionTransformGizmo())
      this.renderer.on('render', () => {
        this.frame++
        this.updateSketchSectionSide()
        this.updateWidgets()
      })
      this.renderer.on('change-view', (position, target) => {
        this.document.viewChanged(position, target)
        if(!this.widgets.length) this.$emit('update:highlight', null)
        // The input stays at the last cursor position, so hide it until the pointer moves again
        this.drawingInputHidden = true
        // The probed surface point no longer lies under the cursor
        this.thermalProbe = null
      })
      this.isReady = true

      // Snapping
      this.snapper = new Snapper(this, (guides, anchor) => {
        this.guides = guides
        this.snapAnchor = anchor
      })

      // Events
      this.bus.on('pick', (type, pickerCoords, color, acceptsInput, references) => {
        this.pickerType = type
        this.handlePick(pickerCoords, color, {
          profile: ProfilePickTool,
          curve: CurvePickTool,
          path: PathPickTool,
          axis: AxisPickTool,
          edge: EdgePickTool,
          face: FacePickTool,
          plane: PlanePickTool,
          solid: SolidPickTool,
          patternInput: PatternInputPickTool,
          point: PointPickTool,
          componentRef: ComponentPickTool,
          joint: JointPickTool,
        }[type], acceptsInput, references)
      })

      this.bus.on('joint-picked', this.jointPicked)

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
      this.bus.on('preview-zoom-all', () => this.previewCamera(view => this.zoomToFit([], true, view)) )
      this.bus.on('preview-zoom-active', () => this.previewCamera(view =>
        this.zoomToFit(this.document.activeComponent.compound.solids(), true, view)
      ))
      this.bus.on('preview-zoom-selection', () => this.previewCamera(view =>
        this.zoomToFit(this.document.selection.items, true, view)
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
      this.bus.on('thermal-result', this.updateSimulationFlow)
      this.bus.on('fluid-result', this.updateSimulationFlow)
      this.bus.on('resize', this.onWindowResize)
      this.bus.on('keydown', this.keyDown)
      this.bus.on('sketch-history', this.changeSketchHistory)
      this.bus.on('sketch-clipboard', this.sketchClipboard)
      this.bus.on('keyup', this.keyUp)

      this.bus.on('alt-pressed', this.setOrbiting)

      this.registerDocument(this.document)
      const initialView = this.document.dirtyView || this.document.previewView || this.activeView || this.document.activeView
      if(initialView) this.applyView(initialView)

      // Window Resize
      setTimeout(() => this.onWindowResize(), 1000)
      this.onWindowResize()
      this.updateSelectionTransformGizmo()
    },

    beforeUnmount: function() {
      this.bus.off('sketch-history', this.changeSketchHistory)
      this.bus.off('sketch-clipboard', this.sketchClipboard)
      clearTimeout(this.zoomEndTimer)
      this.cancelCameraUnpreview()
      this.document.off('regenerated', this.updateExportPreview)
      this.bus.off('resize', this.onWindowResize)
      this.bus.off('thermal-result', this.updateSimulationFlow)
      this.bus.off('fluid-result', this.updateSimulationFlow)
      this.bus.off('joint-picked', this.jointPicked)
      this.bus.off('alt-pressed', this.setOrbiting)
      this.clearSimulationFlow()
      this.clearExportPreview()
      this.clearSelectionTransformGizmo()
      this.renderer.dispose()
    },

    methods: {
      sketchClipboard: function(action) {
        const sketch = this.document.activeSketch
        if(!sketch || this.document.isRegenerating) return
        if(action == 'copy') {
          copySketchSelection(sketch, this.document.selection.items, this.activeTool.historyState?.())
        } else if(hasSketchClipboard()) {
          this.activateTool(ManipulationTool)
          if(this.regionsDirty || sketch.solveNeeded || sketch.profileUpdateNeeded) sketch.solve(this.document.top())
          sketch.recordHistory()
          this.document.selection.clear()
          this.activateTool(PasteSketchTool)
        }
      },

      changeSketchHistory: function(direction) {
        const sketch = this.document.activeSketch
        if(!sketch || this.document.isRegenerating) return
        this.activateTool(this.activeTool instanceof PasteSketchTool ? ManipulationTool : this.activeTool.constructor)
        const tree = this.document.top()
        if(this.regionsDirty || sketch.solveNeeded || sketch.profileUpdateNeeded) sketch.solve(tree)
        sketch.recordHistory()
        if(!sketch.restoreHistory(direction, tree)) return
        this.document.selection.clear()
        this.document.hasChanges = true
        this.regionsDirty = false
        this.$nextTick(() => this.renderer.render())
      },

      updateSketchSectionSide: function() {
        if(!this.renderer || !this.sketchSectionPlane) return
        const normal = new THREE.Vector3().setFromMatrixColumn(this.sketchSectionPlane, 2).normalize()
        const cameraDirection = this.renderer.activeCamera.getWorldDirection(new THREE.Vector3())
        const dot = cameraDirection.dot(normal)
        if(Math.abs(dot) > 1e-6) this.sketchSectionSide = dot < 0
      },

      applyView: function(view) {
        const baseView = this.document.dirtyView || this.document.currentCameraView
        if(!view?.position || !view?.target) {
          if(this.document.previewView === view && baseView?.position && baseView?.target) {
            this.renderer.setView(baseView.position, baseView.target)
          }
          return
        }

        let position = view.position.clone()
        let target = view.target.clone()
        if(view.rotationOnly) {
          const currentPosition = baseView?.position?.clone() ||
            (this.renderer.cameraTarget || this.renderer.activeCamera.position).clone()
          const currentTarget = baseView?.target?.clone() ||
            (this.renderer.viewControlsTarget || this.renderer.viewControls.target).clone()
          const direction = position.sub(target)
          if(direction.lengthSq()) direction.normalize()
          target = currentTarget
          position = target.clone().addScaledVector(direction, currentPosition.distanceTo(currentTarget))
        }

        this.renderer.setView(position, target)
        if(view.zoomToFit) this.renderer.zoomToFit(null, false)

        if(!this.document.previewView && this.document.activeView === view) {
          this.document.cameraViewApplied(
            this.renderer.cameraTarget || this.renderer.activeCamera.position,
            this.renderer.viewControlsTarget || this.renderer.viewControls.target,
          )
        }
      },

      jointPicked: function(joint) {
        if(this.activeTool instanceof JointPickTool) this.activeTool.pick(joint)
      },

      updateSelectionTransformGizmo: function() {
        this.clearSelectionTransformGizmo()
        if(
          this.document.activeFeature ||
          this.document.creatingFeature ||
          this.document.activeSketch ||
          this.document.activeInteractivePanel
        ) return

        const selectedComponents = this.document.selection.items
          .map(item => item?.typename?.() == 'Component' ? item :
            item?.typename?.() == 'Solid' ? item.component : null
          )
          .filter(Boolean)
          .filter(component => component.parent)
          // Keep the latest occurrence so selecting a solid after its
          // component also makes that component the group anchor.
          .filter((component, index, all) =>
            all.map(other => other.id).lastIndexOf(component.id) == index
          )
        if(!selectedComponents.length) return

        const anchor = selectedComponents.at(-1)
        const components = selectedComponents
          .filter((component, _, all) => !all.some(other =>
            other != component && component.hasAncestor(other)
          ))

        const componentIds = components.map(component => component.id)
        const grouped = selectedComponents.length > 1
        // Keep the assembly solver's effector off every local rotation axis
        // so all three rotation rings can drive constrained components.
        const target = new THREE.Vector3(1, 2, 3).normalize().multiplyScalar(10)

        let gizmo
        gizmo = markRaw(new TransformGizmo(worldTransform(anchor), desiredWorld => {
          const tree = this.document.top()
          const current = componentIds.map(id => tree.findChild(id)).filter(Boolean)
          if(current.length != componentIds.length) return

          if(!grouped) {
            solveAssembly(tree, current[0], desiredWorld, 64, target)
            gizmo.setTransform(worldTransform(current[0]))
          } else {
            const start = this.selectionTransformStart
            if(!start) return
            const delta = desiredWorld.clone().multiply(start.gizmo.clone().invert())
            current.forEach(component => {
              const initialWorld = start.worlds.get(component.id)
              if(initialWorld) setWorldTransform(component, delta.clone().multiply(initialWorld))
            })
            gizmo.setTransform(desiredWorld)
          }
          this.renderer.render()
        }))
        const captureStart = () => {
          const tree = this.document.top()
          this.selectionTransformStart = {
            gizmo: gizmo.getTransform(),
            worlds: new Map(componentIds.map(id => {
              const current = tree.findChild(id)
              return [id, current && worldTransform(current)]
            })),
          }
        }
        gizmo.addEventListener('mouseDown', captureStart)
        captureStart()
        this.selectionTransformComponentIds = componentIds
        this.selectionTransformAnchorId = anchor.id
        this.selectionTransformGizmo = gizmo
        this.renderer.addGizmo(gizmo)
      },

      syncSelectionTransformGizmo: function() {
        const gizmo = this.selectionTransformGizmo
        if(!gizmo || gizmo.dragging) return
        const tree = this.document.top()
        const anchor = tree.findChild(this.selectionTransformAnchorId)
        if(anchor) gizmo.setTransform(worldTransform(anchor))
      },

      clearSelectionTransformGizmo: function() {
        if(!this.selectionTransformGizmo || !this.renderer) return
        this.renderer.removeGizmo(this.selectionTransformGizmo)
        this.selectionTransformGizmo = null
        this.selectionTransformComponentIds = null
        this.selectionTransformAnchorId = null
        this.selectionTransformStart = null
      },

      updateExportPreview: function() {
        this.clearExportPreview()
        const preview = this.exportPreview
        if(!preview) {
          this.renderer.render()
          return
        }

        const component = this.document.getComponent(preview.componentId)
        if(!component) {
          this.renderer.render()
          return
        }
        const maxDistance = Number(preview.maxDistance)
        const maxAngle = THREE.MathUtils.degToRad(Number(preview.maxAngle))
        const triangles = componentTriangles(
          component,
          Number.isFinite(maxDistance) && maxDistance > 0 ? maxDistance : 0.1,
          Number.isFinite(maxAngle) && maxAngle > 0 ? maxAngle : THREE.MathUtils.degToRad(10),
        )
        const positions = triangles.flatMap(triangle => triangle.flatMap(vertex => vertex.toArray()))
        const surfaceGeometry = new THREE.BufferGeometry()
        surfaceGeometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3))
        surfaceGeometry.computeVertexNormals()
        const surfaceMaterial = new THREE.MeshStandardMaterial({
          color: '#9eb5c5',
          side: THREE.DoubleSide,
          roughness: 0.8,
          metalness: 0,
          transparent: true,
          opacity: 0.18,
          polygonOffset: true,
          polygonOffsetFactor: 1,
          polygonOffsetUnits: 1,
        })
        const wireMaterial = new THREE.MeshBasicMaterial({
          color: '#d7e4ed',
          wireframe: true,
        })
        const surface = new THREE.Mesh(surfaceGeometry, surfaceMaterial)
        const wire = new THREE.Mesh(surfaceGeometry.clone(), wireMaterial)
        this.exportPreviewMesh = markRaw(new THREE.Group())
        this.exportPreviewMesh.add(surface, wire)
        this.exportPreviewMesh.applyMatrix4(worldTransform(component))
        this.renderer.add(this.exportPreviewMesh)
        this.renderer.render()
      },

      clearExportPreview: function() {
        if(!this.exportPreviewMesh) return
        const previewMaterials = new Set()
        this.exportPreviewMesh.traverse(child => {
          if(child.material) previewMaterials.add(child.material)
        })
        this.renderer.remove(this.exportPreviewMesh)
        previewMaterials.forEach(material => material.dispose())
        this.exportPreviewMesh = null
      },

      registerDocument: function(doc, oldDoc) {
        if(oldDoc) {
          oldDoc.off('force-view')
          oldDoc.off('look-at')
          oldDoc.off('regenerated', this.updateExportPreview)
        }
        doc.on('force-view', (view) => {
          this.renderer.setView(view.position, view.target)
          doc.cameraViewApplied(view.position, view.target)
        })
        doc.on('look-at', (plane) => setTimeout(() => {
          this.renderer.lookAt(plane)
          const sketch = this.document.activeSketch
          if(sketch && sketch.elements.length) this.zoomToFit(sketch.elements)
        }))
        doc.on('regenerated', this.updateExportPreview)
      },

      getMouseCoords: function(e) {
        const rect = this.$refs.canvas.getBoundingClientRect()
        return new THREE.Vector2(e.clientX - rect.left, e.clientY - rect.top)
      },

      click: function(e) {
        if(e.shiftKey || e.altKey || this.document.activeSketch) return
        if(this.activeTool instanceof ManipulationTool) {
          const coords = this.getMouseCoords(e)
          this.pointerCoords = coords
          this.activeTool.canvasClick(coords, e)
        }
      },

      doubleClick: function(e) {
        const coords = this.getMouseCoords(e)
        if(e.shiftKey || e.altKey) {
          this.commitCameraPreview()
          this.renderer.setPivot(coords)
        } else if(this.activeTool instanceof ManipulationTool && !this.document.activeSketch) {
          this.activeTool.doubleClick(coords, e)
        }
      },

      mouseUp: function(e) {
        if(this.renderer.isGizmoDragging()) return
        if(e.altKey) {
          this.pointerCancel()
          return
        }
        if(this.isPanning) {
          this.isPanning = false
          this.activeHandle = null
          this.activeDimension = null
          this.snapper.reset()
          return
        }
        const [vec, coords] = this.snap(e)
        const draggedHandle = this.activeHandle
        this.activeTool?.mouseUp(vec, coords)
        this.activeHandle = null
        this.activeDimension = null
        this.snapper.reset()
        this.updateRegions(false, draggedHandle)
      },

      pointerCancel: function(e) {
        // Capture is released after pointerup, before the native click. The
        // completed gesture must remain available for click-count handling.
        if(e?.type == 'lostpointercapture' && !this.activeTool?.lastCoords) return
        const draggedHandle = this.activeHandle
        this.activeTool?.cancelPointer?.()
        this.isPanning = false
        this.activeHandle = null
        this.activeDimension = null
        this.snapper.reset()
        this.updateRegions(false, draggedHandle)
      },

      updateRegions: function(force, solveHandle) {
        if(!this.regionsDirty && !force) return
        this.updateSketch()
        if(this.document.activeSketch) {
          // Mark the release so Vue refreshes constraint status and profiles
          // without applying another solution to the accepted drag geometry.
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
        // The pointer event is the authoritative modifier state for this drag.
        // This also updates controls recreated while Alt was already held.
        this.setOrbiting(e.altKey)
        if(e.button == 0 && this.renderer.isGizmoHovered()) {
          e.preventDefault()
          return
        }
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
        if(e.pointerId !== undefined && e.currentTarget?.setPointerCapture) {
          try { e.currentTarget.setPointerCapture(e.pointerId) } catch(_) {}
        }
        const [vec, coords] = this.snap(e)
        if(vec || this.activeTool.probesSurface) this.activeTool.mouseDown(vec, coords, e)
      },

      setOrbiting: function(pressed) {
        this.renderer.viewControls.enableRotate = pressed
        this.isOrbiting = pressed
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

      mouseWheel: function(e) {
        this.isZooming = true
        this.renderer.setPickingEnabled(false)
        clearTimeout(this.zoomEndTimer)
        this.zoomEndTimer = setTimeout(() => {
          this.isZooming = false
          this.zoomEndTimer = null
          this.renderer.setPickingEnabled(true)
          // Refresh hover and picker state once, at the camera's settled pose.
          this.mouseMove(e)
        }, 60)
      },

      forwardWheelToCanvas: function(e) {
        // Sketch handles live in an HTML overlay, while OrbitControls listens
        // for wheel input on the canvas. Re-dispatch the wheel gesture there
        // so zoom works without making the handles ignore pointer input.
        this.$refs.canvas.dispatchEvent(new WheelEvent(e.type, {
          bubbles: false,
          cancelable: true,
          composed: true,
          view: window,
          detail: e.detail,
          screenX: e.screenX,
          screenY: e.screenY,
          clientX: e.clientX,
          clientY: e.clientY,
          ctrlKey: e.ctrlKey,
          shiftKey: e.shiftKey,
          altKey: e.altKey,
          metaKey: e.metaKey,
          button: e.button,
          buttons: e.buttons,
          relatedTarget: e.relatedTarget,
          deltaX: e.deltaX,
          deltaY: e.deltaY,
          deltaZ: e.deltaZ,
          deltaMode: e.deltaMode,
        }))
      },

      forwardCameraPointerDown: function(e) {
        const cameraGesture = e.button != 0 || e.shiftKey || e.ctrlKey || e.metaKey || e.altKey
        if(!cameraGesture) return

        // OrbitControls also owns camera drag gestures on the canvas. Forward
        // the real pointer id so it can capture the remainder of this gesture;
        // an unmodified primary click remains exclusively a handle drag.
        e.preventDefault()
        this.$refs.canvas.dispatchEvent(new PointerEvent(e.type, {
          bubbles: false,
          cancelable: true,
          composed: true,
          view: window,
          detail: e.detail,
          screenX: e.screenX,
          screenY: e.screenY,
          clientX: e.clientX,
          clientY: e.clientY,
          ctrlKey: e.ctrlKey,
          shiftKey: e.shiftKey,
          altKey: e.altKey,
          metaKey: e.metaKey,
          button: e.button,
          buttons: e.buttons,
          relatedTarget: e.relatedTarget,
          pointerId: e.pointerId,
          width: e.width,
          height: e.height,
          pressure: e.pressure,
          tangentialPressure: e.tangentialPressure,
          tiltX: e.tiltX,
          tiltY: e.tiltY,
          twist: e.twist,
          pointerType: e.pointerType,
          isPrimary: e.isPrimary,
        }))
      },

      mouseMove: function(e) {
        if(e.button != 0) return
        if(this.renderer.isGizmoDragging()) return
        if(this.renderer.viewInteracting) return
        if(this.isPanning) return
        if(this.isOrbiting) return
        if(this.isZooming) return
        if(e.altKey) return
        const [vec, coords] = this.snap(e)
        this.pointerCoords = coords
        this.drawingInputHidden = false
        if(this.pickingPath) this.pickingPath.target = coords
        if(vec || this.activeTool.probesSurface) this.activeTool.mouseMove(vec, coords)
        // At close zoom, background rays can miss the projection plane.
        // Clear hover even when there is no world position for the tool.
        else this.$emit('update:highlight', null)
      },

      mouseLeave: function() {
        this.thermalProbe = null
      },

      keyDown: function(key) {
        if(key == 'Delete' || key == 'Backspace') {
          // Delete Selection
          if(this.document.selection.items.length) {
            [...this.document.selection.items].forEach(item => {
              const type = item.typename()
              const isAssemblyJoint = (this.document.top().assemblyJoints || []).includes(item)
              if(type != 'Solid' && type != 'Component' && type != 'Face' && type != 'Edge' && !isAssemblyJoint) {
                this.deleteElement(item)
              }
            })
          }
        }
      },

      keyUp: function(key) {
        if(key == 'Alt') {
        } else if(key.toLowerCase() == 'o') {
          const projectionMode = this.projectionMode == 'orthographic'
            ? 'perspective'
            : 'orthographic'
          this.$emit('update:projectionMode', projectionMode)
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

      handlePick: function(pickerCoords, color, Tool, acceptsInput, references = []) {
        if(this.activeTool) this.activeTool.dispose()
        this.resetProxyInteraction()
        this.pickingPath = { target: null, color, origin: pickerCoords }
        this.pickerReferences = references
        this.renderer.setGizmosHidden(true)
        const tool = new Tool(this.document.activeComponent, this, (item) => {
          this.bus.emit('picked', item)
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

      clearSimulationFlow: function() {
        if(!this.simulationFlow) return
        this.renderer.scene.remove(this.simulationFlow)
        this.simulationFlow.dispose()
        this.simulationFlow = null
      },

      updateSimulationFlow: function() {
        if(!this.renderer) return
        this.clearSimulationFlow()
        const simulation = this.document.activeSimulation
        if(!['thermal', 'fluid'].includes(simulation?.mode) || !simulation.result) return this.renderer.render()
        const component = this.document.top().findChild(simulation.result.componentId)
        const group = markRaw(new SimulationFlow(simulation))
        group.applyMatrix4(worldTransform(component))
        this.simulationFlow = group
        this.renderer.scene.add(group)
        this.renderer.render()
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
        // Picker centers are window coordinates; the drawpad uses viewport coordinates.
        const rect = this.$refs.drawpad.getBoundingClientRect()
        const origin = { x: path.origin.x - rect.left, y: path.origin.y - rect.top }
        path.targetPos = path.target.isVector2 ? path.target : this.renderer.toScreen(path.target)
        path.data = this.buildPath(origin, path.targetPos)
      },

      buildPath: function(origin, pos) {
        const sign = this.document.activeFeature ? -1 : 1
        const dx = Math.min(25 + Math.abs(origin.x - pos.x) / 2.0, 200) * sign
        const dy = Math.abs(origin.y - pos.y) / 2.0 * sign
        if(this.document.activeSimulation) {
          return `M ${origin.x} ${origin.y} C ${origin.x + dx} ${origin.y} ${pos.x - dx} ${pos.y} ${pos.x} ${pos.y}`
        }
        return `M ${origin.x} ${origin.y} C ${origin.x} ${origin.y + dx} ${pos.x} ${pos.y - dy} ${pos.x} ${pos.y}`
      },

      activateTool: function(Tool) {
        if(this.activeTool) this.activeTool.dispose()
        this.resetProxyInteraction()
        this.pickingPath = null
        this.pickerReferences = []
        this.pickerType = null
        this.renderer.setGizmosHidden(false)
        this.snapper.reset()
        if(!Tool) return
        const tool = new Tool(this.document.activeComponent, this, this.document.activeSketch)
        this.$emit('update:active-tool', tool)
        this.$emit('update:highlight', null)
        // this.renderer.render()
      },

      restartTool: function() {
        this.activateTool(this.activeTool.constructor)
        this.updateRegions(true)
      },

      resetProxyInteraction: function() {
        // A proxy can disappear without receiving mouseleave (for example the
        // line tool's zero-length next segment). Never carry its handles or
        // dimensions into the next tool's picking state.
        this.activeHandle = null
        this.hoveredHandle = null
        this.activeDimension = null
        this.hoveredDimension = null
        this.selectionBox = null
      },

      deleteElement: function(elem) {
        elem.sketch.remove(elem)
        this.document.selection.delete(elem)
        this.updateRegions(true)
      },

      previewCamera: function(action) {
        this.cancelCameraUnpreview()
        if(!this.cameraPreview) {
          this.cameraPreview = {
            position: (this.renderer.cameraTarget || this.renderer.activeCamera.position).clone(),
            target: (this.renderer.viewControlsTarget || this.renderer.viewControls.target).clone(),
          }
        }
        action(this.cameraPreview)
      },

      commitCameraPreview: function() {
        this.cancelCameraUnpreview()
        this.cameraPreview = null
      },

      unpreviewCamera: function() {
        if(!this.cameraPreview) return
        this.cancelCameraUnpreview()
        // Pointer transitions between adjacent buttons dispatch mouseleave
        // before mouseenter. Waiting one frame lets the next preview replace
        // this one directly, without rendering an intermediate camera reset.
        this.cameraUnpreviewFrame = requestAnimationFrame(() => {
          this.cameraUnpreviewFrame = null
          if(!this.cameraPreview) return
          const view = this.cameraPreview
          this.cameraPreview = null
          this.renderer.setView(view.position, view.target)
        })
      },

      cancelCameraUnpreview: function() {
        if(this.cameraUnpreviewFrame === null) return
        cancelAnimationFrame(this.cameraUnpreviewFrame)
        this.cameraUnpreviewFrame = null
      },

      zoomToFit: function(objects=[], preview=false, view=null) {
        const solidFaces = objects.filter(sel => sel instanceof Solid ).flatMap(solid => solid.faces() )
        const rest = objects.filter(sel => sel instanceof Face || sel instanceof Edge || sel instanceof SketchElement )
        const meshes = solidFaces.concat(rest).map(obj => obj.mesh && obj.mesh() ).filter(Boolean)
        this.renderer.zoomToFit(meshes.length && meshes, !preview, view)
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
