import * as THREE from 'three'

import ArrowControls from './arrowControls.js'
import { rotationFromNormal } from './utils.js'

export class LengthGizmo extends ArrowControls {
  constructor(startPosition, vec, startValue, cb) {
    super(startPosition)

    this.space = 'local'
    this.showX = false
    this.showY = false

    this.dummy.applyQuaternion(new THREE.Quaternion().setFromRotationMatrix(rotationFromNormal(vec)))
    this.set(startValue)

    this.addEventListener('value', (e) => cb(Math.abs(e.value.z)) )
  }

  set(value) {
    this.dummy.position.z = this.startPosition.z + value
  }
}
