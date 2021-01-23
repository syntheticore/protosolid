import * as THREE from 'three'
import { TransformControls } from 'three/examples/jsm/controls/TransformControls.js'

export default class ArrowControls extends TransformControls {
  constructor(startPosition) {
    super(window.alcRenderer.camera, window.alcRenderer.canvas)
    this.startPosition = startPosition

    const canvas = window.alcRenderer.canvas
    this.size = 1200 / (canvas.offsetWidth + canvas.offsetHeight)
    this.space = 'world'
    this.translationSnap = 1.0

    // this.rotationSnap = THREE.MathUtils.degToRad(10)
    // this.setMode('rotate')

    this.dummy = new THREE.Object3D()
    this.dummy.position.copy(startPosition)
    window.alcRenderer.scene.add(this.dummy)
    this.attach(this.dummy)

    let vec = new THREE.Vector3()
    let lastVec = vec.clone()
    this.addEventListener('objectChange', () => {
      vec.copy(this.dummy.position).sub(startPosition)
      if(!vec.equals(lastVec)) this.dispatchEvent({ type: 'value', value: vec });
      lastVec.copy(vec)
    })
  }

  dispose() {
    super.dispose()
    window.alcRenderer.scene.remove(this.dummy)
  }
}
