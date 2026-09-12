import * as THREE from 'three'

import ArrowControls from './arrow-controls.js'
import { TransformControls } from './transform-controls.js'
import { rotationFromNormal } from '../core/utils.js'


export class TransformGizmo extends THREE.Object3D {
  constructor(transform, onChange, space='local') {
    super()

    const renderer = window.alcRenderer
    this.dummy = new THREE.Object3D()
    this.add(this.dummy)

    this.translationControl = new TransformControls(renderer.camera, renderer.canvas)
    this.rotationControl = new TransformControls(renderer.camera, renderer.canvas)
    this.rotationControl.setMode('rotate')
    this.controls = [this.translationControl, this.rotationControl]

    const size = 1200 / (renderer.canvas.offsetWidth + renderer.canvas.offsetHeight)
    this.controls.forEach(control => {
      control.setSpace(space)
      control.setSize(size)
      control.attach(this.dummy)
      this.add(control)

      control.addEventListener('change', () => this.dispatchEvent({ type: 'change' }))
      control.addEventListener('objectChange', () => {
        onChange?.(this.getTransform(), false)
        this.dispatchEvent({ type: 'objectChange' })
      })
      control.addEventListener('dragging-changed', event => {
        // Only one half of the combined control can drag at a time. Disable
        // the other half so overlapping pickers cannot claim the gesture.
        this.controls.forEach(other => {
          if(other != control) other.enabled = this.enabled && !event.value
        })
        this.dispatchEvent({ type: 'dragging-changed', value: event.value })
      })
      control.addEventListener('mouseDown', () => {
        this.dispatchEvent({ type: 'mouseDown' })
      })
      control.addEventListener('mouseUp', () => {
        this.dispatchEvent({ type: 'mouseUp' })
      })
    })

    this.translationControl.setTranslationSnap(1)
    this.rotationControl.setRotationSnap(THREE.MathUtils.degToRad(5))
    this.setTransform(transform)
    this.enabled = true
  }

  get camera() { return this.controls?.[0]?.camera }
  set camera(camera) { this.controls?.forEach(control => { control.camera = camera }) }

  get enabled() { return this._enabled }
  set enabled(enabled) {
    this._enabled = enabled
    this.controls?.forEach(control => {
      if(!this.dragging || control.dragging) control.enabled = enabled
    })
  }

  get axis() {
    return this.controls?.find(control => control.axis)?.axis || null
  }

  get dragging() {
    return !!this.controls?.some(control => control.dragging)
  }

  getTransform() {
    this.dummy.updateMatrix()
    return this.dummy.matrix.clone()
  }

  setTransform(transform) {
    transform.decompose(this.dummy.position, this.dummy.quaternion, this.dummy.scale)
    this.dummy.updateMatrix()
  }

  dispose() {
    this.controls.forEach(control => control.dispose())
    super.dispose()
  }
}

export class LengthGizmo extends ArrowControls {
  constructor(startPosition, direction, startValue, startSide, cb) {
    super(startPosition, new THREE.Euler().setFromRotationMatrix(rotationFromNormal(direction)))
    this.direction = direction

    this.space = 'local'
    this.showX = false
    this.showY = false

    this.set(startValue, startSide)

    this.addEventListener('translation', (e) => {
      const value = Math.round(e.value.length() / this.translationSnap) * this.translationSnap
      cb(value, e.value.dot(direction) > 0)
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
      cb(THREE.MathUtils.radToDeg(Math.abs(e.value.z)))
    })
  }

  set(value) {
    this.dummy.rotation.set(0.0, 0.0, THREE.MathUtils.degToRad(value))
  }
}
