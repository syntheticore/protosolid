import * as THREE from 'three'
import { FullScreenQuad } from 'three/examples/jsm/postprocessing/Pass.js'
import materials from '../materials.js'

const vertexShader = `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`

export default class HoverGlow {
  constructor(renderer, scene) {
    this.renderer = renderer
    this.scene = scene
    this.maskScene = new THREE.Scene()
    this.maskMaterial = new THREE.MeshBasicMaterial({
      color: 'white',
      side: THREE.DoubleSide,
      blending: THREE.NoBlending,
      toneMapped: false,
    })
    this.occluderMaterial = this.maskMaterial.clone()
    this.occluderMaterial.color.set(0)

    this.mask = new THREE.WebGLRenderTarget(1, 1, { samples: 8, resolveDepthBuffer: false })
    this.blurX = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false, type: THREE.HalfFloatType })
    this.blurY = new THREE.WebGLRenderTarget(1, 1, { depthBuffer: false, type: THREE.HalfFloatType })
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
          float value = 0.0;
          float weightSum = 0.0;
          for(int i = -12; i <= 12; i++) {
            float weight = exp(-float(i * i) / 50.0);
            value += texture2D(source, vUv + direction * float(i)).r * weight;
            weightSum += weight;
          }
          gl_FragColor = vec4(vec3(value / weightSum), 1.0);
        }
      `,
      depthTest: false,
      depthWrite: false,
      blending: THREE.NoBlending,
      toneMapped: false,
    })

    this.glowMaterial = new THREE.ShaderMaterial({
      uniforms: {
        mask: { value: this.mask.texture },
        blurred: { value: this.blurY.texture },
        color: { value: materials.highlightSurface.color },
      },
      vertexShader,
      fragmentShader: `
        uniform sampler2D mask;
        uniform sampler2D blurred;
        uniform vec3 color;
        varying vec2 vUv;
        void main() {
          float halo = texture2D(blurred, vUv).r * (1.0 - texture2D(mask, vUv).r);
          float heat = smoothstep(0.3, 0.55, halo) * 0.3;
          vec3 glowColor = mix(color, vec3(0.9, 0.95, 1.0), heat);
          gl_FragColor = vec4(glowColor, clamp(halo * 2.0, 0.0, 0.8));
          #include <colorspace_fragment>
        }
      `,
      transparent: true,
      depthTest: false,
      depthWrite: false,
      toneMapped: false,
    })
    this.quad = new FullScreenQuad(this.blurMaterial)
    this.resize()
  }

  resize() {
    const size = this.renderer.getSize(new THREE.Vector2())
    const drawingSize = this.renderer.getDrawingBufferSize(new THREE.Vector2())
    // Supersample the silhouette independently of the normal scene render.
    this.mask.setSize(Math.ceil(drawingSize.x * 1.5), Math.ceil(drawingSize.y * 1.5))
    this.blurX.setSize(size.x, size.y)
    this.blurY.setSize(this.blurX.width, this.blurX.height)
  }

  render(camera) {
    let count = 0
    // Reuse lightweight proxies so the mask never alters scene materials,
    // visibility, or transparency. Only solid faces participate in occlusion.
    this.scene.traverseVisible(source => {
      if(!source.alcTypes?.includes('face')) return
      let mesh = this.maskScene.children[count++]
      if(!mesh) {
        mesh = new THREE.Mesh(source.geometry, this.occluderMaterial)
        mesh.matrixAutoUpdate = false
        this.maskScene.add(mesh)
      }
      mesh.geometry = source.geometry
      mesh.material = source.alcHovered ? this.maskMaterial : this.occluderMaterial
      mesh.matrix.copy(source.matrixWorld)
    })
    this.maskScene.remove(...this.maskScene.children.slice(count))

    const renderer = this.renderer
    const target = renderer.getRenderTarget()
    const autoClear = renderer.autoClear
    const clearAlpha = renderer.getClearAlpha()
    renderer.getClearColor(this.clearColor)
    renderer.autoClear = false
    renderer.setClearColor(0, 0)
    renderer.setRenderTarget(this.mask)
    renderer.clear()
    renderer.render(this.maskScene, camera)

    this.quad.material = this.blurMaterial
    this.blurMaterial.uniforms.source.value = this.mask.texture
    this.blurMaterial.uniforms.direction.value.set(2 / this.blurX.width, 0)
    renderer.setRenderTarget(this.blurX)
    this.quad.render(renderer)
    this.blurMaterial.uniforms.source.value = this.blurX.texture
    this.blurMaterial.uniforms.direction.value.set(0, 2 / this.blurY.height)
    renderer.setRenderTarget(this.blurY)
    this.quad.render(renderer)

    // Blend only the halo without reprocessing the scene's transparency.
    renderer.setRenderTarget(target)
    this.quad.material = this.glowMaterial
    this.quad.render(renderer)
    renderer.setClearColor(this.clearColor, clearAlpha)
    renderer.autoClear = autoClear
  }

  dispose() {
    this.mask.dispose()
    this.blurX.dispose()
    this.blurY.dispose()
    this.maskMaterial.dispose()
    this.occluderMaterial.dispose()
    this.blurMaterial.dispose()
    this.glowMaterial.dispose()
    this.quad.dispose()
  }
}
