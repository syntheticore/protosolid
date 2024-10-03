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
    const dirR = itemR.curve.direction().negate().normalize()

    const projL = constraint.position.clone().sub(posL).projectOnVector(dirL)
    const projR = constraint.position.clone().sub(posR).projectOnVector(dirR)

    const [closerProj, closerPos] = [[projL, posL], [projR, posR]].minMaxBy(Math.min, ([proj, _]) => proj.length() )
    const [otherProj, otherPos] = [[projL, posL], [projR, posR]].find(([proj, _]) => proj != closerProj )

    const posLT = closerPos.clone().add(closerProj)
    const posRT = otherPos.clone().add(otherProj)

    const lineL = renderer.convertLine([closerPos.toArray(), posLT.toArray()], renderer.materials.wire)
    const lineR = renderer.convertLine([otherPos.toArray(), posRT.toArray()], renderer.materials.wire)

    // const dir = posRT.clone().sub(posLT)
    // const distance = dir.length()
    // dir.normalize()

    // const lineTopL = renderer.convertLine([
    //   posLT.toArray(),
    //   posLT.clone().add(dir.clone().multiplyScalar(distance * 0.4)).toArray(),
    // ], renderer.materials.wire)

    // const lineTopR = renderer.convertLine([
    //   posRT.clone().add(dir.clone().multiplyScalar(-distance * 0.4)).toArray(),
    //   posRT.toArray(),
    // ], renderer.materials.wire)

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
