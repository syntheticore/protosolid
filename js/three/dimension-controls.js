import * as THREE from 'three'


export default class DimensionControls extends THREE.Object3D {
  constructor(constraint, renderer) {
    super()

    this.alcType = 'dimension'
    this.alcObject = constraint

    const itemL = constraint.items()[0]
    const posL = itemL.curve.handles()[0]

    const itemR = constraint.items()[1]
    const posR = itemR.curve.center()

    const dir = posR.clone().sub(posL)
    const upline = dir.clone().normalize().cross(new THREE.Vector3(0,0,1)).multiplyScalar(10)

    const lineL = renderer.convertLine([posL.toArray(), posL.clone().add(upline).toArray()], renderer.materials.wire)
    const lineR = renderer.convertLine([posR.toArray(), posR.clone().add(upline).toArray()], renderer.materials.wire)
    const lineTop = renderer.convertLine([posL.clone().add(upline).toArray(), posR.clone().add(upline).toArray()], renderer.materials.wire)

    this.add(lineL)
    this.add(lineR)
    this.add(lineTop)

    this.applyMatrix4(constraint.sketch.workplane)
  }
}
