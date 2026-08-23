import * as THREE from 'three'
import { createNanoEvents } from 'nanoevents'

import { HDRCubeTextureLoader } from 'three/examples/jsm/loaders/HDRCubeTextureLoader.js'
// import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js'
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
import { Line2 } from 'three/examples/jsm/lines/Line2.js'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
// import { DragControls } from 'three/examples/jsm/controls/DragControls.js'

import materials from './materials.js'
import SketchPlane from './three/sketch-plane.js'
import ShadowCatcher from './three/shadow-catcher.js'
// import ArrowControls from './arrow-controls.js'
import { default as preferences, emitter as prefEmitter } from './preferences.js'


export default class Renderer {
  constructor(canvas) {
    this.canvas = canvas

    // THREE.Object3D.DefaultUp = new THREE.Vector3(0, 0, 1)
    // THREE.Object3D.DEFAULT_UP.set(0, 0, 1)

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

    const basePath = window.ipc
      ? `file://${window.appPath}/app/cubemap/`
      : '/cubemap/'

    new HDRCubeTextureLoader()
    .setPath(basePath)
    // .setDataType(THREE.HalfFloatType)
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
    this.cameraOrtho.position.set(0, 10, 0)
    this.cameraOrtho.lookAt(this.scene.position)
    this.orthoFrustumSize = 200

    // Scene Objects
    this.traceables = new THREE.Object3D()
    this.world = new THREE.Object3D()
    this.traceables.add(this.world)
    this.scene.add(this.traceables)

    // this.scene.add(new THREE.AxesHelper(10.0))

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
    this.setActiveCamera(this.cameraOrtho)

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

  reportViewChange() {
    this.emitter.emit(
      'change-view',
      this.cameraTarget || this.activeCamera.position,
      this.viewControlsTarget || this.viewControls.target
    )
  }

  setActiveCamera(camera) {
    if(this.viewControls) this.viewControls.dispose()

    const target = this.viewControls && this.viewControls.target
    this.viewControls = new OrbitControls(camera, this.renderer.domElement)
    this.viewControls.enableRotate = false
    this.viewControls.enableDamping = true
    this.viewControls.dampingFactor = 0.6
    this.viewControls.panSpeed = 1.0
    this.viewControls.keyPanSpeed = 12
    this.viewControls.zoomSpeed = 2.6
    this.viewControls.zoomToCursor = false
    this.viewControls.screenSpacePanning = true
    this.viewControls.rotateSpeed = 1.2
    this.viewControls.minPolarAngle = - Math.PI
    this.viewControls.maxPolarAngle = Math.PI * 2
    if(target) this.viewControls.target.copy(target)
    this.viewControls.update()

    this.viewControls.addEventListener('change', () => {
      if(camera == this.cameraOrtho && Math.abs(camera.zoom - 1) > 1e-6) {
        const target = this.viewControls.target
        const offset = camera.position.clone().sub(target)
        camera.position.copy(target).addScaledVector(offset, 1 / camera.zoom)
        camera.zoom = 1
        this.updateOrthoProjection()
      }
      this.render()
    } )

    this.viewControls.addEventListener('start', () => {
      // this.isOrbiting = true
      this.gizmos.forEach(gizmo => gizmo.enabled = false)
      this.cameraTarget = null
      this.viewControlsTarget = null
      this.cameraTransitionUp = null
      this.cameraUpTarget = null
      this.reportViewChange()
      this.startAnimation()
    })

    this.viewControls.addEventListener('end', () => {
      // this.isOrbiting = false
      this.gizmos.forEach(gizmo => gizmo.enabled = true)
      this.reportViewChange()
      this.endAnimation()
    })

    this.activeCamera = camera
    this.gizmos.forEach(gizmo => gizmo.camera = camera)
    this.onWindowResize()
  }

  switchCamera() {
    const from = this.activeCamera
    const to = from == this.cameraOrtho ? this.camera : this.cameraOrtho
    const target = (this.viewControlsTarget || this.viewControls.target).clone()
    const position = (this.cameraTarget || from.position).clone()
    const offset = position.sub(target)
    const distance = offset.length()
    if(!distance) return

    const direction = offset.normalize()
    if(to == this.cameraOrtho) {
      // Keep the same view direction and make the orthographic viewport cover
      // the same vertical span at the current target as the perspective view.
      this.orthoFrustumSize = 2 * distance * Math.tan(
        THREE.MathUtils.degToRad(this.camera.getEffectiveFOV()) / 2
      )
      this.cameraOrtho.zoom = 1
      this.cameraOrtho.position.copy(target).addScaledVector(direction, distance)
      this.cameraOrtho.quaternion.copy(from.quaternion)
    } else {
      // Convert the orthographic viewport height back into a perspective
      // distance so switching back does not change the apparent scale.
      const visibleFrustumSize = this.orthoFrustumSize / this.cameraOrtho.zoom
      const perspectiveDistance = visibleFrustumSize / (
        2 * Math.tan(THREE.MathUtils.degToRad(this.camera.fov) / 2)
      )
      this.camera.position.copy(target).addScaledVector(direction, perspectiveDistance)
      this.camera.quaternion.copy(from.quaternion)
    }

    this.setActiveCamera(to)
    this.viewControls.target.copy(target)
    this.viewControls.update()
    this.updateOrthoProjection()
  }

  setProjectionMode(mode) {
    const camera = mode == 'orthographic' ? this.cameraOrtho : this.camera
    if(this.activeCamera != camera) this.switchCamera()
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
    if(!vec) return
    const cameraTarget = vec.clone().sub(this.viewControls.target).add(this.activeCamera.position)
    this.setView(cameraTarget, vec)
  }

  lookAt(plane, report=true) {
    const normal = new THREE.Vector3(0,0,1).applyQuaternion(new THREE.Quaternion().setFromRotationMatrix(plane))
    const controlsTarget = (this.viewControlsTarget || this.viewControls.target)
    const dir = (this.cameraTarget || this.activeCamera.position).clone().sub(controlsTarget).projectOnVector(normal)
    const position = controlsTarget.clone().add(dir)
    const target = controlsTarget.clone()
    this.setView(position, target)
    if(report) this.reportViewChange()
  }

  zoomToFit(objects, report=true, view=null) {
    objects ||= [this.world]
    const box = new THREE.Box3()
    objects.forEach(obj => box.expandByObject(obj) )
    const target = box.getCenter(new THREE.Vector3())
    const size = box.getSize(new THREE.Vector3())
    const radius = Math.max(...size.toArray()) / 2.0
    const oldTarget = view ? view.target : this.viewControlsTarget || this.viewControls.target
    const oldPosition = view ? view.position : this.cameraTarget || this.activeCamera.position
    const direction = oldPosition.clone().sub(oldTarget).normalize()

    if(this.activeCamera == this.cameraOrtho) {
      const vFOV = this.camera.getEffectiveFOV() * THREE.MathUtils.DEG2RAD
      const distanceToFit = radius / Math.sin(vFOV * 0.5) * 1.6
      this.setView(target.clone().addScaledVector(direction, distanceToFit), target)

    } else {
      const vFOV = this.camera.getEffectiveFOV() * THREE.MathUtils.DEG2RAD
      const hFOV = Math.atan(Math.tan(vFOV * 0.5) * this.camera.aspect) * 2.0
      const fov = this.camera.aspect > 1.0 ? vFOV : hFOV
      const distanceToFit = radius / (Math.sin(fov * 0.5))
      const dir = direction.multiplyScalar(distanceToFit * 1.6)
      const position = target.clone().add(dir)
      this.setView(position, target)
    }
    if(report) this.reportViewChange()
  }

  setView(position, target) {
    const camera = this.activeCamera
    const polePosition = this.poleSafePosition(position, target)
    this.cameraTarget = polePosition || position
    this.viewControlsTarget = target
    if(polePosition) {
      camera.updateMatrixWorld()
      this.cameraTransitionUp = new THREE.Vector3()
        .setFromMatrixColumn(camera.matrixWorld, 1)
        .normalize()
      const rotation = new THREE.Matrix4().lookAt(this.cameraTarget, target, camera.up)
      this.cameraUpTarget = new THREE.Vector3()
        .setFromMatrixColumn(rotation, 1)
        .normalize()
    } else {
      this.cameraTransitionUp = null
      this.cameraUpTarget = null
    }
    this.startAnimation()
    this.endAnimation()
  }

  poleSafePosition(position, target) {
    const offset = position.clone().sub(target)
    const distance = offset.length()
    if(!distance) return null

    // OrbitControls cannot preserve the camera roll when its view direction is
    // exactly parallel to the world-up axis. Keep an imperceptible offset toward
    // +Z so the top/bottom view has a stable orientation throughout the lerp.
    const worldUp = THREE.Object3D.DEFAULT_UP
    const isAtPole = Math.abs(offset.dot(worldUp)) > distance * (1 - 1e-10)
    if(!isAtPole) return null

    const poleAxis = Math.abs(worldUp.z) < 0.9
      ? new THREE.Vector3(0, 0, 1)
      : new THREE.Vector3(1, 0, 0)
    const poleOffset = poleAxis
      .projectOnPlane(worldUp)
      .normalize()
      .multiplyScalar(distance * 1e-5)
    return position.clone().add(poleOffset)
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

  animate(timestamp) {
    const delta = this.lastTimestamp ? timestamp - this.lastTimestamp : 1
    this.lastTimestamp = timestamp
    if(this.isAnimating || this.viewControlsTarget || this.cameraTarget || this.cameraUpTarget) requestAnimationFrame(this.animate.bind(this))
    // Transition to target positions
    this.cameraTarget = this.lerp(this.activeCamera.position, this.cameraTarget)
    this.viewControlsTarget = this.lerp(this.viewControls.target, this.viewControlsTarget)
    // Update the projection before OrbitControls renders the new camera pose.
    this.updateOrthoProjection()
    // Let OrbitControls derive the camera orientation from the interpolated
    // position and target, keeping WebGL and projected UI handles in sync.
    if(!this.cameraUpTarget) this.viewControls.update(delta)
    if(this.cameraUpTarget) {
      // Interpolate only the roll; rebuilding the look-at rotation keeps the
      // moving target fixed in the center of the screen throughout the move.
      this.slerpDirection(this.cameraTransitionUp, this.cameraUpTarget)
      const transitionDone = !this.cameraTarget &&
        !this.viewControlsTarget &&
        this.cameraTransitionUp.angleTo(this.cameraUpTarget) < 1e-3
      if(transitionDone) this.cameraTransitionUp.copy(this.cameraUpTarget)
      const viewDirection = this.viewControls.target.clone().sub(this.activeCamera.position).normalize()
      const viewUp = this.cameraTransitionUp.clone().projectOnPlane(viewDirection).normalize()
      const rotation = new THREE.Matrix4().lookAt(this.activeCamera.position, this.viewControls.target, viewUp)
      this.activeCamera.quaternion.setFromRotationMatrix(rotation)
      if(transitionDone) {
        this.cameraTransitionUp = null
        this.cameraUpTarget = null
      }
      this.render()
    }
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

  slerpDirection(direction, target) {
    const rotation = new THREE.Quaternion().setFromUnitVectors(direction, target)
    direction.applyQuaternion(new THREE.Quaternion().slerp(rotation, 0.15)).normalize()
  }

  updateOrthoProjection() {
    if(this.activeCamera != this.cameraOrtho) return
    const target = this.viewControlsTarget || this.viewControls.target
    const distance = this.cameraOrtho.position.distanceTo(target)
    if(!distance) return
    this.orthoFrustumSize = distance * 2 * Math.tan(
      THREE.MathUtils.degToRad(this.camera.getEffectiveFOV()) / 2
    )
    const aspect = this.cameraOrtho.right && this.cameraOrtho.top
      ? (this.cameraOrtho.right - this.cameraOrtho.left) / (this.cameraOrtho.top - this.cameraOrtho.bottom)
      : 1
    this.cameraOrtho.left = -0.5 * this.orthoFrustumSize * aspect
    this.cameraOrtho.right = 0.5 * this.orthoFrustumSize * aspect
    this.cameraOrtho.top = this.orthoFrustumSize / 2
    this.cameraOrtho.bottom = -this.orthoFrustumSize / 2
    this.cameraOrtho.updateProjectionMatrix()
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
      this.updateShadows()
      this.render()
    })

    gizmo.addEventListener('change', () => this.render() )

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
      (mouseCoords.x - 2) / canvas.offsetWidth * 2 - 1,
      (-mouseCoords.y + 2) / canvas.offsetHeight * 2 + 1,
    )
  }

  hitTest(coords, includeInactive) {
    coords = this.getCanvasCoords(coords)
    this.raycaster.setFromCamera(coords, this.activeCamera)
    return this.raycaster.intersectObjects(
      includeInactive ? this.scene.children : this.traceables.children,
      true,
    )
  }

  fromScreen(coords) {
    const intersects = this.hitTest(coords).filter(obj => obj.object.alcProjectable )
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
      Math.round((vector.x * widthHalf) + widthHalf),
      Math.round(-(vector.y * heightHalf) + heightHalf)
    )
  }

  objectsAtScreen(coords, types, includeInactive) {
    if(types && !Array.isArray(types)) types = [types]
    const intersects = this.hitTest(coords, includeInactive)
    const objects = Array.from(new Set(intersects.map(obj => obj.object )))
    return objects.filter(obj => !types || types.some(t =>
      obj.alcTypes && obj.alcTypes.some(ot => ot == t )
    ))
  }

  convertLine(vertices, material) {
    const positions = (vertices || []).flat()
    if(positions.length < 6 || positions.some(value => !Number.isFinite(Math.fround(value)))) return new THREE.Object3D()
    const geometry = new LineGeometry()
    geometry.setPositions(positions)
    // geometry.setColors(positions.map((pos, i) => i / positions.length ))
    geometry.setColors(Array(positions.length).fill(1))
    const line = new Line2(geometry, material)
    line.computeLineDistances()
    return line
  }

  // convertLineBasic(vertices, material) {
  //   var geometry = new THREE.Geometry()
  //   geometry.vertices = vertices.map(vertex => vecToThree(vertex))
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
    const vertices = new Float32Array(bufferGeometry.positions)
    const normals = new Float32Array(bufferGeometry.normals)
    // console.log('vertices', bufferGeometry.positions)
    // bufferGeometry.free()
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
      const frustumSize = this.orthoFrustumSize
      this.cameraOrtho.left = - 0.5 * frustumSize * aspect
      this.cameraOrtho.right = 0.5 * frustumSize * aspect
      this.cameraOrtho.top = frustumSize / 2
      this.cameraOrtho.bottom = - frustumSize / 2
    }
    this.activeCamera.updateProjectionMatrix()
    // Update line materials
    materials.line.resolution.set(width, height)
    materials.selectionLine.resolution.set(width, height)
    materials.highlightLine.resolution.set(width, height)
    materials.referenceLine.resolution.set(width, height)
    materials.referenceSelectionLine.resolution.set(width, height)
    materials.referenceHighlightLine.resolution.set(width, height)
    materials.referenceLine.resolution.set(width, height)
    materials.splineControlLine.resolution.set(width, height)
    materials.wire.resolution.set(width, height)
    materials.ghostWire.resolution.set(width, height)
    this.render()
  }

  dispose() {
    this.viewControls.dispose()
    this.gizmos.forEach(gizmo => gizmo.dispose() )
    this.scene.environment.dispose()
    this.dropResources(this.scene)
  }
}
