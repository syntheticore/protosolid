import * as THREE from 'three'
import { createNanoEvents } from 'nanoevents'

import { HDRCubeTextureLoader } from 'three/examples/jsm/loaders/HDRCubeTextureLoader.js'
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js'
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
import { Line2 } from 'three/examples/jsm/lines/Line2.js'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
// import { DragControls } from 'three/examples/jsm/controls/DragControls.js'

import Materials from './materials.js'
import SketchPlane from './sketch-plane.js'
import ShadowCatcher from './shadow-catcher.js'
import ArrowControls from './arrow-controls.js'
import { vec2three } from './utils.js'
import { default as preferences, emitter as prefEmitter } from './preferences.js'


export default class Renderer {
  constructor(canvas) {
    this.canvas = canvas

    THREE.Object3D.DefaultUp = new THREE.Vector3(0, 0, 1)

    this.emitter = createNanoEvents()

    // Renderer
    this.renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: preferences.antiAlias,
      alpha: true,
      // logarithmicDepthBuffer: true,
    })

    this.renderer.outputEncoding = THREE.sRGBEncoding
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping
    this.renderer.physicallyCorrectLights = true
    this.renderer.shadowMap.autoUpdate = false
    this.renderer.shadowMap.enabled = preferences.shadowMaps

    // this.renderer.shadowMap.type = THREE.VSMShadowMap
    // this.renderer.toneMapping = THREE.ReinhardToneMapping
    // this.renderer.toneMapping = THREE.LinearToneMapping
    // this.renderer.toneMappingExposure = 1.2
    // this.renderer.setClearColor(0x263238)

    // Scene
    this.scene = new THREE.Scene()

    var atmosphere = new THREE.HemisphereLight(0xffffbb, 0x080820, 1)
    this.scene.add(atmosphere)

    new HDRCubeTextureLoader()
    .setPath('textures/cubemap/')
    .setDataType(THREE.UnsignedByteType)
    .load(['px.hdr', 'nx.hdr', 'py.hdr', 'ny.hdr', 'pz.hdr', 'nz.hdr'], (texture) => {
      var pmremGenerator = new THREE.PMREMGenerator(this.renderer)
      pmremGenerator.compileCubemapShader()
      this.scene.environment = pmremGenerator.fromCubemap(texture).texture
      texture.dispose()
      pmremGenerator.dispose()
      this.render()
    })

    // Camera
    this.raycaster = new THREE.Raycaster()

    this.camera = new THREE.PerspectiveCamera(70, 1, 0.5, 10000)
    this.camera.position.set(90, 90, 90)

    this.cameraOrtho = new THREE.OrthographicCamera(-1, 1, 1, -1, -200, 10000)
    this.cameraOrtho.position.set(0, 0, 10)
    this.cameraOrtho.lookAt(this.scene.position)

    // Scene Objects
    this.traceables = new THREE.Object3D()
    this.world = new THREE.Object3D()
    this.traceables.add(this.world)
    this.scene.add(this.traceables)

    // Materials
    this.materials = new Materials()

    // Sketch Plane
    this.sketchPlane = new SketchPlane(this.camera)
    this.traceables.add(this.sketchPlane)

    // Shadow Catcher
    if(preferences.shadowMaps) {
      this.shadowCatcher = new ShadowCatcher(this.renderer, this.world)
      this.scene.add(this.shadowCatcher)
    }

    // Gizmos
    this.gizmos = []

    // var torusGeometry = new THREE.TorusKnotBufferGeometry(1, 0.4, 170, 36)
    // const mesh = new THREE.Mesh(torusGeometry, this.materials.surface)
    // mesh.position.z = 1
    // mesh.castShadow = true
    // mesh.receiveShadow = true
    // // mesh.visible = false
    // this.scene.add(mesh)

    // Init viewport
    this.setActiveCamera(this.camera)

    const setPixelRatio = () => {
      this.renderer.setPixelRatio(this.getPixelRatio())
      this.render()
    }

    setPixelRatio()
    prefEmitter.on('updated', setPixelRatio)

    // Store as global
    window.alcRenderer = this
  }

  getPixelRatio() {
    return preferences.highDPI ? window.devicePixelRatio : 1
  }

  setActiveCamera(camera) {
    if(this.viewControls) this.viewControls.dispose()

    const target = this.viewControls && this.viewControls.target
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
    if(target) this.viewControls.target.copy(target)
    this.viewControls.update()

    this.viewControls.addEventListener('change', () => this.render() )

    this.viewControls.addEventListener('start', () => {
      this.isOrbiting = true
      this.gizmos.forEach(gizmo => gizmo.enabled = false)
      this.cameraTarget = null
      this.viewControlsTarget = null
      this.emitter.emit('change-view', this.camera.position, this.viewControls.target)
      this.startAnimation()
    })

    this.viewControls.addEventListener('end', () => {
      this.isOrbiting = false
      this.gizmos.forEach(gizmo => gizmo.enabled = true)
      this.emitter.emit('change-view', this.camera.position, this.viewControls.target)
      this.endAnimation()
    })

    this.activeCamera = camera
    this.onWindowResize()
  }

  switchCamera() {
    this.setActiveCamera(this.activeCamera == this.cameraOrtho ? this.camera : this.cameraOrtho)
  }

  on(event, cb) {
    return this.emitter.on(event, cb)
  }

  add(obj, selectable) {
    if(selectable) {
      this.world.add(obj)
    } else {
      this.scene.add(obj)
    }
  }

  remove(obj) {
    if(!obj) return
    this.world.remove(obj)
    this.scene.remove(obj)
    this.dropResources(obj, true)
  }

  dropResources(obj, geomOnly) {
    obj.traverse(child => {
      if(child.geometry) child.geometry.dispose()
      if(child.material && !geomOnly) {
        var texture = child.material.map
        if(texture) texture.dispose()
        child.material.dispose()
      }
    })
  }

  setPivot(coords) {
    const vec = this.fromScreen(coords)
    const cameraTarget = vec.clone().sub(this.viewControls.target).add(this.camera.position)
    this.setView(cameraTarget, vec)
  }

  setView(position, target) {
    this.cameraTarget = position
    this.viewControlsTarget = target
    this.startAnimation()
    this.endAnimation()
  }

  setDisplayMode(mode) {
    this.displayMode = mode
  }

  render() {
    this.sketchPlane.update(this.activeCamera)
    this.renderer.render(this.scene, this.activeCamera)
    this.emitter.emit('render')
  }

  startAnimation() {
    clearTimeout(this.dampingTimeout)
    if(!this.isAnimating) {
      this.isAnimating = true
      this.animate()
    }
  }

  endAnimation() {
    // Make sure we keep animating long enough for view damping to settle
    this.dampingTimeout = setTimeout(() => {
      this.isAnimating = false
    }, 500)
  }

  animate() {
    if(this.isAnimating || this.viewControlsTarget || this.cameraTarget) requestAnimationFrame(this.animate.bind(this))
    // Update orbit controls dampening
    this.viewControls.update()
    // Transition to target positions
    this.cameraTarget = this.lerp(this.camera.position, this.cameraTarget)
    this.viewControlsTarget = this.lerp(this.viewControls.target, this.viewControlsTarget)
  }

  lerp(vec, target) {
    if(!target) return
    vec.multiplyScalar(0.7).add(
      target.clone().multiplyScalar(0.3)
    )
    if(target.clone().sub(vec).lengthSq() < 0.0001) {
      vec.copy(target)
      return null
    }
    return target
  }

  updateShadows() {
    if(this.shadowCatcher) this.shadowCatcher.update()
  }

  addGizmo(gizmo) {
    this.gizmos.push(gizmo)

    // Don't orbit when dragging on gizmo
    gizmo.addEventListener('dragging-changed', (event) => {
      this.viewControls.enabled = !event.value
    })

    gizmo.addEventListener('objectChange', () => {
      this.emitter.emit('change-pose')
      this.updateShadows()
      this.render()
    })

    // Recreate View Controls to achieve correct event order
    this.scene.add(gizmo)
    this.setActiveCamera(this.activeCamera)
  }

  removeGizmo(gizmo) {
    if(!gizmo) return
    this.scene.remove(gizmo)
    gizmo.dispose()
    this.render()
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
    return this.raycaster.intersectObjects(this.traceables.children, true)
  }

  fromScreen(coords) {
    const intersects = this.hitTest(coords).filter(obj => obj.object.alcProjectable)
    const hit = intersects[0]
    return hit && hit.point
  }

  toScreen(vec) {
    if(!this.activeCamera) return
    const widthHalf = 0.5 * this.renderer.domElement.width / this.getPixelRatio()
    const heightHalf = 0.5 * this.renderer.domElement.height / this.getPixelRatio()
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

  // convertLineBasic(vertices, material) {
  //   var geometry = new THREE.Geometry()
  //   geometry.vertices = vertices.map(vertex => vec2three(vertex))
  //   const line = new THREE.Line(geometry, material)
  //   return line
  // }

  convertMesh(bufferGeometry, material) {
    const geometry = this.convertBufferGeometry(bufferGeometry)
    const mesh = new THREE.Mesh(geometry, material)
    return mesh
  }

  // convertWireMesh(bufferGeometry, material) {
  //   const geometry = this.convertBufferGeometry(bufferGeometry)
  //   const wireframe = new THREE.WireframeGeometry(geometry);
  //   const line = new THREE.LineSegments(wireframe);
  //   return line
  // }

  convertBufferGeometry(bufferGeometry, material) {
    const geometry = new THREE.BufferGeometry()
    const vertices = new Float32Array(bufferGeometry.position())
    const normals = new Float32Array(bufferGeometry.normal())
    bufferGeometry.free()
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
      const frustumSize = 200
      this.cameraOrtho.left = - 0.5 * frustumSize * aspect
      this.cameraOrtho.right = 0.5 * frustumSize * aspect
      this.cameraOrtho.top = frustumSize / 2
      this.cameraOrtho.bottom = - frustumSize / 2
    }
    this.activeCamera.updateProjectionMatrix()
    // Update line materials
    this.materials.line.resolution.set(width, height)
    this.materials.selectionLine.resolution.set(width, height)
    this.materials.highlightLine.resolution.set(width, height)
    this.materials.wire.resolution.set(width, height)
    this.materials.ghostWire.resolution.set(width, height)
    this.render()
  }

  dispose() {
    this.viewControls.dispose()
    this.gizmos.forEach(gizmo => gizmo.dispose() )
    this.scene.environment.dispose()
    this.dropResources(this.scene)
  }
}
