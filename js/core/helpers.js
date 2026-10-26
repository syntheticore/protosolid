import * as THREE from 'three'
import { makeID } from './id.js'
import { shapex as sx, point3 } from './shapex.js'
import { PlanarReference, HelperReference, AxialReference, PointReference } from './references.js'


export class ConstructionHelper {
  constructor(componentId, id) {
    this.id = id || makeID()
    this.componentId = componentId
  }

  center() {
    return new THREE.Vector3().setFromMatrixPosition(this.transform)
  }

  // The origin, axes and planes every component starts with
  isStandard() {
    return this.id.startsWith(`${this.componentId}/`)
  }
}

export class PlaneHelper extends ConstructionHelper {
  constructor(componentId, plane, id) {
    super(componentId, id)
    this.transform = plane
  }

  typename() { return 'Plane' }

  planarReference() {
    return new PlanarReference(new HelperReference(this))
  }

  getPlane() {
    return this.transform
  }
}

export class AxisHelper extends ConstructionHelper {
  constructor(componentId, axis, id) {
    super(componentId, id)
    this.transform = axis
  }

  typename() { return 'Axis' }

  // Middle of the displayed axis line
  center() {
    return new THREE.Vector3(0, 0, 5).applyMatrix4(this.transform)
  }

  axialReference() {
    return new AxialReference(new HelperReference(this))
  }

  getAxis() {
    return this.transform
  }

  geom() {
    const start = new THREE.Vector3().applyMatrix4(this.transform)
    const end = new THREE.Vector3(0, 0, 20).applyMatrix4(this.transform)
    return sx.TrimmedCurve3.new(sx.Line3.through(point3(start), point3(end)), sx.ParameterRange.new(0, 1))
  }
}

export class PointHelper extends ConstructionHelper {
  constructor(componentId, point, id) {
    super(componentId, id)
    this.transform = point
  }

  typename() { return 'Point' }

  pointReference() {
    return new PointReference(new HelperReference(this))
  }

  getPoint() {
    return this.transform
  }
}
