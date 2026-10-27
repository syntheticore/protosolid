import * as THREE from 'three'
import { FullScreenQuad } from 'three/examples/jsm/postprocessing/Pass.js'

const RESOLUTION = 512
// Space around the model's footprint, relative to its size, for the blur to fade out
const MARGIN = 0.6
// Blur sample spacing in texels. Samples blur the hard silhouette edge into steps,
// which stay invisible as long as there are many of them.
const BLUR_STEP = 1.1
const OPACITY = 0.35

const vertexShader = `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`

// A soft shadow below the model, as if lit evenly from above. It ignores the
// scene's lights: face silhouettes are rendered from straight above, blurred
// and laid onto a ground plane under the model.
export default class GroundShadow {
  constructor(renderer, world) {
    this.renderer = renderer
    this.world = world

    // Looking down with -Z up puts +X right, matching the ground plane's UVs
    this.camera = new THREE.OrthographicCamera()
    this.camera.up.set(0, 0, -1)

    this.maskScene = new THREE.Scene()
    this.maskMaterial = new THREE.MeshBasicMaterial({
      color: 0xffffff,
      side: THREE.DoubleSide,
      toneMapped: false,
    })

    this.mask = new THREE.WebGLRenderTarget(RESOLUTION, RESOLUTION)
    this.blurX = new THREE.WebGLRenderTarget(RESOLUTION, RESOLUTION, { depthBuffer: false, type: THREE.HalfFloatType })
    this.blurY = new THREE.WebGLRenderTarget(RESOLUTION, RESOLUTION, { depthBuffer: false, type: THREE.HalfFloatType })
    this.clearColor = new THREE.Color()

    this.blurMaterial = new THREE.ShaderMaterial({
      uniforms: {
        source: { value: null },
        direction: { value: new THREE.Vector2() },
      },
      vertexShader,
      fragmentShader: `
        uniform sampler2D source;
        uniform vec2 direction;
        varying vec2 vUv;
        void main() {
          vec4 value = vec4(0.0);
          float weightSum = 0.0;
          for(int i = -48; i <= 48; i++) {
            float weight = exp(-float(i * i) / 512.0);
            value += texture2D(source, vUv + direction * float(i)) * weight;
            weightSum += weight;
          }
          gl_FragColor = value / weightSum;
        }
      `,
      depthTest: false,
      depthWrite: false,
      blending: THREE.NoBlending,
      toneMapped: false,
    })
    this.quad = new FullScreenQuad(this.blurMaterial)

    const geometry = new THREE.PlaneGeometry(1, 1)
    geometry.rotateX(-Math.PI / 2)
    // Alpha maps are read from the green channel, which holds the blurred coverage
    this.ground = new THREE.Mesh(geometry, new THREE.MeshBasicMaterial({
      color: 0x000000,
      alphaMap: this.blurY.texture,
      transparent: true,
      opacity: OPACITY,
      depthWrite: false,
      toneMapped: false,
    }))
    this.ground.visible = false
  }

  update() {
    const bounds = new THREE.Box3()
    let count = 0
    this.world.updateMatrixWorld()
    this.world.traverseVisible(source => {
      if(!source.castShadow || !source.alcTypes?.includes('face')) return
      let mesh = this.maskScene.children[count++]
      if(!mesh) {
        mesh = new THREE.Mesh(source.geometry, this.maskMaterial)
        mesh.matrixAutoUpdate = false
        this.maskScene.add(mesh)
      }
      mesh.geometry = source.geometry
      mesh.matrix.copy(source.matrixWorld)
      bounds.expandByObject(source)
    })
    this.maskScene.remove(...this.maskScene.children.slice(count))
    this.ground.visible = count > 0
    if(!count) return

    const center = bounds.getCenter(new THREE.Vector3())
    const size = bounds.getSize(new THREE.Vector3())
    const extent = Math.max(size.x, size.z) * (1 + 2 * MARGIN) / 2
    this.camera.left = this.camera.bottom = -extent
    this.camera.right = this.camera.top = extent
    this.camera.near = 0
    this.camera.far = size.y + 2 * extent
    this.camera.position.set(center.x, bounds.max.y + extent, center.z)
    this.camera.lookAt(center.x, bounds.min.y, center.z)
    this.camera.updateProjectionMatrix()
    this.camera.updateMatrixWorld()

    // Slightly below the model, so its bottom faces don't fight with the shadow
    this.ground.position.set(center.x, bounds.min.y - extent * 0.002, center.z)
    this.ground.scale.set(extent * 2, 1, extent * 2)

    const renderer = this.renderer
    const target = renderer.getRenderTarget()
    const clearAlpha = renderer.getClearAlpha()
    renderer.getClearColor(this.clearColor)
    renderer.setClearColor(0, 0)
    renderer.setRenderTarget(this.mask)
    renderer.render(this.maskScene, this.camera)

    this.blurMaterial.uniforms.source.value = this.mask.texture
    this.blurMaterial.uniforms.direction.value.set(BLUR_STEP / RESOLUTION, 0)
    renderer.setRenderTarget(this.blurX)
    this.quad.render(renderer)
    this.blurMaterial.uniforms.source.value = this.blurX.texture
    this.blurMaterial.uniforms.direction.value.set(0, BLUR_STEP / RESOLUTION)
    renderer.setRenderTarget(this.blurY)
    this.quad.render(renderer)

    renderer.setRenderTarget(target)
    renderer.setClearColor(this.clearColor, clearAlpha)
  }
}
