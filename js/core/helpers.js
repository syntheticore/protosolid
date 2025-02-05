import { makeID } from './id.js'
import { PlanarReference, HelperReference, AxialReference } from './references.js'


export class ConstructionHelper {
  constructor(componentId, id) {
    this.id = id || makeID()
    this.componentId = componentId
  }

  center() {
    return new THREE.Vector3().setFromMatrixPosition(this.transform)
  }
}

export class PlaneHelper extends ConstructionHelper {
  constructor(componentId, plane, id) {
    super(componentId, id)
    this.transform = plane
  }

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

  axialReference() {
    return new AxialReference(new HelperReference(this))
  }

  getAxis() {
    return this.transform
  }
}

export class PointHelper extends ConstructionHelper {
  constructor(componentId, point, id) {
    super(componentId, id)
    this.transform = point
  }

  axialReference() {
    return new PointReference(new HelperReference(this))
  }

  getPoint() {
    return this.transform
  }
}
