import * as THREE from 'three'
import { createNanoEvents } from 'nanoevents'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js'
import { DragControls } from 'three/examples/jsm/controls/DragControls.js'
import { HDRCubeTextureLoader } from 'three/examples/jsm/loaders/HDRCubeTextureLoader.js'
import { LineMaterial } from 'three/examples/jsm/lines/LineMaterial.js'
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js'
import { Line2 } from 'three/examples/jsm/lines/Line2.js'

var rendering = true
var renderer, controls, camera, mesh, lineMaterial, selectionLineMaterial, highlightLineMaterial, pointMaterial

export class Renderer {
  constructor(canvas, viewport) {
    this.viewport = viewport

    this.emitter = createNanoEvents()
    this.isDragging = false

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

    controls.addEventListener('start', () => {
      this.isDragging = true
      // transformControl.enabled = false
    })
    controls.addEventListener('end', () => {
      this.isDragging = false
      // transformControl.enabled = true
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
      renderer.shadowMap.needsUpdate = true
    })
    this.scene.add(transformControl)
    transformControl.attach(mesh)

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

    this.elements = []

    window.addEventListener('resize', this.onWindowResize.bind(this), false)
    this.onWindowResize()
    this.animate()
  }

  on(event, callback) {
    return this.emitter.on(event, callback)
  }

  render() {
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
    this.emitter.emit('render')
  }

  animate() {
    if(!rendering) return
    requestAnimationFrame(this.animate.bind(this))
    controls.update()
    // mesh.rotation.x += 0.01
    // mesh.rotation.y += 0.01
    // renderer.shadowMap.needsUpdate = true
  }

  onWindowResize() {
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
  }

  fromScreen(coords) {
    this.raycaster.setFromCamera(coords, camera)
    const intersects = this.raycaster.intersectObjects(this.scene.children)
    const hit = intersects.filter(obj => obj.object.alcProjectable)[0]
    return hit && hit.point
  }

  toScreen(vec) {
    const widthHalf = 0.5 * renderer.domElement.width
    const heightHalf = 0.5 * renderer.domElement.height
    // camera.updateMatrixWorld()
    const vector = vec.clone().project(camera)
    return {
      x: (vector.x * widthHalf) + widthHalf,
      y: - (vector.y * heightHalf) + heightHalf,
    }
  }

  objectsAtScreen(coords, filter) {
    this.raycaster.setFromCamera(coords, camera)
    const intersects = this.raycaster.intersectObjects(this.scene.children)
    const objects = Array.from(new Set(intersects.map(obj => obj.object)))
    return objects.filter(obj => obj[filter])
  }

  onMouseDown(coords) {
    if(this.isDragging) return
    if(this.viewport.selectedElement) this.viewport.selectedElement.three.material = lineMaterial
    const object = this.objectsAtScreen(coords, 'alcSelectable')[0]
    if(object) {
      object.material = selectionLineMaterial
      this.emitter.emit('element-selected', object.element)
    } else {
      this.emitter.emit('element-selected', null)
    }
    this.render()
  }

  onMouseMove(coords) {
    if(this.isDragging) return
    const object = this.objectsAtScreen(coords, 'alcSelectable')[0]
    if(!object) return this.render()
    const oldMaterial = object.material
    object.material = highlightLineMaterial
    this.render()
    object.material = oldMaterial
  }

  // getSegments(node) {
  //   // const segments = node.get_sketches().flatMap(sketch => sketch.get_segments())
  //   const segments = node.get_sketch_elements()
  //   return segments.concat(node.get_children().flatMap(child => this.getSegments(child)))
  // }

  // loadTree(node, recursive) {
  //   console.log('loadTree')
  //   this.elements.forEach(elem => this.scene.remove(elem))
  //   this.elements.length = 0
  //   this.segments = this.getSegments(node)
  //   this.handles = this.segments.flatMap(seg => seg.get_handles())
  //   this.segments.forEach(segment => {
  //     const vertices = segment.tesselate().map(vertex => new THREE.Vector3().fromArray(vertex))
  //     // const handles = segment.get_handles().map(handle => new THREE.Vector3().fromArray(handle))
  //     // var lineGeom = new THREE.BufferGeometry().setFromPoints(vertices)
  //     // // var pointGeom = new THREE.BufferGeometry().setFromPoints(handles)
  //     // var line = new THREE.Line(lineGeom, lineMaterial)
  //     // // var points = new THREE.Points(pointGeom, pointMaterial)
  //     // this.scene.add(line)
  //     // // this.scene.add(points)
  //     var geometry = new LineGeometry()
  //     geometry.setPositions(vertices.flatMap(vertex => vertex.toArray()))
  //     geometry.setColors(Array(vertices.length * 3).fill(1))
  //     var line = new Line2(geometry, lineMaterial)
  //     line.computeLineDistances()
  //     // line.scale.set(1, 1, 1)
  //     line.alcSelectable = true
  //     this.elements.push(line)
  //     this.scene.add(line)
  //   })
  //   this.render()
  // }

  loadTree(node, recursive, notTop) {
    console.log('loadTree')
    node.threeObjects = node.threeObjects || []
    node.threeObjects.forEach(elem => this.scene.remove(elem))
    node.threeObjects.length = 0
    const segments = node.get_sketch_elements()
    // this.handles = segments.flatMap(seg => seg.get_handles())
    segments.forEach(segment => {
      const vertices = segment.tesselate().map(vertex => new THREE.Vector3().fromArray(vertex))
      var geometry = new LineGeometry()
      geometry.setPositions(vertices.flatMap(vertex => vertex.toArray()))
      geometry.setColors(Array(vertices.length * 3).fill(1))
      var line = new Line2(geometry, lineMaterial)
      line.computeLineDistances()
      // line.scale.set(1, 1, 1)
      line.alcSelectable = true
      node.threeObjects.push(line)
      segment.three = line
      line.component = node
      line.element = segment
      this.scene.add(line)
    })
    if(recursive) node.get_children().each(child => loadTree(child, true, true))
    if(!notTop) this.render()
  }

  dispose() {
    rendering = false
    window.removeEventListener('resize', this.onWindowResize, false)
  }
}
