<template lang="pug">
  .view-port

    canvas(
      ref="canvas"
      @click="click"
      @dblclick="doubleClick"
      @mouseup="mouseUp"
      @mousedown="mouseDown"
      @mousemove="mouseMove"
    )

    svg.drawpad(ref="drawpad" viewBox="0 0 100 100" fill="transparent")
      path(v-for="path in paths" :d="path.data" :stroke="path.color")
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
  // $color = desaturate($highlight, 35%)
  .view-port
    position: relative
    overflow: hidden
    // border-top: 1px solid $color * 0.375
    border-top: 1px solid #2d3840

  canvas
    display: block
    // background: $color * 0.2
    // background: radial-gradient(50% 150%, farthest-corner, $color * 0.35, $color * 0.2)
    // background: #1c2127
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
      stroke-dasharray: 3, 7
    line
      stroke: white
    path
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
    SetPlaneTool
  } from './../tools.js'
  import { Renderer } from './../renderer.js'
  import { Snapper } from './../snapping.js'

  export default {
    name: 'ViewPort',

    props: {
      document: Object,
      activeComponent: Object,
      activeTool: Object,
      selectedElement: Object,
    },

    watch: {
      document: function(document, oldDocument) {
        this.unloadTree(oldDocument.tree, oldDocument, true)
        this.loadTree(document.tree, document, true)
        this.renderer.render()
      },
    },

    data() {
      return {
        snapAnchor: null,
        handles: {},
        paths: [],
        guides: [],
      }
    },

    computed: {
      allHandles: function() {
        return Object.values(this.handles).map(e => Object.values(e)).flat().flat()
      },
    },

    mounted: function() {
      // Renderer
      this.renderer = new Renderer(this.$el.querySelector('canvas'))
      this.renderer.on('render', () => this.updateWidgets() )
      this.renderer.on('change-view', () => this.$emit('change-view') )
      this.renderer.on('change-pose', () => this.$emit('change-pose') )

      // Events
      const handlePick = (pickerCoords, color, tool) => {
        this.$emit('activate-tool', new tool(this.activeComponent, this, (item, mesh) => {
          this.$root.$emit('picked', item)
          this.$root.$emit('activate-toolname', 'Manipulate')
          mesh.geometry.computeBoundingBox();
          const center = mesh.geometry.boundingBox.getCenter();
          this.paths.push({
            target: center,
            origin: pickerCoords,
            data: this.buildPath(pickerCoords, center),
            color,
          })
        }))
      }

      this.$root.$on('pick-profile', (pickerCoords, color) => {
        handlePick(pickerCoords, color, ProfileSelectionTool)
      })

      this.$root.$on('pick-curve', (pickerCoords, color) => {
        handlePick(pickerCoords, color, ObjectSelectionTool)
      })

      this.$root.$on('activate-toolname', this.activateTool)

      this.$root.$on('component-changed', this.componentChanged)

      this.$root.$on('preview-feature', this.previewFeature)

      // Key presses
      this.$refs.canvas.addEventListener('keydown', (e) => {
        if(e.keyCode == 46 || e.keyCode == 8) { // Del / Backspace
          if(this.selectedElement) this.deleteElement(this.selectedElement)
        } else if(e.keyCode == 18) { // alt
          // this.guides = []
        }
      })

      this.$refs.canvas.addEventListener('keyup', (e) => {
        if(e.keyCode == 18) { // alt
        } else if(e.keyCode == 79) { // o
          this.renderer.switchCamera()
        }
      })

      // Snapping
      this.snapper = new Snapper(this, (guides, anchor) => {
        this.guides = guides
        this.snapAnchor = anchor
      })

      // Init tree
      this.loadTree(this.document.tree, true)

      document._debug = {} || document._debug
      document._debug.viewport = this

      // Window Resize
      this.onWindowResize()
      setTimeout(() => this.onWindowResize(), 500)
      this.$root.$on('resize', this.onWindowResize)

      setInterval(() => {
        const info = this.renderer.renderer.info
        info.memory._programs = info.programs.length
        console.log(JSON.stringify(info.memory))
      }, 5 * 1000)
    },

    beforeDestroy: function() {
      this.$root.$off('resize', this.onWindowResize)
      this.renderer.dispose()
    },

    methods: {
      buildPath: function(origin, vec) {
        const pos = this.renderer.toScreen(vec)
        return `M ${origin.x} ${origin.y} C ${origin.x} ${origin.y + 150} ${pos.x} ${pos.y - 150} ${pos.x} ${pos.y}`
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
        this.renderer.animate()
      },

      mouseUp: function(e) {
        this.activeHandle = null
        const [vec, coords] = this.snap(e)
        if(vec) this.activeTool.mouseUp(vec, coords)
        this.snapper.reset()
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
        if(vec) this.activeTool.mouseMove(vec, coords)
      },

      snap: function(e) {
        const coords = this.getMouseCoords(e)
        let vec = this.renderer.fromScreen(coords)
        return this.snapper.snap(vec, coords)
      },

      updateWidgets: function() {
        // Update Snap Anchor
        if(this.snapAnchor) this.snapAnchor.pos = this.renderer.toScreen(this.snapAnchor.vec)
        // Update Handles
        for(let nodeId in this.handles) {
          const node_handles = this.handles[nodeId]
          for(let elemId in node_handles) {
            const elem_handles = node_handles[elemId]
            elem_handles.forEach(handle => {
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
      },

      activateTool: function(toolName) {
        if(this.activeTool) this.activeTool.dispose()
        this.snapper.reset()
        const tools = {
          'Set Plane': SetPlaneTool,
          Manipulate: ManipulationTool,
          Line: LineTool,
          Spline: SplineTool,
          Circle: CircleTool,
          Arc: ArcTool,
        }
        const tool = new tools[toolName](this.activeComponent, this)
        this.$emit('activate-tool', tool)
        this.renderer.render()
      },

      loadElement: function(elem, node) {
        this.unloadElement(elem, node, this.document)
        const vertices = elem.tesselate()
        const line = this.renderer.convertLine(vertices, this.renderer.materials.line)
        line.alcType = 'curve'
        this.document.data[elem.id()] = line
        // line.component = node
        line.alcElement = elem
        this.renderer.add(line)

        const nodeId = node.id()
        const elemId = elem.id()
        this.handles[nodeId] = this.handles[nodeId] || {}
        this.handles[nodeId][elemId] = this.handles[nodeId][elemId] || []
        elem.get_handles().forEach((handle, i) => {
          handle = new THREE.Vector3().fromArray(handle)
          this.handles[nodeId][elemId].push({
            type: 'handle',
            pos: this.renderer.toScreen(handle),
            vec: handle,
            id: Math.random(),
            elem: elem,
            index: i,
          })
        })
        this.document.data[nodeId].curves.push(elem)
      },

      unloadElement: function(elem, node, document) {
        this.renderer.remove(document.data[elem.id()])
        const nodeId = node.id()
        if(this.handles[nodeId]) delete this.handles[nodeId][elem.id()]
        this.handles = Object.assign({}, this.handles)
        const curves = document.data[nodeId].curves
        document.data[nodeId].curves = curves.filter(e => e != elem)
      },

      deleteElement: function(elem) {
        this.renderer.transformControl.detach()
        this.activeComponent.get_sketch().remove_element(elem.id())
        this.componentChanged(this.activeComponent)
        this.$emit('element-selected', null)
      },

      loadTree: function(node, recursive) {
        const compData = this.document.data[node.id()]
        this.unloadTree(node, this.document, recursive)
        compData.regions.forEach(mesh => this.renderer.remove(mesh))
        if(compData.hidden) return
        let solids = node.get_solids()
        solids.forEach(solid => {
          const faces = solid.get_faces()
          faces.forEach(face => {
            const faceMesh = this.renderer.convertMesh(
              face.tesselate(),
              this.renderer.materials.surface
            )
            faceMesh.alcType = 'face'
            faceMesh.alcFace = face
            faceMesh.alcComponent = node
            faceMesh.alcProjectable = true
            faceMesh.castShadow = true
            faceMesh.receiveShadow = true
            this.renderer.add(faceMesh)
            compData.faces.push(faceMesh)
            // const normal = this.convertLine(face.get_normal(), this.renderer.materials.selectionLine)
            // this.renderer.add(normal)
          })
          const wireframe = solid.get_edges()
          compData.wireframe = wireframe.map(edge => {
            // edge = edge.map(vertex => vertex.map(dim => dim + Math.random() / 5))
            const line = this.renderer.convertLine(edge, this.renderer.materials.wire)
            this.renderer.add(line)
            return line
          })
        })
        this.updateRegions(node)
        // Load sketch elements
        const elements = node.get_sketch().get_sketch_elements()
        elements.forEach(element => this.loadElement(element, node))
        if(recursive) node.get_children().forEach(child => this.loadTree(child, true))
      },

      unloadTree: function(node, document, recursive) {
        const nodeData = document.data[node.id()]
        nodeData.curves.forEach(elem => this.unloadElement(elem, node, document))
        nodeData.wireframe.forEach(edge => this.renderer.remove(edge))
        nodeData.faces.forEach(faceMesh => this.renderer.remove(faceMesh))
        if(recursive) node.get_children().forEach(child =>
          this.unloadTree(child, document, true)
        )
      },

      componentChanged: function(comp, recursive) {
        this.renderer.remove(this.previewMesh)
        this.loadTree(comp, recursive)
        this.paths = []
        this.renderer.shadowCatcher.update()
        this.renderer.render()
      },

      elementChanged: function(elem, comp) {
        this.updateRegions(comp)
        this.loadElement(elem, comp)
        this.renderer.render()
      },

      updateRegions: function(comp) {
        const compData = this.document.data[comp.id()]
        const regions = comp.get_sketch().get_regions(false)
        console.log('# regions: ', regions.length)
        compData.regions.forEach(mesh => this.renderer.remove(mesh))
        compData.regions = regions.map(region => {
          // let material = this.renderer.materials.region.clone()
          // material.color = new THREE.Color(Math.random(), Math.random(), Math.random())
          const mesh = this.renderer.convertMesh(
            region.get_mesh(),
            this.renderer.materials.region
          )
          mesh.alcType = 'region'
          mesh.alcRegion = region
          this.renderer.add(mesh)
          return mesh
        })
      },

      previewFeature: function(comp, bufferGeometry) {
        this.renderer.remove(this.previewMesh)
        this.previewMesh = this.renderer.convertMesh(
          bufferGeometry,
          this.renderer.materials.previewAddSurface
        );
        this.renderer.add(this.previewMesh)
        this.renderer.render()
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
