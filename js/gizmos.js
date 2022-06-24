import * as THREE from 'three'

import ArrowControls from './arrowControls.js'
import { rotationFromNormal } from './utils.js'

export class LengthGizmo extends ArrowControls {
  constructor(startPosition, direction, startSide, startValue, cb) {
    super(startPosition)
    this.direction = direction

    this.space = 'local'
    this.showX = false
    this.showY = false

    this.dummy.rotation.setFromRotationMatrix(rotationFromNormal(direction))
    this.set(startValue, startSide)

    this.addEventListener('value', (e) => {
      cb(e.value.length(), e.value.dot(direction) > 0)
    })

    this.addEventListener('mouseUp', this.updateOrientation)
  }

  set(value, side) {
    this.sign = (side ? 1 : -1)
    this.dummy.position.copy(this.direction).multiplyScalar(value * this.sign).add(this.startPosition).sub(this.position)
    if(!this.dragging) this.updateOrientation()
  }

  updateOrientation() {
    this.dummy.rotation.setFromRotationMatrix(rotationFromNormal(this.direction.clone().multiplyScalar(this.sign)))
  }
}
