import * as THREE from 'three'

import { matrix2three } from './utils.js'

export default class SketchPlane extends THREE.Object3D {
  constructor(camera) {
    super()

    // Grid
    this.cachedVec = new THREE.Vector3()
    this.update(camera)

    // Axis Helper
    this.add(new THREE.AxesHelper(10.0))

    // Click Catcher
    var groundGeo = new THREE.PlaneBufferGeometry(99999, 99999)
    var ground = new THREE.Mesh(groundGeo, new THREE.MeshBasicMaterial({
      side: THREE.DoubleSide,
      transparent: true,
      opacity: 0.0,
      depthWrite: false,
    }))
    ground.alcProjectable = true
    this.add(ground)

    this.visible = false
  }

  setPlane(plane) {
    this.position.setFromMatrixPosition(plane)
    this.rotation.setFromRotationMatrix(plane)
  }

  update(camera) {
    const relPos = camera.position.clone().sub(this.position)
    const up = this.up.clone().applyEuler(this.rotation)
    const dist = Math.abs(up.dot(relPos))
    const dot = Math.abs(up.dot(relPos.normalize()))
    const size = Math.pow(10, String(Math.round(10 * dist / 4)).length) / 10
    if(size != this.lastSize && dot > 0.5) {
      this.remove(this.grid)
      const multiple = 3
      this.grid = new THREE.GridHelper(size * multiple, 10 * multiple)
      this.grid.rotateX(Math.PI / 2) // Needed because of custom world up vector
      this.grid.material.opacity = 0.1
      this.grid.material.transparent = true
      this.add(this.grid)
      this.lastSize = size
    }
    // Move grid towards camera to avoid z-fighting
    const lookVec = camera.getWorldDirection(this.cachedVec)
    const facing = this.up.dot(lookVec)
    this.grid.position.z = -0.004 * dist * facing
  }
}
