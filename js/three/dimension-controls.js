import * as THREE from 'three'

import { Circle, Arc } from '../core/geom2d.js'
import materials from '../materials.js'


export default class DimensionControls extends THREE.Object3D {
  constructor(constraint, renderer, parentTransform) {
    super()

    this.alcTypes = ['dimension']
    this.alcObject = constraint

    const itemL = constraint.items[0]
    const itemR = constraint.items[1]

    const isCircle = itemL.curve() instanceof Circle
    const isArc = itemL.curve() instanceof Arc
    const isCircleOffset = constraint.isCircleOffset()
    const isCenterDistance = constraint.isPointDistance() && !isCircleOffset
    const isPointDistance = constraint.isPointDistance()
    const isAngular = constraint.isAngular()

    // Offset between concentric circles
    if(isCircleOffset) {
      const circles = constraint.items.map(item => item.curve())
      const center = circles[0].center()
      const direction = constraint.position.clone().sub(center)
      if(direction.lengthSq() < 1e-12) direction.set(1, 0, 0)
      direction.normalize()
      const points = circles.map(circle => circle.center().clone().add(direction.clone().multiplyScalar(circle.radius)))
      makeLinearDimension(this, renderer, points[0], points[1], constraint.position)

    // Circle center to point, or center to center
    } else if(isCenterDistance) {
      const points = constraint.items.map(item => item.curve() instanceof Circle ?
        item.curve().center().clone() : item.curve().handles()[item.index].clone())
      makeLinearDimension(this, renderer, points[0], points[1], constraint.position)

    // Circle/Arc diameter/radius
    } else if(isCircle || isArc) {
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
          const line = renderer.convertLine([constraintPos.toArray(), p2.toArray()], materials.wire)

          const p3 = constraintPos.clone().add(new THREE.Vector3(left ? 10 : -10, 0, 0))
          const dimLine = renderer.convertLine([constraintPos.toArray(), p3.toArray()], materials.wire)

          this.add(line)
          this.add(dimLine)

          this.add(makeArrow(p1, dir))
          this.add(makeArrow(p2, dir.negate()))

        } else {
          const offset = cdir.clone().multiplyScalar(circle.radius)
          const p1 = circle.center().clone().add(offset)
          const p2 = circle.center().clone().add(offset.negate())

          const line = renderer.convertLine([p1.toArray(), p2.toArray()], materials.wire)
          this.add(line)

          this.add(makeArrow(p1, cdir))
          this.add(makeArrow(p2, cdir.clone().negate()))
        }

      } else {

        if(isOutside) {
          const line = renderer.convertLine([constraintPos.toArray(), p1.toArray()], materials.wire)
          const p3 = constraintPos.clone().add(new THREE.Vector3(left ? 10 : -10, 0, 0))
          const dimLine = renderer.convertLine([constraintPos.toArray(), p3.toArray()], materials.wire)

          this.add(line)
          this.add(dimLine)
          this.add(makeArrow(p1, dir.negate()))
          makeArc(this, renderer, circle, constraintPos)

        } else {

          const p = circle.center().clone().add(cdir.clone().multiplyScalar(circle.radius))
          const line = renderer.convertLine([circle.center().toArray(), p.toArray()], materials.wire)

          this.add(line)
          this.add(makeArrow(p, cdir))
          makeArc(this, renderer, circle, constraint.position)
        }
      }

    // Point to point distance
    } else if(isPointDistance) {
      const points = constraint.items.map(item => item.curve().handles()[item.index].clone())
      makeLinearDimension(this, renderer, points[0], points[1], constraint.position)

    // Line to line angle
    } else if(isAngular) {
      makeAngularDimension(this, renderer, constraint)

    // Line length
    } else if(!itemR) {
      const [start, end] = itemL.curve().endpoints().map(point => point.clone())
      makeLinearDimension(this, renderer, start, end, constraint.position)

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

      const lineL = renderer.convertLine([posL.toArray(), posLT.clone().add(projL.normalize()).toArray()], materials.wire)
      const lineR = renderer.convertLine([posR.toArray(), posRT.clone().add(projR.normalize()).toArray()], materials.wire)

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
      ], materials.wire)

      this.add(lineL)
      this.add(lineR)
      this.add(lineTop)
    }

    this.applyMatrix4(
      (parentTransform || new THREE.Matrix4()).clone().multiply(constraint.sketch.workplane)
    )
  }
}

function makeArrow(pos, dir) {
  return new THREE.ArrowHelper(dir, pos, 0.0, 'darkgray', 1.5, 0.75)
}

function makeLinearDimension(controls, renderer, start, end, position) {
  const direction = end.clone().sub(start).normalize()
  const normal = new THREE.Vector3(-direction.y, direction.x, 0)
  const offset = position.clone().sub(start).dot(normal)
  const offsetDirection = normal.clone().multiplyScalar(Math.sign(offset) || 1)

  const dimStart = start.clone().add(normal.clone().multiplyScalar(offset))
  const dimEnd = end.clone().add(normal.clone().multiplyScalar(offset))
  const extension = offsetDirection.multiplyScalar(1)

  const lineStart = dimStart.clone()
  const lineEnd = dimEnd.clone()
  const labelOffset = position.clone().sub(dimStart).dot(direction)
  const lineLength = start.distanceTo(end)
  if(labelOffset < 0) lineStart.copy(position)
  if(labelOffset > lineLength) lineEnd.copy(position)

  controls.add(renderer.convertLine([start.toArray(), dimStart.clone().add(extension).toArray()], materials.wire))
  controls.add(renderer.convertLine([end.toArray(), dimEnd.clone().add(extension).toArray()], materials.wire))
  controls.add(renderer.convertLine([lineStart.toArray(), lineEnd.toArray()], materials.wire))
  controls.add(makeArrow(dimStart, direction.clone().negate()))
  controls.add(makeArrow(dimEnd, direction))
}

function makeAngularDimension(controls, renderer, constraint) {
  const curves = constraint.items.map(item => item.curve())
  const center = lineIntersection(curves[0], curves[1])
  if(!center) return

  const directions = curves.map(curve => curve.direction().normalize())
  const towardPosition = constraint.position.clone().sub(center)
  const sector = angularSector(directions[0], directions[1], towardPosition, constraint.distance)

  const radius = Math.max(towardPosition.length(), 1)
  const startAngle = Math.atan2(sector.start.y, sector.start.x)
  const delta = sector.sign * THREE.MathUtils.degToRad(Math.abs(constraint.distance))
  const angle = Math.abs(delta)
  const targetAngle = Math.atan2(towardPosition.y, towardPosition.x)
  const targetProgress = THREE.MathUtils.euclideanModulo(
    sector.sign * (targetAngle - startAngle),
    Math.PI * 2,
  )
  const endExtension = targetProgress - angle
  const startExtension = Math.PI * 2 - targetProgress
  const extension = targetProgress <= angle
    ? { start: 0, end: angle }
    : startExtension < endExtension
    ? { start: -startExtension, end: angle }
    : { start: 0, end: targetProgress }
  const pointAt = progress => {
    const pointAngle = startAngle + sector.sign * progress
    return [center.x + Math.cos(pointAngle) * radius, center.y + Math.sin(pointAngle) * radius, center.z]
  }
  const arc = Array.from({ length: 33 }, (_value, index) => pointAt(
    extension.start + (extension.end - extension.start) * index / 32
  ))
  const arrowStart = pointAt(0)
  const arrowEnd = pointAt(angle)

  const ray1 = center.clone().add(sector.start.clone().multiplyScalar(radius + 1))
  const ray2 = center.clone().add(sector.end.clone().multiplyScalar(radius + 1))
  controls.add(renderer.convertLine([center.toArray(), ray1.toArray()], materials.wire))
  controls.add(renderer.convertLine([center.toArray(), ray2.toArray()], materials.wire))
  controls.add(renderer.convertLine(arc, materials.wire))
  controls.add(makeArrow(new THREE.Vector3().fromArray(arrowStart), sector.start.clone().applyAxisAngle(new THREE.Vector3(0, 0, 1), sector.sign * Math.PI / 2).negate()))
  controls.add(makeArrow(new THREE.Vector3().fromArray(arrowEnd), sector.end.clone().applyAxisAngle(new THREE.Vector3(0, 0, 1), -sector.sign * Math.PI / 2).negate()))
}

// Lines are unoriented, so their endpoint directions alone cannot identify
// whether an angular dimension is the acute or obtuse sector. Pick the
// sector matching the stored dimension value and use the label position to
// choose between the two sectors of that size.
function angularSector(directionL, directionR, towardPosition, degrees) {
  const angle = THREE.MathUtils.degToRad(Math.min(Math.abs(degrees), 180))
  const target = towardPosition.lengthSq() > 1e-12 ? towardPosition.clone().normalize() : new THREE.Vector3(1, 0, 0)
  const candidates = []

  for(const start of [directionL, directionL.clone().negate()]) {
    for(const sign of [-1, 1]) {
      const end = start.clone().applyAxisAngle(new THREE.Vector3(0, 0, 1), sign * angle)
      const midpoint = start.clone().applyAxisAngle(new THREE.Vector3(0, 0, 1), sign * angle / 2)
      // Prefer geometrically valid sectors, then the one under the label.
      const lineMatch = Math.abs(end.dot(directionR))
      candidates.push({ start, end, sign, score: lineMatch * 2 + midpoint.dot(target) })
    }
  }

  return candidates.reduce((best, candidate) => candidate.score > best.score ? candidate : best)
}

function lineIntersection(left, right) {
  const originL = left.handles()[0]
  const originR = right.handles()[0]
  const dirL = left.direction()
  const dirR = right.direction()
  const cross = dirL.x * dirR.y - dirL.y * dirR.x
  if(Math.abs(cross) < 1e-8) return
  const diff = originR.clone().sub(originL)
  const t = (diff.x * dirR.y - diff.y * dirR.x) / cross
  return originL.clone().add(dirL.multiplyScalar(t))
}

function makeArc(controls, renderer, circle, constraintPos) {
  const start = circle.geom().get().FirstParameter()
  const end = circle.geom().get().LastParameter()
  const u = new Circle(circle.center(), circle.radius, circle.geom().get().BasisCurve()).unsample(constraintPos)

  if(u < start || u > end) {
    const closer = [start, end].minMaxBy(Math.min, param => Math.abs(u - param) )

    const arc = new Arc(circle.center().clone(), circle.radius, [closer, u].sort())
    arc.update()

    controls.add(renderer.convertLine(arc.tesselate(), materials.wire))
  }
}
