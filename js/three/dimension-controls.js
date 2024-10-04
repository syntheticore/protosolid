import * as THREE from 'three'


export default class DimensionControls extends THREE.Object3D {
  constructor(constraint, renderer) {
    super()

    this.alcType = 'dimension'
    this.alcObject = constraint

    const itemL = constraint.items()[0]
    const itemR = constraint.items()[1]

    const [posL, posR] = constraint.items().map(item =>
      item.curve.handles().minMaxBy(Math.min, handle => handle.distanceTo(constraint.position) ).clone()
    )

    const dirL = itemL.curve.direction().normalize()
    const dirR = itemR.curve.direction().normalize()

    const projL = constraint.position.clone().sub(posL).projectOnVector(dirL)
    const projR = constraint.position.clone().sub(posR).projectOnVector(dirR)

    let posLT = posL.clone().add(projL)
    let posRT = posR.clone().add(projR)

    const lineL = renderer.convertLine([posL.toArray(), posLT.toArray()], renderer.materials.wire)
    const lineR = renderer.convertLine([posR.toArray(), posRT.toArray()], renderer.materials.wire)

    const cross = posRT.clone().sub(posLT)
    const contraintRel = constraint.position.clone().sub(posLT)

    const helperDir = cross.clone().normalize()

    const arrowHelperL = new THREE.ArrowHelper(helperDir.clone().negate(), posLT, 0.0, 'darkgray', 2, 1)
    const arrowHelperR = new THREE.ArrowHelper(helperDir, posRT, 0.0, 'darkgray', 2, 1)

    this.add(arrowHelperL)
    this.add(arrowHelperR)

    if(cross.dot(contraintRel) > 0.0) {
      if(contraintRel.length() > cross.length()) {
        posRT = constraint.position
      }
    } else {
      posLT = constraint.position
    }

    const lineTop = renderer.convertLine([
      posLT.toArray(),
      posRT.toArray(),
    ], renderer.materials.wire)

    this.add(lineL)
    this.add(lineR)
    this.add(lineTop)

    this.applyMatrix4(constraint.sketch.workplane)
  }
}
