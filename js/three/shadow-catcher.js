import * as THREE from 'three'

export default class ShadowCatcher extends THREE.Object3D {
  constructor(renderer, world) {
    super()
    this.renderer = renderer
    this.world = world

    this.sun = new THREE.DirectionalLight(0xdfebff, 1)
    this.sun.position.set(10, 250, 500)
    this.sun.castShadow = true
    this.sun.shadow.bias = -0.0002
    this.sun.shadow.mapSize.width = 4096
    this.sun.shadow.mapSize.height = 4096
    this.add(this.sun)

    var groundGeo = new THREE.PlaneGeometry(1.0, 1.0)
    groundGeo.rotateX(- Math.PI / 2)
    this.ground = new THREE.Mesh(groundGeo, new THREE.ShadowMaterial({
      side: THREE.FrontSide,
      opacity: 0.2,
      depthWrite: false,
    }))
    this.ground.receiveShadow = true
    this.add(this.ground)
  }

  update() {
    if(!this.world.children.length) return
    var bbox = new THREE.Box3().setFromObject(this.world, false)
    this.ground.position.set(
      (bbox.min.x + bbox.max.x) / 2.0,
      bbox.min.y - 0.1,
      (bbox.min.z + bbox.max.z) / 2.0,
    )
    bbox.getSize(this.ground.scale)
    this.ground.scale.multiplyScalar(7)
    let shadowFrustum = Math.max(this.ground.scale.x, this.ground.scale.y) / 2
    this.sun.shadow.camera = new THREE.OrthographicCamera(
      -shadowFrustum,
      shadowFrustum,
      shadowFrustum,
      -shadowFrustum,
      1, 2000
    )
    this.renderer.shadowMap.needsUpdate = true
  }
}
