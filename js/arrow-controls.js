import * as THREE from 'three'
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js'

export default class ArrowControls extends TransformControls {
  constructor(startPosition, startRotation) {
    super(window.alcRenderer.camera, window.alcRenderer.canvas)
    this.startPosition = startPosition

    this.space = 'world'
    this.translationSnap = 1.0
    // this.rotationSnap = THREE.MathUtils.degToRad(10)
    // this.setMode('rotate')

    // Keep size constant
    const canvas = window.alcRenderer.canvas
    this.size = 1200 / (canvas.offsetWidth + canvas.offsetHeight)

    // Use invisible dummy to measure change
    this.dummy = new THREE.Object3D()
    this.dummy.position.copy(startPosition)
    this.dummy.rotation.copy(startRotation)
    window.alcRenderer.scene.add(this.dummy)
    this.attach(this.dummy)

    // Hack arrow colors
    const line = this.children[0].children[0].children[6]
    const mesh1 = this.children[0].children[0].children[7]
    const mesh2 = this.children[0].children[0].children[8]
    line.material.color.set('white')
    mesh1.material.color.set('white')
    mesh2.material.color.set('white')

    // Dispatch value events with change vector
    let vec = new THREE.Vector3()
    let lastVec = vec.clone()
    let rot = new THREE.Vector3()
    let lastRot = rot.clone()
    this.addEventListener('objectChange', () => {
      vec.copy(this.dummy.position).sub(startPosition)
      rot.fromArray(this.dummy.rotation.toArray()).sub(new THREE.Vector3().fromArray(startRotation.toArray()))
      // Only dispatch when actual snapped value changed
      if(!vec.equals(lastVec)) this.dispatchEvent({ type: 'translation', value: vec });
      if(!rot.equals(lastRot)) this.dispatchEvent({ type: 'rotation', value: rot });
      lastVec.copy(vec)
      lastRot.copy(rot)
    })
  }

  dispose() {
    super.dispose()
    window.alcRenderer.scene.remove(this.dummy)
  }
}
