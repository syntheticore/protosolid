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

  import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
  import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js'
  import { DragControls } from 'three/examples/jsm/controls/DragControls.js'
  import { HDRCubeTextureLoader } from 'three/examples/jsm/loaders/HDRCubeTextureLoader.js'
  import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
  import { Line2 } from 'three/examples/jsm/lines/Line2.js'

  import { Snapper } from './../snapping.js'
  import { SketchPlane } from './../sketchPlane.js'
  import { ShadowCatcher } from './../shadowCatcher.js'
  import { Materials } from './../materials.js'
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

  const frustumSize = 10

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
        this.render()
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
      THREE.Object3D.DefaultUp = new THREE.Vector3(0, 0, 1)

      // Renderer
      this.renderer = new THREE.WebGLRenderer({
        canvas: this.$el.querySelector('canvas'),
        // antialias: window.devicePixelRatio <= 1.0,
        antialias: true,
        alpha: true,
      })

      this.renderer.setPixelRatio(window.devicePixelRatio)
      this.renderer.outputEncoding = THREE.sRGBEncoding
      this.renderer.toneMapping = THREE.ACESFilmicToneMapping
      this.renderer.physicallyCorrectLights = true
      this.renderer.shadowMap.enabled = true
      this.renderer.shadowMap.autoUpdate = false
      // this.renderer.shadowMap.type = THREE.VSMShadowMap
      // this.renderer.toneMapping = THREE.ReinhardToneMapping
      // this.renderer.toneMapping = THREE.LinearToneMapping
      // this.renderer.toneMappingExposure = 1.2
      // this.renderer.setClearColor(0x263238)

      // Camera
      this.raycaster = new THREE.Raycaster()

      this.camera = new THREE.PerspectiveCamera(70, 1, 0.01, 10000)
      this.camera.position.set(6, 6, 4)

      this.cameraOrtho = new THREE.OrthographicCamera(-1, 1, 1, -1, -100, 10000)
      this.cameraOrtho.position.set(0, 0, 10)

      // Scene
      this.scene = new THREE.Scene()
      this.cameraOrtho.lookAt( this.scene.position )

      // this.scene.fog = new THREE.Fog(0xcce0ff, 0.1, 80)
      // this.scene.add(new THREE.AmbientLight(0x666666))
      var atmosphere = new THREE.HemisphereLight(0xffffbb, 0x080820, 1)
      this.scene.add(atmosphere)

      new HDRCubeTextureLoader()
      .setPath('textures/cubemap/')
      .setDataType(THREE.UnsignedByteType)
      .load(['px.hdr', 'nx.hdr', 'py.hdr', 'ny.hdr', 'pz.hdr', 'nz.hdr'], (texture) => {
        var pmremGenerator = new THREE.PMREMGenerator(this.renderer)
        pmremGenerator.compileCubemapShader()
        var envMap = pmremGenerator.fromCubemap(texture).texture
        this.scene.environment = envMap
        texture.dispose()
        pmremGenerator.dispose()
        this.render()
      })

      // Scene Objects
      this.world = new THREE.Object3D()
      this.scene.add(this.world)

      // Materials
      this.materials = new Materials()

      // Sketch Plane
      this.sketchPlane = new SketchPlane()
      this.scene.add(this.sketchPlane)

      // Shadow Catcher
      this.shadowCatcher = new ShadowCatcher(this.renderer, this.world)
      this.scene.add(this.shadowCatcher)

      // var torusGeometry = new THREE.TorusKnotBufferGeometry(1, 0.4, 170, 36)
      // const mesh = new THREE.Mesh(torusGeometry, this.materials.surface)
      // mesh.position.z = 1
      // mesh.castShadow = true
      // mesh.receiveShadow = true
      // // mesh.visible = false
      // this.scene.add(mesh)

      // Transform Controls
      this.transformControl = new TransformControls(this.camera, this.renderer.domElement)
      this.transformControl.space = 'world'
      // this.transformControl.translationSnap = 0.5
      // this.transformControl.rotationSnap = THREE.MathUtils.degToRad(10)
      // this.transformControl.setMode('rotate')
      // this.transformControl.addEventListener('change', () => this.render())
      this.transformControl.addEventListener('dragging-changed', (event) => {
        this.viewControls.enabled = !event.value
      })

      this.transformControl.addEventListener('objectChange', (event) => {
        this.$emit('change-pose')
        this.shadowCatcher.update()
        this.render()
      })

      this.scene.add(this.transformControl)

      // View Controls
      const setActiveCamera = (camera) => {
        if(this.viewControls) this.viewControls.dispose()
        this.viewControls = new OrbitControls(camera, this.renderer.domElement)
        this.viewControls.enableDamping = true
        this.viewControls.dampingFactor = 0.4
        this.viewControls.panSpeed = 1.0
        this.viewControls.keyPanSpeed = 12
        this.viewControls.zoomSpeed = 0.6
        this.viewControls.screenSpacePanning = true
        this.viewControls.rotateSpeed = 1.2

        this.viewControls.minPolarAngle = - Math.PI
        this.viewControls.maxPolarAngle = Math.PI * 2

        this.viewControls.addEventListener('change', () => {
          this.render()
          this.$emit('change-view')
        })

        let dampingTimeout

        this.viewControls.addEventListener('start', () => {
          this.transformControl.enabled = false
          this.isOrbiting = true
          clearTimeout(dampingTimeout)
          if(!this.isAnimating) {
            this.isAnimating = true
            this.animate()
          }
        })

        this.viewControls.addEventListener('end', () => {
          this.transformControl.enabled = true
          this.isOrbiting = false
          // Make sure we keep animating long enough for view damping to settle
          dampingTimeout = setTimeout(() => {
            this.isAnimating = false
          }, 500)
        })

        this.activeCamera = camera

        this.onWindowResize()
      }

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
          setActiveCamera(this.activeCamera == this.cameraOrtho ? this.camera : this.cameraOrtho)
        }
      })

      // Snapping
      this.snapper = new Snapper(this, (guides, anchor) => {
        this.guides = guides
        this.snapAnchor = anchor
      })

      // Init tree
      this.loadTree(this.document.tree, true)

      // Init viewport
      setActiveCamera(this.camera)

      // Window Resize
      this.onWindowResize()
      setTimeout(() => this.onWindowResize(), 500)
      this.$root.$on('resize', () => this.onWindowResize() )

      document._debug = {} || document._debug
      document._debug.viewport = this
    },

    beforeDestroy: function() {
      this.animating = false
      window.removeEventListener('resize', this.onWindowResize, false)
    },

    methods: {
      buildPath: function(origin, vec) {
        const pos = this.toScreen(vec)
        return `M ${origin.x} ${origin.y} C ${origin.x} ${origin.y + 150} ${pos.x} ${pos.y - 150} ${pos.x} ${pos.y}`
      },

      getMouseCoords: function(e) {
        var rect = this.$refs.canvas.getBoundingClientRect()
        return new THREE.Vector2(e.clientX, e.clientY - rect.top)
      },

      click: function(e) {
        this.viewControls.enabled = true
        const [vec, coords] = this.snap(e)
        if(coords.x != this.lastCoords.x || coords.y != this.lastCoords.y) return this.render()
        if(e.altKey) return
        this.activeTool.click(vec, coords)
      },

      doubleClick: function(e) {
        const coords = this.getMouseCoords(e)
        this.viewControlsTarget = this.fromScreen(coords)
        this.animate()
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
        if(this.isOrbiting) return
        if(e.altKey) return
        const [vec, coords] = this.snap(e)
        if(vec) this.activeTool.mouseMove(vec, coords)
      },

      snap: function(e) {
        const coords = this.getMouseCoords(e)
        let vec = this.fromScreen(coords)
        return this.snapper.snap(vec, coords)
      },

      updateWidgets: function() {
        // Update Snap Anchor
        if(this.snapAnchor) this.snapAnchor.pos = this.toScreen(this.snapAnchor.vec)
        // Update Handles
        for(let nodeId in this.handles) {
          const node_handles = this.handles[nodeId]
          for(let elemId in node_handles) {
            const elem_handles = node_handles[elemId]
            elem_handles.forEach(handle => {
              handle.pos = this.toScreen(handle.vec)
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
        this.render()
      },

      render: function() {
        this.renderer.render(this.scene, this.activeCamera)
        this.updateWidgets()
      },

      animate: function() {
        this.viewControls.update()
        if(this.isAnimating || this.viewControlsTarget) requestAnimationFrame(this.animate.bind(this))
        // Transition to manual view target
        if(!this.viewControlsTarget) return
        if(this.viewControlsTarget.clone().sub(this.viewControls.target).lengthSq() < 0.001) {
          this.viewControlsTarget = null
          return
        }
        this.viewControls.target.multiplyScalar(0.7).add(
          this.viewControlsTarget.clone().multiplyScalar(0.3)
        )
      },

      onWindowResize: function() {
        const canvas = this.renderer.domElement
        if(!canvas) return
        // Set canvas size
        const parent = canvas.parentElement
        const width = parent.offsetWidth
        const height = parent.offsetHeight
        this.renderer.setSize(width, height)
        this.$refs.drawpad.setAttribute('viewBox', '0 0 ' + width + ' ' + height)
        // Update camera projection
        const aspect = width / height
        if(this.activeCamera == this.camera) {
          this.camera.aspect = aspect
        } else {
          this.cameraOrtho.left = - 0.5 * frustumSize * aspect / 2
          this.cameraOrtho.right = 0.5 * frustumSize * aspect / 2
          this.cameraOrtho.top = frustumSize / 2
          this.cameraOrtho.bottom = - frustumSize / 2
        }
        this.activeCamera.updateProjectionMatrix()
        // Update line materials
        this.materials.line.resolution.set(width, height)
        this.materials.selectionLine.resolution.set(width, height)
        this.materials.highlightLine.resolution.set(width, height)
        this.materials.wire.resolution.set(width, height)
        this.render()
      },

      getCanvasCoords: function(mouseCoords) {
        const canvas = this.$refs.canvas
        return new THREE.Vector2(
          mouseCoords.x / canvas.offsetWidth * 2 - 1,
          -mouseCoords.y / canvas.offsetHeight * 2 + 1,
        )
      },

      hitTest: function(coords) {
        coords = this.getCanvasCoords(coords)
        this.raycaster.setFromCamera(coords, this.activeCamera)
        return this.raycaster.intersectObjects(this.scene.children, true)
      },

      fromScreen: function(coords) {
        const intersects = this.hitTest(coords).filter(obj => obj.object.alcProjectable)
        const hit = intersects[0]
        return hit && hit.point
      },

      toScreen: function(vec) {
        if(!this.activeCamera) return
        const widthHalf = 0.5 * this.renderer.domElement.width / window.devicePixelRatio
        const heightHalf = 0.5 * this.renderer.domElement.height / window.devicePixelRatio
        // this.camera.updateMatrixWorld()
        const vector = vec.clone().project(this.activeCamera)
        return new THREE.Vector2(
          (vector.x * widthHalf) + widthHalf,
          - (vector.y * heightHalf) + heightHalf
        )
      },

      objectsAtScreen: function(coords, types) {
        const intersects = this.hitTest(coords)
        const objects = Array.from(new Set(intersects.map(obj => obj.object)))
        return objects.filter(obj => types.some(t => obj.alcType == t))
      },

      loadElement: function(elem, node) {
        this.unloadElement(elem, node, this.document)
        const vertices = elem.tesselate()
        const line = this.convertLine(vertices, this.materials.line)
        line.alcType = 'curve'
        this.document.data[elem.id()] = line
        // line.component = node
        line.alcElement = elem
        this.world.add(line)

        const nodeId = node.id()
        const elemId = elem.id()
        this.handles[nodeId] = this.handles[nodeId] || {}
        this.handles[nodeId][elemId] = this.handles[nodeId][elemId] || []
        elem.get_handles().forEach((handle, i) => {
          handle = new THREE.Vector3().fromArray(handle)
          this.handles[nodeId][elemId].push({
            type: 'handle',
            pos: this.toScreen(handle),
            vec: handle,
            id: Math.random(),
            elem: elem,
            index: i,
          })
        })
        this.document.data[nodeId].curves.push(elem)
      },

      unloadElement: function(elem, node, document) {
        this.world.remove(document.data[elem.id()])
        const nodeId = node.id()
        if(this.handles[nodeId]) delete this.handles[nodeId][elem.id()]
        this.handles = Object.assign({}, this.handles)
        const curves = document.data[nodeId].curves
        document.data[nodeId].curves = curves.filter(e => e != elem)
      },

      deleteElement: function(elem) {
        this.transformControl.detach()
        this.activeComponent.get_sketch().remove_element(elem.id())
        this.componentChanged(this.activeComponent)
        this.$emit('element-selected', null)
      },

      loadTree: function(node, recursive) {
        const compData = this.document.data[node.id()]
        this.unloadTree(node, this.document, recursive)
        compData.regions.forEach(mesh => this.world.remove(mesh))
        if(compData.hidden) return
        let solids = node.get_solids()
        solids.forEach(solid => {
          const faces = solid.get_faces()
          faces.forEach(face => {
            const faceMesh = this.convertMesh(face.tesselate(), this.materials.surface)
            faceMesh.alcType = 'face'
            faceMesh.alcFace = face
            faceMesh.alcComponent = node
            faceMesh.alcProjectable = true
            faceMesh.castShadow = true
            faceMesh.receiveShadow = true
            this.world.add(faceMesh)
            compData.faces.push(faceMesh)
            // const normal = this.convertLine(face.get_normal(), this.materials.selectionLine)
            // this.world.add(normal)
          })
          const wireframe = solid.get_edges()
          compData.wireframe = wireframe.map(edge => {
            // edge = edge.map(vertex => vertex.map(dim => dim + Math.random() / 5))
            const line = this.convertLine(edge, this.materials.wire)
            this.world.add(line)
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
        nodeData.wireframe.forEach(edge => this.world.remove(edge))
        nodeData.faces.forEach(faceMesh => this.world.remove(faceMesh))
        if(recursive) node.get_children().forEach(child =>
          this.unloadTree(child, document, true)
        )
      },

      componentChanged: function(comp, recursive) {
        this.world.remove(this.previewMesh)
        this.loadTree(comp, recursive)
        this.paths = []
        this.shadowCatcher.update()
        this.render()
      },

      elementChanged: function(elem, comp) {
        this.updateRegions(comp)
        this.loadElement(elem, comp)
        this.render()
      },

      updateRegions: function(comp) {
        const compData = this.document.data[comp.id()]
        const regions = comp.get_sketch().get_regions(false)
        console.log('# regions: ', regions.length)
        compData.regions.forEach(mesh => this.world.remove(mesh))
        compData.regions = regions.map(region => {
          // let material = this.materials.region.clone()
          // material.color = new THREE.Color(Math.random(), Math.random(), Math.random())
          const mesh = this.convertMesh(region.get_mesh(), this.materials.region)
          mesh.alcType = 'region'
          mesh.alcRegion = region
          this.world.add(mesh)
          return mesh
        })
      },

      previewFeature: function(comp, bufferGeometry) {
        this.world.remove(this.previewMesh)
        this.previewMesh = this.convertMesh(bufferGeometry, this.materials.previewAddSurface);
        this.world.add(this.previewMesh)
        this.render()
      },

      convertLine: function(vertices, material) {
        const geometry = new LineGeometry()
        const positions = vertices.flat();
        geometry.setPositions(positions)
        // geometry.setColors(positions.map((pos, i) => i / positions.length ))
        geometry.setColors(Array(positions.length).fill(1))
        const line = new Line2(geometry, material)
        line.computeLineDistances()
        return line
      },

      convertLineBasic: function(line, material) {
        var geometry = new THREE.Geometry();
        geometry.vertices = line.map(vertex => new THREE.Vector3().fromArray(vertex))
        return new THREE.Line(geometry, material);
      },

      convertBufferGeometry: function(bufferGeometry, material) {
        const geometry = new THREE.BufferGeometry()
        const vertices = new Float32Array(bufferGeometry.position())
        const normals = new Float32Array(bufferGeometry.normal())
        // const uvs = new Float32Array(Array(vertices.length / 3 * 2).fill(1))
        geometry.setAttribute('position', new THREE.BufferAttribute(vertices, 3))
        geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3))
        // geometry.setAttribute('color', new THREE.BufferAttribute(vertices, 3) Array(vertices.length).fill(1))
        // geometry.setAttribute('uv', new THREE.BufferAttribute(uvs, 2))
        // geometry.computeFaceNormals()
        // geometry.computeVertexNormals()
        // geometry.normalizeNormals()
        return geometry
      },

      convertMesh: function(bufferGeometry, material) {
        const geometry = this.convertBufferGeometry(bufferGeometry)
        const mesh = new THREE.Mesh(geometry, material)
        return mesh
      },

      convertWireMesh: function(bufferGeometry, material) {
        const geometry = this.convertBufferGeometry(bufferGeometry)
        const wireframe = new THREE.WireframeGeometry(geometry);
        const line = new THREE.LineSegments(wireframe);
        return line
      },
    }
  }
</script>
