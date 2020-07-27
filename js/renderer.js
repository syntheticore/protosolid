import * as THREE from 'three'
import { createNanoEvents } from 'nanoevents'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js'
import { DragControls } from 'three/examples/jsm/controls/DragControls.js'
import { HDRCubeTextureLoader } from 'three/examples/jsm/loaders/HDRCubeTextureLoader.js'

var rendering = true
var renderer, controls, scene, camera, mesh, lineMaterial, pointMaterial

export class Renderer {
  constructor(canvas) {
    this.emitter = createNanoEvents()

    renderer = new THREE.WebGLRenderer({
      canvas: canvas,
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
      this.emitter.emit('change-view')
    })

    var transformControl = new TransformControls(camera, renderer.domElement)
    transformControl.space = 'world'
    // transformControl.translationSnap = 0.5
    // transformControl.rotationSnap = THREE.MathUtils.degToRad(10)
    // transformControl.setMode('rotate')
    transformControl.addEventListener('change', () => this.render())
    transformControl.addEventListener('dragging-changed', (event) => {
      controls.enabled = !event.value
    })
    transformControl.addEventListener('objectChange', (event) => {
      this.emitter.emit('change-pose')
    })
    scene.add(transformControl)
    transformControl.attach(mesh)

    lineMaterial = new THREE.LineBasicMaterial({ color: '#2590e1', linewidth: 2, fog: true })
    pointMaterial = new THREE.PointsMaterial({ color: 'white', size: 8, sizeAttenuation: false, map: new THREE.TextureLoader().load('textures/disc.png'), alphaTest: 0.01, transparent: true })
    pointMaterial.color.setHSL(0.1, 0.9, 0.6)
    // this.loadTree(this.tree)

    // var dragcontrols = new DragControls([mesh], camera, renderer.domElement)
    // dragcontrols.addEventListener('hoveron', function(event) {
    //   transformControl.attach(event.object)
    // })

    window.addEventListener('resize', this.onWindowResize.bind(this), false)
    this.onWindowResize()
    this.animate()
  }

  on(event, callback) {
    return this.emitter.on(event, callback)
  }

  render() {
    renderer.render(scene, camera)
    this.emitter.emit('render')
  }

  animate() {
    if(!rendering) return
    requestAnimationFrame(this.animate.bind(this))
    controls.update()
    // mesh.rotation.x += 0.01
    // mesh.rotation.y += 0.01
    // renderer.shadowMap.needsUpdate = true
    // this.render()
  }

  onWindowResize() {
    const canvas = renderer.domElement
    if(!canvas) return
    renderer.setSize(canvas.parentElement.offsetWidth, canvas.parentElement.offsetHeight)
    camera.aspect = canvas.parentElement.offsetWidth / canvas.parentElement.offsetHeight
    camera.updateProjectionMatrix()
    this.render()
  }

  toScreen(vec) {
    const widthHalf = 0.5 * renderer.domElement.width
    const heightHalf = 0.5 * renderer.domElement.height
    // camera.updateMatrixWorld()
    const vector = vec.project(camera)
    return {
      x: (vector.x * widthHalf) + widthHalf,
      y: - (vector.y * heightHalf) + heightHalf,
    }
  }

  getSegments(node) {
    const segments = node.get_sketches().flatMap(sketch => sketch.get_segments())
    return segments.concat(node.get_children().flatMap(child => this.getSegments(child)))
  }

  loadTree(tree) {
    this.segments = this.getSegments(tree)
    this.handles = this.segments.flatMap(seg => seg.get_handles())
    this.segments.forEach(segment => {
      const vertices = segment.tesselate(60).map(vertex => new THREE.Vector3().fromArray(vertex))
      const handles = segment.get_handles().map(handle => new THREE.Vector3().fromArray(handle))
      var lineGeom = new THREE.BufferGeometry().setFromPoints(vertices)
      var pointGeom = new THREE.BufferGeometry().setFromPoints(handles)
      var line = new THREE.Line(lineGeom, lineMaterial)
      var points = new THREE.Points(pointGeom, pointMaterial)
      scene.add(line)
      scene.add(points)
    })
  }

  dispose() {
    rendering = false
    window.removeEventListener('resize', this.onWindowResize, false)
  }
}
