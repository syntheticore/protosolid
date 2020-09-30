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
      path(v-for="path in paths" :d="path.data" stroke="red" stroke-width="2")
    transition-group.anchors(name="anchors" tag="ul")
      //- li(
      //-   v-for="anchor in snapAnchors"
      //-   :key="anchor.id"
      //-   :style="{top: anchor.pos.y + 'px', left: anchor.pos.x + 'px'}"
      //-   @click="anchorClicked(anchor)"
      //- )
      li(
        v-if="snapAnchor"
        :key="snapAnchor.id"
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
  $color = desaturate($highlight, 35%)
  .view-port
    position: relative
    overflow: hidden
    border-top: 1px solid $color * 0.375

  canvas
    display: block
    background: $color * 0.2
    background: radial-gradient(50% 150%, farthest-corner, $color * 0.35, $color * 0.2)

  .drawpad
    position: absolute
    left: 0
    top: 0
    pointer-events: none
    width: 100%
    height: 100%

  .handles, .anchors
    position: absolute
    left: 0
    right: 0
    top: 0
    bottom: 0
    pointer-events: none
    li
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

  .anchors li
    // pointer-events: auto
    &::after
      // opacity: 0
      // transform: scale(2)
      transform: scale(1)
      opacity: 1
      width:  calc(100% - 2px)
      height: calc(100% - 2px)
      border: 2px solid purple * 1.6
      // transition: all 0.2s
      animation-duration: 0.2s
      animation-name: slidein
    // &:hover
    //   &::after
    //     transform: scale(1)
    //     opacity: 1
    @keyframes slidein {
      from {
        opacity: 0
        transform: scale(2)
      }

      to {
        transform: scale(1)
        opacity: 1
      }
    }

  .handles li
    // cursor: move
    pointer-events: auto
    &:hover
      &::before
        background: $highlight * 1.6
    &:active
      &::before
        width:  5px
        height: 5px

  .anchors-enter-active
  .anchors-leave-active
    transition: all 0.2s
  .anchors-enter
  .anchors-leave-to
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

  import { ManipulationTool, SelectionTool, LineTool, SplineTool, CircleTool, ExtrudeTool } from './../tools.js'

  function getMouseCoords(e, canvas) {
    var coords = new THREE.Vector2()
    // var rect = e.target.getBoundingClientRect();
    // console.log(rect)
    // coords.x = (e.clientX - rect.left) / canvas.offsetWidth * 2 - 1
    // coords.y = - (e.clientY - rect.top) / canvas.offsetHeight * 2 + 1
    coords.x = (e.clientX - 0) / canvas.offsetWidth * 2 - 1
    coords.y = - (e.clientY - 39) / canvas.offsetHeight * 2 + 1
    return coords
  }

  var rendering = true
  var renderer, camera, mesh, pointMaterial

  const snapDistance = 10.5 // px

  let isDragging = false

  export default {
    name: 'ViewPort',

    props: {
      tree: Object,
      activeComponent: Object,
      activeTool: Object,
      selectedElement: Object,
      data: Object,
    },

    watch: {
      tree: function() {
        this.loadTree(this.tree, true)
      },

      activeTool: function() {
        // this.snapAnchors.length = 0
      },
    },

    data() {
      return {
        snapAnchor: null,
        handles: {},
        paths: [],
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
        antialias: window.devicePixelRatio <= 1.0,
        alpha: true
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
      var ground = new THREE.Mesh(groundGeo, new THREE.ShadowMaterial({opacity: 0.2}))
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

      this.viewControls.addEventListener('start', () => {
        this.transformControl.enabled = false
        this.isOrbiting = true
      })

      this.viewControls.addEventListener('end', () => {
        this.transformControl.enabled = true
        this.isOrbiting = false
      })

      this.lineMaterial = new LineMaterial({
        color: 'yellow',
        linewidth: 3,
        vertexColors: true,
        dashed: false
      })

      this.selectionLineMaterial = this.lineMaterial.clone()
      this.selectionLineMaterial.color.set('red')

      this.highlightLineMaterial = this.lineMaterial.clone()
      this.highlightLineMaterial.color.set('white')

      this.$root.$on('pick-profile', (pickerCoords) => {
        this.$emit('activate-tool', new SelectionTool(this.activeComponent, this, (profile) => {
          this.$root.$emit('picked-profile', profile)
          this.$root.$emit('activate-toolname', 'Manipulate')
          if(!profile) return
          const target = new THREE.Vector3().fromArray(profile.get_handles()[0])
          this.paths.push({
            target,
            origin: pickerCoords,
            data: this.buildPath(pickerCoords, target),
          })
        }))
      })

      this.$root.$on('activate-toolname', this.activateTool)

      this.$root.$on('component-changed', this.componentChanged)

      this.$refs.canvas.addEventListener('keydown', (e) => {
        if(e.keyCode === 46 || e.keyCode === 8) { // Del / Backspace
          if(this.selectedElement) this.deleteElement(this.selectedElement)
        }
      });

      this.onWindowResize()
      window.addEventListener('resize', this.onWindowResize.bind(this), false)

      this.loadTree(this.tree, true)
      this.animate()
    },

    beforeDestroy: function() {
      rendering = false
      window.removeEventListener('resize', this.onWindowResize, false)
    },

    methods: {
      buildPath: function(origin, vec) {
        const pos = this.toScreen(vec)
        return `M ${origin.x} ${origin.y} C ${origin.x} ${origin.y + 150} ${pos.x} ${pos.y - 150} ${pos.x} ${pos.y}`
      },

      click: function(e) {
        this.viewControls.enabled = true
        const coords = getMouseCoords(e, this.$refs.canvas)
        if(coords.x != this.lastCoords.x || coords.y != this.lastCoords.y) return this.render()
        this.activeTool.click(coords)
      },

      doubleClick: function(e) {
        const coords = getMouseCoords(e, this.$refs.canvas)
        this.viewControlsTarget = this.fromScreen(coords)
        this.render()
      },

      mouseUp: function(e) {
        this.activeHandle = null
        this.snapAnchor = null
        isDragging = false
      },

      mouseDown: function(e) {
        if(e.button != 0) return
        const coords = getMouseCoords(e, this.$refs.canvas)
        const vec = this.snapVector(this.fromScreen(coords))
        if(vec) this.activeTool.mouseDown(vec, coords)
        const toolName = this.activeTool.constructor.name
        // if(toolName != 'ManipulationTool' && toolName != 'SelectionTool') this.viewControls.enabled = false
        this.lastCoords = coords
        isDragging = true
      },

      mouseMove: function(e) {
        if(e.button != 0) return
        if(this.isOrbiting) return
        const coords = getMouseCoords(e, this.$refs.canvas)
        let vec = this.fromScreen(coords)
        if(this.activeTool.constructor.name != 'ManipulationTool' || isDragging) vec = this.snapVector(vec)
        if(vec) this.activeTool.mouseMove(vec, coords)
      },

      snapVector: function(vec) {
        if(!vec) return
        const vecScreen = this.toScreen(vec)
        const sketchElements = this.activeComponent.get_sketch_elements()
        const snapPoints = sketchElements.flatMap(elem => {
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
        let closestDist = 99999
        let target
        snapPoints.forEach(p => {
          const dist = this.toScreen(p).distanceTo(vecScreen)
          if(dist < snapDistance && dist < closestDist) {
            closestDist = dist
            target = p
          }
        })
        if(target) {
          if(this.snapAnchor && this.snapAnchor.vec.equals(target)) return target
          this.snapAnchor = {
            type: 'snap',
            pos: this.toScreen(target),
            vec: target,
            id: '' + vec.x + vec.y + vec.z,
          }
        } else {
          this.snapAnchor = null
        }
        return target || vec
      },

      anchorClicked: function(anchor) {
        this.activeTool.mouseDown(anchor.vec, anchor.pos)
      },

      handleMouseDown: function(e, handle) {
        if(this.activeTool.constructor.name == 'ManipulationTool') {
          this.activeHandle = handle
        }
        this.mouseDown(e)
      },

      update_widgets: function() {
        if(this.snapAnchor) this.snapAnchor.pos = this.toScreen(this.snapAnchor.vec)

        for(let node_id in this.handles) {
          const node_handles = this.handles[node_id]
          for(let elem_id in node_handles) {
            const elem_handles = node_handles[elem_id]
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
        const tools = {
          Manipulate: ManipulationTool,
          Select: SelectionTool,
          Line: LineTool,
          Spline: SplineTool,
          Circle: CircleTool,
          Extrude: ExtrudeTool,
        }
        const tool = new tools[toolName](this.activeComponent, this)
        this.$emit('activate-tool', tool)
      },

      render: function() {
        renderer.render(this.scene, camera)
        this.update_widgets()
      },

      animate: function() {
        if(!rendering) return
        requestAnimationFrame(this.animate.bind(this))
        this.viewControls.update()
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
        const widthHalf = 0.5 * renderer.domElement.width
        const heightHalf = 0.5 * renderer.domElement.height
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
        this.unloadElement(elem, node)
        const vertices = elem.default_tesselation()
        var geometry = new LineGeometry()
        geometry.setPositions(vertices.flatMap(vertex => vertex))
        geometry.setColors(Array(vertices.length * 3).fill(1))
        var line = new Line2(geometry, this.lineMaterial)
        line.computeLineDistances()
        // line.scale.set(1, 1, 1)
        line.alcSelectable = true
        this.data[elem.id()] = line
        // line.component = node
        line.element = elem
        this.scene.add(line)

        const node_id = node.id()
        const elem_id = elem.id()
        this.handles[node_id] = this.handles[node_id] || {}
        this.handles[node_id][elem_id] = this.handles[node_id][elem_id] || []
        elem.get_handles().forEach((handle, i) => {
          handle = new THREE.Vector3().fromArray(handle)
          this.handles[node_id][elem_id].push({
            type: 'handle',
            pos: this.toScreen(handle),
            vec: handle,
            id: Math.random(),
            elem: elem,
            index: i,
          })
        })

        this.data[node_id].cachedElements = this.data[node_id].cachedElements || []
        this.data[node_id].cachedElements.push(elem)
      },

      unloadElement: function(elem, node) {
        this.scene.remove(this.data[elem.id()])
        const node_id = node.id()
        const cache = this.data[node_id]
        if(this.handles[node_id]) delete this.handles[node_id][elem.id()]
        this.handles = Object.assign({}, this.handles)
        const cachedElements = this.data[node_id].cachedElements
        if(cachedElements) this.data[node_id].cachedElements = cachedElements.filter(e => e != elem)
      },

      deleteElement: function(elem) {
        // this.unloadElement(elem, node)
        this.transformControl.detach()
        this.activeComponent.remove_element(elem.id())
        this.componentChanged(this.activeComponent)
        this.$emit('element-selected', null)
      },

      loadTree: function(node, recursive) {
        this.unloadTree(node, recursive)
        if(this.data[node.id()].hidden) return
        const elements = node.get_sketch_elements()
        elements.forEach(element => this.loadElement(element, node))
        // const regions = node.get_regions()
        // const splits = node.get_all_split()
        // console.log(splits.map(elem => elem.get_handles()))
        // console.log(regions)
        if(recursive) node.get_children().forEach(child => this.loadTree(child, true))
      },

      unloadTree: function(node, recursive) {
        const node_id = node.id()
        const cache = this.data[node_id]
        const cachedElements = this.data[node_id].cachedElements
        cachedElements && cachedElements.forEach(elem => this.unloadElement(elem, node))
        if(recursive) node.get_children().forEach(child => this.unloadTree(child, true))
      },

      componentChanged: function(comp, recursive) {
        this.loadTree(comp, recursive)
        this.render()
      },

      elementChanged: function(elem, comp) {
        this.loadElement(elem, comp)
        this.render()
      },
    }
  }
</script>
