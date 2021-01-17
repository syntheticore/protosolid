import * as THREE from 'three'

export class SketchPlane extends THREE.Object3D {
  constructor(camera) {
    super()

    // Grid
    this.update(camera)

    // Axis Helper
    this.add(new THREE.AxesHelper(0.5));

    // Click Catcher
    var groundGeo = new THREE.PlaneBufferGeometry(99999, 99999)
    // groundGeo.rotateX(- Math.PI / 2)
    var ground = new THREE.Mesh(groundGeo, new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      transparent: true,
      opacity: 0.0,
      depthWrite: false,
    }))
    ground.alcProjectable = true
    this.add(ground)
  }

  update(camera) {
    const pos = this.grid && this.grid.position || new THREE.Vector3(0.0, 0.0, 0.0)
    const dist = (pos).distanceTo(camera.position)
    const size = Math.pow(10, String(Math.round(dist / 4)).length)
    if(size != this.lastSize) {
      this.remove(this.grid)
      const multiple = 2
      this.grid = new THREE.GridHelper(size * multiple, 10 * multiple)
      this.grid.rotateX(Math.PI / 2)
      this.grid.material.opacity = 0.1
      this.grid.material.transparent = true
      this.add(this.grid)
      this.lastSize = size
    }
    this.grid.position.z = 0.002 * dist
  }
}
