import * as THREE from 'three'

import { Circle, Arc } from '../core/geom2d.js'
import materials from '../materials.js'

const LEADER_LENGTH_PX = 50
const ARC_EXTENSION_STEPS = 32
const ARROW_LENGTH_PX = 12


export default class DimensionControls extends THREE.Object3D {
  constructor(constraint, renderer, parentTransform) {
    super()

    this.alcTypes = ['dimension']
    this.alcObject = constraint
    this.renderer = renderer
    this.layout = () => {}

    const itemL = constraint.items[0]
    const itemR = constraint.items[1]

    const isCircle = itemL.curve() instanceof Circle
    const isArc = itemL.curve() instanceof Arc
    const isCircleOffset = constraint.isCircleOffset()
    const isCenterDistance = constraint.isPointDistance() && !isCircleOffset
    const isPointDistance = constraint.isPointDistance()
    const isAngular = constraint.isAngular()

    // Offset between concentric circles
    if(constraint.dimensionPoints) {
      const [start, end] = constraint.dimensionPoints()
      makeOffsetDimension(this, renderer, start, end, constraint.position)

    } else if(isCircleOffset) {
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
      makeCircularDimension(this, renderer, itemL.curve(), constraint, isCircle, parentTransform)

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

  updateLayout() {
    this.layout()
    this.scaleArrows()
  }

  // Keep arrow heads at a constant on-screen size regardless of zoom.
  scaleArrows() {
    const camera = this.renderer.activeCamera
    const canvas = this.renderer.renderer.domElement
    const width = canvas.width / this.renderer.getPixelRatio()
    const height = canvas.height / this.renderer.getPixelRatio()
    this.updateWorldMatrix(true, false)
    const project = point => point.applyMatrix4(this.matrixWorld).project(camera)
    for(const arrow of this.children) {
      if(!(arrow instanceof THREE.ArrowHelper)) continue
      const origin = project(arrow.position.clone())
      const projectedLength = offset => {
        const point = project(arrow.position.clone().add(offset))
        return Math.hypot((point.x - origin.x) * width * 0.5, (point.y - origin.y) * height * 0.5)
      }
      const pixelsPerUnit = (projectedLength(new THREE.Vector3(1, 0, 0)) + projectedLength(new THREE.Vector3(0, 1, 0))) * 0.5
      arrow.scale.setScalar(ARROW_LENGTH_PX / pixelsPerUnit)
      arrow.updateMatrixWorld(true)
    }
  }
}

function makeArrow(pos, dir) {
  return new THREE.ArrowHelper(dir, pos, 0.0, 'darkgray', 1, 0.5)
}

function makeCircularDimension(controls, renderer, circle, constraint, isDiameter, parentTransform) {
  const center = circle.center()
  const position = constraint.position
  const transform = (parentTransform || new THREE.Matrix4()).clone().multiply(constraint.sketch.workplane)
  const inverse = transform.clone().invert()
  const plane = new THREE.Plane(new THREE.Vector3(0, 0, 1), 0).applyMatrix4(transform)
  // Keep segment counts fixed so position and color buffers stay aligned.
  const line = renderer.convertLine([center.toArray(), position.toArray(), position.toArray()], materials.wire)
  const arrow = makeArrow(center, new THREE.Vector3(1, 0, 0))
  const oppositeArrow = isDiameter ? makeArrow(center, new THREE.Vector3(-1, 0, 0)) : null
  const extension = !isDiameter ? renderer.convertLine(Array.from({ length: ARC_EXTENSION_STEPS + 1 }, () => center.toArray()), materials.wire) : null
  controls.add(line, arrow)
  if(oppositeArrow) controls.add(oppositeArrow)
  if(extension) controls.add(extension)

  const lastView = new THREE.Matrix4()
  const view = new THREE.Matrix4()
  const raycaster = new THREE.Raycaster()
  let lastWidth = 0
  const update = () => {
    const camera = renderer.activeCamera
    const width = renderer.renderer.domElement.width / renderer.getPixelRatio()
    view.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse)
    if(view.equals(lastView) && width === lastWidth) return
    const label = position.clone().applyMatrix4(transform).project(camera)
    const elbows = [-1, 1].map(direction => {
      raycaster.setFromCamera(new THREE.Vector2(label.x + direction * 2 * LEADER_LENGTH_PX / width, label.y), camera)
      return raycaster.ray.intersectPlane(plane, new THREE.Vector3())?.applyMatrix4(inverse)
    })
    // Seen edge-on, or before the camera is set up, the screen offsets miss
    // the sketch plane. Lay out once they meet it again.
    if(elbows.some(elbow => !elbow)) return
    lastView.copy(view)
    lastWidth = width
    const projectedCenter = center.clone().applyMatrix4(transform).project(camera)
    const labelOutside = position.distanceTo(center) > circle.radius
    const elbowAt = direction => elbows[direction > 0 ? 1 : 0]
    const outwardSide = label.x > projectedCenter.x ? 1 : -1
    const stepLength = elbowAt(outwardSide).distanceTo(position)
    const towardCenter = labelOutside || position.distanceTo(center) + stepLength > circle.radius
    const side = outwardSide * (towardCenter ? -1 : 1)
    let elbow = elbowAt(side)
    // Keep an outside leader's elbow outside the circle as well.
    if(labelOutside && elbow.distanceTo(center) <= circle.radius) {
      elbow = elbowAt(-side)
    }
    const insideDiameter = isDiameter && !labelOutside
    const direction = (insideDiameter ? position : elbow).clone().sub(center).normalize()
    const near = center.clone().addScaledVector(direction, circle.radius)
    const far = center.clone().addScaledVector(direction, -circle.radius)
    let outside = elbow.distanceTo(center) > circle.radius
    let end = isDiameter ? far : near
    let arcPoints = []
    if(!isDiameter) {
      const radiusLayout = bend => {
        const radial = bend.clone().sub(center).normalize()
        const nearPoint = center.clone().addScaledVector(radial, circle.radius)
        const isOutside = bend.distanceTo(center) > circle.radius
        const extensionPoints = arcExtensionPoints(circle, nearPoint)
        return { end: nearPoint, points: extensionPoints, outside: isOutside }
      }
      let layout = radiusLayout(elbow)
      const exit = elbow.clone().applyMatrix4(transform).project(camera).x - label.x
      const target = layout.end.clone().applyMatrix4(transform).project(camera).x - label.x - exit
      if(exit * target < 0) {
        const alternateElbow = elbowAt(exit > 0 ? -1 : 1)
        const alternate = radiusLayout(alternateElbow)
        const alternateExit = alternateElbow.clone().applyMatrix4(transform).project(camera).x - label.x
        const alternateTarget = alternate.end.clone().applyMatrix4(transform).project(camera).x - label.x - alternateExit
        if(alternateExit * alternateTarget > 0) {
          elbow = alternateElbow
          layout = alternate
        }
      }
      end = layout.end
      arcPoints = layout.points
      outside = layout.outside
    }
    const points = insideDiameter
      ? [near.toArray(), position.toArray(), far.toArray()]
      : [position.toArray(), elbow.toArray(), end.toArray()]
    line.geometry.setPositions(points.flat())
    line.computeLineDistances()
    arrow.position.copy(isDiameter ? near : end)
    const arrowDirection = (isDiameter ? near : end).clone().sub(center).normalize()
    arrow.setDirection(arrowDirection.multiplyScalar(outside && !isDiameter ? -1 : 1))
    arrow.updateMatrixWorld(true)
    if(oppositeArrow) {
      oppositeArrow.position.copy(far)
      oppositeArrow.setDirection(direction.clone().negate())
      oppositeArrow.updateMatrixWorld(true)
    }
    if(extension) {
      extension.geometry.setPositions((arcPoints.length ? arcPoints : Array.from({ length: ARC_EXTENSION_STEPS + 1 }, () => end.toArray())).flat())
      extension.computeLineDistances()
    }
  }
  controls.layout = update
  update()
}

function makeOffsetDimension(controls, renderer, start, end, position) {
  const direction = end.clone().sub(start).normalize()
  const labelDistance = position.clone().sub(start).dot(direction)
  const lineStart = start.clone().addScaledVector(direction, Math.min(0, labelDistance))
  const lineEnd = start.clone().addScaledVector(direction, Math.max(start.distanceTo(end), labelDistance))
  controls.add(renderer.convertLine([lineStart.toArray(), lineEnd.toArray()], materials.wire))
  controls.add(makeArrow(start, direction.clone().negate()))
  controls.add(makeArrow(end, direction))
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

function arcExtensionPoints(circle, constraintPos) {
  const curve = circle.geom(), range = curve.range()
  const start = range.start, end = range.end, direction = Math.sign(range.span())
  const basis = curve.curve()
  const u = Math.atan2(constraintPos.y - circle.center().y, constraintPos.x - circle.center().x)
  const period = Math.PI * 2
  const progress = THREE.MathUtils.euclideanModulo(direction * (u - start), period)
  const span = Math.abs(range.span())
  if(progress <= span) return []

  // Compare gaps around the circle, including across the parameter seam.
  const fromEnd = progress - span
  const fromStart = period - progress
  const extensionStart = fromStart < fromEnd ? start - direction * fromStart : end
  const extensionEnd = fromStart < fromEnd ? start : end + direction * fromEnd
  return Array.from({ length: ARC_EXTENSION_STEPS + 1 }, (_value, index) => {
    const point = basis.evaluate(extensionStart + (extensionEnd - extensionStart) * index / ARC_EXTENSION_STEPS).point
    return [point.x, point.y, circle.center().z]
  })
}
