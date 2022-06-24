import * as THREE from 'three'

import ArrowControls from './arrowControls.js'
import { rotationFromNormal } from './utils.js'

export class LengthGizmo extends ArrowControls {
  constructor(startPosition, direction, startValue, cb) {
    super(startPosition)
    this.direction = direction

    this.space = 'local'
    this.showX = false
    this.showY = false

    this.dummy.applyQuaternion(
      new THREE.Quaternion().setFromRotationMatrix(rotationFromNormal(direction))
    )
    this.set(startValue)

    this.addEventListener('value', (e) => cb(e.value.length()) )
  }

  set(value) {
    this.dummy.position.copy(this.direction).multiplyScalar(value).add(this.startPosition).sub(this.position)
  }
}

Checken seit wann Halbmonde nicht mehr geBREPt werden
