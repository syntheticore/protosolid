import * as THREE from 'three'

import { Circle, Arc } from '../core/kernel.js'


export default class DimensionControls extends THREE.Object3D {
  constructor(constraint, renderer) {
    super()

    this.alcType = 'dimension'
    this.alcObject = constraint

    const itemL = constraint.items[0]
    const itemR = constraint.items[1]

    const isCircle = itemL.curve() instanceof Circle
    const isArc = itemL.curve() instanceof Arc

    // Circle/Arc diameter/radius
    if(isCircle || isArc) {
      const circle = itemL.curve()

      const left = constraint.position.clone().sub(circle.center()).x > 0
      const constraintPos = constraint.position.clone().add(new THREE.Vector3(left ? -10 : 10, 0, 0))

      const cdir = constraint.position.clone().sub(circle.center())
      const isOutside = (cdir.length() > circle.radius)
      cdir.normalize()
      const dir = constraintPos.clone().sub(circle.center()).normalize()
      const pOffset = dir.clone().multiplyScalar(circle.radius)
      const p1 = circle.center().clone().add(pOffset)
      const p2 = circle.center().clone().add(pOffset.negate())

      if(isCircle) {

        if(isOutside) {
          const line = renderer.convertLine([constraintPos.toArray(), p2.toArray()], renderer.materials.wire)

          const p3 = constraintPos.clone().add(new THREE.Vector3(left ? 10 : -10, 0, 0))
          const dimLine = renderer.convertLine([constraintPos.toArray(), p3.toArray()], renderer.materials.wire)

          this.add(line)
          this.add(dimLine)

          this.add(makeArrow(p1, dir))
          this.add(makeArrow(p2, dir.negate()))

        } else {
          const offset = cdir.clone().multiplyScalar(circle.radius)
          const p1 = circle.center().clone().add(offset)
          const p2 = circle.center().clone().add(offset.negate())

          const line = renderer.convertLine([p1.toArray(), p2.toArray()], renderer.materials.wire)
          this.add(line)

          this.add(makeArrow(p1, cdir))
          this.add(makeArrow(p2, cdir.clone().negate()))
        }

      } else {

        if(isOutside) {
          const line = renderer.convertLine([constraintPos.toArray(), p1.toArray()], renderer.materials.wire)
          const p3 = constraintPos.clone().add(new THREE.Vector3(left ? 10 : -10, 0, 0))
          const dimLine = renderer.convertLine([constraintPos.toArray(), p3.toArray()], renderer.materials.wire)

          this.add(line)
          this.add(dimLine)
          this.add(makeArrow(p1, dir.negate()))
          makeArc(this, renderer, circle, constraintPos)

        } else {

          const p = circle.center().clone().add(cdir.clone().multiplyScalar(circle.radius))
          const line = renderer.convertLine([circle.center().toArray(), p.toArray()], renderer.materials.wire)

          this.add(line)
          this.add(makeArrow(p, cdir))
          makeArc(this, renderer, circle, constraint.position)
        }
      }

    // Line/line distance
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

      const arrowHelperL = makeArrow(posLT, helperDir.clone().negate())
      const arrowHelperR = makeArrow(posRT, helperDir)

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

function makeArrow(pos, dir) {
  return new THREE.ArrowHelper(dir, pos, 0.0, 'darkgray', 1.5, 0.75)
}

function makeArc(controls, renderer, circle, constraintPos) {
  const start = circle.geom().get().FirstParameter()
  const end = circle.geom().get().LastParameter()
  const u = new Circle(circle.center(), circle.radius, circle.geom().get().BasisCurve()).unsample(constraintPos)

  if(u < start || u > end) {
    const closer = [start, end].minMaxBy(Math.min, param => Math.abs(u - param) )

    const arc = new Arc(circle.center().clone(), circle.radius, [closer, u].sort())
    arc.update()

    controls.add(renderer.convertLine(arc.tesselate(), renderer.materials.wire))
  }
}
