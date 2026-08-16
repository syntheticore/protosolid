import * as THREE from 'three'
import { Line, Circle, Arc, supportsPointOnCurveConstraint } from './core/geom2d.js'
import { vecFromOc } from './core/utils.js'
import { CoincidentConstraint } from './core/sketch.js'

const snapDistance = 14 // px
const maxSnapReferences = 5
const fullTurn = Math.PI * 2.0

function positiveAngle(angle) {
  return ((angle % fullTurn) + fullTurn) % fullTurn
}

function closestPointOnCurve(elem, point) {
  if(elem instanceof Line) {
    const [start, end] = elem.endpoints()
    const segment = end.clone().sub(start)
    const lengthSquared = segment.lengthSq()
    if(lengthSquared.almost(0.0)) return
    const parameter = THREE.MathUtils.clamp(point.clone().sub(start).dot(segment) / lengthSquared, 0.0, 1.0)
    return start.clone().addScaledVector(segment, parameter)
  }

  const center = elem.center()
  const radial = point.clone().sub(center)
  if(elem instanceof Circle) {
    if(radial.lengthSq().almost(0.0)) radial.set(1.0, 0.0, 0.0)
    return center.add(radial.normalize().multiplyScalar(elem.radius))
  }

  if(elem instanceof Arc) {
    const endpoints = elem.endpoints()
    if(radial.lengthSq().almost(0.0)) {
      return endpoints.minMaxBy(Math.min, endpoint => endpoint.distanceTo(point)).clone()
    }

    const projected = center.clone().add(radial.normalize().multiplyScalar(elem.radius))
    const angles = endpoints.map(endpoint =>
      Math.atan2(endpoint.y - center.y, endpoint.x - center.x)
    )
    const candidateAngle = Math.atan2(projected.y - center.y, projected.x - center.x)
    const direction = elem.forward ? -1.0 : 1.0
    const candidateSweep = positiveAngle(direction * (candidateAngle - angles[0]))
    const arcSweep = positiveAngle(direction * (angles[1] - angles[0]))
    if(candidateSweep <= arcSweep) return projected
    return endpoints.minMaxBy(Math.min, endpoint => endpoint.distanceTo(point)).clone()
  }
}

function curveGuideIntersections(elem, guidePoint, axis) {
  if(elem instanceof Line) {
    const [start, end] = elem.endpoints()
    const delta = end.clone().sub(start)
    const denominator = axis == 'x' ? delta.x : delta.y
    if(denominator.almost(0.0)) return []
    const value = axis == 'x' ? guidePoint.x : guidePoint.y
    const parameter = (value - (axis == 'x' ? start.x : start.y)) / denominator
    if(parameter < 0.0 || parameter > 1.0) return []
    return [start.clone().addScaledVector(delta, parameter)]
  }

  const center = elem.center()
  const value = axis == 'x' ? guidePoint.x : guidePoint.y
  const offset = value - (axis == 'x' ? center.x : center.y)
  const remainder = elem.radius * elem.radius - offset * offset
  if(remainder < 0.0) return []
  const span = Math.sqrt(Math.max(0.0, remainder))
  const points = [-span, span].map(other => axis == 'x' ?
    new THREE.Vector3(value, center.y + other, center.z)
    :
    new THREE.Vector3(center.x + other, value, center.z)
  )
  const unique = points.filter((point, index) => !index || !point.almost(points[0]))
  if(elem instanceof Circle) return unique
  return unique.filter(point => closestPointOnCurve(elem, point).almost(point))
}

export default class Snapper {
  constructor(viewport, updateView) {
    this.viewport = viewport
    this.updateView = updateView
    this.guides = []
    this.lastSnaps = []
    this.snapAnchor = null
    this.planeTransform = new THREE.Matrix4()
  }

  reset() {
    this.guides = []
    this.snapAnchor = null
    this.lastSnaps = []
    this.snapped = {}
    this.updateView([], null)
  }

  snap(vec, coords, snapToGuides, snapToPoints, localSpace) {
    this.guides = []
    this.snapped = {}
    if(localSpace) {
      vec = this.snapToGuides(vec, snapToGuides, snapToPoints)
      if(vec) vec.z = 0.0 //XXX project vec to plane before snapping
    }
    this.updateView(this.guides, snapToPoints ? this.snapAnchor : null)
    return vec
  }

  getActiveHandleConnections() {
    const handle = this.viewport.activeHandle
    const connected = new Set()
    if(!handle) return connected

    const pending = [{ elem: handle.elem, index: handle.index }]
    const visitedPoints = []
    const visitedConstraints = new Set()
    while(pending.length) {
      const point = pending.pop()
      if(visitedPoints.some(other => other.elem == point.elem && other.index == point.index)) continue
      visitedPoints.push(point)
      connected.add(point.elem)

      this.viewport.document.activeSketch.constraints.forEach(constraint => {
        if(visitedConstraints.has(constraint) || !constraint.items.some(item =>
          item.curve() == point.elem && item.index == point.index
        )) return
        visitedConstraints.add(constraint)
        constraint.items.forEach(item => connected.add(item.curve()))
        if(constraint instanceof CoincidentConstraint) {
          constraint.items.forEach(item => pending.push({ elem: item.curve(), index: item.index }))
        }
      })
    }
    return connected
  }

  getSnapElements(connectedElements) {
    const sketch = this.viewport.document.activeSketch
    let sketchElements = [...sketch.elements]
    // Filter out sketch element actively being drawn
    const tool = this.viewport.activeTool
    if(tool.curve) sketchElements.pop()
    sketchElements.push(...sketch.projections.map(projection => projection.geometry()).filter(Boolean))
    if(this.viewport.activeHandle) {
      sketchElements = sketchElements.filter(elem => !connectedElements.has(elem))
    }
    return sketchElements
  }

  getSnapPointTargets(elements, connectedElements) {
    const sketch = this.viewport.document.activeSketch
    const points = elements.flatMap(elem =>
      elem.snapPoints().map(point => ({
        point,
        elem,
        index: elem.handles().findIndex(handle => handle.almost(point)),
        midpoint: elem instanceof Line && point.almost(elem.midpoint()),
      }))
    )
    const origin = sketch.originPoint()
    const originHelper = sketch.origin()
    const originConstrained = originHelper && connectedElements.has(originHelper)
    return origin && !originConstrained ? [{ point: origin, origin: true }, ...points] : points
  }

  catchSnapTargets(localVec, coords) {
    const connectedElements = this.getActiveHandleConnections()
    const elements = this.getSnapElements(connectedElements)
    let pointDist = Infinity
    let pointTarget
    let midpointTarget
    let guidePointDist = Infinity
    let guidePointTarget
    this.getSnapPointTargets(elements, connectedElements).forEach(target => {
      const dist = this.viewport.renderer.toScreen(target.point.clone().applyMatrix4(this.planeTransform)).distanceTo(coords)
      if(dist < snapDistance && dist < guidePointDist) {
        guidePointDist = dist
        guidePointTarget = target
      }
      if((target.origin || target.index != -1) && dist < snapDistance && dist < pointDist) {
        pointDist = dist
        pointTarget = target
      }
      if(target.midpoint && dist < snapDistance && (!midpointTarget || dist < midpointTarget.distance)) {
        midpointTarget = { ...target, distance: dist }
      }
    })

    // Handles and the sketch origin always take precedence over curves.
    if(pointTarget) return this.rememberSnapPoint(pointTarget)

    // An intersection near the pointer must be near both participating
    // curves. Reject the rest before invoking OpenCascade's substantially
    // more expensive curve/curve intersection routine.
    const nearbyCurves = elements
      .filter(supportsPointOnCurveConstraint)
      .map(elem => {
        const point = closestPointOnCurve(elem, localVec)
        if(!point) return
        const distance = this.viewport.renderer
          .toScreen(point.clone().applyMatrix4(this.planeTransform))
          .distanceTo(coords)
        return { elem, point, distance }
      })
      .filter(target => target && target.distance < snapDistance)

    let intersectionTarget
    nearbyCurves.forEach((target, index, curves) =>
      curves.slice(index + 1).forEach(otherTarget => {
        target.elem.intersect([otherTarget.elem]).forEach(ocPoint => {
          const point = vecFromOc(ocPoint)
          const dist = this.viewport.renderer.toScreen(point.clone().applyMatrix4(this.planeTransform)).distanceTo(coords)
          if(dist < snapDistance && (!intersectionTarget || dist < intersectionTarget.distance)) {
            intersectionTarget = { curves: [target.elem, otherTarget.elem], point, distance: dist }
          }
        })
      })
    )
    if(intersectionTarget) {
      this.rememberSnapPoint({ point: intersectionTarget.point })
      return intersectionTarget
    }

    if(midpointTarget) return this.rememberSnapPoint(midpointTarget)

    const curveTarget = nearbyCurves.reduce((closest, target) =>
      !closest || target.distance < closest.distance ? target : closest
    , null)

    if(curveTarget) return { elem: curveTarget.elem, point: curveTarget.point }

    if(guidePointTarget) return this.rememberSnapPoint(guidePointTarget)
  }

  rememberSnapPoint(pointTarget) {
    const point = pointTarget.point
    if(!(this.lastSnaps[0] && this.lastSnaps[0].equals(point))) {
      this.lastSnaps.unshift(point)
      if(this.lastSnaps.length > maxSnapReferences) this.lastSnaps.pop()
    }
    return { pointTarget }
  }

  getGuideSnapPoints() {
    const activePoints = this.viewport.activeTool.guideSnapPoints()
    const origin = this.viewport.document.activeSketch.originPoint()
    return [...activePoints, ...(origin ? [origin] : []), ...this.lastSnaps].filter((point, index, points) =>
      points.findIndex(other => other.equals(point)) === index
    )
  }

  curveGuideSnap(curve, guideSnapPoints, coords) {
    let closest
    guideSnapPoints.forEach(guidePoint => ['x', 'y'].forEach(axis => {
      curveGuideIntersections(curve, guidePoint, axis).forEach(point => {
        const worldPoint = point.clone().applyMatrix4(this.planeTransform)
        const distance = this.viewport.renderer.toScreen(worldPoint).distanceTo(coords)
        if(distance >= snapDistance || (closest && distance >= closest.distance)) return
        closest = { point, distance, [axis]: guidePoint }
      })
    }))
    return closest
  }

  snapToGuides(vec, snapToGuides, snapToPoints) {
    if(!vec) return

    const localTransform = this.planeTransform.clone().invert()
    const localVec = vec.clone().applyMatrix4(localTransform)

    if(!(snapToGuides || snapToPoints)) return localVec

    const snapTarget = snapToPoints &&
      this.catchSnapTargets(localVec, this.viewport.renderer.toScreen(vec))

    const screenVec = this.viewport.renderer.toScreen(vec)
    const guideSnapPoints = this.getGuideSnapPoints()

    if(snapTarget && snapTarget.curves) {
      const snapVec = snapTarget.point
      const worldSnapVec = snapVec.clone().applyMatrix4(this.planeTransform)
      this.snapped = { intersection: snapTarget.curves, point: snapVec }
      this.snapAnchor = {
        type: 'snap',
        pos: this.viewport.renderer.toScreen(worldSnapVec),
        vec: worldSnapVec,
        id: 'intersection-' + snapTarget.curves.map(curve => curve.id).join('-'),
      }
      return snapVec
    }

    if(snapTarget && snapTarget.elem) {
      const guideSnap = snapToGuides && this.curveGuideSnap(snapTarget.elem, guideSnapPoints, screenVec)
      const snapVec = guideSnap ? guideSnap.point : snapTarget.point
      const worldSnapVec = snapVec.clone().applyMatrix4(this.planeTransform)
      this.snapped = { curve: snapTarget.elem, ...guideSnap }
      delete this.snapped.point
      delete this.snapped.distance
      this.snapAnchor = {
        type: 'snap',
        pos: this.viewport.renderer.toScreen(worldSnapVec),
        vec: worldSnapVec,
        id: 'curve-' + snapTarget.elem.id,
      }
      if(guideSnap) {
        const guidePoint = guideSnap.x || guideSnap.y
        const start = this.viewport.renderer.toScreen(guidePoint.clone().applyMatrix4(this.planeTransform))
        this.guides.push({
          id: (guideSnap.x ? 'v' : 'h') + start.x + start.y,
          start,
          end: this.viewport.renderer.toScreen(worldSnapVec),
        })
      }
      return snapVec
    }

    let snapX = guideSnapPoints.find(snap => {
      // Compare plane space X axis..
      const testSnap = snap.clone()//.applyMatrix4(localTransform)
      testSnap.setY(localVec.y)
      testSnap.setZ(localVec.z)
      testSnap.applyMatrix4(this.planeTransform)
      const screenSnap = this.viewport.renderer.toScreen(testSnap)
      // .. in screen space
      return screenVec.distanceTo(screenSnap) < snapDistance
    })
    let snapY = guideSnapPoints.find(snap => {
      const testSnap = snap.clone()//.applyMatrix4(localTransform)
      testSnap.setX(localVec.x)
      testSnap.setZ(localVec.z)
      testSnap.applyMatrix4(this.planeTransform)
      const screenSnap = this.viewport.renderer.toScreen(testSnap)
      return screenVec.distanceTo(screenSnap) < snapDistance
    })
    const directSnap = snapTarget && snapTarget.pointTarget
    const pointTarget = directSnap &&
      snapX && snapY && snapX.equals(directSnap.point) && snapY.equals(directSnap.point) ? directSnap : null
    this.snapped = { x: snapX, y: snapY, pointTarget }
    // const snapVec = new THREE.Vector3(
    //   snapX ? snapX.clone().applyMatrix4(localTransform).x : localVec.x,
    //   snapY ? snapY.clone().applyMatrix4(localTransform).y : localVec.y,
    //   localVec.z
    // ) //XXX z-imprecision
    const snapVec = new THREE.Vector3(
      snapX ? snapX.x : localVec.x,
      snapY ? snapY.y : localVec.y,
      localVec.z
    )
    const worldSnapVec = snapVec.clone().applyMatrix4(this.planeTransform)
    const screenSnapVec = this.viewport.renderer.toScreen(worldSnapVec)
    if(snapToGuides) {
      if(snapX) {
        const start = this.viewport.renderer.toScreen(snapX.clone().applyMatrix4(this.planeTransform))
        this.guides.push({
          id: 'v' + start.x + start.y,
          start,
          end: screenSnapVec,
        })
      }
      if(snapY) {
        const start = this.viewport.renderer.toScreen(snapY.clone().applyMatrix4(this.planeTransform))
        this.guides.push({
          id: 'h' + start.x + start.y,
          start,
          end: screenSnapVec,
        })
      }
    }
    if(snapX && snapY) {
      this.snapAnchor = {
        type: 'snap',
        pos: screenSnapVec,
        vec: worldSnapVec,
        id: '' + snapVec.x + snapVec.y + snapVec.z,
      }
      if(snapX === snapY && snapToPoints) return snapX//.clone().applyMatrix4(localTransform)
    } else {
      this.snapAnchor = null
    }

    if((snapX || snapY) && snapToGuides) return snapVec

    return localVec
  }
}
