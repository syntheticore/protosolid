<template lang="pug">
  .view-port
    canvas(
      ref="canvas"
      @click="click"
      @mousedown="mouseDown"
      @mousemove="mouseMove"
    )
    ul.widgets
    transition-group.widgets(name="widgets" tag="ul")
      li(
        v-for="widget in widgets"
        :key="widget.pos.x + '|' + widget.pos.y"
        @click="widgetClicked(widget)"
        :style="{top: widget.pos.y + 'px', left: widget.pos.x + 'px'}"
     )
        //- @mouseenter="widgetHovered(widget)"
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

  .widgets
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
      pointer-events: auto
      // cursor: move
      display: flex
      align-items: center
      justify-content: center
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
        width:  calc(100% - 4px)
        height: calc(100% - 4px)
        border-radius: 99px
        border: 2px solid $highlight * 1.6
        transition: all 0.2s
        opacity: 0
        transform: scale(1.5)
        pointer-events: none
      &:hover
        &::after
          transform: scale(1)
          opacity: 1

  .widgets-enter-active
  .widgets-leave-active
    transition: all 0.2s
  .widgets-enter
  .widgets-leave-to
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
    var rect = e.target.getBoundingClientRect();
    coords.x = (e.clientX - rect.left) / canvas.offsetWidth * 2 - 1
    coords.y = - (e.clientY - rect.top) / canvas.offsetHeight * 2 + 1
    return coords
  }

  var rendering = true
  var renderer, camera, mesh, pointMaterial

  const snapDistance = 10.5 // px

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
        this.widgets.length = 0
      },
    },

    data() {
      return {
        widgets: [],
      }
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
      // this.transformControl.attach(mesh)

      // View Controls
      this.viewControls = new OrbitControls(camera, renderer.domElement)
      this.viewControls.enableDamping = true
      this.viewControls.dampingFactor = 0.4
      this.viewControls.panSpeed = 1.0
      this.viewControls.keyPanSpeed = 12
      this.viewControls.zoomSpeed = 0.4
      this.viewControls.screenSpacePanning = true
      this.viewControls.rotateSpeed = 1.2
      // this.viewControls.autoRotate = true

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

      // lineMaterial = new THREE.LineBasicMaterial({ color: '#2590e1', linewidth: 2, fog: true })
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

      // pointMaterial = new THREE.PointsMaterial({
      //   color: 'yellow',
      //   size: 8,
      //   sizeAttenuation: false,
      //   map: new THREE.TextureLoader().load('textures/disc.png'),
      //   alphaTest: 0.01,
      //   transparent: true
      // })

      // var dragcontrols = new DragControls([mesh], camera, renderer.domElement)
      // dragcontrols.addEventListener('hoveron', function(event) {
      //   this.transformControl.attach(event.object)
      // })

      window.addEventListener('resize', this.onWindowResize.bind(this), false)
      this.onWindowResize()
      this.animate()

      this.$root.$on('activate-toolname', (toolName) => {
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
      })

      this.$root.$on('component-changed', this.componentChanged)

      window.addEventListener('keydown', (e) => {
        if(e.keyCode === 46 || e.keyCode === 8) { // Del / Backspace
          if(this.selectedElement) this.deleteElement(this.selectedElement)
        }
      });

      this.loadTree(this.tree, true)
    },

    beforeDestroy: function() {
      rendering = false
      window.removeEventListener('resize', this.onWindowResize, false)
    },

    methods: {
      click: function(e) {
        this.viewControls.enabled = true
        const coords = getMouseCoords(e, this.$refs.canvas)
        if(coords.x != this.lastCoords.x || coords.y != this.lastCoords.y) return this.render()
        this.activeTool.click(coords)
      },

      mouseDown: function(e) {
        if(e.button != 0) return
        const coords = getMouseCoords(e, this.$refs.canvas)
        const vec = this.snapVector(this.fromScreen(coords))
        if(vec) this.activeTool.mouseDown(vec, coords)
        if(this.activeTool.constructor.name != 'ManipulationTool') this.viewControls.enabled = false
        this.lastCoords = coords
      },

      mouseMove: function(e) {
        this.widgets.length = 0
        this.widgets = this.widgets.slice()
        if(e.button != 0) return
        if(this.isOrbiting) return
        const coords = getMouseCoords(e, this.$refs.canvas)
        const vec = this.snapVector(this.fromScreen(coords))
        if(vec) this.activeTool.mouseMove(vec, coords)
      },

      snapVector: function(vec) {
        this.widgets.length = 0
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
          this.widgets.push({
            type: 'vertex',
            pos: this.toScreen(target),
            vec: target,
          })
        }
        return target || vec
      },

      widgetClicked: function(widget) {
        this.activeTool.mouseDown(widget.vec, widget.pos)
      },

      // widgetHovered: function(widget) {
      //   this.activeTool.mouseMove(widget.vec, widget.pos)
      // },

      updateWidgets: function() {
        this.widgets.forEach((widget, i) => {
          widget.pos = this.toScreen(widget.vec)
          this.$set(this.widgets, i, widget)
        })
      },

      render: function() {
        renderer.render(this.scene, camera)
        this.updateWidgets()
      },

      animate: function() {
        if(!rendering) return
        requestAnimationFrame(this.animate.bind(this))
        this.viewControls.update()
        // mesh.rotation.x += 0.01
        // mesh.rotation.y += 0.01
        // renderer.shadowMap.needsUpdate = true
      },

      onWindowResize: function() {
        const canvas = renderer.domElement
        if(!canvas) return
        const parent = canvas.parentElement
        renderer.setSize(parent.offsetWidth, parent.offsetHeight)
        camera.aspect = parent.offsetWidth / parent.offsetHeight
        camera.updateProjectionMatrix()
        this.lineMaterial.resolution.set(parent.offsetWidth, parent.offsetHeight)
        this.selectionLineMaterial.resolution.set(parent.offsetWidth, parent.offsetHeight)
        this.highlightLineMaterial.resolution.set(parent.offsetWidth, parent.offsetHeight)
        this.render()
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
        this.data[node.id()].threeObjects.push(line)
        elem.three = line
        line.component = node
        line.element = elem
        this.scene.add(line)
      },

      unloadElement: function(elem, node) {
        this.scene.remove(elem.three)
        const cache = this.data[node.id()]
        cache.threeObjects = cache.threeObjects || []
        cache.threeObjects = cache.threeObjects.filter(obj => obj !== elem.three)
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
        if(recursive) node.get_children().forEach(child => this.loadTree(child, true))
      },

      unloadTree: function(node, recursive) {
        const cache = this.data[node.id()]
        cache.threeObjects = cache.threeObjects || []
        cache.threeObjects.forEach(obj => this.scene.remove(obj))
        cache.threeObjects.length = 0
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
