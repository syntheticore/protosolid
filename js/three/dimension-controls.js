import * as THREE from 'three'

import { Circle } from '../core/kernel.js'


export default class DimensionControls extends THREE.Object3D {
  constructor(constraint, renderer) {
    super()

    this.alcType = 'dimension'
    this.alcObject = constraint

    const itemL = constraint.items[0]
    const itemR = constraint.items[1]

    if(itemL.curve() instanceof Circle) {
      const circle = itemL.curve()

      const left = constraint.position.clone().sub(circle.center()).x > 0
      const constraintPos = constraint.position.clone().add(new THREE.Vector3(left ? -10 : 10, 0, 0))

      const dir = constraintPos.clone().sub(circle.center()).normalize().multiplyScalar(circle.radius)
      const p1 = circle.center().clone().add(dir)
      const p2 = circle.center().clone().add(dir.negate())

      const line = renderer.convertLine([constraintPos.toArray(), p2.toArray()], renderer.materials.wire)

      const p3 = constraintPos.clone().add(new THREE.Vector3(left ? 10 : -10, 0, 0))
      const dimLine = renderer.convertLine([constraintPos.toArray(), p3.toArray()], renderer.materials.wire)

      this.add(line)
      this.add(dimLine)

    } else {
      const [posL, posR] = constraint.items.map(item =>
        item.curve().endpoints().minMaxBy(Math.min, handle => handle.distanceTo(constraint.position) ).clone()
      )

      const dirL = itemL.curve().direction().normalize()
      const dirR = itemR.curve().direction().normalize()

      const projL = constraint.position.clone().sub(posL).projectOnVector(dirL)
      const projR = constraint.position.clone().sub(posR).projectOnVector(dirR)

      let posLT = posL.clone().add(projL)
      let posRT = posR.clone().add(projR)

      const lineL = renderer.convertLine([posL.toArray(), posLT.clone().add(projL.normalize()).toArray()], renderer.materials.wire)
      const lineR = renderer.convertLine([posR.toArray(), posRT.clone().add(projR.normalize()).toArray()], renderer.materials.wire)

      const cross = posRT.clone().sub(posLT)
      const contraintRel = constraint.position.clone().sub(posLT)

      const helperDir = cross.clone().normalize()

      const arrowHelperL = new THREE.ArrowHelper(helperDir.clone().negate(), posLT, 0.0, 'darkgray', 1.5, 0.75)
      const arrowHelperR = new THREE.ArrowHelper(helperDir, posRT, 0.0, 'darkgray', 1.5, 0.75)

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
    }

    this.applyMatrix4(constraint.sketch.workplane)
  }
}
