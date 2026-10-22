import * as THREE from 'three'

import ArrowControls from './arrow-controls.js'
import { TransformControls } from './transform-controls.js'
import { rotationFromNormal } from '../core/utils.js'
import { snapTranslation } from './gizmo-snapping.js'


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

    this.translationControl.adaptiveTranslationSnap = true
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

export class CanvasPositionGizmo extends TransformControls {
  constructor(frame, x, y, onChange) {
    const renderer = window.alcRenderer
    super(renderer.camera, renderer.canvas)

    // Keep the position in canvas-plane coordinates, including when its
    // component is transformed in an assembly.
    this.frame = new THREE.Object3D()
    this.dummy = new THREE.Object3D()
    this.frame.add(this.dummy)
    renderer.scene.add(this.frame)
    this.setFrame(frame)
    this.setPosition(x, y)
    this.setSpace('local')
    this.showZ = false
    this.adaptiveTranslationSnap = true
    this.setSize(1200 / (renderer.canvas.offsetWidth + renderer.canvas.offsetHeight))
    this.attach(this.dummy)

    this.addEventListener('objectChange', () => {
      onChange(Number(this.dummy.position.x.toPrecision(15)), Number(this.dummy.position.y.toPrecision(15)))
    })
  }

  setFrame(transform) {
    transform.decompose(this.frame.position, this.frame.quaternion, this.frame.scale)
    this.frame.updateMatrixWorld(true)
  }

  setPosition(x, y) {
    this.dummy.position.set(Number(x) || 0, Number(y) || 0, 0)
    this.frame.updateMatrixWorld(true)
  }

  dispose() {
    super.dispose()
    window.alcRenderer.scene.remove(this.frame)
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
      const value = snapTranslation(e.value.length(), this.translationSnap)
      cb(value, e.value.dot(direction) > 0)
    })

    this.addEventListener('mouseUp', this.updateOrientation)
  }

  set(value, side) {
    this.sign = (side ? 1 : -1)
    this.dummy.position.copy(this.direction).multiplyScalar(value * this.sign).add(this.startPosition).sub(this.position)
    if(!this.dragging) this.updateOrientation()
  }

  setFrame(center, direction) {
    this.startPosition.copy(center)
    this.translationSnapOrigin.copy(center)
    this.direction.copy(direction)
  }

  updateOrientation() {
    this.dummy.rotation.setFromRotationMatrix(rotationFromNormal(this.direction.clone().multiplyScalar(this.sign)))
  }
}


export class AngleGizmo extends ArrowControls {
  constructor(startPosition, startRotation, startValue, cb) {
    super(startPosition, startRotation)
    // Snap the measured angle independently of the face/component rotation.
    this.frame = new THREE.Object3D()
    window.alcRenderer.scene.add(this.frame)
    this.frame.add(this.dummy)
    this.dummy.position.set(0, 0, 0)
    this.setFrame(startPosition, startRotation)

    this.space = 'local'
    this.setMode('rotate')
    this.rotationSnap = THREE.MathUtils.degToRad(5)
    this.showX = false
    this.showY = false

    this.set(startValue)

    this.addEventListener('mouseDown', () => {
      this.dragStartValue = Math.round(THREE.MathUtils.degToRad(this.value) / this.rotationSnap) * this.rotationSnap
    })
    this.addEventListener('objectChange', () => {
      // Euler angles wrap at 180 degrees; the drag delta preserves full turns.
      cb(THREE.MathUtils.radToDeg(this.dragStartValue + this.rotationAngle))
    })
  }

  setFrame(center, rotation) {
    this.startPosition.copy(center)
    this.frame.position.copy(center)
    this.frame.rotation.copy(rotation)
  }

  set(value) {
    this.value = value
    this.dummy.rotation.set(0.0, 0.0, THREE.MathUtils.degToRad(value))
  }

  dispose() {
    super.dispose()
    window.alcRenderer.scene.remove(this.frame)
  }
}


export function faceGizmoFrame({ positions, normals }, target) {
  const triangle = new THREE.Triangle()
  const point = new THREE.Vector3()
  const center = new THREE.Vector3()
  const direction = new THREE.Vector3()
  let closest = Infinity
  for(let i = 0; i < positions.length; i += 9) {
    triangle.a.fromArray(positions, i)
    triangle.b.fromArray(positions, i + 3)
    triangle.c.fromArray(positions, i + 6)
    const distance = triangle.closestPointToPoint(target, point).distanceToSquared(target)
    if(distance < closest) {
      closest = distance
      center.copy(point)
      direction.fromArray(normals, i)
        .add(new THREE.Vector3().fromArray(normals, i + 3))
        .add(new THREE.Vector3().fromArray(normals, i + 6)).normalize()
    }
  }
  return { center, direction }
}


// Work from the displayed polyline and triangles; placement needs no kernel
// surface evaluation. The nearest triangle also gives the inward direction
// on concave faces and hole boundaries, where a face centroid would not.
export function edgeGizmoFrame(points, faceMeshes) {
  const middle = Math.floor((points.length - 1) / 2)
  const a = new THREE.Vector3().fromArray(points[middle])
  const b = new THREE.Vector3().fromArray(points[middle + 1])
  const center = a.clone().lerp(b, 0.5)
  const tangent = b.sub(a).normalize()
  const directions = faceMeshes.map(({ positions }) => {
    const triangle = new THREE.Triangle()
    const nearest = new THREE.Triangle()
    const point = new THREE.Vector3()
    let closest = Infinity
    for(let i = 0; i < positions.length; i += 9) {
      triangle.a.fromArray(positions, i)
      triangle.b.fromArray(positions, i + 3)
      triangle.c.fromArray(positions, i + 6)
      const distance = triangle.closestPointToPoint(center, point).distanceToSquared(center)
      if(distance < closest) {
        closest = distance
        nearest.copy(triangle)
      }
    }
    const direction = nearest.getNormal(new THREE.Vector3()).cross(tangent).normalize()
    if(direction.dot(nearest.getMidpoint(point).sub(center)) < 0) direction.negate()
    return direction
  })
  return { center, directions }
}


export class FeatureGizmos {
  constructor() {
    this.controls = new Map()
  }

  update(descriptors, transform) {
    const renderer = window.alcRenderer
    for(const [key, control] of this.controls) {
      if(!descriptors[key]) {
        renderer.removeGizmo(control)
        this.controls.delete(key)
      }
    }
    for(const [key, descriptor] of Object.entries(descriptors)) {
      const { type, value, cb, side = true } = descriptor
      const center = descriptor.center.clone().applyMatrix4(transform)
      const orientation = type == 'angle' ?
        new THREE.Euler().setFromRotationMatrix(new THREE.Matrix4().extractRotation(transform)
          .multiply(descriptor.rotation)) : descriptor.direction.clone().transformDirection(transform)
      let control = this.controls.get(key)
      if(!control) {
        control = type == 'angle' ? new AngleGizmo(center, orientation, value, cb) :
          new LengthGizmo(center, orientation, value, side, cb)
        this.controls.set(key, control)
        control.addEventListener('dragging-changed', event => {
          for(const other of this.controls.values()) {
            if(other != control) other.enabled = control.enabled && !event.value
          }
        })
        renderer.addGizmo(control)
      }
      control.setFrame(center, orientation)
      control.set(value, side)
      if(descriptor.size) control.setSize(descriptor.size * 1200 / (renderer.canvas.offsetWidth + renderer.canvas.offsetHeight))
    }
  }

  dispose() {
    for(const control of this.controls.values()) window.alcRenderer.removeGizmo(control)
    this.controls.clear()
  }
}
