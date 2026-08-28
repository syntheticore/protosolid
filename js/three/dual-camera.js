import * as THREE from 'three'

export default class DualCamera {
  constructor() {
    this.perspective = new THREE.PerspectiveCamera(70, 1, 0.5, 10000)
    this.perspective.position.set(90, 90, 90)

    this.orthographic = new THREE.OrthographicCamera(-1, 1, 1, -1, -200, 10000)
    this.orthographic.position.set(0, 10, 0)
    this.orthographic.lookAt(0, 0, 0)

    this.active = this.orthographic
    this.aspect = 1
    this.orthoFrustumSize = 200
    this.perspectiveBlend = { value: 0 }
  }

  get isTransitioning() {
    return !!this.transition
  }

  setMode(mode, target, position) {
    const camera = mode == 'orthographic' ? this.orthographic : this.perspective
    if(this.transition?.to == camera) return false
    if(!this.transition && this.active == camera) return false

    const wasTransitioning = !!this.transition
    const fromProjection = this.active.projectionMatrix.clone()
    if(!wasTransitioning && this.active == this.perspective) {
      this.normalizePerspectiveProjection(fromProjection, this.active, target)
    }

    this.prepare(camera, target, position)
    const toProjection = camera.projectionMatrix.clone()
    if(camera == this.perspective) {
      this.normalizePerspectiveProjection(toProjection, camera, target)
    }

    this.active.projectionMatrix.copy(fromProjection)
    this.active.projectionMatrixInverse.copy(fromProjection).invert()
    this.transition = {
      to: camera,
      target: target.clone(),
      toProjection,
      toPerspective: camera == this.perspective ? 1 : 0,
    }
    return true
  }

  prepare(camera, target, position) {
    const offset = position.clone().sub(target)
    const distance = offset.length()
    if(!distance) return

    const direction = offset.normalize()
    if(camera == this.orthographic) {
      this.orthoFrustumSize = 2 * distance * Math.tan(
        THREE.MathUtils.degToRad(this.perspective.getEffectiveFOV()) / 2
      )
      camera.zoom = 1
      camera.position.copy(target).addScaledVector(direction, distance)
      camera.quaternion.copy(this.active.quaternion)
      this.updateOrthographic(target, true)
    } else {
      const visibleFrustumSize = this.orthoFrustumSize / this.orthographic.zoom
      const perspectiveDistance = visibleFrustumSize / (
        2 * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2)
      )
      camera.position.copy(target).addScaledVector(direction, perspectiveDistance)
      camera.quaternion.copy(this.active.quaternion)
      camera.aspect = this.aspect
      camera.updateProjectionMatrix()
    }
  }

  normalizePerspectiveProjection(projection, camera, target) {
    const distance = camera.position.distanceTo(target)
    if(distance) projection.multiplyScalar(1 / distance)
  }

  update() {
    const transition = this.transition
    if(!transition) return

    const remaining = transition.toPerspective - this.perspectiveBlend.value
    this.perspectiveBlend.value += remaining * 0.3
    const to = transition.toProjection.elements
    const projection = this.active.projectionMatrix
    for(let i = 0; i < projection.elements.length; i++) {
      projection.elements[i] += (to[i] - projection.elements[i]) * 0.3
    }
    this.active.projectionMatrixInverse.copy(projection).invert()

    if(Math.abs(transition.toPerspective - this.perspectiveBlend.value) >= 0.01) return false

    const camera = transition.to
    this.perspectiveBlend.value = transition.toPerspective
    camera.position.copy(this.active.position)
    camera.quaternion.copy(this.active.quaternion)
    this.transition = null
    return { camera, target: transition.target }
  }

  updateOrthographic(target, force=false) {
    if(!force && this.active != this.orthographic) return
    const distance = this.orthographic.position.distanceTo(target)
    if(!distance) return

    this.orthoFrustumSize = distance * 2 * Math.tan(
      THREE.MathUtils.degToRad(this.perspective.getEffectiveFOV()) / 2
    )
    this.orthographic.left = -0.5 * this.orthoFrustumSize * this.aspect
    this.orthographic.right = 0.5 * this.orthoFrustumSize * this.aspect
    this.orthographic.top = this.orthoFrustumSize / 2
    this.orthographic.bottom = -this.orthoFrustumSize / 2
    this.orthographic.updateProjectionMatrix()
  }

  normalizeOrthographicZoom(target) {
    const camera = this.orthographic
    if(this.active != camera || Math.abs(camera.zoom - 1) <= 1e-6) return

    const offset = camera.position.clone().sub(target)
    camera.position.copy(target).addScaledVector(offset, 1 / camera.zoom)
    camera.zoom = 1
    this.updateOrthographic(target)
  }

  resize(aspect) {
    this.aspect = aspect
    if(this.active == this.perspective) {
      this.perspective.aspect = aspect
    } else {
      this.orthographic.left = -0.5 * this.orthoFrustumSize * aspect
      this.orthographic.right = 0.5 * this.orthoFrustumSize * aspect
      this.orthographic.top = this.orthoFrustumSize / 2
      this.orthographic.bottom = -this.orthoFrustumSize / 2
    }
    this.active.updateProjectionMatrix()
  }

  setRaycaster(raycaster, coords) {
    if(!this.transition) return raycaster.setFromCamera(coords, this.active)

    const near = new THREE.Vector3(coords.x, coords.y, -1).unproject(this.active)
    const far = new THREE.Vector3(coords.x, coords.y, 1).unproject(this.active)
    raycaster.ray.set(near, far.sub(near).normalize())
    raycaster.camera = this.active
  }

  prepareMaterials(root) {
    root.traverse(object => {
      const objectMaterials = Array.isArray(object.material) ? object.material : [object.material]
      objectMaterials.filter(Boolean).forEach(material => this.prepareMaterial(material))
    })
  }

  prepareMaterial(material) {
    if(!material.isMeshStandardMaterial || material.alcProjectionPerspective) return

    const perspectiveBlend = this.perspectiveBlend
    const onBeforeCompile = material.onBeforeCompile
    const customProgramCacheKey = material.customProgramCacheKey.bind(material)
    material.onBeforeCompile = function(shader, renderer) {
      onBeforeCompile.call(this, shader, renderer)
      shader.uniforms.projectionPerspective = perspectiveBlend
      const lights = THREE.ShaderChunk.lights_fragment_begin.replace(
        'vec3 geometryViewDir = ( isOrthographic ) ? vec3( 0, 0, 1 ) : normalize( vViewPosition );',
        'vec3 geometryViewDir = normalize( mix( vec3( 0, 0, 1 ), normalize( vViewPosition ), projectionPerspective ) );'
      )
      shader.fragmentShader = shader.fragmentShader
        .replace('#include <common>', '#include <common>\nuniform float projectionPerspective;')
        .replace('#include <lights_fragment_begin>', lights)
    }
    material.customProgramCacheKey = () => `${customProgramCacheKey()}:projection-perspective`
    material.alcProjectionPerspective = true
    material.needsUpdate = true
  }
}
