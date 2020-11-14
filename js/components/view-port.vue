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
        @click="anchorClicked(snapAnchor)"
      )

    ul.handles
      li(
        v-for="handle in allHandles"
        :key="handle.id"
        :style="{top: handle.pos.y + 'px', left: handle.pos.x + 'px'}"
        @mouseup="mouseUp"
        @mousedown="handleMouseDown($event, handle)"
        @mousemove="mouseMove"
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
  import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js'
  import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
  import { Line2 } from 'three/examples/jsm/lines/Line2.js'

  import { ManipulationTool, ObjectSelectionTool, ProfileSelectionTool, LineTool, SplineTool, CircleTool } from './../tools.js'

  const snapDistance = 10.5 // px
  const maxSnapReferences = 5

  var renderer, camera, mesh, pointMaterial

  function getCanvasCoords(mouseCoords, canvas) {
    return new THREE.Vector2(
      mouseCoords.x / canvas.offsetWidth * 2 - 1,
      -mouseCoords.y / canvas.offsetHeight * 2 + 1
    )
  }

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
      renderer = new THREE.WebGLRenderer({
        canvas: this.$el.querySelector('canvas'),
        // antialias: window.devicePixelRatio <= 1.0,
        antialias: true,
        alpha: true,
      })

      renderer.setPixelRatio(window.devicePixelRatio)
      renderer.outputEncoding = THREE.sRGBEncoding
      renderer.physicallyCorrectLights = true
      renderer.shadowMap.enabled = true
      renderer.shadowMap.autoUpdate = false
      renderer.shadowMap.needsUpdate = true
      // renderer.shadowMap.type = THREE.VSMShadowMap
      // renderer.toneMapping = THREE.ReinhardToneMapping
      // renderer.toneMapping = THREE.LinearToneMapping
      renderer.toneMapping = THREE.ACESFilmicToneMapping
      // renderer.toneMappingExposure = 1.2
      // renderer.setClearColor(0x263238)

      this.raycaster = new THREE.Raycaster()

      camera = new THREE.PerspectiveCamera(70, 1, 0.01, 10000)
      camera.position.set(3, 2, 3)

      this.scene = new THREE.Scene()
      // this.scene.fog = new THREE.Fog(0xcce0ff, 0.1, 20)
      // this.scene.add(new THREE.AmbientLight(0x666666))
      var sun = new THREE.DirectionalLight(0xdfebff, 1)
      sun.position.set(0, 100, 0)
      sun.castShadow = true
      sun.shadow.mapSize.width = 4096
      sun.shadow.mapSize.height = 4096
      let shadowFrustum = 20 / 2
      sun.shadow.camera = new THREE.OrthographicCamera(-shadowFrustum, shadowFrustum, shadowFrustum, -shadowFrustum, 1, 200)
      this.scene.add(sun)

      var light = new THREE.HemisphereLight(0xffffbb, 0x080820, 1)
      this.scene.add(light)

      // var torusGeometry = new THREE.TorusKnotBufferGeometry(1, 0.4, 170, 36)
      // var material = new THREE.MeshStandardMaterial({
      //   color: 'coral',
      //   roughness: 0,
      //   metalness: 0.1,
      // })

      // mesh = new THREE.Mesh(torusGeometry, material)
      // mesh.position.y = 1.8
      // mesh.castShadow = true
      // mesh.receiveShadow = true
      // // mesh.alcSelectable = true
      // // mesh.visible = false
      // this.scene.add(mesh)

      var groundGeo = new THREE.PlaneBufferGeometry(20, 20)
      groundGeo.rotateX(- Math.PI / 2)
      var ground = new THREE.Mesh(groundGeo, new THREE.ShadowMaterial({opacity: 0.2, side: THREE.DoubleSide}))
      ground.receiveShadow = true
      ground.position.y = -0.01
      ground.alcProjectable = true
      this.scene.add(ground)

      var grid = new THREE.GridHelper(20, 20)
      grid.material.opacity = 0.1
      grid.material.transparent = true
      this.scene.add(grid)

      var pmremGenerator = new THREE.PMREMGenerator(renderer)
      pmremGenerator.compileCubemapShader()

      new HDRCubeTextureLoader()
      .setPath('textures/cubemap/')
      .setDataType(THREE.UnsignedByteType)
      .load(['px.hdr', 'nx.hdr', 'py.hdr', 'ny.hdr', 'pz.hdr', 'nz.hdr'], (texture) => {
        var envMap = pmremGenerator.fromCubemap(texture).texture
        this.scene.environment = envMap
        texture.dispose()
        pmremGenerator.dispose()
        this.render()
      })

      // Transform Controls
      this.transformControl = new TransformControls(camera, renderer.domElement)
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
        renderer.shadowMap.needsUpdate = true
        this.render()
      })

      this.scene.add(this.transformControl)

      // View Controls
      this.viewControls = new OrbitControls(camera, renderer.domElement)
      this.viewControls.enableDamping = true
      this.viewControls.dampingFactor = 0.4
      this.viewControls.panSpeed = 1.0
      this.viewControls.keyPanSpeed = 12
      this.viewControls.zoomSpeed = 0.6
      this.viewControls.screenSpacePanning = true
      this.viewControls.rotateSpeed = 1.2

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

      this.lineMaterial = new LineMaterial({
        color: 'yellow',
        linewidth: 3,
        vertexColors: true,
        dashed: false
      })

      this.selectionLineMaterial = this.lineMaterial.clone()
      this.selectionLineMaterial.color.set('#0070ff')

      this.highlightLineMaterial = this.lineMaterial.clone()
      this.highlightLineMaterial.color.set('#2590e1')

      this.highlightRegionMaterial = new THREE.MeshBasicMaterial({
        color: new THREE.Color('blue'),
        side: THREE.DoubleSide,
        transparent: true,
        opacity: 0.5,
      })

      const handlePick = (pickerCoords, color, tool) => {
        this.$emit('activate-tool', new tool(this.activeComponent, this, (item, center) => {
          this.$root.$emit('picked', item)
          this.$root.$emit('activate-toolname', 'Manipulate')
          if(!item) return
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

      this.$refs.canvas.addEventListener('keydown', (e) => {
        if(e.keyCode == 46 || e.keyCode == 8) { // Del / Backspace
          if(this.selectedElement) this.deleteElement(this.selectedElement)
        } else if(e.keyCode == 18) {
          this.altPressed = true
          // this.guides = []
        }
      })

      this.$refs.canvas.addEventListener('keyup', (e) => {
        if(e.keyCode == 18) {
          this.altPressed = false
        }
      })

      this.lastSnaps = []

      this.onWindowResize()
      setTimeout(() => this.onWindowResize(), 500)
      this.$root.$on('resize', () => this.onWindowResize() )

      this.loadTree(this.document.tree, true)
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
        const coords = getCanvasCoords(this.getMouseCoords(e), this.$refs.canvas)
        if(coords.x != this.lastCoords.x || coords.y != this.lastCoords.y) return this.render()
        if(this.altPressed) return
        this.activeTool.click(coords)
      },

      doubleClick: function(e) {
        const coords = getCanvasCoords(this.getMouseCoords(e), this.$refs.canvas)
        this.viewControlsTarget = this.fromScreen(coords)
        this.animate()
      },

      mouseUp: function(e) {
        this.activeHandle = null
        this.snapAnchor = null
        this.guides = []
        this.isDragging = false
      },

      mouseDown: function(e) {
        if(e.button != 0) return
        if(this.altPressed) return
        const [vec, coords, canvasCoords] = this.snap(e)
        if(vec) this.activeTool.mouseDown(vec, canvasCoords)
        // if(toolName != 'ManipulationTool' && this.activeTool.constructor != ObjectSelectionTool) this.viewControls.enabled = false
        this.lastCoords = canvasCoords
      },

      anchorClicked: function(anchor) {
        this.activeTool.mouseDown(anchor.vec, anchor.pos)
      },

      handleMouseDown: function(e, handle) {
        if(this.activeTool.constructor == ManipulationTool) {
          this.lastSnaps = []
          this.activeHandle = handle
          this.isDragging = true
        }
        this.mouseDown(e)
      },

      mouseMove: function(e) {
        if(e.button != 0) return
        if(this.isOrbiting) return
        if(this.altPressed) return
        const [vec, coords, canvasCoords] = this.snap(e)
        if(vec) this.activeTool.mouseMove(vec, canvasCoords)
      },

      snap: function(e) {
        const coords = this.getMouseCoords(e)
        const canvasCoords = getCanvasCoords(coords, this.$refs.canvas)
        let vec = this.fromScreen(canvasCoords)
        this.guides = []
        this.catchSnapPoints(coords)
        if(this.activeTool.constructor != ManipulationTool || this.isDragging) vec = this.snapToGuides(vec) || vec
        return [vec, coords, canvasCoords]
      },

      getSnapPoints: function() {
        const sketchElements = this.activeComponent.get_sketch_elements()
        return sketchElements.flatMap(elem => {
          let points = elem.get_snap_points().map(p => new THREE.Vector3().fromArray(p))
          // Filter out last point of the sketch element actively being drawn
          if(elem == sketchElements.slice(-1)[0]) {
            const handles = elem.get_handles()
            const lastHandle = new THREE.Vector3().fromArray(handles[handles.length - 1])
            points = points.filter(p => !p.equals(lastHandle))
          }
          // Filter out handle actively being dragged
          if(this.activeHandle && elem.id() == this.activeHandle.elem.id()) {
            const handlePoint = new THREE.Vector3().fromArray(this.activeHandle.elem.get_handles()[this.activeHandle.index])
            points = points.filter(p => !p.equals(handlePoint))
          }
          return points
        })
      },

      catchSnapPoints: function(coords) {
        const snapPoints = this.getSnapPoints()
        let closestDist = 99999
        let target
        snapPoints.forEach(p => {
          const dist = this.toScreen(p).distanceTo(coords)
          if(dist < snapDistance && dist < closestDist) {
            closestDist = dist
            target = p
          }
        })
        if(!target) return
        if(!(this.lastSnaps[0] && this.lastSnaps[0].equals(target))) {
          this.lastSnaps.unshift(target)
          if(this.lastSnaps.length >= maxSnapReferences) this.lastSnaps.pop()
        }
      },

      snapToGuides: function(vec) {
        if(!vec) return
        let snapX
        this.lastSnaps.some(snap => {
          if(Math.abs(vec.x - snap.x) < 0.1) {
            snapX = snap
            return true
          }
        })
        let snapZ
        this.lastSnaps.some(snap => {
          if(Math.abs(vec.z - snap.z) < 0.1) {
            snapZ = snap
            return true
          }
        })
        const snapVec = new THREE.Vector3(snapX ? snapX.x : vec.x, vec.y, snapZ ? snapZ.z : vec.z)
        const screenSnapVec = this.toScreen(snapVec)
        if(snapX) {
          const start = this.toScreen(snapX)
          this.guides.push({
            id: 'v' + start.x + start.y,
            start,
            end: screenSnapVec,
          })
        }
        if(snapZ) {
          const start = this.toScreen(snapZ)
          this.guides.push({
            id: 'h' + start.x + start.y,
            start,
            end: screenSnapVec,
          })
        }
        if(snapX && snapZ) {
          if(this.snapAnchor && this.snapAnchor.vec.equals(snapVec)) return snapVec
          this.snapAnchor = {
            type: 'snap',
            pos: this.toScreen(snapVec),
            vec: snapVec,
            id: '' + snapVec.x + snapVec.y + snapVec.z,
          }
          return snapVec
        } else {
          this.snapAnchor = null
        }
        if(snapX || snapZ) return snapVec
      },

      updateWidgets: function() {
        if(this.snapAnchor) this.snapAnchor.pos = this.toScreen(this.snapAnchor.vec)

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

        this.paths.forEach((path, i) => {
          path.data = this.buildPath(path.origin, path.target)
          this.$set(this.paths, i, path)
        })
      },

      activateTool: function(toolName) {
        if(this.activeTool) this.activeTool.dispose()
        this.lastSnaps = []
        const tools = {
          Manipulate: ManipulationTool,
          Line: LineTool,
          Spline: SplineTool,
          Circle: CircleTool,
        }
        const tool = new tools[toolName](this.activeComponent, this)
        this.$emit('activate-tool', tool)
        this.render()
      },

      render: function() {
        renderer.render(this.scene, camera)
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
        this.viewControls.target.multiplyScalar(0.7).add(this.viewControlsTarget.clone().multiplyScalar(0.3))
      },

      onWindowResize: function() {
        const canvas = renderer.domElement
        if(!canvas) return
        const parent = canvas.parentElement
        const width = parent.offsetWidth
        const height = parent.offsetHeight
        renderer.setSize(width, height)
        camera.aspect = width / height
        camera.updateProjectionMatrix()
        this.lineMaterial.resolution.set(width, height)
        this.selectionLineMaterial.resolution.set(width, height)
        this.highlightLineMaterial.resolution.set(width, height)
        this.render()
        this.$refs.drawpad.setAttribute('viewBox', '0 0 ' + width + ' ' + height)
      },

      fromScreen: function(coords) {
        this.raycaster.setFromCamera(coords, camera)
        const intersects = this.raycaster.intersectObjects(this.scene.children)
        const hit = intersects.filter(obj => obj.object.alcProjectable)[0]
        return hit && hit.point
      },

      toScreen: function(vec) {
        const widthHalf = 0.5 * renderer.domElement.width / window.devicePixelRatio
        const heightHalf = 0.5 * renderer.domElement.height / window.devicePixelRatio
        // camera.updateMatrixWorld()
        const vector = vec.clone().project(camera)
        return new THREE.Vector2(
          (vector.x * widthHalf) + widthHalf,
          - (vector.y * heightHalf) + heightHalf
        )
      },

      objectsAtScreen: function(coords, filter) {
        this.raycaster.setFromCamera(coords, camera)
        const intersects = this.raycaster.intersectObjects(this.scene.children)
        const objects = Array.from(new Set(intersects.map(obj => obj.object)))
        return objects.filter(obj => obj[filter])
      },

      loadElement: function(elem, node) {
        this.unloadElement(elem, node, this.document)
        const vertices = elem.default_tesselation()
        const geometry = new LineGeometry()
        geometry.setPositions(vertices.flatMap(vertex => vertex))
        geometry.setColors(Array(vertices.length * 3).fill(1))
        const line = new Line2(geometry, this.lineMaterial)
        line.computeLineDistances()
        // line.scale.set(1, 1, 1)
        line.alcSelectable = true
        this.document.data[elem.id()] = line
        // line.component = node
        line.element = elem
        this.scene.add(line)

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

        this.document.data[nodeId].cachedElements = this.document.data[nodeId].cachedElements || []
        this.document.data[nodeId].cachedElements.push(elem)
      },

      unloadElement: function(elem, node, document) {
        this.scene.remove(document.data[elem.id()])
        const nodeId = node.id()
        const cache = document.data[nodeId]
        if(this.handles[nodeId]) delete this.handles[nodeId][elem.id()]
        this.handles = Object.assign({}, this.handles)
        const cachedElements = document.data[nodeId].cachedElements
        if(cachedElements) document.data[nodeId].cachedElements = cachedElements.filter(e => e != elem)
      },

      deleteElement: function(elem) {
        // this.unloadElement(elem, node, this.document)
        this.transformControl.detach()
        this.activeComponent.remove_element(elem.id())
        this.componentChanged(this.activeComponent)
        this.$emit('element-selected', null)
      },

      loadTree: function(node, recursive) {
        this.unloadTree(node, this.document, recursive)
        if(this.document.data[node.id()].hidden) return
        const elements = node.get_sketch_elements()
        elements.forEach(element => this.loadElement(element, node))
        if(recursive) node.get_children().forEach(child => this.loadTree(child, true))
      },

      unloadTree: function(node, document, recursive) {
        const nodeId = node.id()
        const cachedElements = document.data[nodeId].cachedElements
        cachedElements && cachedElements.forEach(elem => this.unloadElement(elem, node, document))
        if(recursive) node.get_children().forEach(child => this.unloadTree(child, document, true))
      },

      componentChanged: function(comp, recursive) {
        this.loadTree(comp, recursive)
        this.render()
      },

      elementChanged: function(elem, comp) {
        // const elements = comp.get_sketch_elements()
        // console.log(elements.map(elem => elem.get_handles()))
        // const regionInfo = comp.get_region_info()
        // console.log('regionInfo', {
        //   cut: regionInfo.cut,
        //   islands: regionInfo.islands,
        //   regions: regionInfo.regions,
        // })
        const regions = comp.get_regions(false)
        console.log(regions.length)
        this.regionMeshes = this.regionMeshes || []
        this.regionMeshes.forEach(mesh => this.scene.remove(mesh))
        this.regionMeshes = regions.map(region => {
          const geometry = new THREE.BufferGeometry()
          const vertices = new Float32Array(region.position())
          const normals = new Float32Array(Array(vertices.length / 3).fill([0,1,0]).flat())
          const uvs = new Float32Array(Array(vertices.length / 3 * 2).fill(1))
          // console.log(vertices)
          // console.log(normals)
          // console.log(uvs)
          geometry.setAttribute('position', new THREE.BufferAttribute(vertices, 3))
          geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3))
          // geometry.setAttribute('color', new THREE.BufferAttribute(vertices, 3) Array(vertices.length).fill(1))
          // geometry.setAttribute('uv', new THREE.BufferAttribute(uvs, 2))

          const material = new THREE.MeshBasicMaterial({color: new THREE.Color(Math.random(), Math.random(), Math.random())})
          material.side = THREE.DoubleSide
          const mesh = new THREE.Mesh(geometry, material)
          mesh.alcRegion = region
          this.scene.add(mesh)
          return mesh
        })
        this.loadElement(elem, comp)
        this.render()
      },
    }
  }
</script>
