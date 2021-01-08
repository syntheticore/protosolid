import * as THREE from 'three'

export class ShadowCatcher extends THREE.Object3D {
  constructor(renderer, world) {
    super()
    this.renderer = renderer
    this.world = world

    this.sun = new THREE.DirectionalLight(0xdfebff, 1)
    this.sun.position.set(2, 50, 100)
    this.sun.castShadow = true
    this.sun.shadow.bias = - 0.00015
    this.sun.shadow.mapSize.width = 4096
    this.sun.shadow.mapSize.height = 4096
    this.add(this.sun)

    var groundGeo = new THREE.PlaneBufferGeometry(1.0, 1.0)
    // groundGeo.rotateX(- Math.PI / 2)
    this.ground = new THREE.Mesh(groundGeo, new THREE.ShadowMaterial({
      side: THREE.FrontSide,
      opacity: 0.2,
      depthWrite: false,
    }))
    this.ground.receiveShadow = true
    this.add(this.ground)
  }

  update() {
    var bbox = new THREE.Box3().setFromObject(this.world);
    this.ground.position.set(
      (bbox.min.x + bbox.max.x) / 2.0,
      (bbox.min.y + bbox.max.y) / 2.0,
      bbox.min.z,
    )
    bbox.getSize(this.ground.scale)
    this.ground.scale.multiplyScalar(2.5)
    let shadowFrustum = Math.max(this.ground.scale.x, this.ground.scale.y) / 2
    this.sun.shadow.camera = new THREE.OrthographicCamera(
      -shadowFrustum,
      shadowFrustum,
      shadowFrustum,
      -shadowFrustum,
      1, 200
    )
    this.renderer.shadowMap.needsUpdate = true
  }
}
