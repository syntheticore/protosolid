import * as THREE from 'three'

import ArrowControls from './arrowControls.js'
import { rotationFromNormal } from './utils.js'

export class LengthGizmo extends ArrowControls {
  constructor(startPosition, direction, startSide, startValue, cb) {
    super(startPosition, new THREE.Euler().setFromRotationMatrix(rotationFromNormal(direction)))
    this.direction = direction

    this.space = 'local'
    this.showX = false
    this.showY = false

    this.set(startValue, startSide)

    this.addEventListener('translation', (e) => {
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


export class AngleGizmo extends ArrowControls {
  constructor(startPosition, startRotation, startValue, cb) {
    super(startPosition, startRotation)

    this.space = 'local'
    this.setMode('rotate')
    this.rotationSnap = THREE.MathUtils.degToRad(5)
    this.showX = false
    this.showY = false

    this.set(startValue)

    this.addEventListener('rotation', (e) => {
      cb(Math.abs(e.value.z) * 57.2958) // radians -> degrees
    })
  }

  set(value) {
    this.dummy.rotation.set(0.0, 0.0, value / 57.2958)
  }
}
