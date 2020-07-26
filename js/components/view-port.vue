<template lang="pug">
  .view-port
    canvas
    ul.widgets
      li(
        v-for="widget in widgets"
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
      size = 15px
      position: absolute
      // background: red
      width: size
      height: size
      margin-left: -(size / 2)
      margin-top: -(size / 2)
      pointer-events: auto
      border-radius: 99px
      border: 2px solid #85de85
      cursor: move
      &:hover
        border-color: red
</style>


<script>
  import * as THREE from 'three'
  import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
  import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js'
  import { DragControls } from 'three/examples/jsm/controls/DragControls.js'
  import { HDRCubeTextureLoader } from 'three/examples/jsm/loaders/HDRCubeTextureLoader.js'

  var rendering = true
  var renderer, controls, scene, camera, mesh, lineMaterial, pointMaterial

  function toScreen(vec) {
    const widthHalf = 0.5 * renderer.domElement.width
    const heightHalf = 0.5 * renderer.domElement.height
    // camera.updateMatrixWorld()
    const vector = vec.project(camera)
    return {
      x: (vector.x * widthHalf) + widthHalf,
      y: - (vector.y * heightHalf) + heightHalf,
    }
  }

  export default {
    name: 'ViewPort',

    props: {
      tree: Object,
    },

    data() {
      return {
        widgets: [],
        segments: [],
        handles: [],
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

      camera = new THREE.PerspectiveCamera(70, 1, 0.01, 10000)
      camera.position.x = 3
      camera.position.y = 2
      camera.position.z = 3

      scene = new THREE.Scene()
      // scene.fog = new THREE.Fog(0xcce0ff, 0.1, 20)
      // scene.add(new THREE.AmbientLight(0x666666))
      var sun = new THREE.DirectionalLight(0xdfebff, 1)
      sun.position.set(0, 100, 0)
      sun.castShadow = true
      sun.shadow.mapSize.width = 2048
      sun.shadow.mapSize.height = 2048
      scene.add(sun)

      var light = new THREE.HemisphereLight(0xffffbb, 0x080820, 1)
      scene.add(light)

      // geometry = new THREE.BoxGeometry(0.2, 0.2, 0.2)
      var geometry = new THREE.TorusKnotBufferGeometry(1, 0.4, 170, 26)
      var material = new THREE.MeshStandardMaterial({
        color: 'coral',
        roughness: 0,
        metalness: 0.1,
      })

      mesh = new THREE.Mesh(geometry, material)
      mesh.castShadow = true
      mesh.receiveShadow = true
      scene.add(mesh)

      var groundGeo = new THREE.PlaneBufferGeometry(10, 10)
      groundGeo.rotateX(- Math.PI / 2)
      var ground = new THREE.Mesh(groundGeo, new THREE.ShadowMaterial({opacity: 0.2}))
      ground.receiveShadow = true
      ground.position.y = -1.85
      scene.add(ground)

      var grid = new THREE.GridHelper(20, 20)
      grid.position.y = -1.8
      grid.material.opacity = 0.1
      grid.material.transparent = true
      scene.add(grid)

      var pmremGenerator = new THREE.PMREMGenerator(renderer)
      pmremGenerator.compileCubemapShader()

      new HDRCubeTextureLoader()
      .setPath('textures/cubemap/')
      .setDataType(THREE.UnsignedByteType)
      .load(['px.hdr', 'nx.hdr', 'py.hdr', 'ny.hdr', 'pz.hdr', 'nz.hdr'], (texture) => {
        var envMap = pmremGenerator.fromCubemap(texture).texture
        scene.environment = envMap
        texture.dispose()
        pmremGenerator.dispose()
        this.render()
      })

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

      var transformControl = new TransformControls(camera, renderer.domElement)
      transformControl.space = 'world'
      // transformControl.translationSnap = 0.5
      // transformControl.rotationSnap = THREE.MathUtils.degToRad(10)
      // transformControl.setMode('rotate')
      transformControl.addEventListener('change', this.render)
      transformControl.addEventListener('dragging-changed', function(event) {
        controls.enabled = !event.value
      })
      transformControl.addEventListener('objectChange', (event) => {
        this.$emit('change-pose')
      })
      scene.add(transformControl)
      transformControl.attach(mesh)

      lineMaterial = new THREE.LineBasicMaterial({ color: '#2590e1', linewidth: 2, fog: true })
      pointMaterial = new THREE.PointsMaterial({ color: 'white', size: 8, sizeAttenuation: false, map: new THREE.TextureLoader().load('textures/disc.png'), alphaTest: 0.01, transparent: true })
      pointMaterial.color.setHSL(0.1, 0.9, 0.6)
      this.loadTree(this.tree)

      // var dragcontrols = new DragControls([mesh], camera, renderer.domElement)
      // dragcontrols.addEventListener('hoveron', function(event) {
      //   transformControl.attach(event.object)
      // })

      window.addEventListener('resize', this.onWindowResize.bind(this), false)
      this.onWindowResize()
      this.animate()
    },

    beforeDestroy: function() {
      rendering = false
      window.removeEventListener('resize', this.onWindowResize, false)
    },

    methods: {
      render: function() {
        renderer.render(scene, camera)
        this.updateWidgets()
      },

      animate: function() {
        if(!rendering) return
        requestAnimationFrame(this.animate.bind(this))
        controls.update()
        // mesh.rotation.x += 0.01
        // mesh.rotation.y += 0.01
        // renderer.shadowMap.needsUpdate = true
        // this.render()
      },

      onWindowResize: function() {
        const canvas = this.$el.querySelector('canvas')
        if(!canvas) return
        renderer.setSize(canvas.parentElement.offsetWidth, canvas.parentElement.offsetHeight)
        camera.aspect = canvas.parentElement.offsetWidth / canvas.parentElement.offsetHeight
        camera.updateProjectionMatrix()
        this.render()
      },

      updateWidgets: function() {
        this.widgets.length = 0
        this.handles.forEach((point, i) => {
          const pos = toScreen(new THREE.Vector3().fromArray(point))
          this.$set(this.widgets, i, {pos, type: 'vertex'})
        })
      },

      getSegments: function(node) {
        const segments = node.sketches.flatMap(sketch => sketch.segments)
        return segments.concat(node.children.flatMap(child => this.getSegments(child)))
      },

      loadTree: function(tree) {
        this.segments = this.getSegments(tree)
        this.handles = this.segments.flatMap(seg => seg.handles)
        this.segments.forEach(segment => {
          const vertices = segment.vertices.map(vertex => new THREE.Vector3().fromArray(vertex))
          const handles = segment.handles.map(handle => new THREE.Vector3().fromArray(handle))
          var lineGeom = new THREE.BufferGeometry().setFromPoints(vertices)
          var pointGeom = new THREE.BufferGeometry().setFromPoints(handles)
          var line = new THREE.Line(lineGeom, lineMaterial)
          var points = new THREE.Points(pointGeom, pointMaterial)
          scene.add(line)
          scene.add(points)
        })
      },
    }
  }
</script>
