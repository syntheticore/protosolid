import * as THREE from 'three'
import { WebGLPathTracer } from 'three-gpu-pathtracer'
import { CubeToEquirectGenerator } from 'three-gpu-pathtracer/src/utils/CubeToEquirectGenerator.js'

const MAX_SAMPLES = 256


export default class PathTracingViewport {
  constructor(renderer) {
    this.renderer = renderer
    this.pathTracer = null
    this.frame = null
    this.active = false
    this.sceneSignature = null
    this.cameraSignature = null
  }

  setEnvironment(cubeTexture) {
    this.environment?.dispose()
    this.cubeEnvironment?.dispose()
    this.environment = null
    this.cubeEnvironment = cubeTexture
    this.invalidate()
  }

  invalidate() {
    this.sceneSignature = null
    this.cameraSignature = null
    this.stop()
  }

  ensurePathTracer() {
    if(this.pathTracer || !this.renderer.capabilities.isWebGL2) return this.pathTracer

    // Constructing the render targets and compiling the tracing shader is
    // expensive, so keep initialization lazy until the mode is first used.
    this.pathTracer = new WebGLPathTracer(this.renderer)
    this.pathTracer.bounces = 4
    this.pathTracer.tiles.set(1, 1)
    this.pathTracer.renderDelay = 0
    this.pathTracer.minSamples = 1
    this.pathTracer.fadeDuration = 0
    this.pathTracer.rasterizeScene = false
    return this.pathTracer
  }

  ensureEnvironment() {
    if(this.environment || !this.cubeEnvironment) return

    const cubeTexture = this.cubeEnvironment
    const generator = new CubeToEquirectGenerator(this.renderer)
    this.environment = generator.generate(cubeTexture)
    generator._quad.material.dispose()
    generator.dispose()
    cubeTexture.dispose()
    this.cubeEnvironment = null
  }

  collectMeshes(scene) {
    const meshes = []
    scene.traverse(object => {
      if(
        object.isMesh &&
        object.visible &&
        object.alcTypes?.includes('face')
      ) meshes.push(object)
    })
    return meshes
  }

  materialSignature(material) {
    if(Array.isArray(material)) return material.map(item => this.materialSignature(item)).join(';')
    return [
      material.uuid,
      material.version,
      material.color?.getHexString(),
      material.emissive?.getHexString(),
      material.emissiveIntensity,
      material.roughness,
      material.metalness,
      material.clearcoat,
      material.clearcoatRoughness,
      material.transmission,
      material.ior,
      material.thickness,
      material.opacity,
    ].join(':')
  }

  getSceneSignature(meshes) {
    return [
      this.environment?.uuid,
      ...meshes.map(mesh => {
        const position = mesh.geometry.getAttribute('position')
        const material = mesh.alcPathTracingMaterial || mesh.material
        return [
          mesh.uuid,
          mesh.geometry.uuid,
          position?.version,
          this.materialSignature(material),
          ...mesh.matrixWorld.elements,
        ].join(',')
      }),
    ].join('|')
  }

  getCameraSignature(camera) {
    camera.updateMatrixWorld()
    return [
      camera.uuid,
      ...camera.matrixWorld.elements,
      ...camera.projectionMatrix.elements,
    ].join(',')
  }

  makeScene(meshes) {
    const scene = new THREE.Scene()
    // Keep the CSS viewport background while using the HDR environment for
    // physically based illumination and reflections.
    scene.environment = this.environment || null
    scene.background = null

    meshes.forEach(source => {
      const mesh = new THREE.Mesh(
        source.geometry,
        source.alcPathTracingMaterial || source.material,
      )
      mesh.matrixAutoUpdate = false
      mesh.matrix.copy(source.matrixWorld)
      mesh.matrixWorldNeedsUpdate = true
      scene.add(mesh)
    })
    return scene
  }

  render(scene, camera) {
    this.active = true
    this.ensureEnvironment()
    const pathTracer = this.ensurePathTracer()
    if(!pathTracer) {
      this.active = false
      return false
    }

    scene.updateMatrixWorld(true)
    const meshes = this.collectMeshes(scene)
    if(!meshes.length) {
      this.active = false
      return false
    }

    const sceneSignature = this.getSceneSignature(meshes)
    const cameraSignature = this.getCameraSignature(camera)
    if(sceneSignature != this.sceneSignature) {
      this.scene = this.makeScene(meshes)
      pathTracer.setScene(this.scene, camera)
      this.sceneSignature = sceneSignature
      this.cameraSignature = cameraSignature
    } else if(cameraSignature != this.cameraSignature) {
      pathTracer.setCamera(camera)
      this.cameraSignature = cameraSignature
    }

    if(pathTracer.isCompiling || pathTracer.samples < MAX_SAMPLES) this.queueFrame()
    return true
  }

  queueFrame() {
    if(this.frame !== null) return
    this.frame = requestAnimationFrame(() => {
      this.frame = null
      if(!this.active || !this.pathTracer) return

      this.pathTracer.renderSample()
      if(this.pathTracer.isCompiling || this.pathTracer.samples < MAX_SAMPLES) {
        this.queueFrame()
      }
    })
  }

  stop() {
    this.active = false
    if(this.frame === null) return
    cancelAnimationFrame(this.frame)
    this.frame = null
  }

  dispose() {
    this.stop()
    if(this.pathTracer) {
      this.pathTracer.dispose()
      this.pathTracer._lowResPathTracer.dispose()
      this.pathTracer._internalBackground?.dispose()
      this.pathTracer._colorBackground?.dispose()
    }
    this.environment?.dispose()
    this.cubeEnvironment?.dispose()
  }
}
