import * as THREE from 'three'

import { createNanoEvents } from 'nanoevents'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js'
// import { DragControls } from 'three/examples/jsm/controls/DragControls.js'
import { HDRCubeTextureLoader } from 'three/examples/jsm/loaders/HDRCubeTextureLoader.js'
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js'
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
import { Line2 } from 'three/examples/jsm/lines/Line2.js'

import { Materials } from './materials.js'
import { SketchPlane } from './sketchPlane.js'
import { ShadowCatcher } from './shadowCatcher.js'


export class Renderer {
  constructor(canvas) {
    THREE.Object3D.DefaultUp = new THREE.Vector3(0, 0, 1)

    this.emitter = createNanoEvents()

    // Renderer
    this.renderer = new THREE.WebGLRenderer({
      canvas,
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
      this.emitter.emit('change-pose')
      this.shadowCatcher.update()
      this.render()
    })

    this.scene.add(this.transformControl)

    // Init viewport
    this.setActiveCamera(this.camera)
  }

  on(event, callback) {
    return this.emitter.on(event, callback)
  }

  add(obj) {
    this.world.add(obj)
  }

  remove(obj) {
    if(!obj) return
    this.world.remove(obj)
    this.drop_resources(obj, true)
  }

  drop_resources(obj, geomOnly) {
    obj.traverse((child) => {
      if(child.geometry) child.geometry.dispose()
      if(child.material && !geomOnly) {
        var texture = child.material.map
        if(texture) texture.dispose()
        child.material.dispose()
      }
    })
  }

  switchCamera() {
    this.setActiveCamera(this.activeCamera == this.cameraOrtho ? this.camera : this.cameraOrtho)
  }

  setPivot(coords) {
    this.viewControlsTarget = this.fromScreen(coords)
  }

  setActiveCamera(camera) {
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
      this.emitter.emit('change-view')
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

  render() {
    this.renderer.render(this.scene, this.activeCamera)
    this.emitter.emit('render')
  }

  animate() {
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
  }

  getCanvasCoords(mouseCoords) {
    const canvas = this.renderer.domElement
    return new THREE.Vector2(
      mouseCoords.x / canvas.offsetWidth * 2 - 1,
      -mouseCoords.y / canvas.offsetHeight * 2 + 1,
    )
  }

  hitTest(coords) {
    coords = this.getCanvasCoords(coords)
    this.raycaster.setFromCamera(coords, this.activeCamera)
    return this.raycaster.intersectObjects(this.scene.children, true)
  }

  fromScreen(coords) {
    const intersects = this.hitTest(coords).filter(obj => obj.object.alcProjectable)
    const hit = intersects[0]
    return hit && hit.point
  }

  toScreen(vec) {
    if(!this.activeCamera) return
    const widthHalf = 0.5 * this.renderer.domElement.width / window.devicePixelRatio
    const heightHalf = 0.5 * this.renderer.domElement.height / window.devicePixelRatio
    // this.camera.updateMatrixWorld()
    const vector = vec.clone().project(this.activeCamera)
    return new THREE.Vector2(
      (vector.x * widthHalf) + widthHalf,
      - (vector.y * heightHalf) + heightHalf
    )
  }

  objectsAtScreen(coords, types) {
    const intersects = this.hitTest(coords)
    const objects = Array.from(new Set(intersects.map(obj => obj.object)))
    return objects.filter(obj => types.some(t => obj.alcType == t))
  }

  convertLine(vertices, material) {
    const geometry = new LineGeometry()
    const positions = vertices.flat()
    geometry.setPositions(positions)
    // geometry.setColors(positions.map((pos, i) => i / positions.length ))
    geometry.setColors(Array(positions.length).fill(1))
    const line = new Line2(geometry, material)
    line.computeLineDistances()
    return line
  }

  convertLineBasic(vertices, material) {
    var geometry = new THREE.Geometry()
    geometry.vertices = vertices.map(vertex => new THREE.Vector3().fromArray(vertex))
    const line = new THREE.Line(geometry, material)
    return line
  }

  convertMesh(bufferGeometry, material) {
    const geometry = this.convertBufferGeometry(bufferGeometry)
    const mesh = new THREE.Mesh(geometry, material)
    return mesh
  }

  convertWireMesh(bufferGeometry, material) {
    const geometry = this.convertBufferGeometry(bufferGeometry)
    const wireframe = new THREE.WireframeGeometry(geometry);
    const line = new THREE.LineSegments(wireframe);
    return line
  }

  convertBufferGeometry(bufferGeometry, material) {
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
  }

  onWindowResize() {
    const canvas = this.renderer.domElement
    if(!canvas) return
    // Set canvas size
    const parent = canvas.parentElement
    const width = parent.offsetWidth
    const height = parent.offsetHeight
    this.renderer.setSize(width, height)

    // Update camera projection
    const aspect = width / height
    if(this.activeCamera == this.camera) {
      this.camera.aspect = aspect
    } else {
      const frustumSize = 10
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
  }

  dispose() {
    this.renderer.renderLists.dispose()
    this.scene.environment.dispose()
    this.viewControls.dispose()
    this.transformControl.dispose()
    this.drop_resources(this.scene)
  }
}
