import * as THREE from 'three'
import { Line, Circle, Arc } from './core/geom2d.js'
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
    })

    // Handles and the sketch origin always take precedence over curves.
    if(pointTarget) return this.rememberSnapPoint(pointTarget)

    let curveDist = Infinity
    let curveTarget
    elements
      .filter(elem => elem instanceof Line || elem instanceof Circle || elem instanceof Arc)
      .forEach(elem => {
        const projected = closestPointOnCurve(elem, localVec)
        if(!projected) return
        const dist = this.viewport.renderer.toScreen(projected.clone().applyMatrix4(this.planeTransform)).distanceTo(coords)
        if(dist < snapDistance && dist < curveDist) {
          curveDist = dist
          curveTarget = { elem, point: projected }
        }
      })

    if(curveTarget) return curveTarget

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

  snapToGuides(vec, snapToGuides, snapToPoints) {
    if(!vec) return

    const localTransform = this.planeTransform.clone().invert()
    const localVec = vec.clone().applyMatrix4(localTransform)

    if(!(snapToGuides || snapToPoints)) return localVec

    const snapTarget = snapToPoints &&
      this.catchSnapTargets(localVec, this.viewport.renderer.toScreen(vec))

    if(snapTarget && snapTarget.elem) {
      const snapVec = snapTarget.point
      const worldSnapVec = snapVec.clone().applyMatrix4(this.planeTransform)
      this.snapped = { curve: snapTarget.elem }
      this.snapAnchor = {
        type: 'snap',
        pos: this.viewport.renderer.toScreen(worldSnapVec),
        vec: worldSnapVec,
        id: 'curve-' + snapTarget.elem.id,
      }
      return snapVec
    }

    const screenVec = this.viewport.renderer.toScreen(vec)
    const guideSnapPoints = this.getGuideSnapPoints()
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
