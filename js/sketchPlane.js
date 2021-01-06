import * as THREE from 'three'

export class SketchPlane extends THREE.Object3D {
  constructor() {
    super()
    var groundGeo = new THREE.PlaneBufferGeometry(60, 60)
    // groundGeo.rotateX(- Math.PI / 2)
    var ground = new THREE.Mesh(groundGeo, new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      transparent: true,
      opacity: 0.0,
      depthWrite: false,
    }))
    ground.alcProjectable = true
    this.add(ground)

    // Grid
    var grid = new THREE.GridHelper(20, 20)
    grid.rotateX(Math.PI / 2)
    grid.material.opacity = 0.1
    grid.material.transparent = true
    // grid.material.depthWrite = false
    grid.position.z = 0.0001
    this.add(grid)

    // Axis Helper
    this.add(new THREE.AxesHelper(0.5));
  }
}
