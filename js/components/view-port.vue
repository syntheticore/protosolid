<template lang="pug">
  .view-port
    canvas(
      ref="canvas"
      @click="click"
      @mousedown="mouseDown"
      @mousemove="mouseMove"
    )
    ul.widgets
      li(
        v-for="widget in widgets"
        @click="widgetClicked(widget)"
        @mouseenter="widgetHovered(widget)"
        :style="{top: widget.pos.y + 'px', left: widget.pos.x + 'px'}"
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
        background: #ffefae
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

  import { LineTool, SplineTool } from './../tools.js'

  function getMouseCoords(e, canvas) {
    var coords = new THREE.Vector2()
    var rect = e.target.getBoundingClientRect();
    coords.x = (e.clientX - rect.left) / canvas.offsetWidth * 2 - 1
    coords.y = - (e.clientY - rect.top) / canvas.offsetHeight * 2 + 1
    return coords
  }

  var rendering = true
  var renderer, controls, camera, mesh, lineMaterial, selectionLineMaterial, highlightLineMaterial, pointMaterial

  export default {
    name: 'ViewPort',

    props: {
      tree: Object,
      activeComponent: Object,
      activeTool: Object,
      selectedElement: Object,
    },

    watch: {
      tree: function() {
        this.loadTree(this.tree)
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

      // geometry = new THREE.BoxGeometry(0.2, 0.2, 0.2)
      var torusGeometry = new THREE.TorusKnotBufferGeometry(1, 0.4, 170, 36)
      var material = new THREE.MeshStandardMaterial({
        color: 'coral',
        roughness: 0,
        metalness: 0.1,
      })

      mesh = new THREE.Mesh(torusGeometry, material)
      mesh.castShadow = true
      mesh.receiveShadow = true
      // mesh.alcSelectable = true
      // mesh.visible = false
      this.scene.add(mesh)

      var groundGeo = new THREE.PlaneBufferGeometry(20, 20)
      groundGeo.rotateX(- Math.PI / 2)
      var ground = new THREE.Mesh(groundGeo, new THREE.ShadowMaterial({opacity: 0.2}))
      ground.receiveShadow = true
      ground.position.y = -1.85
      ground.alcProjectable = true
      this.scene.add(ground)

      var grid = new THREE.GridHelper(20, 20)
      grid.position.y = -1.8
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
      var transformControl = new TransformControls(camera, renderer.domElement)
      transformControl.space = 'world'
      // transformControl.translationSnap = 0.5
      // transformControl.rotationSnap = THREE.MathUtils.degToRad(10)
      // transformControl.setMode('rotate')
      // transformControl.addEventListener('change', () => this.render())
      transformControl.addEventListener('dragging-changed', (event) => {
        controls.enabled = !event.value
      })
      transformControl.addEventListener('objectChange', (event) => {
        this.$emit('change-pose')
        renderer.shadowMap.needsUpdate = true
        this.render()
      })
      this.scene.add(transformControl)
      transformControl.attach(mesh)

      // View Controls
      controls = new OrbitControls(camera, renderer.domElement)
      controls.enableDamping = true
      controls.dampingFactor = 0.25
      controls.panSpeed = 1.0
      controls.keyPanSpeed = 12
      controls.zoomSpeed = 0.4
      controls.screenSpacePanning = true
      controls.rotateSpeed = 1.2
      // controls.autoRotate = true

      controls.addEventListener('change', () => {
        this.render()
        this.$emit('change-view')
      })

      controls.addEventListener('start', () => {
        transformControl.enabled = false
        this.isOrbiting = true
      })

      controls.addEventListener('end', () => {
        transformControl.enabled = true
        this.isOrbiting = false
      })

      // lineMaterial = new THREE.LineBasicMaterial({ color: '#2590e1', linewidth: 2, fog: true })
      lineMaterial = new LineMaterial({
        color: 'yellow',
        linewidth: 4,
        vertexColors: true,
        dashed: false
      })

      selectionLineMaterial = lineMaterial.clone()
      selectionLineMaterial.color.set('red')

      highlightLineMaterial = lineMaterial.clone()
      highlightLineMaterial.color.set('white')

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
      //   transformControl.attach(event.object)
      // })

      window.addEventListener('resize', this.onWindowResize.bind(this), false)
      this.onWindowResize()
      this.animate()


      this.$root.$on('activate-toolname', (toolName) => {
        let tool
        switch(toolName) {
          case 'Line':
            tool = new LineTool(this.activeComponent, this)
            break;
          case 'Spline':
            tool = new SplineTool(this.activeComponent, this)
            break;
        }
        this.$emit('activate-tool', tool)
      })

      this.loadTree(this.tree)
    },

    beforeDestroy: function() {
      rendering = false
      window.removeEventListener('resize', this.onWindowResize, false)
    },

    methods: {
      updateWidgets: function() {
        // if(!this.renderer.handles) return
        // this.widgets.length = 0
        // this.renderer.handles.forEach((point, i) => {
        //   const vec = new THREE.Vector3().fromArray(point)
        //   const pos = this.renderer.toScreen(vec)
        //   this.$set(this.widgets, i, {pos, vec, type: 'vertex'})
        // })
      },

      click: function(e) {
        controls.enabled = true
        const coords = getMouseCoords(e, this.$refs.canvas)
        if(coords.x != this.lastCoords.x || coords.y != this.lastCoords.y) return
        const object = this.objectsAtScreen(coords, 'alcSelectable')[0]
        if(object) return
        if(this.selectedElement) this.selectedElement.three.material = lineMaterial
        this.$emit('element-selected', null)
        this.render()
      },

      mouseDown: function(e) {
        if(e.button != 0) return
        const coords = getMouseCoords(e, this.$refs.canvas)
        if(this.activeTool) {
          const vec = this.fromScreen(coords)
          if(this.activeTool && vec) this.activeTool.mouseDown(vec)
        } else {
          const object = this.objectsAtScreen(coords, 'alcSelectable')[0]
          if(object) {
            if(this.selectedElement) this.selectedElement.three.material = lineMaterial
            object.material = selectionLineMaterial
            this.$emit('element-selected', object.element)
            controls.enabled = false
            this.render()
          }
        }
        this.lastCoords = coords
      },

      mouseMove: function(e) {
        if(e.button != 0) return
        if(this.isOrbiting) return
        const coords = getMouseCoords(e, this.$refs.canvas)
        if(this.activeTool) {
          const vec = this.fromScreen(coords)
          if(vec) this.activeTool.mouseMove(vec)
        } else {
          const object = this.objectsAtScreen(coords, 'alcSelectable')[0]
          if(object) {
            const oldMaterial = object.material
            object.material = highlightLineMaterial
            this.render()
            object.material = oldMaterial
          } else {
            this.render()
          }
        }
      },

      widgetClicked: function(widget) {
        if(this.activeTool) this.activeTool.mouseDown(widget.vec)
      },

      widgetHovered: function(widget) {
        if(this.activeTool) this.activeTool.mouseMove(widget.vec)
      },

      render: function() {
        this.planes && this.planes.forEach((plane, i) => {
          var po = this.planeObjects[i]
          plane.coplanarPoint(po.position)
          po.lookAt(
            po.position.x - plane.normal.x,
            po.position.y - plane.normal.y,
            po.position.z - plane.normal.z,
          )
        })
        renderer.render(this.scene, camera)
        this.updateWidgets()
      },

      animate: function() {
        if(!rendering) return
        requestAnimationFrame(this.animate.bind(this))
        controls.update()
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
        lineMaterial.resolution.set(parent.offsetWidth, parent.offsetHeight)
        selectionLineMaterial.resolution.set(parent.offsetWidth, parent.offsetHeight)
        highlightLineMaterial.resolution.set(parent.offsetWidth, parent.offsetHeight)
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
        return {
          x: (vector.x * widthHalf) + widthHalf,
          y: - (vector.y * heightHalf) + heightHalf,
        }
      },

      objectsAtScreen: function(coords, filter) {
        this.raycaster.setFromCamera(coords, camera)
        const intersects = this.raycaster.intersectObjects(this.scene.children)
        const objects = Array.from(new Set(intersects.map(obj => obj.object)))
        return objects.filter(obj => obj[filter])
      },

      loadElement: function(element, node) {
        this.scene.remove(element.three)
        node.threeObjects.filter(obj => obj !== element.three)
        const vertices = element.default_tesselation().map(vertex => new THREE.Vector3().fromArray(vertex))
        var geometry = new LineGeometry()
        geometry.setPositions(vertices.flatMap(vertex => vertex.toArray()))
        geometry.setColors(Array(vertices.length * 3).fill(1))
        var line = new Line2(geometry, lineMaterial)
        line.computeLineDistances()
        // line.scale.set(1, 1, 1)
        line.alcSelectable = true
        node.threeObjects.push(line)
        element.three = line
        line.component = node
        line.element = element
        this.scene.add(line)
      },

      loadTree: function(node, recursive) {
        console.log('loadTree')
        node.threeObjects = node.threeObjects || []
        node.threeObjects.forEach(elem => this.scene.remove(elem))
        node.threeObjects.length = 0
        const elements = node.get_sketch_elements()
        // this.handles = elements.flatMap(seg => seg.get_handles())
        elements.forEach(element => this.loadElement(element, node))
        if(recursive) node.get_children().each(child => this.loadTree(child, true, true))
      },

      componentChanged: function(comp) {
        this.loadTree(comp)
        this.render()
      },

      elementChanged: function(elem, comp) {
        this.loadElement(elem, comp)
        this.render()
      },
    }
  }
</script>
