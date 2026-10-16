import * as THREE from 'three'
import { Line2 } from 'three/examples/jsm/lines/Line2.js'
import materials from '../materials.js'

export default class HiddenLine extends Line2 {
  constructor(line) {
    super(line.geometry, materials.hiddenLine.clone())
    this.line = line
    this.dashPosition = new THREE.Vector4()
    this.visible = false
    // Compare against face depth before visible edges and sketch overlays.
    this.renderOrder = -1
    this.raycast = () => {}
  }

  onBeforeRender(renderer, scene, camera) {
    super.onBeforeRender(renderer)
    const material = this.material
    material.color.copy(this.line.material.color)
    material.toneMapped = this.line.material.toneMapped
    // Keep dashes in screen pixels through zoom and projection changes.
    const center = this.geometry.boundingSphere.center
    this.dashPosition.set(center.x, center.y, center.z, 1)
      .applyMatrix4(this.matrixWorld)
      .applyMatrix4(camera.matrixWorldInverse)
      .applyMatrix4(camera.projectionMatrix)
    material.dashScale = material.resolution.y * camera.projectionMatrix.elements[5] / (2 * Math.abs(this.dashPosition.w))
  }
}
